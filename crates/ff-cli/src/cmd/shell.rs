//! `ff shell` — the alias, and only the alias (`alias git='ff git'`).
//! Marked-line rc editing: install appends one line tagged with the fufu
//! marker; uninstall removes exactly the marked lines. A hand-written alias
//! is detected, respected, and never touched. All paths are env-resolved
//! (HOME, ZDOTDIR, XDG_CONFIG_HOME, SHELL) so tests stay hermetic.

use std::path::PathBuf;

use ff_core::{Error, Result};

use crate::cli::ShellAction;

const MARKER: &str = "# fufu — added by `ff shell install`";
const SHELLS: [&str; 3] = ["bash", "zsh", "fish"];

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn home() -> Result<PathBuf> {
    env_path("HOME").ok_or_else(|| Error::msg("HOME is not set"))
}

fn rc_file(shell: &str) -> Result<PathBuf> {
    Ok(match shell {
        "bash" => home()?.join(".bashrc"),
        "zsh" => match env_path("ZDOTDIR") {
            Some(zdot) => zdot.join(".zshrc"),
            None => home()?.join(".zshrc"),
        },
        "fish" => {
            let config = match env_path("XDG_CONFIG_HOME") {
                Some(xdg) => xdg,
                None => home()?.join(".config"),
            };
            config.join("fish/config.fish")
        }
        other => {
            return Err(Error::msg(format!(
                "unsupported shell {other:?} (supported: bash, zsh, fish)"
            )));
        }
    })
}

fn alias_line(shell: &str) -> &'static str {
    // fish aliases take a space-separated body; bash/zsh take `=`.
    if shell == "fish" {
        "alias git 'ff git'"
    } else {
        "alias git='ff git'"
    }
}

fn default_shell() -> Result<String> {
    let shell = std::env::var("SHELL")
        .map_err(|_| Error::msg("SHELL is not set; pass a shell name (bash, zsh, fish)"))?;
    let name = std::path::Path::new(&shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    if name.is_empty() {
        return Err(Error::msg("could not determine the shell from $SHELL"));
    }
    Ok(name)
}

#[derive(Debug, PartialEq)]
enum AliasState {
    Installed,
    HandWritten,
    Absent,
}

fn state_of(contents: &str) -> AliasState {
    for line in contents.lines() {
        if line.contains(MARKER) {
            return AliasState::Installed;
        }
    }
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("alias git") && trimmed.contains("ff git") {
            return AliasState::HandWritten;
        }
    }
    AliasState::Absent
}

pub fn run(action: ShellAction) -> Result<()> {
    match action {
        ShellAction::Install { shell } => install(&resolve_shell(shell)?),
        ShellAction::Uninstall { shell } => uninstall(&resolve_shell(shell)?),
        ShellAction::List => list(),
    }
}

fn resolve_shell(arg: Option<String>) -> Result<String> {
    let shell = match arg {
        Some(shell) => shell,
        None => default_shell()?,
    };
    if !SHELLS.contains(&shell.as_str()) {
        return Err(Error::msg(format!(
            "unsupported shell {shell:?} (supported: bash, zsh, fish)"
        )));
    }
    Ok(shell)
}

fn install(shell: &str) -> Result<()> {
    let rc = rc_file(shell)?;
    let contents = std::fs::read_to_string(&rc).unwrap_or_default();
    match state_of(&contents) {
        AliasState::Installed => {
            println!("{shell}: already installed in {}", rc.display());
            return Ok(());
        }
        AliasState::HandWritten => {
            println!(
                "{shell}: {} already aliases git to ff by hand — leaving it alone",
                rc.display()
            );
            return Ok(());
        }
        AliasState::Absent => {}
    }
    if let Some(parent) = rc.parent() {
        std::fs::create_dir_all(parent).map_err(Error::repo)?;
    }
    let mut updated = contents;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&format!("{}  {MARKER}\n", alias_line(shell)));
    std::fs::write(&rc, updated).map_err(Error::repo)?;
    println!("{shell}: added `{}` to {}", alias_line(shell), rc.display());
    println!("restart the shell (or source the file) to activate it");
    Ok(())
}

fn uninstall(shell: &str) -> Result<()> {
    let rc = rc_file(shell)?;
    let Ok(contents) = std::fs::read_to_string(&rc) else {
        println!("{shell}: nothing installed ({} not found)", rc.display());
        return Ok(());
    };
    match state_of(&contents) {
        AliasState::Installed => {}
        AliasState::HandWritten => {
            println!(
                "{shell}: the alias in {} was written by hand — not touching it",
                rc.display()
            );
            return Ok(());
        }
        AliasState::Absent => {
            println!("{shell}: nothing installed in {}", rc.display());
            return Ok(());
        }
    }
    let kept: Vec<&str> = contents
        .lines()
        .filter(|line| !line.contains(MARKER))
        .collect();
    let mut updated = kept.join("\n");
    if contents.ends_with('\n') && !updated.is_empty() {
        updated.push('\n');
    }
    std::fs::write(&rc, updated).map_err(Error::repo)?;
    println!("{shell}: removed the fufu alias from {}", rc.display());
    Ok(())
}

fn list() -> Result<()> {
    for shell in SHELLS {
        let rc = rc_file(shell)?;
        let contents = std::fs::read_to_string(&rc).unwrap_or_default();
        let state = match state_of(&contents) {
            AliasState::Installed => "installed",
            AliasState::HandWritten => "hand-written alias (not fufu-managed)",
            AliasState::Absent => "not installed",
        };
        println!("{shell:<5} {state}  ({})", rc.display());
    }
    Ok(())
}
