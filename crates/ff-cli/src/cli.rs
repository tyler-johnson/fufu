use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "ff",
    version,
    about = "a friendlier interface to plain git",
    disable_help_subcommand = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Show the working tree status
    Status {
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
    },
    /// Show the commit history from HEAD
    Log {
        /// Emit machine-readable JSON
        #[arg(long)]
        json: bool,
        /// Number of commits to show; 0 means unlimited
        #[arg(short = 'n', long = "max-count", default_value_t = 25)]
        count: usize,
    },
}
