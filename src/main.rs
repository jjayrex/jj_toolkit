mod hash;
mod image;
mod crypt;
mod compression;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(author, version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Hash(hash::HashArgs),
    HashVerify(hash::HashVerifyArgs),
    Encrypt(crypt::EncryptArgs),
    Decrypt(crypt::DecryptArgs),
    Compress(compression::CompressionArgs),
    Decompress(compression::DecompressionArgs),
    ImageConvert(image::ConvertArgs),
    ImageScale(image::ScaleArgs),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Hash(a) => hash::hash(a),
        Commands::HashVerify(a) => hash::hash_verify(a),
        Commands::Encrypt(a) => crypt::encrypt(a),
        Commands::Decrypt(a) => crypt::decrypt(a),
        Commands::Compress(a) => compression::compress(a),
        Commands::Decompress(a) => compression::decompress(a),
        Commands::ImageConvert(a) => image::convert(a),
        Commands::ImageScale(a) => image::scale(a),
    }
}
