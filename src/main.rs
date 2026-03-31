mod cache;
mod cli;
mod commands;
mod doctl;

use clap::Parser;
use cli::Cli;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    cache::init();
    let cli = Cli::parse();
    commands::ctx(cli.context).await
}
