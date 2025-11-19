use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use hex::encode_upper;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Clone, Copy, ValueEnum, Debug)]
pub enum Algorithm {
    Blake3,
    Md5,
    Sha1,
    Sha256,
    Crc32,
    Crc32c,
}

#[derive(Args)]
#[command[name = "hash", about = "Simple file hashing and manifest generation using Blake3, SHA256, SHA1 and MD5"]]
pub struct HashArgs {
    path: PathBuf,
    #[arg(short = 'd', long)]
    directory: bool,
    #[arg(short, long, default_value_t = Algorithm::Blake3)]
    algorithm: Algorithm,
    #[arg(long)]
    decimal: bool,
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(Args)]
#[command[name = "verify-hash", about = "Simple file/manifest hash verification supporting Blake3, SHA256, SHA1 and MD5"]]
pub struct HashVerifyArgs {
    path: PathBuf,
    #[arg(short = 'e', long)]
    expected: Option<String>,
    #[arg(short, long)]
    algorithm: Option<Algorithm>,
    #[arg(long)]
    decimal: bool,
}

impl std::fmt::Display for Algorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Algorithm::Blake3 => "blake3",
            Algorithm::Md5 => "md5",
            Algorithm::Sha1 => "sha1",
            Algorithm::Sha256 => "sha256",
            Algorithm::Crc32 => "crc32",
            Algorithm::Crc32c => "crc32c",
        })
    }
}

// CORE

fn ensure_decimal_supported(algorithm: Algorithm, decimal: bool) -> Result<()> {
    if decimal {
        match algorithm {
            Algorithm::Crc32 | Algorithm::Crc32c => Ok(()),
            _ => bail!("--decimal is only supported for CRC32 and CRC32C algorithms"),
        }
    } else {
        Ok(())
    }
}
fn hash_reader(mut r: impl Read, algorithm: Algorithm, decimal: bool) -> Result<String> {
    const BUFFER: usize = 1024 * 1024;
    match algorithm {
        Algorithm::Blake3 => {
            let mut h = blake3::Hasher::new();
            let mut buf = vec![0u8; BUFFER];
            loop {
                let n = r.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            let output = h.finalize();
            Ok(encode_upper(output.as_bytes()))
        }
        Algorithm::Md5 => {
            use md5::{Digest, Md5};
            let mut h = Md5::new();
            let mut buf = vec![0u8; BUFFER];
            loop {
                let n = r.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            let output = h.finalize();
            Ok(encode_upper(output))
        }
        Algorithm::Sha1 => {
            use sha1::{Digest, Sha1};
            let mut h = Sha1::new();
            let mut buf = vec![0u8; BUFFER];
            loop {
                let n = r.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            let output = h.finalize();
            Ok(encode_upper(output))
        }
        Algorithm::Sha256 => {
            use sha2::{Digest, Sha256};
            let mut h = Sha256::new();
            let mut buf = vec![0u8; BUFFER];
            loop {
                let n = r.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            let output = h.finalize();
            Ok(encode_upper(output))
        }
        Algorithm::Crc32 => {
            let mut h = crc32fast::Hasher::new();
            let mut buf = vec![0u8; BUFFER];
            loop {
                let n = r.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                h.update(&buf[..n]);
            }
            let value = h.finalize();
            if decimal {
                Ok(value.to_string())
            } else {
                Ok(format!("{:08X}", value))
            }
        }
        Algorithm::Crc32c => {
            let mut crc: u32 = 0;
            let mut buf = vec![0u8; BUFFER];
            loop {
                let n = r.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                crc = crc32c::crc32c_append(crc, &buf[..n]);
            }
            if decimal {
                Ok(crc.to_string())
            } else {
                Ok(format!("{:08X}", crc))
            }
        }
    }
}

fn hash_file(path: &Path, algorithm: Algorithm, decimal: bool) -> Result<String> {
    let f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    hash_reader(f, algorithm, decimal)
}

// COMMANDS
pub fn hash(a: HashArgs) -> Result<()> {
    ensure_decimal_supported(a.algorithm, a.decimal)?;

    if a.directory {
        let root = fs::canonicalize(&a.path).unwrap_or(a.path.clone());
        let top = root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "root".to_string());

        let out_path = match &a.output {
            Some(p) => p.clone(),
            None => std::env::current_dir()?.join(format!("{top}.{}", a.algorithm)),
        };

        let mut out = File::create(&out_path)?;

        for entry in WalkDir::new(&root) {
            let entry = entry?;
            if entry.file_type().is_dir() {
                continue;
            }

            let abs = entry.path();
            let rel = abs.strip_prefix(&root).unwrap_or(abs);
            let rel_with_top = Path::new(&top).join(rel);

            let line_path_unix = rel_with_top.to_string_lossy().replace('\\', "/");
            let line_path_win = rel_with_top.to_string_lossy().replace('/', "\\");

            let hex = hash_file(abs, a.algorithm, a.decimal)?;
            writeln!(out, "#{}#{}", a.algorithm, line_path_win)?;
            writeln!(out, "{} *{}", hex, line_path_unix)?;
        }

        println!("Wrote manifest: {}", out_path.display());
    } else {
        let hex = hash_file(&a.path, a.algorithm, a.decimal)?;
        if let Some(out) = a.output {
            let name = a
                .path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| a.path.to_string_lossy().into_owned());
            let unix = name.replace('\\', "/");
            let win = name.replace('/', "\\");

            let mut w = File::create(&out)?;
            writeln!(w, "#{}#{}", a.algorithm, win)?;
            writeln!(w, "{} *{}", hex, unix)?;
        } else {
            println!("{hex}  {}", a.path.display());
        }
    }
    Ok(())
}

pub fn hash_verify(a: HashVerifyArgs) -> Result<()> {
    if a.expected.is_some() {
        let expected = a.expected.context("error with provided --expected")?;
        let algorithm = a.algorithm.unwrap_or(Algorithm::Blake3);

        ensure_decimal_supported(algorithm, a.decimal)?;

        let got = hash_file(&a.path, algorithm, a.decimal)?;
        if eq_hex(&got, &expected) {
            println!("OK  {}", a.path.display());
            Ok(())
        } else {
            println!(
                "MISMATCH  {}\nexpected {}\n     got {}",
                a.path.display(),
                expected,
                got
            );
            bail!("hash mismatch")
        }
    } else {
        let (algo, map_expected) = read_manifest(&a.path)?;
        if map_expected.is_empty() {
            bail!("manifest has no entries");
        }

        ensure_decimal_supported(algo, a.decimal)?;

        // Detect top prefix from first key: "TopDir/inner/file"
        let first_key = map_expected.keys().next().unwrap();
        let (with_top, manifest_top) = if let Some((prefix, _)) = first_key.split_once('/') {
            (true, prefix.to_string())
        } else {
            (false, String::new())
        };

        // Infer root dir from CWD and manifest top (if present)
        let cwd = std::env::current_dir()?;
        let root = if with_top {
            let candidate = cwd.join(&manifest_top);
            if !candidate.is_dir() {
                bail!(
                    "cannot locate directory '{}'\nlooked at: {}",
                    manifest_top,
                    candidate.display()
                );
            }
            candidate
        } else {
            cwd
        };

        // Walk filesystem and compute hashes
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut mismatches: Vec<(String, String, String)> = vec![];

        for entry in WalkDir::new(&root) {
            let entry = entry?;
            if entry.file_type().is_dir() {
                continue;
            }
            let p = entry.path();
            let rel = p.strip_prefix(&root).unwrap_or(p);
            let rel_unix = rel.to_string_lossy().replace('\\', "/");

            // Key must match manifest keys shape
            let key = if with_top {
                format!("{}/{}", manifest_top, rel_unix)
            } else {
                rel_unix
            };

            let got = hash_file(p, algo, a.decimal)?;
            seen.insert(key.clone());
            if let Some(exp) = map_expected.get(&key) {
                if !eq_hex(&got, exp) {
                    mismatches.push((key, exp.clone(), got));
                }
            }
        }

        // Missing and extra
        let expected_set: BTreeSet<_> = map_expected.keys().cloned().collect();
        let missing: Vec<_> = expected_set.difference(&seen).cloned().collect();
        let extra: Vec<_> = seen.difference(&expected_set).cloned().collect();

        if mismatches.is_empty() && missing.is_empty() && extra.is_empty() {
            println!("OK  directory matches manifest");
            return Ok(());
        }

        if !mismatches.is_empty() {
            println!("MISMATCHED FILES:");
            for (k, exp, got) in mismatches {
                println!("  {k}\n    expected {exp}\n    got      {got}");
            }
        }
        if !missing.is_empty() {
            println!("MISSING FILES:");
            for k in missing {
                println!("  {k}");
            }
        }
        if !extra.is_empty() {
            println!("EXTRA FILES:");
            for k in extra {
                println!("  {k}");
            }
        }
        bail!("verification failed")
    }
}

// HELPERS
fn eq_hex(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn parse_algorithm(s: &str) -> Result<Algorithm> {
    let s = s.trim();
    let a = match s.to_ascii_lowercase().as_str() {
        "blake3" => Algorithm::Blake3,
        "md5" => Algorithm::Md5,
        "sha1" => Algorithm::Sha1,
        "sha256" => Algorithm::Sha256,
        "crc32" => Algorithm::Crc32,
        "crc32c" => Algorithm::Crc32c,
        _ => bail!("unknown algorithm '{s}'"),
    };
    Ok(a)
}

fn read_manifest(path: &Path) -> Result<(Algorithm, BTreeMap<String, String>)> {
    let f = File::open(path)?;
    let r = BufReader::new(f);

    let mut algorithm: Option<Algorithm> = None;
    let mut map = BTreeMap::new();

    let mut last_path_unified: Option<String> = None;

    for (i, line) in r.lines().enumerate() {
        let line = line?;
        let t = line.trim();
        if t.is_empty() {
            continue;
        }

        if let Some(rest) = t.strip_prefix('#') {
            if let Some((alg, p)) = rest.split_once('#') {
                if algorithm.is_none() {
                    algorithm = Some(parse_algorithm(alg)?);
                }
                // unify to unix for keys
                let path_unix = p.replace('\\', "/");
                last_path_unified = Some(path_unix);
                continue;
            } else {
                bail!("bad header at line {}", i + 1);
            }
        }

        // body: "<HASH> *path" (hash may be hex or decimal)
        if let Some((hash, p)) = t.split_once(" *") {
            let path_unix = p.replace('\\', "/");
            let key = path_unix.clone();
            map.insert(key, hash.trim().to_string());
            last_path_unified = None;
        } else if let Some(prev) = last_path_unified.take() {
            // tolerate body without leading " *"
            let parts: Vec<_> = t.split_whitespace().collect();
            if parts.len() == 1 {
                map.insert(prev, parts[0].to_string());
            } else {
                bail!("bad body at line {}", i + 1);
            }
        } else {
            bail!("unexpected line {}", i + 1);
        }
    }

    let algo = algorithm.context("manifest missing algorithm header")?;
    Ok((algo, map))
}
