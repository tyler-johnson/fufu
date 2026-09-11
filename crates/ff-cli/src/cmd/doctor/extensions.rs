use super::Row;

/// Every `ff-<name>` on PATH, whether it is declared, and for a declared one
/// whether the binary's manifest still matches what was recorded.
///
/// Undeclared is fufu's default working as designed — the shell surface
/// `ff-<name>` has always had, unchanged by nobody registering it — so it
/// earns an `info` row at most and never a `WARN`. The findings here are the
/// registry not reading as one, a record from a contract this fufu does not
/// speak, a declared binary that has left PATH, and a declared binary whose
/// manifest no longer matches what `ff extension <name>` recorded: the same
/// severity and the same shape `wiring.rs`'s stale-hook row already uses.
///
/// The drifted-manifest row is the one that covers the channels no script
/// runs. A Homebrew upgrade or a hand copy replaces the binary and runs no
/// `ff extension <name>`, so the record falls behind the binary, and `ff hook`
/// keeps writing the skills the record names until something re-records
/// it. Version and contract share the row: either one moving is the same
/// finding with the same repair. A record whose contract this fufu does
/// not speak sits outside the declared list, so it is asked here the way a
/// declared one is, and a binary on PATH answering this fufu's contract
/// turns the stale record into that same drift row rather than the
/// aggregate one.
///
/// A handshake runs for every declared extension found on PATH — one spawn
/// apiece. The trigger fan-out trusts the record rather than pay that cost
/// on every call; doctor is the one place slow and thorough is the point,
/// so it asks each binary directly rather than taking the registry's word
/// for what is still true.
pub(super) fn extension_rows() -> Vec<Row> {
    let registry = crate::registry::read();
    let mut rows = Vec::new();

    if let Some(why) = &registry.unreadable {
        rows.push(Row::warn(
            "extensions",
            format!("the registry does not read as one: {why} — nothing is declared until it does"),
        ));
    }

    // A stale record whose binary on PATH answers this fufu's contract is
    // a record behind its binary, and gets the drift row with its repair.
    // The rest — no binary, or one that does not speak this contract
    // either — stay aggregated, since re-declaring would refuse them.
    let mut named: Vec<String> = Vec::new();
    for stale in &registry.stale {
        let live = crate::ext::resolve(&stale.name)
            .and_then(|path| crate::manifest::ask(&path, &stale.name).ok());
        match live {
            Some(live) => rows.push(drifted_row(
                &stale.name,
                stale.version.as_deref().unwrap_or("?"),
                stale.contract,
                &live,
            )),
            None => named.push(format!("{} (contract {})", stale.name, stale.contract)),
        }
    }
    if !named.is_empty() {
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
        rows.push(declared_row(declared));
    }

    // A stale record is a declaration too, one this fufu does not speak:
    // its binary is not undeclared, and its row is above.
    let undeclared: Vec<String> = crate::ext::on_path()
        .into_iter()
        .filter(|name| {
            registry.get(name).is_none() && !registry.stale.iter().any(|stale| &stale.name == name)
        })
        .collect();
    if !undeclared.is_empty() {
        let named: Vec<String> = undeclared.iter().map(|name| format!("ff-{name}")).collect();
        rows.push(Row::info(
            "extensions",
            format!(
                "{} on PATH, undeclared: {} (ff extension <name> declares one)",
                undeclared.len(),
                named.join(", ")
            ),
        ));
    }

    rows
}

/// One declared extension: gone from PATH, failed its handshake, drifted
/// from what was recorded, or matches. Named for the extension rather than
/// for "extensions", the way a wiring row is named for its client.
fn declared_row(declared: &crate::registry::Declared) -> Row {
    let name = declared.name();
    let recorded = &declared.manifest;

    let Some(path) = declared.resolve() else {
        return Row::warn(
            name.to_string(),
            format!(
                "declared {} — no ff-{name} on PATH any more (ff extension -d {name} forgets it)",
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
            drifted_row(name, &recorded.version, recorded.contract, &live)
        }
        Ok(live) => Row::ok(
            name.to_string(),
            format!("{} matches ff-{name} on PATH", live.version),
        ),
    }
}

/// The record is behind the binary: what was recorded, what `ff-<name>` on
/// PATH answers now, and the `ff extension <name>` that re-records it. One
/// row for a version that moved, a contract that moved, or both, and the
/// same row whether the record was on the declared list or stale.
fn drifted_row(
    name: &str,
    recorded_version: &str,
    recorded_contract: u32,
    live: &crate::manifest::Manifest,
) -> Row {
    Row::warn(
        name.to_string(),
        format!(
            "recorded {recorded_version} (contract {recorded_contract}), ff-{name} on PATH now \
             answers {} (contract {}) — ff extension {name} re-declares it",
            live.version, live.contract
        ),
    )
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
            version: Some("0.4.1".into()),
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
        let row = declared_row(&entry);
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
