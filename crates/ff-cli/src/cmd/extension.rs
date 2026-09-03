//! `ff extension` — what this machine has declared, and the two gestures
//! that change it.
//!
//! `add` is the whole of the verb's weight: it runs the `--ff-manifest`
//! handshake, checks the contract the manifest claims against fufu's own,
//! and records what came back. From then on fufu will describe the
//! extension — the tool serves its verbs, the card names them, its briefing
//! line rides fufu's, its skills install beside fufu's, the neutral agent
//! event fans out to it. `list` is that record read back, and `remove`
//! takes a name off it.
//!
//! Declaring buys the extension no capability and no environment; an
//! undeclared `ff-<name>` runs from a shell exactly as it ran before. What
//! it buys is fufu vouching for the verb, which is why the family is
//! shell-only — the registry is the allowlist, so an agent must not be able
//! to write it through the tool.

use ff_core::{Error, Result};

use crate::cli::ExtensionAction;
use crate::ctx::Ctx;
use crate::manifest::{Briefing, Manifest};
use crate::registry;

pub fn run(ctx: &Ctx, action: Option<ExtensionAction>) -> Result<()> {
    // Bare `ff extension` is the list, on the same rule as bare `ff branch`.
    match action {
        None | Some(ExtensionAction::List) => list(ctx),
        Some(ExtensionAction::Add { name }) => add(ctx, &name),
        Some(ExtensionAction::Remove { name }) => remove(ctx, &name),
    }
}

fn add(ctx: &Ctx, name: &str) -> Result<()> {
    // Read before the write, not after: `declare` replaces a record in
    // place, so the version it is replacing is only knowable from this side
    // of it. One read is enough — `add` is a one-shot process, and nothing
    // re-reads the registry after a write.
    let replaced = registry::read()
        .get(name)
        .map(|declared| declared.manifest.version.clone());

    let shook = crate::manifest::handshake(name)?;
    registry::declare(&shook)?;
    let manifest = &shook.manifest;

    if ctx.json {
        let payload = serde_json::json!({
            "declared": {
                "manifest": manifest,
                "path": shook.path,
            },
            // The version this declaration wrote over, and null when the
            // name was not on the list.
            "replaced": replaced,
            "file": registry::path(),
        });
        return crate::machine::emit("extension add", &payload);
    }

    let colored = crate::pager::color_enabled();
    let verb = if replaced.is_some() {
        "re-declared"
    } else {
        "declared"
    };
    match &replaced {
        Some(was) if was != &manifest.version => println!(
            "{verb} {name} {} from {} (was {was})",
            manifest.version,
            shook.path.display()
        ),
        _ => println!(
            "{verb} {name} {} from {}",
            manifest.version,
            shook.path.display()
        ),
    }
    println!("  its verbs: {}", verbs(manifest));
    // Each line is one thing declaring just bought, and it prints only when
    // the manifest asked for it — a machine that declared a plain extension
    // reads three lines, not seven.
    for note in bought(manifest) {
        println!(
            "{}",
            crate::render::paint_dim(&format!("  {note}"), colored)
        );
    }
    println!(
        "{}",
        crate::render::paint_dim(&format!("undo: ff extension remove {name}"), colored)
    );
    Ok(())
}

/// What the manifest asked fufu to do beyond serving its verbs, one note
/// apiece, in the order the manifest lists them.
fn bought(manifest: &Manifest) -> Vec<String> {
    let mut notes = Vec::new();
    // Not a line about a promise fufu keeps for every extension: this one
    // is the extension saying its writes are its own.
    if !manifest.undoable {
        notes.push("it says its writes are its own — ff undo does not reach them".to_string());
    }
    match &manifest.briefing {
        Some(Briefing::Line(_)) => notes.push("its briefing line rides fufu's".to_string()),
        Some(Briefing::Ask(true)) => {
            notes.push(format!(
                "ff-{} briefing is asked for its line each time",
                manifest.name
            ));
        }
        Some(Briefing::Ask(false)) | None => {}
    }
    if !manifest.skills.is_empty() {
        let plural = if manifest.skills.len() == 1 { "" } else { "s" };
        notes.push(format!(
            "it ships {} skill file{plural}, installed beside fufu's by ff hook",
            manifest.skills.len()
        ));
    }
    if !manifest.events.is_empty() {
        let kinds: Vec<&str> = manifest
            .events
            .iter()
            .filter_map(|event| crate::manifest::kind_name(event.kind))
            .collect();
        notes.push(format!("it subscribes to {}", kinds.join(", ")));
    }
    if manifest.mcp.is_some() {
        notes.push("it brings a server of its own, registered beside fufu's".to_string());
    }
    notes
}

fn remove(ctx: &Ctx, name: &str) -> Result<()> {
    // `forget` answers whether the name was on the list, and the refusal is
    // the verb's rather than the registry's: a writer that failed on a name
    // it was asked to take off would be refusing an outcome that already
    // holds.
    if !registry::forget(name)? {
        return Err(Error::coded(
            "extension/not-declared",
            format!("nothing on this machine is declared under `{name}`"),
            vec![
                "ff extension list".into(),
                format!("ff extension add {name}"),
            ],
        ));
    }

    if ctx.json {
        let payload = serde_json::json!({
            "removed": name,
            "file": registry::path(),
        });
        return crate::machine::emit("extension remove", &payload);
    }

    let colored = crate::pager::color_enabled();
    println!("removed {name}");
    println!(
        "{}",
        crate::render::paint_dim(
            &format!("ff-{name} still runs from a shell; fufu says nothing about it now"),
            colored
        )
    );
    Ok(())
}

fn list(ctx: &Ctx) -> Result<()> {
    let registry = registry::read();

    if ctx.json {
        let declared: Vec<serde_json::Value> = registry
            .declared()
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "manifest": entry.manifest,
                    "path": entry.path,
                    "declared_at": entry.declared_at,
                    // The fresh PATH walk, not the recorded path: a binary
                    // that moved is found and one that was uninstalled is
                    // null, which is the row a person is looking for.
                    "resolved": entry.resolve(),
                })
            })
            .collect();
        let stale: Vec<serde_json::Value> = registry
            .stale
            .iter()
            .map(|stale| serde_json::json!({"name": stale.name, "contract": stale.contract}))
            .collect();
        let payload = serde_json::json!({
            "file": registry.path,
            "declared": declared,
            "stale": stale,
            "unreadable": registry.unreadable,
        });
        return crate::machine::emit("extension list", &payload);
    }

    let colored = crate::pager::color_enabled();
    // A file that is there and does not read as one is a warning rather
    // than a failure: the reader's answer is still an honest empty list,
    // and this is where the person who owns the file is standing.
    if let Some(why) = &registry.unreadable {
        eprintln!(
            "{}",
            crate::render::paint_warn(
                &format!("ff: the registry does not read as one: {why}"),
                colored
            )
        );
    }

    if registry.is_empty() {
        println!("nothing is declared on this machine");
        println!(
            "{}",
            crate::render::paint_dim("ff extension add <name> declares one", colored)
        );
    } else {
        let name_width = registry
            .declared()
            .iter()
            .map(|entry| entry.name().chars().count())
            .max()
            .unwrap_or_default();
        let version_width = registry
            .declared()
            .iter()
            .map(|entry| entry.manifest.version.chars().count())
            .max()
            .unwrap_or_default();
        for entry in registry.declared() {
            println!(
                "{}  {}  {}",
                cell(entry.name(), name_width),
                cell(&entry.manifest.version, version_width),
                verbs(&entry.manifest)
            );
            // The one thing a row cannot show by standing there: dispatch
            // is the PATH walk, so a record whose binary is gone is a name
            // fufu describes and cannot run.
            if entry.resolve().is_none() {
                println!(
                    "{}",
                    crate::render::paint_dim(
                        &format!("  no ff-{} on PATH any more", entry.name()),
                        colored
                    )
                );
            }
        }
    }

    // Records from a contract this fufu does not speak: kept in the file,
    // described to nobody, and named here so the file and the listing agree
    // about what is in it.
    if !registry.stale.is_empty() {
        println!();
        println!(
            "{}",
            crate::render::paint_dim("from a contract this fufu does not speak", colored)
        );
        for stale in &registry.stale {
            println!("  {}  contract {}", stale.name, stale.contract);
        }
    }
    Ok(())
}

/// A declared extension's verbs, in the order the manifest lists them.
fn verbs(manifest: &Manifest) -> String {
    manifest
        .verbs
        .iter()
        .map(|verb| verb.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// A left-aligned cell. Nothing here is painted, so the pad is the plain
/// arithmetic the worktree listing does after its paint.
fn cell(text: &str, width: usize) -> String {
    let pad = " ".repeat(width.saturating_sub(text.chars().count()));
    format!("{text}{pad}")
}
