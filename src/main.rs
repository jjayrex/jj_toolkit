mod hash;
mod image;

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
    #[command(subcommand)]
    Hash(hash::HashCmd),
    #[command(subcommand)]
    Image(image::ImageCmd),
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Hash(c) => hash::run(c),
        Commands::Image(c) => image::run(c),
    }
}
