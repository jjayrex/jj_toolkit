use anyhow::{Context, Result, bail, ensure};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce, aead::Aead};
use clap::{Args};
use rand::TryRngCore;
use rand::rngs::OsRng;
use std::ffi::OsStr;
use std::fs::File;
use std::io::{BufReader, BufWriter, Cursor, Read, Write};
use std::path::{Path, PathBuf};
use tar::{Archive as TarArchive, Builder as TarBuilder};
use zeroize::Zeroize;

const MAGIC: &[u8; 6] = b"JJTOOL";
const VERSION: u8 = 2;

#[repr(u8)]
enum Kind {
    File = 0,
    Directory = 1,
}


#[derive(Args)]
pub struct EncryptArgs {
    input: PathBuf,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(short = 'd', long)]
    directory: bool,
    #[arg(long, default_value_t = 19_456)]
    m_cost_kib: u32,
    #[arg(long, default_value_t = 2)]
    t_cost: u32,
    #[arg(long, default_value_t = 1)]
    p_cost: u32,
}

#[derive(Args)]
pub struct DecryptArgs {
    input: PathBuf,
    #[arg(short, long)]
    output: Option<PathBuf>,
}

pub fn encrypt(a: EncryptArgs) -> Result<()> {
    let input_path = &a.input;
    let output_path = a.output.clone().unwrap_or_else(|| {
        let mut out = input_path.clone();
        out.set_extension("jj");
        out
    });

    // Ask for password
    let mut password = rpassword::prompt_password("Password: ")?;
    let kdf_params =
        Params::new(a.m_cost_kib, a.t_cost, a.p_cost, None).context("invalid Argon2 params")?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, kdf_params);

    // Salt + Key
    let mut salt = [0u8; 16];
    OsRng.try_fill_bytes(&mut salt)?;
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .context("argon2 key derivation failed")?;

    // Cipher + Nonce
    let cipher = XChaCha20Poly1305::new((&key).into());
    let mut nonce_bytes = [0u8; 24];
    OsRng.try_fill_bytes(&mut nonce_bytes)?;
    let nonce = XNonce::from(nonce_bytes);

    // Build package
    let pkg = if a.directory {
        ensure!(input_path.is_dir(), "input is not a directory");

        // Base name
        let base_name = input_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("dir");
        let base_bytes = base_name.as_bytes();
        let base_len = u16::try_from(base_bytes.len()).context("base dir name too long")?;

        // TAR
        let mut tar_buf = Vec::new();
        {
            let mut builder = TarBuilder::new(&mut tar_buf);
            builder
                .append_dir_all(base_name, input_path)
                .with_context(|| format!("tar {}", input_path.display()))?;
            builder.finish()?;
        }
        let zstd_bytes =
            zstd::encode_all(Cursor::new(tar_buf), 10).context("zstd encode failed")?;

        let mut pkg = Vec::with_capacity(1 + 2 + base_bytes.len() + zstd_bytes.len());
        pkg.push(Kind::Directory as u8);
        pkg.extend_from_slice(&base_len.to_le_bytes());
        pkg.extend_from_slice(&base_bytes[..base_len as usize]);
        pkg.extend_from_slice(&zstd_bytes);
        pkg
    } else {
        ensure!(input_path.is_file(), "input is not a file");

        // Read file
        let mut reader = BufReader::new(
            File::open(input_path).with_context(|| format!("open {}", input_path.display()))?,
        );
        let mut file_bytes = Vec::new();
        reader
            .read_to_end(&mut file_bytes)
            .with_context(|| format!("read {}", input_path.display()))?;

        // Extension
        let ext_str = input_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or_default();
        let ext_bytes = ext_str.as_bytes();
        let ext_len = u16::try_from(ext_bytes.len()).unwrap_or(u16::MAX);

        // Payload
        let mut pkg = Vec::with_capacity(1 + 2 + ext_bytes.len() + file_bytes.len());
        pkg.push(Kind::File as u8);
        pkg.extend_from_slice(&ext_len.to_le_bytes());
        pkg.extend_from_slice(&ext_bytes[..ext_len as usize]);
        pkg.extend_from_slice(&file_bytes);

        file_bytes.zeroize();
        pkg
    };

    // Encrypt
    let ciphertext = cipher.encrypt(&nonce, pkg.as_ref()).unwrap();

    // Zeroize secrets
    password.zeroize();
    key.zeroize();

    // Write header + cipher text
    let mut w = BufWriter::new(
        File::create(&output_path).with_context(|| format!("create {}", output_path.display()))?,
    );
    w.write_all(MAGIC)?;
    w.write_all(&[VERSION])?;
    w.write_all(&a.m_cost_kib.to_le_bytes())?;
    w.write_all(&a.t_cost.to_le_bytes())?;
    w.write_all(&a.p_cost.to_le_bytes())?;
    w.write_all(&salt)?;
    w.write_all(&nonce_bytes)?;
    let ct_len = ciphertext.len() as u64;
    w.write_all(&ct_len.to_le_bytes())?;
    w.write_all(&ciphertext)?;
    w.flush()?;
    Ok(())
}

pub fn decrypt(a: DecryptArgs) -> Result<()> {
    let input_path = &a.input;

    // Parse header
    let mut r = BufReader::new(
        File::open(input_path).with_context(|| format!("open {}", input_path.display()))?,
    );

    let mut magic = [0u8; 6];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        bail!("wrong magic");
    }

    let mut ver = [0u8; 1];
    r.read_exact(&mut ver)?;
    if ver[0] != 1 && ver[0] != VERSION {
        bail!("unsupported version {}", ver[0]);
    }
    let payload_version = ver[0];

    let m_cost_kib = read_u32(&mut r)?;
    let t_cost = read_u32(&mut r)?;
    let p_cost = read_u32(&mut r)?;

    let mut salt = [0u8; 16];
    r.read_exact(&mut salt)?;
    let mut nonce_bytes = [0u8; 24];
    r.read_exact(&mut nonce_bytes)?;

    // Read cipher text
    let ct_len = read_u64(&mut r)?;
    let mut ciphertext = vec![0u8; ct_len as usize];
    r.read_exact(&mut ciphertext)?;

    // Password + Key
    let mut password = rpassword::prompt_password("Password: ")?;
    let kdf_params = Params::new(m_cost_kib, t_cost, p_cost, None)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, kdf_params);

    let mut key = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), &salt, &mut key)
        .context("argon2 key derivation failed")?;

    // Decrypt
    let cipher = XChaCha20Poly1305::new((&key).into());
    let nonce = XNonce::from(nonce_bytes);
    let pkg = cipher.decrypt(&nonce, ciphertext.as_ref()).unwrap();

    // Zeroize secrets
    password.zeroize();
    key.zeroize();

    // Legacy V1
    if payload_version == 1 {
        ensure!(pkg.len() >= 2, "truncated payload");
        let ext_len = u16::from_le_bytes([pkg[0], pkg[1]]) as usize;
        ensure!(pkg.len() >= 2 + ext_len, "truncated payload");
        let ext_bytes = &pkg[2..2 + ext_len];
        let file_bytes = &pkg[2 + ext_len..];
        let org_ext = String::from_utf8_lossy(ext_bytes).to_string();

        let output_path = a.output.clone().unwrap_or_else(|| {
            let stem = input_path
                .file_stem()
                .unwrap_or_else(|| OsStr::new("output"));
            let parent = input_path.parent().unwrap_or(Path::new("."));
            let mut out = parent.join(stem);
            let ext_for_out = if org_ext.is_empty() { "out" } else { &org_ext };
            out.set_extension(ext_for_out);
            out
        });

        let mut w = BufWriter::new(
            File::create(&output_path)
                .with_context(|| format!("create {}", output_path.display()))?,
        );
        w.write_all(file_bytes)?;
        w.flush()?;
        return Ok(());
    }

    // Active V2
    ensure!(pkg.len() >= 1, "truncated payload");
    let kind = pkg[0];

    if kind == Kind::File as u8 {
        ensure!(pkg.len() >= 1 + 2, "truncated payload");
        let ext_len = u16::from_le_bytes([pkg[1], pkg[2]]) as usize;
        ensure!(pkg.len() >= 3 + ext_len + 1, "truncated payload");
        let ext_bytes = &pkg[3..3 + ext_len];
        let data = &pkg[3 + ext_len..];

        let file_bytes = data.to_vec();

        let org_ext = String::from_utf8_lossy(ext_bytes).to_string();
        let output_path = a.output.clone().unwrap_or_else(|| {
            let stem = input_path
                .file_stem()
                .unwrap_or_else(|| OsStr::new("output"));
            let parent = input_path.parent().unwrap_or(Path::new("."));
            let mut out = parent.join(stem);
            let ext_for_out = if org_ext.is_empty() { "out" } else { &org_ext };
            out.set_extension(ext_for_out);
            out
        });

        let mut w = BufWriter::new(
            File::create(&output_path)
                .with_context(|| format!("create {}", output_path.display()))?,
        );
        w.write_all(&file_bytes)?;
        w.flush()?;
    } else {
        ensure!(pkg.len() >= 1 + 2, "truncated payload");
        let name_len = u16::from_le_bytes([pkg[1], pkg[2]]) as usize;
        ensure!(pkg.len() >= 3 + name_len + 1, "truncated payload");
        let _base_name = &pkg[3..3 + name_len]; // informational
        let data = &pkg[3 + name_len..];

        let decoded: Box<dyn Read> =
            Box::new(zstd::Decoder::new(Cursor::new(data)).context("zstd decoder init failed")?);

        // Extraction point
        let extract_parent = if let Some(out) = a.output.clone() {
            if !out.exists() {
                std::fs::create_dir_all(&out)
                    .with_context(|| format!("create {}", out.display()))?;
            }
            out
        } else {
            input_path.parent().unwrap_or(Path::new(".")).to_path_buf()
        };

        // Extraction
        let mut ar = TarArchive::new(decoded);
        for entry in ar.entries().context("reading tar entries failed")? {
            let mut e = entry.context("invalid tar entry")?;
            e.unpack_in(&extract_parent).context("tar unpack failed")?;
        }
    }
    Ok(())
}

fn read_u32(r: &mut dyn Read) -> Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(r: &mut dyn Read) -> Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}