//! The three verbs: `ff hook`, `ff unhook`, `ff trigger`.
//!
//! Two contracts, kept apart by which verb you are in rather than by a
//! check somewhere in the middle. `hook` and `unhook` are for humans: an
//! unknown slug is a real error, every failure is loud, and `--json` emits
//! a report envelope. `trigger` is machine surface for every source but
//! `manual`: it exits 0 whatever happens, it never vetoes, and it says
//! nothing about a failure unless `FF_DEBUG=1`.

use std::io::{IsTerminal, Read};

use ff_core::{Error, Result};

use super::{Change, InstallOptions, Integration, Source, Status, Wiring};
use crate::ctx::Ctx;

/// The same cap the trigger runtime reads under, for the one place that has
/// to look at stdin before it knows whether it is a trigger at all.
const MAX_PEEK: u64 = 8 * 1024 * 1024;

// ---- ff hook / ff unhook ---------------------------------------------------

pub fn hook(
    ctx: &Ctx,
    slugs: Vec<String>,
    all: bool,
    list: bool,
    settings: bool,
    skill: Option<String>,
) -> Result<()> {
    // First, ahead of everything: a print reads no stdin and opens no
    // config file on its way out. It is also the one route to the manual
    // for a client that reads no skills directory — and for one fufu has
    // never heard of.
    if let Some(name) = skill {
        return print_skill(ctx, &name);
    }
    if let Some(result) = legacy(ctx, &slugs, settings) {
        return result;
    }
    if list {
        return report(ctx, &super::statuses(), &[]);
    }

    let targets = targets(ctx, &slugs, all, "hook")?;
    let opts = InstallOptions { settings };
    act(ctx, &targets, &opts, Verb::Hook)
}

/// The shipped text, verbatim, so `ff hook --skill > rules.md` produces a
/// file byte-identical to the one an install writes. The JSON form carries
/// the same string, because anything fufu tells a person a script can read
/// as data.
///
/// A `name` beyond fufu's own is a declared extension's, printed the same
/// bytes an install would write into `skills/<name>/` — concatenated in the
/// manifest's own order when it names more than one file. A name nothing on
/// this machine declares is refused the way `ff extension remove` refuses
/// one; a name that is declared but names no file fufu could read prints
/// nothing, on the same doctrine a hook install applies to it.
fn print_skill(ctx: &Ctx, name: &str) -> Result<()> {
    let text = if name == super::skill::NAME {
        super::skill::SKILL.to_string()
    } else {
        let declared = crate::registry::read().get(name).ok_or_else(|| {
            Error::coded(
                "extension/not-declared",
                format!("nothing on this machine is declared under `{name}`"),
                vec!["ff extension list".into()],
            )
        })?;
        super::skill::sources(declared)
            .into_iter()
            .map(|file| String::from_utf8_lossy(&file.bytes).into_owned())
            .collect::<Vec<_>>()
            .join("\n")
    };
    if ctx.json {
        return crate::machine::emit(ctx.command, &serde_json::json!({ "skill": text }));
    }
    print!("{text}");
    Ok(())
}

pub fn unhook(ctx: &Ctx, slugs: Vec<String>, all: bool) -> Result<()> {
    let targets = targets(ctx, &slugs, all, "unhook")?;
    act(ctx, &targets, &InstallOptions::default(), Verb::Unhook)
}

#[derive(Clone, Copy)]
enum Verb {
    Hook,
    Unhook,
}

/// Which slugs this invocation acts on.
///
/// Named slugs are taken as given. `--all` is everything detected. Naming
/// nothing reports first and then asks, because a command that rewrites
/// config files on four different clients should say what it found before
/// it touches any of them — and when nothing may prompt, the report *is*
/// the answer and nothing is touched.
fn targets(
    ctx: &Ctx,
    slugs: &[String],
    all: bool,
    verb: &str,
) -> Result<Vec<&'static dyn Integration>> {
    if !slugs.is_empty() {
        if all {
            return Err(Error::coded(
                "usage/bad-flags",
                "--all is every slug detected, so naming one alongside it says less, not more",
                vec![
                    format!("ff {verb} --all"),
                    format!("ff {verb} {}", slugs.join(" ")),
                ],
            ));
        }
        return slugs
            .iter()
            .map(|slug| {
                super::by_slug(slug).ok_or_else(|| {
                    Error::coded(
                        "usage/unknown-slug",
                        format!("unknown slug {slug:?} (known: {})", super::slugs()),
                        vec![format!("ff {verb} -l")],
                    )
                })
            })
            .collect();
    }

    let detected: Vec<&'static dyn Integration> = super::all()
        .into_iter()
        .filter(|i| i.detect().is_present())
        .collect();

    if all {
        return Ok(detected);
    }

    // Bare `ff hook`: report, then ask.
    report(ctx, &super::statuses(), &[])?;
    if detected.is_empty() {
        return Ok(Vec::new());
    }
    if !crate::machine::interactive() {
        // Nothing may prompt here, so nothing is acted on either. The
        // report already went out, which is the useful half.
        println!();
        println!("{}", nothing_hooked(&detected, verb));
        return Ok(Vec::new());
    }
    let names: Vec<&str> = detected.iter().map(|i| i.slug()).collect();
    if crate::machine::confirm(&format!("{verb} {}?", names.join(", ")))? {
        Ok(detected)
    } else {
        println!("{}", nothing_hooked(&detected, verb));
        Ok(Vec::new())
    }
}

/// Declining prints the explicit form, so the slugs are teachable rather
/// than something to go and look up.
fn nothing_hooked(detected: &[&'static dyn Integration], verb: &str) -> String {
    let names: Vec<&str> = detected.iter().map(|i| i.slug()).collect();
    format!(
        "nothing {}ed. name what you want: ff {verb} {}",
        verb.trim_end_matches('e'),
        names.join(" ")
    )
}

fn act(
    ctx: &Ctx,
    targets: &[&'static dyn Integration],
    opts: &InstallOptions,
    verb: Verb,
) -> Result<()> {
    if targets.is_empty() {
        // `--all` over a machine with nothing on it. Saying so beats
        // exiting silently, which reads as having done something.
        if !ctx.json {
            println!(
                "nothing to {} — no agent client or shell was detected",
                match verb {
                    Verb::Hook => "hook",
                    Verb::Unhook => "unhook",
                }
            );
        }
        return Ok(());
    }
    let colored = crate::pager::color_enabled();
    let mut acted: Vec<&'static str> = Vec::new();
    // Every failure is loud, and the first one stops the run: these
    // rewrite config files, and carrying on past one that would not write
    // is how half a machine ends up wired.
    for integration in targets {
        let change: Change = match verb {
            Verb::Hook => integration.install(opts)?,
            Verb::Unhook => integration.uninstall(opts)?,
        };
        if change.changed {
            acted.push(integration.slug());
        }
        if !ctx.json {
            for (n, line) in change.lines.iter().enumerate() {
                if n == 0 {
                    println!(
                        "{} {line}",
                        crate::render::paint_ok(integration.slug(), colored)
                    );
                } else {
                    println!("  {}", crate::render::paint_dim(line, colored));
                }
            }
        }
    }
    if ctx.json {
        return report(ctx, &super::statuses(), &acted);
    }
    Ok(())
}

// ---- the report ------------------------------------------------------------

/// One rendering of `statuses()`, which is also what `ff doctor` reads —
/// the two cannot disagree, because there is only one derivation.
fn report(ctx: &Ctx, statuses: &[Status], acted: &[&'static str]) -> Result<()> {
    if ctx.json {
        return crate::machine::emit(
            ctx.command,
            &serde_json::json!({ "integrations": statuses, "changed": acted }),
        );
    }
    let colored = crate::pager::color_enabled();
    let rows: Vec<(&str, String, String)> = statuses
        .iter()
        .map(|status| (status.slug, client_of(status), describe(status)))
        .collect();
    let slug_width = rows.iter().map(|(slug, ..)| slug.len()).max().unwrap_or(6);
    let client_width = rows
        .iter()
        .map(|(_, client, _)| client.chars().count())
        .max()
        .unwrap_or(0);

    for (status, (slug, client, wiring)) in statuses.iter().zip(&rows) {
        let painted = match &status.wiring {
            Wiring::Wired { .. } => crate::render::paint_ok(wiring, colored),
            Wiring::Partial { .. } => crate::render::paint_warn(wiring, colored),
            _ => crate::render::paint_dim(wiring, colored),
        };
        // Pad on the plain text and append after painting, so the escape
        // bytes never inflate a column.
        let pad = " ".repeat(client_width - client.chars().count());
        println!(
            "{slug:slug_width$}  {}{pad}  {painted}",
            crate::render::paint_dim(client, colored)
        );
        if let Some(note) = &status.note {
            println!(
                "{:slug_width$}  {:client_width$}  {}",
                "",
                "",
                crate::render::paint_dim(note, colored)
            );
        }
    }
    Ok(())
}

fn client_of(status: &Status) -> String {
    match &status.presence {
        super::Presence::Present { evidence } => evidence.display().to_string(),
        super::Presence::Absent => "not on this machine".into(),
    }
}

/// The wiring, in one phrase. A slug with independently-wired pieces says
/// what each piece is doing, because that is what a person needs to act on.
fn describe(status: &Status) -> String {
    if !status.parts.is_empty() {
        return status
            .parts
            .iter()
            .map(|part| format!("{} {}", part.name, part.wiring.word()))
            .collect::<Vec<_>>()
            .join(", ");
    }
    let mut line = match status.wiring.at() {
        Some(at) => format!("{} — {}", status.wiring.word(), at.display()),
        None => status.wiring.word(),
    };
    // The skill rides along on the same line rather than earning a row: it
    // is a property of a client that is already listed, and a client with
    // no skill at all should not have to say so.
    if let Some(super::Wiring::Wired { .. }) = &status.skill {
        line.push_str(", skill");
    }
    // The MCP server on the same rule, and for the same reason.
    if let Some(super::Wiring::Wired { .. }) = &status.mcp {
        line.push_str(", mcp");
    }
    line
}

// ---- ff trigger ------------------------------------------------------------

pub fn trigger(ctx: &Ctx, source: Option<String>, message: Option<String>) -> Result<()> {
    match super::resolve_trigger(source.as_deref()) {
        Some(Source::Manual) => super::manual::run(ctx, message),
        Some(Source::Registered(integration, forced)) => {
            integration.trigger(ctx, forced);
            Ok(())
        }
        // The published extension point: an unknown name exits 0 in
        // silence, which is what makes a fufu trigger safe to install into
        // a client fufu has never heard of.
        None => Ok(()),
    }
}

// ---- legacy spellings ------------------------------------------------------

/// The spellings `ff hook` used to take.
///
/// Typed forms — `ff hook agent install claude`, `ff hook shell list` — a
/// person types once, so they forward and can go whenever. Stored forms sit
/// in files fufu can only rewrite when somebody runs the installer again,
/// which they may never do, so `ff hook agent trigger claude` and
/// `ff hook shell trigger` are accepted forever.
///
/// Returns `None` when this is not a legacy spelling at all.
fn legacy(ctx: &Ctx, slugs: &[String], settings: bool) -> Option<Result<()>> {
    // The landmine. `ff hook claude` used to mean *trigger* and under this
    // grammar means *install*. A stale Phase 1 settings entry would
    // otherwise run the installer on every PreToolUse — printing into the
    // agent's context on UserPromptSubmit, and capturing nothing.
    //
    // The shim is deliberately narrow: this one spelling, only when
    // nothing may prompt, and only when stdin actually holds a client
    // payload. Installing never reads stdin otherwise, and the canonical
    // `ff trigger claude` is never ambiguous.
    if slugs.len() == 1 && slugs[0] == "claude" {
        if let Some(payload) = payload_on_stdin()
            && let Some(integration) = super::by_slug("claude")
            && let Some(proto) = integration.protocol()
        {
            super::runtime::agent_payload(ctx, "claude", proto, None, &payload);
            return Some(Ok(()));
        }
        return None;
    }

    let (kind, verb, name) = match slugs {
        [kind, verb, rest @ ..] if matches!(kind.as_str(), "agent" | "shell" | "editor") => {
            (kind.as_str(), verb.as_str(), rest.first().cloned())
        }
        _ => return None,
    };

    Some(match (kind, verb) {
        ("agent", "trigger") => trigger(ctx, Some(name.unwrap_or_else(|| "claude".into())), None),
        ("shell", "trigger") => trigger(ctx, Some("shell".into()), None),
        ("agent", "install") => hook(
            ctx,
            vec![name.unwrap_or_else(|| "claude".into())],
            false,
            false,
            settings,
            None,
        ),
        ("agent", "uninstall") => unhook(ctx, vec![name.unwrap_or_else(|| "claude".into())], false),
        ("shell", "install") => {
            match name.or_else(|| super::shell::default_shell().map(str::to_string)) {
                Some(shell) => hook(ctx, vec![shell], false, false, settings, None),
                None => Err(no_shell()),
            }
        }
        ("shell", "uninstall") => {
            match name.or_else(|| super::shell::default_shell().map(str::to_string)) {
                Some(shell) => unhook(ctx, vec![shell], false),
                None => Err(no_shell()),
            }
        }
        (_, "list") => report(ctx, &super::statuses(), &[]),
        // `ff hook editor` was reserved and never installed anything; there
        // is nothing to forward it to.
        _ => Err(Error::coded(
            "usage/unknown-slug",
            format!(
                "`ff hook {kind} {verb}` is retired; slugs are flat now (known: {})",
                super::slugs()
            ),
            vec!["ff hook -l".into(), "ff hook claude".into()],
        )),
    })
}

fn no_shell() -> Error {
    Error::coded(
        "usage/unknown-slug",
        "SHELL names no shell fufu wires; name one",
        vec![
            "ff hook bash".into(),
            "ff hook zsh".into(),
            "ff hook fish".into(),
            "ff hook powershell".into(),
        ],
    )
}

/// A client payload waiting on stdin, or `None`.
///
/// Reads only when stdin is not a terminal *and* nothing may prompt, so a
/// person typing the legacy spelling at a prompt is never left blocked on
/// input they were not asked for.
fn payload_on_stdin() -> Option<Vec<u8>> {
    if std::io::stdin().is_terminal() || crate::machine::interactive() {
        return None;
    }
    let mut buf = Vec::new();
    std::io::stdin()
        .lock()
        .take(MAX_PEEK)
        .read_to_end(&mut buf)
        .ok()?;
    // A payload is a JSON object naming an event or a directory. Anything
    // else — an empty pipe, a here-doc of prose — is not a trigger, and the
    // install runs as typed.
    let value: serde_json::Value = serde_json::from_slice(&buf).ok()?;
    let object = value.as_object()?;
    (object.contains_key("hook_event_name") || object.contains_key("cwd")).then_some(buf)
}
