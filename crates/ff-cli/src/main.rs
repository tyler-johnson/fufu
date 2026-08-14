mod autotrim;
mod cadence;
mod capture;
mod cli;
mod cmd;
mod help;
mod pager;
mod provenance;
mod render;

mod selfupdate;

use clap::Parser;

fn main() {
    // A Ctrl-C between ref lock and ref commit would leave a stale `.lock`
    // behind and silently turn every later capture into Contended. gix's
    // interrupt handler cleans up tempfiles (which back ref locks) on
    // signals; the engine itself never installs handlers.
    let _interrupt = unsafe { ff_core::gix::interrupt::init_handler(1, || {}) };

    let args = cli::Cli::parse();
    let result = match args.command {
        None => cmd::snap::run(args.message, args.json),
        Some(cli::Command::Status { json }) => cmd::status::run(json),
        Some(cli::Command::Log {
            json,
            count,
            commits,
            ops,
        }) => cmd::log::run(json, count, commits, ops),
        Some(cli::Command::Evolog { json, count }) => cmd::evolog::run(json, count),
        Some(cli::Command::Git { args }) => cmd::git::run(args),
        Some(cli::Command::Restore {
            at,
            all,
            paths,
            json,
        }) => cmd::restore::run(at, all, paths, json),
        Some(cli::Command::Trim {
            dry_run,
            gone,
            json,
        }) => cmd::trim::run(dry_run, gone, json),
        Some(cli::Command::Commit {
            message,
            no_verify,
            branch,
            json,
        }) => cmd::commit::run(message, no_verify, branch, json),
        Some(cli::Command::Switch { target, json }) => cmd::switch::run(target, json),
        Some(cli::Command::Undo { op, force, json }) => cmd::undo::run(op, force, json),
        Some(cli::Command::Branch { name, delete, json }) => cmd::branch::run(name, delete, json),
        Some(cli::Command::New {
            target,
            message,
            branch,
            json,
        }) => cmd::new::run(target, message, branch, json),
        Some(cli::Command::Describe {
            message,
            branch,
            json,
        }) => cmd::describe::run(message, branch, json),
        Some(cli::Command::Hook { kind }) => cmd::hook::run(kind),
        Some(cli::Command::Config {
            key,
            value,
            unset,
            global,
            json,
        }) => cmd::config::run(key, value, unset, global, json),
        Some(cli::Command::Doctor { fix, json }) => cmd::doctor::run(fix, json),
        Some(cli::Command::Update { check }) => cmd::update::run(check),
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
