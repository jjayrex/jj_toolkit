mod hash;

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
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Hash(c) => hash::run(c),
    }
}
