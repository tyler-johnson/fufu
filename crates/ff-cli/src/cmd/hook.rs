//! `ff hook` — everything that feeds the capture floor is a hook, under one
//! grammar: `ff hook <agent|shell|editor> <install|uninstall|list|trigger>
//! [name]`. The agent-trigger contract is absolute: it ALWAYS exits 0 (a
//! hook must never veto an agent action), prints nothing except the
//! once-per-session notice, and treats every failure as a silent success
//! (`FF_DEBUG=1` for diagnostics).

use std::io::Read;

use ff_core::{Error, Provenance, Result};
use serde::Deserialize;

use crate::cli::{HookKind, HookVerb};
use crate::ctx::Ctx;

const MAX_PAYLOAD: u64 = 8 * 1024 * 1024;
const HOOK_COMMAND: &str = "ff hook agent trigger claude";
/// Spellings older installs may still carry; recognized (and upgraded by
/// `install`) so capture never silently stops behind a stale settings entry.
const LEGACY_HOOK_COMMANDS: [&str; 1] = ["ff hook claude"];
const PRE_TOOL_MATCHER: &str = "Bash|Edit|Write|NotebookEdit";

/// What the agent is told, once per session, on UserPromptSubmit stdout.
/// Every command here is real and spelled the way the CLI takes it — a
/// retired or mistyped form teaches the agent to fail. Keep it short: this
/// is context the agent pays for on every session.
///
/// Every line is quoted from the CLI, and each source carries a matching
/// `// agent notice quotes this` marker in cli.rs — retiring a verb or
/// renaming a flag there is an edit here too. Adding a command to this text
/// means adding that marker at its definition, so the trail stays two-way:
/// `grep -rn "agent notice" crates/ff-cli/src`.
const NOTICE: &str = "\
fufu (`ff`) is capturing this repository: the worktree is snapshotted before every \
tool action, so no edit can lose file state. Work directly — no backup copies, no \
hedging.

Use `ff`, not `git`, for anything that writes:
- commit: `ff commit -m \"…\"` — no add, no staging; the worktree is the change
- begin work: `ff start` · move: `ff switch <branch>` (parks and resumes dirty trees)
- undo the last operation, whole repo: `ff undo`
- discard edits to a file: `ff restore <path>`
- go back in time: `ff restore --all --at 2h`, or `ff op log` for ids then \
`ff restore <path> --at-op <id>`
- where you can go back to, one row per undo step: `ff history`
- fold a fix into an earlier commit: `ff absorb --into <rev>`
- take in base and remote: `ff sync` · send it out: `ff publish`
- anything else git does: `ff git <args…>` — snapshots, then runs git verbatim

Reading with git is fine. `ff status` and `ff log` say more than their git \
counterparts: the open change, and what the next rebase or push will do.
";

pub fn run(ctx: &Ctx, kind: HookKind) -> Result<()> {
    match kind {
        HookKind::Agent { verb } => agent(ctx, verb),
        HookKind::Shell { verb } => super::shell::run(verb),
        HookKind::Editor { verb } => editor(verb),
        HookKind::Other(args) => {
            // The Phase 1 spelling `ff hook claude` is committed history and
            // may live in a settings file: forward it to the trigger rather
            // than silently dropping the capture.
            if args.first().and_then(|a| a.to_str()) == Some("claude") {
                return agent(
                    ctx,
                    HookVerb::Trigger {
                        name: Some("claude".into()),
                    },
                );
            }
            // Anything else unknown exits 0 silently: whatever invoked us
            // keeps working.
            Ok(())
        }
    }
}

fn agent(ctx: &Ctx, verb: HookVerb) -> Result<()> {
    match verb {
        HookVerb::Trigger { name } => {
            match name.as_deref() {
                // A trigger must never veto, and never speak on failure.
                None | Some("claude") => {
                    if let Err(err) = runtime_claude(ctx)
                        && std::env::var_os("FF_DEBUG").is_some()
                    {
                        eprintln!("ff[debug]: hook failed: {err}");
                    }
                }
                // Unknown agents included: exit 0, say nothing.
                Some(_) => {}
            }
            Ok(())
        }
        HookVerb::Install { name } => {
            require_agent(name.as_deref())?;
            install()
        }
        HookVerb::Uninstall { name } => {
            require_agent(name.as_deref())?;
            uninstall()
        }
        HookVerb::List { name } => {
            require_agent(name.as_deref())?;
            list()
        }
    }
}

/// Installers are for humans: unknown names are real errors there.
fn require_agent(name: Option<&str>) -> Result<()> {
    match name {
        None | Some("claude") => Ok(()),
        Some(other) => Err(Error::msg(format!(
            "unknown agent {other:?} (supported: claude)"
        ))),
    }
}

/// Reserved in the grammar; nothing exists yet (deferred until a real need
/// shows up).
fn editor(verb: HookVerb) -> Result<()> {
    match verb {
        HookVerb::List { .. } => {
            println!("no editor hooks exist yet");
            Ok(())
        }
        _ => Err(Error::msg(
            "no editor hooks exist yet (deferred until a real need shows up)",
        )),
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Payload {
    hook_event_name: String,
    session_id: String,
    cwd: String,
    tool_name: String,
    tool_input: ToolInput,
    prompt: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ToolInput {
    command: String,
    file_path: String,
    notebook_path: String,
}

fn runtime_claude(ctx: &Ctx) -> Result<()> {
    let mut buf = Vec::new();
    std::io::stdin()
        .lock()
        .take(MAX_PAYLOAD + 1)
        .read_to_end(&mut buf)
        .map_err(Error::repo)?;
    if buf.len() as u64 > MAX_PAYLOAD {
        return Err(Error::msg("hook payload exceeds 8MiB"));
    }
    let payload: Payload = serde_json::from_slice(&buf).map_err(Error::repo)?;
    if payload.cwd.is_empty() {
        return Err(Error::msg("hook payload has no cwd"));
    }
    let repo = ff_core::discover(&payload.cwd)?;
    if repo.workdir().is_none() {
        return Ok(());
    }

    let prov = provenance_for(ctx, &payload, &repo);
    // Contended and NoOp are fine; only real errors matter (and only under
    // FF_DEBUG at that).
    ff_core::capture(&repo, &prov)?;

    // The session notice rides UserPromptSubmit stdout, which Claude injects
    // into the agent's context. Marker first — written durably BEFORE the
    // notice prints — so a crash can only under-notify, never spam.
    if payload.hook_event_name == "UserPromptSubmit" {
        let marker = repo.git_dir().join("fufu/claude-session");
        let already = std::fs::read_to_string(&marker).is_ok_and(|prev| prev == payload.session_id);
        if !already {
            if let Some(parent) = marker.parent() {
                std::fs::create_dir_all(parent).map_err(Error::repo)?;
            }
            let tmp = marker.with_extension("tmp");
            {
                use std::io::Write;
                // Sync through the write handle: Windows refuses to flush a
                // handle opened read-only.
                let mut file = std::fs::File::create(&tmp).map_err(Error::repo)?;
                file.write_all(payload.session_id.as_bytes())
                    .map_err(Error::repo)?;
                file.sync_all().map_err(Error::repo)?;
            }
            std::fs::rename(&tmp, &marker).map_err(Error::repo)?;
            print!("{NOTICE}");
        }
    }
    crate::selfupdate::notify::maybe_spawn_check(&repo);
    // The agent hook is often the only trigger a repo has, so it carries the
    // lane too — a daily inline walk is the price of an engine that maintains itself.
    crate::autotrim::maybe_trim(&repo);
    Ok(())
}

fn provenance_for(ctx: &Ctx, payload: &Payload, repo: &ff_core::gix::Repository) -> Provenance {
    let detail = match payload.hook_event_name.as_str() {
        "PreToolUse" => match payload.tool_name.as_str() {
            "Bash" => format!(
                "Bash({})",
                crate::provenance::truncate(&payload.tool_input.command, 64)
            ),
            "Edit" | "Write" => format!(
                "{}({})",
                payload.tool_name,
                rela_path(repo, &payload.tool_input.file_path)
            ),
            "NotebookEdit" => format!(
                "NotebookEdit({})",
                rela_path(repo, &payload.tool_input.notebook_path)
            ),
            // Unknown tools are labeled honestly; the snapshot happens anyway.
            other => format!("tool {other}"),
        },
        "UserPromptSubmit" => {
            format!(
                "prompt \"{}\"",
                crate::provenance::truncate(&payload.prompt, 60)
            )
        }
        other => format!("event {other}"),
    };
    crate::provenance::claude(ctx, &payload.session_id, detail)
}

/// Show tool paths relative to the worktree when possible.
fn rela_path(repo: &ff_core::gix::Repository, path: &str) -> String {
    if let Some(workdir) = repo.workdir()
        && let Ok(rel) = std::path::Path::new(path).strip_prefix(workdir)
    {
        let s = rel.to_string_lossy();
        if !s.is_empty() {
            // Provenance lands in commit subjects: use git's path notation
            // (forward slashes) regardless of the host separator.
            if cfg!(windows) {
                return s.replace('\\', "/");
            }
            return s.into_owned();
        }
    }
    path.to_string()
}

// ---- installer -------------------------------------------------------------

fn settings_path() -> Result<std::path::PathBuf> {
    let home = std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        // Windows has USERPROFILE where unix has HOME.
        .or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty()))
        .ok_or_else(|| Error::msg("HOME is not set"))?;
    Ok(std::path::PathBuf::from(home).join(".claude/settings.json"))
}

/// The two events fufu hooks: (event, matcher).
const EVENTS: [(&str, Option<&str>); 2] = [
    ("PreToolUse", Some(PRE_TOOL_MATCHER)),
    ("UserPromptSubmit", None),
];

fn load_settings(path: &std::path::Path) -> Result<serde_json::Map<String, serde_json::Value>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => String::from("{}"),
        Err(err) => return Err(Error::repo(err)),
    };
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|err| {
        Error::msg(format!(
            "{}: not valid JSON ({err}); file untouched",
            path.display()
        ))
    })?;
    match value {
        serde_json::Value::Object(map) => Ok(map),
        _ => Err(Error::msg(format!(
            "{}: top level is not an object; file untouched",
            path.display()
        ))),
    }
}

fn is_our_command(command: Option<&str>) -> bool {
    command.is_some_and(|cmd| cmd == HOOK_COMMAND || LEGACY_HOOK_COMMANDS.contains(&cmd))
}

fn entry_has_our_command(entry: &serde_json::Value) -> bool {
    entry["hooks"]
        .as_array()
        .is_some_and(|hooks| hooks.iter().any(|h| is_our_command(h["command"].as_str())))
}

pub(crate) enum AgentWiring {
    /// HOME unset, or settings unreadable/invalid — the complaint text.
    Unavailable(String),
    Events {
        path: std::path::PathBuf,
        pre_tool: bool,
        prompt: bool,
    },
}

pub(crate) fn agent_wiring() -> AgentWiring {
    let path = match settings_path() {
        Ok(p) => p,
        Err(err) => return AgentWiring::Unavailable(err.to_string()),
    };
    let settings = match load_settings(&path) {
        Ok(s) => s,
        Err(err) => return AgentWiring::Unavailable(err.to_string()),
    };
    let pre_tool = settings
        .get("hooks")
        .and_then(|h| h.get("PreToolUse"))
        .and_then(|e| e.as_array())
        .is_some_and(|entries| entries.iter().any(entry_has_our_command));
    let prompt = settings
        .get("hooks")
        .and_then(|h| h.get("UserPromptSubmit"))
        .and_then(|e| e.as_array())
        .is_some_and(|entries| entries.iter().any(entry_has_our_command));
    AgentWiring::Events {
        path,
        pre_tool,
        prompt,
    }
}

fn install() -> Result<()> {
    let path = settings_path()?;
    let mut settings = load_settings(&path)?;

    let hooks = settings
        .entry("hooks".to_string())
        .or_insert_with(|| serde_json::Value::Object(Default::default()));
    let hooks = hooks.as_object_mut().ok_or_else(|| {
        Error::msg(format!(
            "{}: \"hooks\" is not an object; file untouched",
            path.display()
        ))
    })?;

    let mut changed = false;
    for (event, matcher) in EVENTS {
        let entries = hooks
            .entry(event.to_string())
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        let entries = entries.as_array_mut().ok_or_else(|| {
            Error::msg(format!(
                "{}: hooks.{event} is not an array; file untouched",
                path.display()
            ))
        })?;
        // Upgrade any legacy spelling in place before the idempotence check.
        for entry in entries.iter_mut() {
            let Some(cmds) = entry.get_mut("hooks").and_then(|h| h.as_array_mut()) else {
                continue;
            };
            for cmd in cmds.iter_mut() {
                if cmd["command"]
                    .as_str()
                    .is_some_and(|c| LEGACY_HOOK_COMMANDS.contains(&c))
                {
                    cmd["command"] = HOOK_COMMAND.into();
                    changed = true;
                }
            }
        }
        if entries.iter().any(entry_has_our_command) {
            continue;
        }
        let mut entry = serde_json::Map::new();
        if let Some(matcher) = matcher {
            entry.insert("matcher".into(), matcher.into());
        }
        entry.insert(
            "hooks".into(),
            serde_json::json!([{ "type": "command", "command": HOOK_COMMAND }]),
        );
        entries.push(serde_json::Value::Object(entry));
        changed = true;
    }

    if changed {
        write_settings(&path, &settings)?;
        println!("installed the fufu hooks into {}", path.display());
    } else {
        println!("fufu hooks already installed in {}", path.display());
    }
    Ok(())
}

fn uninstall() -> Result<()> {
    let path = settings_path()?;
    let mut settings = load_settings(&path)?;
    let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        println!("no fufu hooks installed in {}", path.display());
        return Ok(());
    };

    let mut changed = false;
    for (event, _) in EVENTS {
        let Some(entries) = hooks.get_mut(event).and_then(|e| e.as_array_mut()) else {
            continue;
        };
        for entry in entries.iter_mut() {
            let Some(cmds) = entry.get_mut("hooks").and_then(|h| h.as_array_mut()) else {
                continue;
            };
            let before = cmds.len();
            cmds.retain(|h| !is_our_command(h["command"].as_str()));
            changed |= cmds.len() != before;
        }
        let before = entries.len();
        // Drop entries whose hook list we emptied; foreign entries keep
        // whatever shape they had.
        entries.retain(|entry| {
            entry["hooks"]
                .as_array()
                .is_none_or(|cmds| !cmds.is_empty())
        });
        changed |= entries.len() != before;
        if entries.is_empty() {
            hooks.remove(event);
            changed = true;
        }
    }
    if hooks.is_empty() {
        settings.remove("hooks");
        changed = true;
    }

    if changed {
        write_settings(&path, &settings)?;
        println!("removed the fufu hooks from {}", path.display());
    } else {
        println!("no fufu hooks installed in {}", path.display());
    }
    Ok(())
}

fn list() -> Result<()> {
    match agent_wiring() {
        AgentWiring::Unavailable(msg) => Err(Error::msg(msg)),
        AgentWiring::Events {
            path: _,
            pre_tool,
            prompt,
        } => {
            for (event, _) in EVENTS {
                let installed = match event {
                    "PreToolUse" => pre_tool,
                    "UserPromptSubmit" => prompt,
                    _ => false,
                };
                println!(
                    "{event:<16} {}",
                    if installed {
                        "installed"
                    } else {
                        "not installed"
                    }
                );
            }
            Ok(())
        }
    }
}

fn write_settings(
    path: &std::path::Path,
    settings: &serde_json::Map<String, serde_json::Value>,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(Error::repo)?;
    }
    let mut body = serde_json::to_string_pretty(&serde_json::Value::Object(settings.clone()))
        .map_err(Error::repo)?;
    body.push('\n');
    let tmp = path.with_extension("json.ff-tmp");
    std::fs::write(&tmp, &body).map_err(Error::repo)?;
    std::fs::rename(&tmp, path).map_err(Error::repo)?;
    Ok(())
}

// ---- the notice is a contract with the CLI ---------------------------------

/// `NOTICE` is prose that an agent reads as instructions, so it rots in a way
/// the compiler cannot see: a retired verb or a renamed flag still reads fine
/// and simply teaches the agent to fail. These guards make it fail here
/// instead — clap is the authority on what the notice is allowed to say.
#[cfg(test)]
mod notice {
    use clap::{CommandFactory, Parser};

    use super::NOTICE;
    use crate::cli::Cli;

    /// cli.rs as text, for the marker trail. Read as source rather than
    /// through clap because a comment is exactly what clap discards.
    const CLI_SRC: &str = include_str!("../cli.rs");
    const MARKER: &str = "// agent notice quotes this";

    /// Every `ff …` the notice spells, as argv. Placeholders (`<path>`,
    /// `"…"`) become a value, since the point is the grammar around them.
    fn quoted() -> Vec<Vec<String>> {
        NOTICE
            .split('`')
            // Odd fields are the ones between a pair of backticks.
            .skip(1)
            .step_by(2)
            .filter(|span| *span == "ff" || span.starts_with("ff "))
            .map(|span| {
                span.split_whitespace()
                    .map(|tok| {
                        if tok.starts_with('<') || tok.starts_with('"') {
                            "x".to_string()
                        } else {
                            tok.to_string()
                        }
                    })
                    .collect()
            })
            .collect()
    }

    fn find_arg<'a>(cmd: &'a clap::Command, flag: &str) -> Option<&'a clap::Arg> {
        cmd.get_arguments().find(|arg| {
            if let Some(long) = flag.strip_prefix("--") {
                arg.get_long() == Some(long)
            } else {
                flag.strip_prefix('-')
                    .and_then(|s| s.chars().next())
                    .is_some_and(|c| arg.get_short() == Some(c))
            }
        })
    }

    /// The guard that the two rotted spellings needed. Parsing alone is not
    /// enough: retired surface (`-m`, `--ops`, `ff checkout`) is *declared*,
    /// hidden, so that typing it reaches an answer — it parses and then
    /// refuses. So hidden is disqualifying here, not just unknown.
    #[test]
    fn only_live_documented_surface() {
        let root = Cli::command();
        let commands = quoted();
        assert!(
            commands.len() >= 8,
            "the notice stopped teaching verbs: {commands:?}"
        );
        for tokens in &commands {
            let line = tokens.join(" ");
            let mut cmd = &root;
            let mut rest = &tokens[1..];
            while let Some(sub) = rest.first().and_then(|name| cmd.find_subcommand(name)) {
                assert!(
                    !sub.is_hide_set(),
                    "`{line}` names {:?}, which is hidden — retired or undocumented \
                     surface must not be taught to an agent",
                    sub.get_name()
                );
                cmd = sub;
                rest = &rest[1..];
            }
            for flag in rest.iter().filter(|tok| tok.starts_with('-')) {
                let arg = find_arg(cmd, flag)
                    .or_else(|| find_arg(&root, flag))
                    .unwrap_or_else(|| panic!("`{line}` passes {flag}, which does not exist"));
                assert!(
                    !arg.is_hide_set(),
                    "`{line}` passes {flag}, which is hidden — retired or undocumented \
                     surface must not be taught to an agent"
                );
            }
            Cli::try_parse_from(tokens)
                .unwrap_or_else(|err| panic!("`{line}` does not parse:\n{err}"));
        }
    }

    /// The other half of the trail: a command in the notice has a marker at
    /// its definition, so whoever retires it there sees this text named.
    #[test]
    fn every_quoted_command_is_marked_in_cli_rs() {
        let markers: Vec<&str> = CLI_SRC
            .lines()
            .map(str::trim_start)
            .filter(|line| line.starts_with(MARKER))
            .collect();
        assert!(
            markers.len() >= 8,
            "the marker trail is gone from cli.rs; the notice has nothing pointing at it"
        );
        for tokens in quoted() {
            // Bare `ff` — the notice names the tool before it names a verb.
            let Some(verb) = tokens.get(1) else { continue };
            assert!(
                markers.iter().any(|m| m.contains(&format!("`ff {verb}"))),
                "the notice teaches `ff {verb}` but no `{MARKER}` in cli.rs claims it: \
                 add one at its definition"
            );
        }
    }

    /// The notice is context the agent pays for on every session. There is no
    /// exact token count to assert, so the budget is bytes: roughly 250
    /// tokens, and a rewrite that doubles it has to say so here.
    #[test]
    fn stays_within_its_budget() {
        assert!(
            NOTICE.len() <= 1_200,
            "the notice is {} bytes; trim it or raise the budget deliberately",
            NOTICE.len()
        );
    }
}
