use clap::Parser;

#[derive(Parser)]
#[command(name = "kuber")]
#[command(about = "Ephemeral kubeconfig manager - wraps kubie with on-demand DigitalOcean configs")]
#[command(version)]
pub struct Cli {
    /// Context name to switch to directly (skip interactive selection)
    pub context: Option<String>,
}
