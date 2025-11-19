use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use serde_json::Value;
use std::{fs, path::PathBuf};
use std::fmt::Debug;

#[derive(Args)]
#[command[name = "format", about = "Simple format converter for JSON, BSON and BINCODE"]]
pub struct FormatArgs {
    input: PathBuf,
    /// Target format: JSON, BSON or BINCODE
    #[arg(short = 'f', long, value_enum, default_value_t = Format::Bson)]
    format: Format,
    #[arg(short = 'o', long)]
    output: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
pub enum Format {
    Json,
    Bson,
    Bincode,
}

impl Format {
    fn name(self) -> &'static str {
        match self {
            Format::Json => "JSON",
            Format::Bson => "BSON",
            Format::Bincode => "BINCODE",
        }
    }

    fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "json" => Some(Format::Json),
            "bson" => Some(Format::Bson),
            "bin" | "bincode" => Some(Format::Bincode),
            _ => None,
        }
    }

    fn default_extension(self) -> &'static str {
        match self {
            Format::Json => "json",
            Format::Bson => "bson",
            Format::Bincode => "bin",
        }
    }
}

pub fn format_convert(a: FormatArgs) -> Result<()> {
    let input_path = a.input;

    if !input_path.is_file() {
        bail!("Input path {:?} is not a file", input_path);
    }

    let input_format = input_path
        .extension()
        .and_then(|e| e.to_str())
        .and_then(Format::from_extension)
        .context("Could not detect input format from file extension. Use .json, .bson or .bin")?;

    let target_format = a.format;

    // Read file as bytes
    let data = fs::read(&input_path)
        .with_context(|| format!("Failed to read input file {:?}", input_path))?;

    // Parse input
    let value = read_as_value(&data, input_format)
        .with_context(|| format!("Failed to deserialize input as {:?}", input_format.name()))?;

    // Serialize to target format
    let out_bytes = write_from_value(&value, target_format)
        .with_context(|| format!("Failed to serialize to {:?}", target_format.name()))?;

    // Output
    let output_path = a.output.unwrap_or_else(|| {
        let mut p = input_path.clone();
        p.set_extension(target_format.default_extension());
        p
    });

    fs::write(&output_path, &out_bytes)
        .with_context(|| format!("Failed to write output file {:?}", output_path))?;

    println!(
        "Converted {:?} ({:?}) -> {:?} ({:?})",
        input_path, input_format.name(), output_path, target_format.name()
    );

    Ok(())
}

fn read_as_value(bytes: &[u8], format: Format) -> Result<Value> {
    match format {
        Format::Json => {
            let v: Value = serde_json::from_slice(bytes)?;
            Ok(v)
        }
        Format::Bson => {
            let v: Value = bson::de::deserialize_from_slice(bytes)?;
            Ok(v)
        }
        Format::Bincode => {
            let v: Value = bincode::serde::decode_from_slice(bytes, bincode::config::standard())?.0;
            Ok(v)
        }
    }
}

fn write_from_value(value: &Value, format: Format) -> Result<Vec<u8>> {
    match format {
        Format::Json => {
            let bytes = serde_json::to_vec_pretty(value)?;
            Ok(bytes)
        }
        Format::Bson => {
            let bytes = bson::ser::serialize_to_vec(value)?;
            Ok(bytes)
        }
        Format::Bincode => {
            let bytes = bincode::serde::encode_to_vec(value, bincode::config::standard())?;
            Ok(bytes)
        }
    }
}
