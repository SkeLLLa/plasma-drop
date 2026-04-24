use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "plasma-drop",
    version,
    about = "KDE Plasma 6 dropdown app launcher"
)]
pub struct Cli {
    #[arg(short = 'c', long = "config")]
    pub config: Option<PathBuf>,

    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Init {
        #[arg(long)]
        systemd: bool,

        #[arg(long)]
        force: bool,
    },
    PrintExampleConfig,
    PrintSystemdService,
}
