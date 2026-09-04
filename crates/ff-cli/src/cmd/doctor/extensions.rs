use super::Row;

/// Every `ff-<name>` on PATH, whether it is declared, and for a declared one
/// whether the binary's manifest still matches what was recorded.
///
/// Undeclared is fufu's default working as designed — the shell surface
/// `ff-<name>` has always had, unchanged by nobody registering it — so it
/// earns an `info` row at most and never a `WARN`. The findings here are the
/// registry not reading as one, a record from a contract this fufu does not
/// speak, a declared binary that has left PATH, and a declared binary whose
/// manifest no longer matches what `ff extension add` recorded: the same
/// severity and the same shape `wiring.rs`'s stale-hook row already uses.
///
/// A handshake runs for every declared extension found on PATH — one spawn
/// apiece. `ff mcp` and the trigger fan-out both trust the record rather
/// than pay that cost on every call; doctor is the one place slow and
/// thorough is the point, so it asks each binary directly rather than
/// taking the registry's word for what is still true.
///
/// An extension whose manifest promises tools costs a second spawn beside
/// the first — `--ff-manifest`, then `--ff-tools` — doubling doctor's cost
/// for exactly the extensions that promised tools. That is the same
/// tradeoff on the same side: `ff mcp` and the trigger fan-out stay silent
/// on a failed or missing tools handshake, on the trigger doctrine, which
/// is exactly why this is the one place a person finds out.
pub(super) fn extension_rows(statuses: &[crate::integ::Status], fix: bool) -> Vec<Row> {
    let registry = crate::registry::read();
    let mut rows = Vec::new();

    if let Some(why) = &registry.unreadable {
        rows.push(Row::warn(
            "extensions",
            format!("the registry does not read as one: {why} — nothing is declared until it does"),
        ));
    }

    if !registry.stale.is_empty() {
        let mut named: Vec<String> = registry
            .stale
            .iter()
            .map(|stale| format!("{} (contract {})", stale.name, stale.contract))
            .collect();
        named.sort();
        rows.push(Row::warn(
            "extensions",
            format!(
                "recorded under a contract this fufu does not speak: {}",
                named.join(", ")
            ),
        ));
    }

    for declared in registry.declared() {
        rows.push(declared_row(declared, statuses, fix));
    }

    let undeclared: Vec<String> = crate::ext::on_path()
        .into_iter()
        .filter(|name| registry.get(name).is_none())
        .collect();
    if !undeclared.is_empty() {
        let named: Vec<String> = undeclared.iter().map(|name| format!("ff-{name}")).collect();
        rows.push(Row::info(
            "extensions",
            format!(
                "{} on PATH, undeclared: {} (ff extension add <name> declares one)",
                undeclared.len(),
                named.join(", ")
            ),
        ));
    }

    if let Some(row) = orphaned_mcp_row(statuses) {
        rows.push(row);
    }

    rows
}

/// One declared extension: gone from PATH, failed its handshake, drifted
/// from what was recorded, or matches. Named for the extension rather than
/// for "extensions", the way a wiring row is named for its client.
///
/// A manifest naming a server of its own folds that server's registration
/// into this same row rather than a row of its own — a server sits on both
/// the client axis and the extension axis, and the extension axis already
/// has a row. Silent when the manifest names no server, which is most of
/// them.
fn declared_row(
    declared: &crate::registry::Declared,
    statuses: &[crate::integ::Status],
    fix: bool,
) -> Row {
    let name = declared.name();
    let recorded = &declared.manifest;

    let Some(path) = declared.resolve() else {
        return Row::warn(
            name.to_string(),
            format!(
                "declared {} — no ff-{name} on PATH any more (ff extension remove {name} forgets it)",
                recorded.version
            ),
        );
    };

    match crate::manifest::ask(&path, name) {
        Err(err) => Row::warn(
            name.to_string(),
            format!(
                "declared {}, but the handshake failed: {err}",
                recorded.version
            ),
        ),
        // The registry never keeps a record whose contract this fufu does
        // not speak (that is `registry.stale`), and `ask` refuses a binary
        // whose live contract is not this fufu's own — so in practice only
        // `version` ever drifts here. The contract is compared anyway: this
        // is the one check standing between an agent and a manifest that
        // has quietly moved, and it should not depend on that invariant
        // holding forever to catch a drifted contract too.
        Ok(live) if live.version != recorded.version || live.contract != recorded.contract => {
            Row::warn(
                name.to_string(),
                format!(
                    "recorded {} (contract {}), ff-{name} on PATH now answers {} (contract {}) \
                     — ff extension add {name} re-declares it",
                    recorded.version, recorded.contract, live.version, live.contract
                ),
            )
        }
        Ok(live) => {
            let mut row = Row::ok(
                name.to_string(),
                format!("{} matches ff-{name} on PATH", live.version),
            );
            if let Some(mcp) = mcp_extension_row(declared, statuses, fix) {
                row = merge(row, mcp);
            }
            if let Some(tools) = tools_row(name, &path, &live) {
                row = merge(row, tools);
            }
            row
        }
    }
}

/// A declared extension's own tools, asked for when its manifest promises
/// them — the second spawn `extension_rows` argues for, hung off the same
/// row `mcp_extension_row` hangs a server clause on. `None` when the
/// manifest promises none, which is most of them.
///
/// A failed or missing handshake is a `WARN`: `ff mcp` and the trigger
/// fan-out are silent about it on the trigger doctrine, so a promise kept
/// only in the manifest and never in what the binary produces is invisible
/// everywhere but here. A count that came back is a clause on an otherwise
/// healthy row, naming the tools rather than only how many.
fn tools_row(name: &str, path: &std::path::Path, live: &crate::manifest::Manifest) -> Option<Row> {
    if !live.tools {
        return None;
    }
    Some(match crate::manifest::ask_tools(path, name) {
        Err(err) => Row::warn(
            name.to_string(),
            format!("promises tools, but the handshake failed: {err}"),
        ),
        Ok(tools) => {
            let names: Vec<&str> = tools.iter().map(|tool| tool.name.as_str()).collect();
            Row::ok(
                name.to_string(),
                format!(
                    "produces {} tool{}: {}",
                    tools.len(),
                    if tools.len() == 1 { "" } else { "s" },
                    names.join(", ")
                ),
            )
        }
    })
}

/// `base` and an added clause about the same extension, folded into one
/// row: the worse level wins, `fixable` is inherited from whichever side
/// set it, and the two details join into one sentence.
fn merge(base: Row, extra: Row) -> Row {
    use super::Level;

    let warn = matches!(base.level, Level::Warn) || matches!(extra.level, Level::Warn);
    let info = matches!(base.level, Level::Info) || matches!(extra.level, Level::Info);
    Row {
        level: if warn {
            Level::Warn
        } else if info {
            Level::Info
        } else {
            Level::Ok
        },
        name: base.name,
        detail: format!("{}; {}", base.detail, extra.detail),
        fixable: base.fixable || extra.fixable,
    }
}

/// A declared extension's own server, folded across the clients whose
/// files carry it — the same aggregation `wiring::mcp_row` does for
/// fufu's own, returned as a clause for `declared_row` rather than a
/// second aggregate. `None` when the manifest names no server, or when no
/// client on this machine shows any trace of one.
fn mcp_extension_row(
    declared: &crate::registry::Declared,
    statuses: &[crate::integ::Status],
    fix: bool,
) -> Option<Row> {
    use crate::integ::Wiring;
    use crate::integ::mcp::ServerWiring;

    declared.manifest.mcp.as_ref()?;
    let name = declared.name();

    let entries: Vec<(&crate::integ::Status, &ServerWiring)> = statuses
        .iter()
        .filter_map(|status| {
            status
                .mcp_extensions
                .iter()
                .find(|ext| ext.name == name)
                .map(|ext| (status, &ext.wiring))
        })
        .filter(|(status, wiring)| status.presence.is_present() || wiring.at().is_some())
        .collect();
    if entries.is_empty() {
        return None;
    }

    // A wired hook with no server beside it: an install predating the
    // manifest naming one, repaired the same way a missing fufu server is.
    if let Some((status, _)) = entries.iter().find(|(status, wiring)| {
        matches!(wiring, ServerWiring::NotWired)
            && matches!(status.wiring, Wiring::Wired { .. } | Wiring::Partial { .. })
    }) {
        return Some(super::wiring::fixed_or_fixable_named(
            name.to_string(),
            status,
            format!(
                "its MCP server is not registered with {}, whose hook is wired",
                status.slug
            ),
            &format!("`ff hook {}`", status.slug),
            fix,
        ));
    }

    // A stale entry: the same binary, arguments the manifest has since
    // moved past. `ff hook <slug>` will not overwrite it — the ownership
    // test that would let a repair rewrite an entry is exactly the one a
    // stale entry fails — so this is never offered as fixable.
    if let Some((status, wiring)) = entries
        .iter()
        .find(|(_, wiring)| matches!(wiring, ServerWiring::Stale { .. }))
    {
        let at = wiring
            .at()
            .map(|at| at.display().to_string())
            .unwrap_or_default();
        return Some(Row::warn(
            name.to_string(),
            format!(
                "its MCP server in {at} runs an older argument list than the manifest now \
                 declares — `ff hook {}` will not overwrite an entry it does not own outright; \
                 remove it there and run `ff hook {}` again",
                status.slug, status.slug
            ),
        ));
    }

    let registered: Vec<&str> = entries
        .iter()
        .filter(|(_, wiring)| matches!(wiring, ServerWiring::Wired { .. }))
        .map(|(status, _)| status.slug)
        .collect();
    let hand: Vec<&str> = entries
        .iter()
        .filter(|(_, wiring)| matches!(wiring, ServerWiring::HandWritten))
        .map(|(status, _)| status.slug)
        .collect();
    if registered.is_empty() && hand.is_empty() {
        let slugs: Vec<&str> = entries.iter().map(|(status, _)| status.slug).collect();
        return Some(Row::info(
            name.to_string(),
            format!(
                "its MCP server is not registered (optional — `ff hook {}` registers it)",
                slugs.join(" ")
            ),
        ));
    }
    let mut detail = String::new();
    if !registered.is_empty() {
        detail.push_str(&format!(
            "its MCP server registered with {}",
            registered.join(", ")
        ));
    }
    if !hand.is_empty() {
        if !detail.is_empty() {
            detail.push_str("; ");
        }
        detail.push_str(&format!(
            "a hand-written entry stands for it in {}",
            hand.join(", ")
        ));
    }
    Some(if registered.is_empty() || !hand.is_empty() {
        Row::info(name.to_string(), detail)
    } else {
        Row::ok(name.to_string(), detail)
    })
}

/// Names registered as an MCP server in some client's file that no
/// declared extension names any more — the trace `ff extension remove`
/// leaves behind. News, never a finding, the same as an upstream section
/// pointing at a branch's still-published shared copy: it is what a plain
/// removal leaves on purpose, or a hand-written entry doctor cannot tell
/// apart from one.
fn orphaned_mcp_row(statuses: &[crate::integ::Status]) -> Option<Row> {
    let mut named: Vec<String> = statuses
        .iter()
        .flat_map(|status| {
            status
                .mcp_orphaned
                .iter()
                .map(|name| format!("{name} ({})", status.slug))
        })
        .collect();
    if named.is_empty() {
        return None;
    }
    named.sort();
    Some(Row::info(
        "extensions",
        format!(
            "registered as an MCP server but declared by nothing here: {}",
            named.join(", ")
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd::doctor::Level;
    use crate::registry::Registry;

    /// A manifest of the smallest shape, under whatever name, version and
    /// contract the test needs.
    fn manifest(name: &str, version: &str, contract: u32) -> crate::manifest::Manifest {
        crate::manifest::parse(serde_json::json!({
            "name": name,
            "version": version,
            "contract": contract,
            "verbs": [{"name": "board", "read_only": true}],
            "undoable": true,
        }))
        .expect("a manifest the page types")
    }

    fn declared(name: &str, version: &str, path: std::path::PathBuf) -> crate::registry::Declared {
        crate::registry::Declared {
            manifest: manifest(name, version, crate::machine::CONTRACT),
            path,
            declared_at: 1,
        }
    }

    /// A registry that will not read as a registry is a finding, not a
    /// silent empty answer. The rest of the coverage — a healthy declared
    /// extension, one that has drifted, and an undeclared one raising
    /// nothing — needs a controlled PATH, and lives in
    /// `tests/doctor_extensions.rs` for that reason.
    #[test]
    fn a_corrupt_registry_is_a_warning() {
        let mut registry = Registry::default();
        registry.unreadable = Some("a record is missing its name".into());
        let why = registry.unreadable.as_deref().unwrap();
        let row = Row::warn(
            "extensions",
            format!("the registry does not read as one: {why} — nothing is declared until it does"),
        );
        assert!(matches!(row.level, Level::Warn));
        assert!(
            row.detail.contains("a record is missing its name"),
            "{}",
            row.detail
        );
    }

    /// A record from a contract this fufu does not speak is a finding, and
    /// names the contract it claims.
    #[test]
    fn a_stale_record_names_its_contract() {
        let mut registry = Registry::default();
        registry.stale.push(crate::registry::Stale {
            name: "tower".into(),
            contract: 99,
        });
        let mut named: Vec<String> = registry
            .stale
            .iter()
            .map(|stale| format!("{} (contract {})", stale.name, stale.contract))
            .collect();
        named.sort();
        assert_eq!(named, ["tower (contract 99)"]);
    }

    /// A declared binary that has left PATH is a warning, not a silent
    /// `None` — the same fake name `registry.rs`'s own tests use, since
    /// nothing on any real PATH answers to it.
    #[test]
    fn a_declared_binary_gone_from_path_is_a_warning() {
        let entry = declared(
            "nothing-on-path-answers-to-this",
            "0.4.1",
            std::path::PathBuf::from("/usr/local/bin/ff-nothing-on-path-answers-to-this"),
        );
        let row = declared_row(&entry, &[], false);
        assert!(matches!(row.level, Level::Warn), "{}", row.detail);
        assert!(!row.fixable);
        assert!(
            row.detail
                .contains("no ff-nothing-on-path-answers-to-this on PATH any more"),
            "{}",
            row.detail
        );
    }
}
