mod hash;
mod image;
mod crypt;
mod compression;
mod keygen;
mod format;
mod steganography;
mod raster;

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
    Keygen(keygen::KeygenArgs),
    Format(format::FormatArgs),
    ImageConvert(image::ConvertArgs),
    ImageScale(image::ScaleArgs),
    ImageGetcolor(image::GetColorArgs),
    SteganoEmbed(steganography::EmbedArgs),
    SteganoExtract(steganography::ExtractArgs),
    Rasterize(raster::RasterizeArgs)
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
        Commands::Keygen(a) => keygen::generate_key(a),
        Commands::Format(a) => format::format_convert(a),
        Commands::ImageConvert(a) => image::convert(a),
        Commands::ImageScale(a) => image::scale(a),
        Commands::ImageGetcolor(a) => image::get_color(a),
        Commands::SteganoEmbed(a) => steganography::embed(a),
        Commands::SteganoExtract(a) => steganography::extract(a),
        Commands::Rasterize(a) => raster::rasterize(a),
    }
}
