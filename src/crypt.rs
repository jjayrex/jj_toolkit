use std::ffi::OsStr;
use anyhow::{Context, Result, bail, ensure};
use clap::{Args, Subcommand};
use rand::rngs::OsRng;
use rand::TryRngCore;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::io::{Read, Write, BufReader, BufWriter};
use argon2::{Argon2, Algorithm, Version, Params};
use chacha20poly1305::{aead::Aead, XChaCha20Poly1305, KeyInit, XNonce};
use zeroize::Zeroize;

const MAGIC: &[u8; 6] = b"JJTOOL";
const VERSION: u8 = 1;

#[derive(Subcommand)]
pub enum CryptCmd {
    Encrypt(EncryptArgs),
    Decrypt(DecryptArgs),
}

#[derive(Args)]
pub struct EncryptArgs {
    input: PathBuf,
    #[arg(short, long)]
    output: Option<PathBuf>,
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

pub fn run(cmd: CryptCmd) -> Result<()> {
    match cmd {
        CryptCmd::Encrypt(a) => encrypt_file(a),
        CryptCmd::Decrypt(a) => decrypt_file(a),
    }
}

fn encrypt_file(a: EncryptArgs) -> Result<()> {
    let input_path = &a.input;
    let output_path = a.output.clone().unwrap_or_else(|| {
        let mut out = input_path.clone();
        out.set_extension("jj");
        out
    });

    // Read text
    let mut reader = BufReader::new(File::open(input_path).with_context(|| format!("open {}", input_path.display()))?);
    let mut file_bytes = Vec::new();
    reader.read_to_end(&mut file_bytes).with_context(|| format!("read {}", input_path.to_string_lossy()))?;

    // Ask for password
    let mut password = rpassword::prompt_password("Password: ")?;
    let kdf_params = Params::new(a.m_cost_kib, a.t_cost, a.p_cost, None).context("invalid Argon2 params")?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, kdf_params);

    // Salt + Key
    let mut salt = [0u8; 16];
    OsRng.try_fill_bytes(&mut salt)?;

    let mut key = [0u8; 32];
    argon2.hash_password_into(password.as_bytes(), &salt, &mut key).context("argon2 key derivation failed")?;

    // Cipher + Nonce
    let cipher = XChaCha20Poly1305::new((&key).into());
    let mut nonce_bytes = [0u8; 24];
    OsRng.try_fill_bytes(&mut nonce_bytes)?;
    let nonce = XNonce::from(nonce_bytes);

    // Extension
    let ext_str = input_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    let ext_bytes = ext_str.as_bytes();
    let ext_len = u16::try_from(ext_bytes.len()).unwrap_or(u16::MAX);

    let mut pkg = Vec::with_capacity(2 + ext_bytes.len() + file_bytes.len());
    pkg.extend_from_slice(&ext_len.to_le_bytes());
    pkg.extend_from_slice(&ext_bytes[..ext_len as usize]);
    pkg.extend_from_slice(&file_bytes);
    file_bytes.zeroize();

    // Encrypt
    let ciphertext = cipher.encrypt(&nonce, pkg.as_ref()).unwrap();

    // Zeroize buffers
    password.zeroize();
    key.zeroize();

    // Write header + cipher text
    let mut w = BufWriter::new(File::create(&output_path).with_context(|| format!("create {}", output_path.display()))?);

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

fn decrypt_file(a: DecryptArgs) -> Result<()> {
    let input_path = &a.input;

    // Parse header
    let mut r = BufReader::new(File::open(input_path).with_context(|| format!("open {}", input_path.display()))?);

    let mut magic = [0u8; 6];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC { bail!("wrong magic"); }

    let mut ver = [0u8; 1];
    r.read_exact(&mut ver)?;
    if ver[0] != VERSION { bail!("wrong version {}", ver[0]); }

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
    argon2.hash_password_into(password.as_bytes(), &salt, &mut key).context("argon2 key derivation failed")?;

    // Decrypt
    let cipher = XChaCha20Poly1305::new((&key).into());
    let nonce = XNonce::from(nonce_bytes);

    let pkg = cipher.decrypt(&nonce, ciphertext.as_ref()).unwrap();

    // Parse the payload
    if pkg.len() < 2 { bail!("truncated payload"); }
    let ext_len = u16::from_le_bytes([pkg[0], pkg[1]]) as usize;
    ensure!(pkg.len() >= 2 + ext_len, "truncated payload");
    let ext_bytes = &pkg[2 .. 2 + ext_len];
    let file_bytes = &pkg[2 + ext_len ..];

    let org_ext = String::from_utf8_lossy(ext_bytes).to_string();

    // Output Path
    let output_path = a.output.clone().unwrap_or_else(|| {
        let stem = input_path.file_stem().unwrap_or_else(|| OsStr::new("output"));
        let parent = input_path.parent().unwrap_or(Path::new("."));
        let mut out = parent.join(stem);
        let ext_for_out = if org_ext.is_empty() { "out" } else { &org_ext };
        out.set_extension(ext_for_out);
        out
    });

    // Zeroize
    password.zeroize();
    key.zeroize();

    // Write text
    let mut w = BufWriter::new(File::create(&output_path).with_context(|| format!("create {}", output_path.display()))?);
    w.write_all(file_bytes)?;
    w.flush()?;
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