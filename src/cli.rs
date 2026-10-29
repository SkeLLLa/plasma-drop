use clap::{Parser, Subcommand};
use std::ffi::OsString;
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

    #[arg(
        long = "log-level",
        value_name = "LEVEL",
        help = "Override log level (error, warn, info, debug, trace)"
    )]
    pub log_level: Option<String>,

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
    /// Runs a command in a new systemd user scope; the daemon starts apps through it.
    #[command(hide = true)]
    LaunchScoped {
        #[arg(long)]
        unit: String,
        #[arg(long)]
        description: String,
        #[arg(last = true, required = true)]
        command: Vec<OsString>,
    },
}
