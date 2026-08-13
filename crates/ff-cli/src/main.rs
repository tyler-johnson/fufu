mod cli;
mod cmd;
mod render;

use clap::Parser;

fn main() {
    let args = cli::Cli::parse();
    let result = match args.command {
        cli::Command::Status { json } => cmd::status::run(json),
        cli::Command::Log { json, count } => cmd::log::run(json, count),
    };
    if let Err(err) = result {
        eprintln!("ff: {err}");
        std::process::exit(1);
    }
    // Exit without unwinding: skips munmap/thread teardown of the repo, which
    // is pure latency. Stdout is flushed explicitly first.
    use std::io::Write;
    let _ = std::io::stdout().flush();
    std::process::exit(0);
}
