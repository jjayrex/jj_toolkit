use anyhow::{Result, bail};
use std::path::PathBuf;
use std::fs::File;
use std::io;
use std::io::{Read, Write};
use clap::{Args, ValueEnum};

#[derive(Args)]
pub struct CompressionArgs {
    input: PathBuf,
    // #[arg(short = 'r', long)]
    // recursive: bool,
    #[arg(short, long, default_value_t = Algorithm::Zstd)]
    algorithm: Algorithm,
    #[arg(short, long, default_value = "5")]
    compression_level: u32,
    #[arg(short, long)]
    output: Option<PathBuf>,
    #[arg(short = 't', long)]
    threads: Option<u32>,
}

#[derive(Args)]
pub struct DecompressionArgs {
    input: PathBuf,
    // #[arg(short = 'r', long)]
    // recursive: bool,
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
}

impl std::fmt::Display for Algorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Algorithm::Zstd => "zstd",
            Algorithm::Lz4 => "lz4",
            Algorithm::Brotli => "brotli",
        })
    }
}

impl Algorithm {
    const fn extension(self) -> &'static str {
        match self {
            Algorithm::Zstd => "zst",
            Algorithm::Lz4 => "lz4",
            Algorithm::Brotli => "br",
        }
    }
}

pub fn compress(a: CompressionArgs) -> Result<()> {
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
            if a.threads.is_some() {
                compress_zstd(&input_file, &output_file, a.compression_level as i32, a.threads.unwrap())
            } else {
                compress_zstd(&input_file, &output_file, a.compression_level as i32, 1)
            }
        },
        Algorithm::Lz4 => {
            println!("Compressing: {} -> {} with {}", &a.input.display(), &output_path.display(), "LZ4");
            compress_lz4(&mut input_file, &output_file)
        },
        Algorithm::Brotli => {
            println!("Compressing: {} -> {} with {}@{}", &a.input.display(), &output_path.display(), "Brotli", a.compression_level);
            compress_brotli(&mut input_file, &output_file, a.compression_level)
        },
    }
}

pub fn decompress(a: DecompressionArgs) -> Result<()> {
    let ext = a.input.extension().unwrap().to_str().unwrap();
    let algorithm = if a.algorithm.is_some() {
        a.algorithm.unwrap()
    } else {
        match ext {
            "zst" => Algorithm::Zstd,
            "lz4" => Algorithm::Lz4,
            "br" => Algorithm::Brotli,
            _ => bail!("cannot identify compression algorithm, please specify --algorithm"),
        }
    };

    let output_path = a.output.unwrap_or_else(|| {
        let stem = a.input.file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "output".to_string());
        PathBuf::from(format!("{}", stem))
    });
    output_path.with_extension("");

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
    }
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