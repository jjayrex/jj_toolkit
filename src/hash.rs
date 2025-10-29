use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand, ValueEnum};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Subcommand)]
pub enum HashCmd {
    File(FileArgs),
    Dir(DirArgs),
    VerifyFile(VerifyFileArgs),
    VerifyDir(VerifyDirArgs),
}

#[derive(Clone, Copy, ValueEnum, Debug)]
pub enum Algorithm {
    Blake3,
    Md5,
    Sha1,
    Sha256,
}

#[derive(Args)]
pub struct FileArgs {
    pub path: PathBuf,
    #[arg(short, long, default_value_t = Algorithm::Blake3)]
    pub algorithm: Algorithm,
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Args)]
pub struct DirArgs {
    pub path: PathBuf,
    #[arg(short, long, default_value_t = Algorithm::Blake3)]
    pub algorithm: Algorithm,
    #[arg(short, long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub hidden: bool,
}

#[derive(Args)]
pub struct VerifyFileArgs {
    pub path: PathBuf,
    #[arg(short = 'e', long)]
    pub expected: Option<String>,
    #[arg(long)]
    pub expected_file: Option<PathBuf>,
    #[arg(short, long)]
    pub algorithm: Option<Algorithm>,
}

#[derive(Args)]
pub struct VerifyDirArgs {
    pub path: PathBuf,
    pub manifest: PathBuf,
}

impl std::fmt::Display for Algorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Algorithm::Blake3 => "blake3",
            Algorithm::Md5 => "md5",
            Algorithm::Sha1 => "sha1",
            Algorithm::Sha256 => "sha256",
        })
    }
}

pub fn run(cmd: HashCmd) -> Result<()> {
    match cmd {
        HashCmd::File(a) => hash_file_cmd(a),
        HashCmd::Dir(a) => hash_dir_cmd(a),
        HashCmd::VerifyFile(a) => verify_file_cmd(a),
        HashCmd::VerifyDir(a) => verify_dir_cmd(a),
    }
}

// CORE
fn hash_reader(mut r: impl Read, algorithm: Algorithm) -> Result<String> {
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
            Ok(h.finalize().to_hex().to_string())
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
            Ok(hex::encode(h.finalize()))
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
            Ok(hex::encode(h.finalize()))
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
            Ok(hex::encode(h.finalize()))
        }
    }
}

fn hash_file(path: &Path, algorithm: Algorithm) -> Result<String> {
    let f = File::open(path).with_context(|| format!("open {}", path.display()))?;
    hash_reader(f, algorithm)
}

// COMMANDS
fn hash_file_cmd(a: FileArgs) -> Result<()> {
    let hex = hash_file(&a.path, a.algorithm)?;
    if let Some(output) = a.output {
        let mut w = File::create(&output)?;
        writeln!(w, "algorithm={:?}", a.algorithm)?;
        writeln!(w, "{hex}")?;
    } else {
        println!("{hex}  {}", a.path.display());
    }
    Ok(())
}

fn hash_dir_cmd(a: DirArgs) -> Result<()> {
    let root = fs::canonicalize(&a.path).unwrap_or(a.path.clone());

    let output_path = match &a.output {
        Some(p) => p.clone(),
        None => {
            let name = root.file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "hashes".to_string());
            let file = format!("{}.{}", name, a.algorithm);
            std::env::current_dir()?.join(file)
        }
    };

    let mut output = File::create(&output_path)?;
    writeln!(output, "# algorithm={:?}", a.algorithm)?;

    for entry in WalkDir::new(&root).into_iter().filter_entry(|e| {
        if e.file_type().is_dir() { return true; }
        if !a.hidden {
            if let Some(name) = e.file_name().to_str() {
                if name.starts_with('.') { return false; }
            }
        }
        true
    }) {
        let entry = entry?;
        if entry.file_type().is_dir() { continue; }
        let p = entry.path();
        let rel = path_unixy(p.strip_prefix(&root).unwrap_or(p));
        let hex = hash_file(p, a.algorithm)?;
        writeln!(output, "{hex}  {rel}")?;
    }
    println!("Wrote manifest: {}", output_path.display());
    Ok(())
}

fn verify_file_cmd(a: VerifyFileArgs) -> Result<()> {
    let (algorithm, expected) = match (&a.expected, &a.expected_file) {
        (Some(hex), _) => (a.algorithm.context("algo is required when using --expected")?, hex.trim().to_string()),
        (None, Some(f)) => read_expected_with_optional_header(f, a.algorithm)?,
        _ => bail!("provide --expected HEX with --algo, or --expected-file FILE"),
    };

    let got = hash_file(&a.path, algorithm)?;
    if eq_hex(&got, &expected) {
        println!("OK  {}", a.path.display());
        Ok(())
    } else {
        println!("MISMATCH  {}\nexpected {}\n     got {}", a.path.display(), expected, got);
        bail!("hash mismatch")
    }
}

fn verify_dir_cmd(a: VerifyDirArgs) -> Result<()> {
    let (algorithm, map_expected) = read_manifest(&a.manifest)?;
    let root = fs::canonicalize(&a.path).unwrap_or(a.path.clone());

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut mismatches: Vec<(String, String, String)> = vec![]; // rel, exp, got

    for entry in WalkDir::new(&root) {
        let entry = entry?;
        if entry.file_type().is_dir() { continue; }
        let p = entry.path();
        let rel = path_unixy(p.strip_prefix(&root).unwrap_or(p));
        let got = hash_file(p, algorithm)?;
        seen.insert(rel.clone());
        if let Some(exp) = map_expected.get(&rel) {
            if !eq_hex(&got, exp) {
                mismatches.push((rel.clone(), exp.clone(), got));
            }
        }
    }

    // missing and extra
    let expected_set: BTreeSet<_> = map_expected.keys().cloned().collect();
    let missing: Vec<_> = expected_set.difference(&seen).cloned().collect();
    let extra: Vec<_> = seen.difference(&expected_set).cloned().collect();

    // report
    if mismatches.is_empty() && missing.is_empty() && extra.is_empty() {
        println!("OK  directory matches manifest");
        return Ok(());
    }

    if !mismatches.is_empty() {
        println!("MISMATCHED FILES:");
        for (rel, exp, got) in mismatches { println!("  {rel}\n    expected {exp}\n    got      {got}"); }
    }
    if !missing.is_empty() {
        println!("MISSING FILES:");
        for rel in missing { println!("  {rel}"); }
    }
    if !extra.is_empty() {
        println!("EXTRA FILES:");
        for rel in extra { println!("  {rel}"); }
    }
    bail!("verification failed")
}

// HELPERS
fn path_unixy(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    s.replace('\\', "/")
}

fn eq_hex(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

fn read_expected_with_optional_header(path: &Path, cli_algo: Option<Algorithm>) -> Result<(Algorithm, String)> {
    let f = File::open(path)?;
    let mut r = BufReader::new(f);
    let mut first = String::new();
    let n = r.read_line(&mut first)?;
    if n > 0 && first.trim_start().starts_with("algorithm=") {
        let algorithm = parse_algorithm(first.trim_start().trim_start_matches("algorithm=").trim())?;
        let mut hex = String::new();
        r.read_line(&mut hex)?;
        Ok((algorithm, hex.trim().to_string()))
    } else {
        let algo = cli_algo.context("manifest has no 'algo=' header. Supply --algo")?;
        let hex = if n == 0 { String::new() } else { first };
        Ok((algo, hex.trim().to_string()))
    }
}

fn parse_algorithm(s: &str) -> Result<Algorithm> {
    let s = s.trim();
    let a = match s.to_ascii_lowercase().as_str() {
        "blake3" => Algorithm::Blake3,
        "md5" => Algorithm::Md5,
        "sha1" => Algorithm::Sha1,
        "sha256" => Algorithm::Sha256,
        _ => bail!("unknown algo '{s}'"),
    };
    Ok(a)
}

fn read_manifest(path: &Path) -> Result<(Algorithm, BTreeMap<String,String>)> {
    let f = File::open(path)?;
    let r = BufReader::new(f);
    let mut algorithm: Option<Algorithm> = None;
    let mut map = BTreeMap::new();

    for (i, line) in r.lines().enumerate() {
        let line = line?;
        let t = line.trim();
        if t.is_empty() { continue; }
        if t.starts_with('#') {
            if let Some(rest) = t.strip_prefix("# algorithm=") {
                algorithm = Some(parse_algorithm(rest)?);
            }
            continue;
        }
        // "<hex>  <relpath>"
        if let Some((hex, rel)) = t.split_once("  ") {
            map.insert(rel.to_string(), hex.to_string());
        } else {
            bail!("bad manifest line {}: {t}", i + 1);
        }
    }
    let algo = algorithm.context("manifest missing '# algorithm=...' header")?;
    Ok((algo, map))
}