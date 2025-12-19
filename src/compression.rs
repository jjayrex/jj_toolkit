use anyhow::{Result, bail};
use std::path::{Path, PathBuf};
use std::{fs, fs::File};
use std::{io, io::{Read, Write}};
use clap::{Args, ValueEnum};
use walkdir::WalkDir;

#[derive(Args)]
#[command[name = "compression", about = "Simple file compression using Zstd, LZ4, Brotli or Snappy"]]
pub struct CompressionArgs {
    input: PathBuf,
    #[arg(short = 'r', long)]
    recursive: bool,
    #[arg(short, long, value_enum, default_value_t = Algorithm::Zstd)]
    algorithm: Algorithm,
    #[arg(short, long, default_value_t = 5)]
    compression_level: u32,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(short = 't', long)]
    threads: Option<u32>,
}

#[derive(Args)]
#[command[name = "decompression", about = "Simple file decompression supporting Zstd, LZ4 or Brotli"]]
pub struct DecompressionArgs {
    input: PathBuf,
    #[arg(short = 'r', long)]
    recursive: bool,
    #[arg(short, long)]
    algorithm: Option<Algorithm>,
    #[arg(short, long)]
    output: Option<PathBuf>,
}

#[derive(Clone, Copy, ValueEnum, Debug)]
pub enum Algorithm {
    Zstd,
    Lz4,
    Brotli,
    Snappy,
}

impl Algorithm {
    const fn extension(self) -> &'static str {
        match self {
            Algorithm::Zstd => "zst",
            Algorithm::Lz4 => "lz4",
            Algorithm::Brotli => "br",
            Algorithm::Snappy => "sz",
        }
    }
}

pub fn compress(a: CompressionArgs) -> Result<()> {
    if a.input.is_file() {
        let ext = a.input.extension().unwrap().to_str().unwrap();
        let output_path = a.output.unwrap_or_else(|| {
            let stem = a.input.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "output".to_string());
            PathBuf::from(format!("{}.{}.{}", stem, ext, &a.algorithm.extension()))
        });
        let mut input_file = File::open(&a.input)?;
        let output_file = File::create(&output_path)?;

        match a.algorithm {
            Algorithm::Zstd => {
                println!("Compressing: {} -> {} with {}@{}", &a.input.display(), &output_path.display(), "ZSTD", a.compression_level);
                compress_zstd(&input_file, &output_file, a.compression_level as i32, a.threads.unwrap_or(1))
            }
            Algorithm::Lz4 => {
                println!("Compressing: {} -> {} with {}", &a.input.display(), &output_path.display(), "LZ4");
                compress_lz4(&mut input_file, &output_file)
            }
            Algorithm::Brotli => {
                println!("Compressing: {} -> {} with {}@{}", &a.input.display(), &output_path.display(), "Brotli", a.compression_level);
                compress_brotli(&input_file, &output_file, a.compression_level)
            }
            Algorithm::Snappy => {
                println!("Compressing: {} -> {} with {}", &a.input.display(), &output_path.display(), "Snappy");
                compress_snappy(&mut input_file, &output_file)
            }
        }
    } else if a.input.is_dir() {
        if !a.recursive { bail!("'{}' is a directory. Use -r/--recursive.", a.input.display()); }
        let output_root = a.output.clone();
        if let Some(dir) = &output_root {fs::create_dir_all(dir)?;}

        for entry in WalkDir::new(&a.input).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() { continue }
            let input_path = entry.path();

            let relative = input_path.strip_prefix(&a.input)?;
            let relative_parent = relative.parent().unwrap_or_else(|| Path::new(""));

            let output_dir = if let Some(root) = &output_root {
                let d = root.join(relative_parent);
                fs::create_dir_all(&d)?;
                d
            } else {
                input_path.parent().unwrap().to_path_buf()
            };

            // Add extension
            let new_name = format!("{}.{}", input_path.file_name().unwrap().to_string_lossy(), a.algorithm.extension());
            let output_path = output_dir.join(new_name);

            let mut input_file = File::open(input_path)?;
            let output_file = File::create(&output_path)?;

            match a.algorithm {
                Algorithm::Zstd => {
                    println!("Compressing: {} -> {} with {}@{}", &input_path.display(), &output_path.display(), "ZSTD", a.compression_level);
                    compress_zstd(&input_file, &output_file, a.compression_level as i32, a.threads.unwrap_or(1))?
                }
                Algorithm::Lz4 => {
                    println!("Compressing: {} -> {} with {}", &input_path.display(), &output_path.display(), "LZ4");
                    compress_lz4(&mut input_file, &output_file)?
                }
                Algorithm::Brotli => {
                    println!("Compressing: {} -> {} with {}@{}", &input_path.display(), &output_path.display(), "Brotli", a.compression_level);
                    compress_brotli(&input_file, &output_file, a.compression_level)?
                }
                Algorithm::Snappy => {
                    println!("Compressing: {} -> {} with {}", &input_path.display(), &output_path.display(), "Snappy");
                    compress_snappy(&mut input_file, &output_file)?
                }
            }
        }
        Ok(())
    } else {
        bail!("Cannot find: {:?}", a.input);
    }
}

pub fn decompress(a: DecompressionArgs) -> Result<()> {
    if a.input.is_file() {
        let ext = a.input.extension().and_then(|e| e.to_str()).unwrap_or("");

        let algorithm = if let Some(alg) = a.algorithm {
            alg
        } else if let Some(alg) = sniff_magic(&a.input)? {
            alg
        } else if let Some(alg) = check_extension(ext) {
            alg
        } else {
            bail!("cannot identify compression algorithm")
        };

        let file_name = a.input.file_name().unwrap().to_string_lossy();
        let stripped = strip_suffix(&file_name, algorithm);
        let default_name = if stripped == file_name { format!("{}.out", stripped) } else { stripped };
        let output_path = a.output.unwrap_or_else(|| {
            a.input.parent().unwrap_or(Path::new("")).join(default_name)
        });

        let input_file = File::open(&a.input)?;
        let mut output_file = File::create(&output_path)?;

        match algorithm {
            Algorithm::Zstd => {
                println!("Decompressing: {} -> {} with {}", &a.input.display(), &output_path.display(), "ZSTD");
                decompress_zstd(&input_file, &mut output_file)
            },
            Algorithm::Lz4 => {
                println!("Decompressing: {} -> {} with {}", &a.input.display(), &output_path.display(), "LZ4");
                decompress_lz4(&input_file, &mut output_file)
            },
            Algorithm::Brotli => {
                println!("Decompressing: {} -> {} with {}", &a.input.display(), &output_path.display(), "Brotli");
                decompress_brotli(&input_file, &mut output_file)
            },
            Algorithm::Snappy => {
                println!("Decompressing: {} -> {} with {}", &a.input.display(), &output_path.display(), "Snappy");
                decompress_snappy(&input_file, &mut output_file)
            },
        }
    } else if a.input.is_dir() {
        if !a.recursive { bail!("'{}' is a directory. Use -r/--recursive.", a.input.display()); }
        let output_root = a.output.clone();
        if let Some(dir) = &output_root { std::fs::create_dir_all(dir)?; }

        for entry in walkdir::WalkDir::new(&a.input).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() { continue; }
            let input_path = entry.path();

            let per_file_alg = if let Some(alg) = a.algorithm {
                Some(alg)
            } else if let Ok(Some(alg)) = sniff_magic(input_path) {
                Some(alg)
            } else {
                input_path.extension()
                    .and_then(|e| e.to_str())
                    .and_then(check_extension)
            };
            let Some(alg) = per_file_alg else { continue };

            let relative = input_path.strip_prefix(&a.input).unwrap();
            let relative_parent = relative.parent().unwrap_or(Path::new(""));
            let output_dir = if let Some(root) = &output_root {
                let d = root.join(relative_parent);
                std::fs::create_dir_all(&d)?; d
            } else {
                input_path.parent().unwrap().to_path_buf()
            };

            let in_name = input_path.file_name().unwrap().to_string_lossy();
            let stripped = strip_suffix(&in_name, alg);
            let out_name = if stripped == in_name { format!("{}.out", stripped) } else { stripped };
            let output_path = output_dir.join(out_name);

            let input_file = File::open(input_path)?;
            let mut output_file = File::create(&output_path)?;

            match alg {
                Algorithm::Zstd => {
                    println!("Decompressing: {} -> {} with ZSTD", &input_path.display(), &output_path.display());
                    decompress_zstd(&input_file, &mut output_file)?
                }
                Algorithm::Lz4 => {
                    println!("Decompressing: {} -> {} with LZ4", &input_path.display(), &output_path.display());
                    decompress_lz4(&input_file, &mut output_file)?
                }
                Algorithm::Brotli => {
                    println!("Decompressing: {} -> {} with Brotli", &input_path.display(), &output_path.display());
                    decompress_brotli(&input_file, &mut output_file)?
                }
                Algorithm::Snappy => {
                    println!("Decompressing: {} -> {} with Snappy", &input_path.display(), &output_path.display());
                    decompress_snappy(&input_file, &mut output_file)?
                }
            }
        }
        Ok(())
    } else {
        bail!("Cannot find: {:?}", a.input);
    }
}

fn strip_suffix(name: &str, alg: Algorithm) -> String {
    let suffix = format!(".{}", alg.extension());
    if let Some(stripped) = name.strip_suffix(&suffix) {
        stripped.to_string()
    } else {
        name.to_string()
    }
}

fn check_extension(ext: &str) -> Option<Algorithm> {
    match ext {
        "zst" => Some(Algorithm::Zstd),
        "lz4" => Some(Algorithm::Lz4),
        "br" => Some(Algorithm::Brotli),
        "sz" => Some(Algorithm::Snappy),
        _ => None,
    }
}

fn sniff_magic(path: &Path) -> Result<Option<Algorithm>> {
    let mut file = File::open(path)?;
    let mut buffer = [0u8; 4];
    let n = file.read(&mut buffer)?;
    if n < 4 { return Ok(None); }

    // Zstd Magic: 28 B5 2F FD
    if buffer == [0x28, 0xB5, 0x2F, 0xFD] {
        return Ok(Some(Algorithm::Zstd));
    }

    // LZ4 Magic: 04 22 4D 18
    if buffer == [0x04, 0x22, 0x4D, 0x18] {
        return Ok(Some(Algorithm::Lz4));
    }

    // Snappy Magic: 73 4E 61 50 70 59 (only first 4 bytes used)
    if buffer == [0x73, 0x4E, 0x61, 0x50] {
        return Ok(Some(Algorithm::Snappy));
    }

    Ok(None)
}

fn compress_zstd(input: &File, output: &File, comp_level: i32, threads: u32) -> Result<()> {
    let mut reader = io::BufReader::new(input);
    let mut writer = io::BufWriter::new(output);

    let mut encoder = zstd::stream::write::Encoder::new(&mut writer, comp_level)?;

    encoder.multithread(threads)?;

    let mut buffer = vec![0u8; zstd::stream::write::Encoder::<io::BufWriter<File>>::recommended_input_size()];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 { break }
        encoder.write_all(&buffer[..n])?;
    }

    let _result = encoder.finish()?;
    Ok(())
}

fn decompress_zstd(input: &File, output: &File) -> Result<()> {
    let mut reader = io::BufReader::new(input);
    let mut writer = io::BufWriter::new(output);

    let mut decoder = zstd::stream::read::Decoder::new(&mut reader)?;

    io::copy(&mut decoder, &mut writer)?;
    writer.flush()?;
    Ok(())
}

fn compress_lz4(input: &mut File, output: &File) -> Result<()> {
    let mut encoder = lz4_flex::frame::FrameEncoder::new(output);

    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 { break }
        encoder.write_all(&buffer[..n])?;
    }
    encoder.finish()?.flush()?;
    Ok(())
}

fn decompress_lz4(input: &File, mut output: &mut File) -> Result<()> {
    let mut decoder = lz4_flex::frame::FrameDecoder::new(input);
    std::io::copy(&mut decoder, &mut output)?;
    Ok(())
}

fn compress_brotli(input: &File, output: &File, comp_level: u32) -> Result<()> {
    let mut reader = io::BufReader::new(input);
    let writer = io::BufWriter::new(output);

    let mut params = brotli2::CompressParams::new();
    params.quality(comp_level).lgwin(22);

    let mut encoder = brotli2::write::BrotliEncoder::from_params(writer, &params);

    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 { break }
        encoder.write_all(&buffer[..n])?;
    }
    encoder.finish()?;
    Ok(())
}

fn decompress_brotli(input: &File, output: &File) -> Result<()> {
    let mut reader = io::BufReader::new(input);
    let mut writer = io::BufWriter::new(output);

    let mut decoder = brotli2::write::BrotliDecoder::new(&mut writer);

    std::io::copy(&mut reader, &mut decoder)?;
    decoder.flush()?;
    Ok(())
}

fn compress_snappy(input: &mut File, output: &File) -> Result<()> {
    let mut encoder = snap::write::FrameEncoder::new(output);

    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 { break }
        encoder.write_all(&buffer[..n])?;
    }
    encoder.flush()?;
    Ok(())
}

fn decompress_snappy(input: &File, mut output: &mut File) -> Result<()> {
    let mut decoder = snap::read::FrameDecoder::new(input);
    std::io::copy(&mut decoder, &mut output)?;
    Ok(())
}