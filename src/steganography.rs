use std::fs;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use clap::Args;
use image::{ImageBuffer, Rgba};

#[derive(Args)]
#[command[name = "stegano-embed", about = "Embed data into a PNG or BMP image using LSB steganography"]]
pub struct EmbedArgs {
    /// Input image path
    input: PathBuf,
    /// Output image path
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Message to embed
    #[arg(short, long, conflicts_with = "file")]
    message: Option<String>,
    /// File to embed
    #[arg(short, long, conflicts_with = "message")]
    file: Option<PathBuf>,
}

#[derive(Args)]
#[command[name = "stegano-extract", about = "Extract data embedded in a PNG or BMP image using LSB steganography"]]
pub struct ExtractArgs {
    /// Input image path
    input: PathBuf,
    /// Optional output file. If omitted, prints as UTF-8 text.
    #[arg(short, long)]
    output: Option<PathBuf>,
}

pub fn embed(a: EmbedArgs) -> Result<()> {
    // Load image
    let img =
        image::open(&a.input).with_context(|| format!("failed to load image {:?}", a.input))?;
    let mut img = img.to_rgba8();

    // Get payload bytes
    let payload: Vec<u8> = if let Some(msg) = a.message {
        msg.into_bytes()
    } else if let Some(path) = a.file {
        fs::read(&path).with_context(|| format!("failed to read file {:?}", path))?
    } else {
        return Err(anyhow!("You must provide either --message or --file"));
    };

    // Build bitstream
    if payload.len() > u32::MAX as usize {
        return Err(anyhow!("Payload too large"));
    }

    let mut data = Vec::with_capacity(4 + payload.len());
    let len = payload.len() as u32;
    data.extend_from_slice(&len.to_be_bytes());
    data.extend_from_slice(&payload);

    embed_data(&mut img, &data).with_context(|| "failed to embed data into the image")?;

    // Save image
    if let Some(path) = &a.output {
        img.save(path)
            .with_context(|| format!("failed to save image to {:?}", path))?;
    } else {
        let mut out = a.input.clone();
        let mut name = a.input.file_stem().unwrap().to_str().unwrap().to_string();
        name += "_embedded";

        out.set_file_name(name);
        out.set_extension(a.input.extension().unwrap());
        img.save(&out)
            .with_context(|| format!("failed to save image to {:?}", out))?;
    }

    Ok(())
}

pub fn extract(a: ExtractArgs) -> Result<()> {
    // Load image
    let img =
        image::open(&a.input).with_context(|| format!("failed to load image {:?}", a.input))?;
    let img = img.to_rgba8();

    let extracted = extract_data(&img).with_context(|| "failed to extract data")?;

    if let Some(path) = a.output {
        let mut f =
            fs::File::create(&path).with_context(|| format!("failed to create file {:?}", path))?;
        f.write_all(&extracted)
            .with_context(|| format!("failed to write to file {:?}", path))?;
        println!("Extracted {} bytes to {:?}", extracted.len(), path);
    } else {
        // Try to parse as UTF-8; else show length
        match String::from_utf8(extracted.clone()) {
            Ok(s) => println!("{s}"),
            Err(_) => println!(
                "Extracted {} bytes. Use --output to save to a file.",
                extracted.len()
            ),
        }
    }

    Ok(())
}

/// Embed data bytes into the image using 1 bit per channel LSB.
fn embed_data(img: &mut ImageBuffer<Rgba<u8>, Vec<u8>>, data: &[u8]) -> Result<()> {
    let buffer = img.as_mut();

    let capacity_bits = buffer.len();
    let required_bits = data.len() * 8;

    if required_bits > capacity_bits {
        return Err(anyhow!("Embedded data too large, data's {required_bits} bits, need to be < {capacity_bits} bits"));
    }

    let mut bit_idx = 0usize;

    for &byte in data {
        for bit_pos in (0..8).rev() {
            let bit = (byte >> bit_pos) & 1;
            let idx = bit_idx;
            let org = buffer[idx];
            // Set LSB to `bit`
            let new = (org & 0xFE) | bit;
            buffer[idx] = new;

            bit_idx += 1;
        }
    }

    Ok(())
}

fn extract_data(img: &ImageBuffer<Rgba<u8>, Vec<u8>>) -> Result<Vec<u8>> {
    let buffer = img.as_raw();

    let capacity_bits = buffer.len();
    if capacity_bits < 32 {
        return Err(anyhow!("Image too small to contain length prefix"));
    }

    let mut bit_idx = 0usize;

    // Read length
    let mut len_bytes = [0u8; 4];
    for byte in &mut len_bytes {
        let mut val = 0u8;
        for _ in 0..8 {
            let idx = bit_idx;
            let bit = buffer[idx] & 1;
            val = (val << 1) | bit;
            bit_idx += 1;
        }
        *byte = val;
    }
    let payload_len = u32::from_be_bytes(len_bytes) as usize;

    let required_bits = 32 + payload_len * 8;
    if required_bits > capacity_bits {
        return Err(anyhow!("Encoded length ({payload_len} bytes) exceeds image capacity"));
    }

    let mut out = Vec::with_capacity(payload_len);
    for _ in 0..payload_len {
        let mut val = 0u8;
        for _ in 0..8 {
            let idx = bit_idx;
            let bit = buffer[idx] & 1;
            val = (val << 1) | bit;
            bit_idx += 1;
        }
        out.push(val);
    }

    Ok(out)
}