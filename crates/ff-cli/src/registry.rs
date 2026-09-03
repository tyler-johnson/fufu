//! The extension registry: which extensions somebody declared on this
//! machine, and the one reader everything else asks.
//!
//! `ff extension add <name>` records the manifest the handshake just read,
//! and from then on fufu will describe the extension — the MCP tool serves
//! its verbs, the card names them, `ff help <name>` and `ff explain
//! <name>/<id>` delegate to the binary, its briefing line rides fufu's, its
//! skills install beside fufu's, the neutral agent event fans out to it, and
//! a server of its own registers beside fufu's. This file is the allowlist
//! for all of that, which is why it sits under the user's config directory
//! rather than in a repository, and why `ff extension` is one of the verbs
//! the tool does not offer: declaring is a decision about this machine, and
//! not one an agent makes for itself.
//!
//! The file is `<config_root>/fufu/extensions.json`, pretty-printed because
//! a person owns it, and it is a list rather than a map because the order is
//! load-bearing: subscribers are fanned out in it and the card names verbs
//! in it.
//!
//! ```json
//! {
//!   "ff": 1,
//!   "extensions": [
//!     {
//!       "path": "/usr/local/bin/ff-tower",
//!       "declared_at": 1788462398,
//!       "manifest": {"name": "tower", "version": "0.4.1", "contract": 1, "…": "…"}
//!     }
//!   ]
//! }
//! ```
//!
//! `path` is the binary the PATH walk landed on when it was declared and
//! `declared_at` is unix seconds; with the manifest's own `version` and
//! `contract` beside them they are what `ff doctor` compares a binary
//! against to report drift. Nothing resolves `path` at read time — dispatch
//! is still the PATH walk, exactly as it is for an undeclared extension, so
//! a recorded path is evidence about the past rather than a route.
//!
//! The top-level `ff` is the contract the fufu that last wrote the file
//! speaks. Nothing reads it: a record carries its own contract, and refusing
//! a whole file on this number would lose a registry to one downgrade. It is
//! here so a later change to the file's own shape has something to hang off.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ff_core::{Error, Result};
use serde::{Deserialize, Serialize};

use crate::manifest::{Handshake, Manifest};

/// The file, under the directory fufu owns inside the config root.
pub fn path() -> Option<PathBuf> {
    Some(crate::userdirs::config_root()?.join("fufu").join(FILE))
}

const FILE: &str = "extensions.json";

/// One declared extension, as the registry recorded it.
#[derive(Debug, Clone)]
pub struct Declared {
    /// The manifest the handshake read, kept whole — unknown fields
    /// included, so a record written by a later fufu survives this one.
    pub manifest: Manifest,
    /// The binary the PATH walk landed on when it was declared. What
    /// `ff doctor` compares against; not what anything runs.
    pub path: PathBuf,
    /// Unix seconds.
    pub declared_at: i64,
}

impl Declared {
    /// The `<name>` in `ff-<name>`.
    pub fn name(&self) -> &str {
        &self.manifest.name
    }

    /// Where `ff-<name>` is now, and `None` when it has left PATH since it
    /// was declared.
    ///
    /// A fresh walk rather than the recorded path, because declaring buys
    /// no route: fufu resolves a declared extension exactly as it resolves
    /// an undeclared one, so a binary that moved to another PATH directory
    /// is found and one that was uninstalled is not.
    pub fn resolve(&self) -> Option<PathBuf> {
        crate::ext::resolve(self.name())
    }
}

/// A record the file carries that this fufu will not describe, because it
/// claims a contract fufu does not speak.
#[derive(Debug, Clone)]
pub struct Stale {
    pub name: String,
    pub contract: u32,
}

/// What this machine has declared, and the trouble reading it if there was
/// any.
#[derive(Debug, Default)]
pub struct Registry {
    /// The file fufu read, or would have read. `None` when the environment
    /// names no config directory.
    pub path: Option<PathBuf>,
    entries: Vec<Declared>,
    /// Records the file carries that this fufu will not describe, with the
    /// contract each claims. `ff doctor` reports them; nothing else looks.
    pub stale: Vec<Stale>,
    /// Why the registry read as empty with a file sitting there, in the
    /// words of whatever refused to read it. `None` whenever the empty
    /// answer is an honest one.
    pub unreadable: Option<String>,
}

impl Registry {
    /// Every declared extension fufu will describe, in the order they were
    /// declared.
    pub fn declared(&self) -> &[Declared] {
        &self.entries
    }

    /// The one declared under `name`.
    pub fn get(&self, name: &str) -> Option<&Declared> {
        self.entries.iter().find(|entry| entry.name() == name)
    }

    /// Whether this machine will describe anything. The common answer is
    /// yes it will not: most machines declare nothing.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// What this machine has declared. This is the reader; there is no other.
///
/// It never fails. A missing file is the normal empty case, and so is an
/// environment naming no config directory. A file that is there and does not
/// read as a registry is [`Registry::unreadable`] and an empty list: the
/// registry is the allowlist for everything fufu says about an extension, so
/// describing nothing is the safe answer when fufu cannot tell what is on
/// it, and `ff doctor` is where a person is told about the file.
///
/// A record naming a contract this fufu does not speak is not on the list
/// either. It is in [`Registry::stale`] instead, because describing a verb
/// whose shapes fufu can no longer promise is the thing declaring exists to
/// avoid. No caller has to remember that: the list is what fufu will
/// describe, and the two fields beside it are what `ff doctor` reports.
///
/// Nothing is looked for on disk. A read is one file parse and no PATH walk,
/// because the trigger fan-out and the MCP relay both call this per event
/// and per tool call. [`Declared::resolve`] is the walk, taken by whoever is
/// about to run the binary, and it answers `None` when the binary has left
/// PATH — a record outliving its binary, which costs a caller a `None` and
/// costs every other caller nothing.
///
/// The file is read once per process. `ff extension add` and `remove` are
/// the only writers and each is a one-shot process, so nothing re-reads
/// after a write. The one long-lived reader is `ff mcp`, where serving the
/// verbs that were advertised at handshake for the life of the connection is
/// what keeps the card and the tool agreeing.
pub fn read() -> &'static Registry {
    static ONCE: OnceLock<Registry> = OnceLock::new();
    ONCE.get_or_init(|| load(path().as_deref()))
}

/// [`read`] without the cache, against a named file. The tests' door, and
/// the reason the cache is a detail of `read` rather than of the parse.
pub fn load(file: Option<&Path>) -> Registry {
    let Some(file) = file else {
        return Registry::default();
    };
    let mut registry = Registry {
        path: Some(file.to_path_buf()),
        ..Registry::default()
    };

    let records = match raw(file) {
        Ok(records) => records,
        Err(why) => {
            registry.unreadable = Some(why);
            return registry;
        }
    };

    for record in records {
        // The contract is read before the manifest is parsed, not after: a
        // record from a contract fufu does not speak may be shaped by rules
        // fufu has never seen, and parsing it to find that out would fail
        // and take the whole file down with it.
        let name = record.manifest.get("name").and_then(|value| value.as_str());
        let contract = record
            .manifest
            .get("contract")
            .and_then(|value| value.as_u64());
        let (Some(name), Some(contract)) = (name, contract) else {
            registry.entries.clear();
            registry.stale.clear();
            registry.unreadable = Some(
                "a record is missing its name or its contract, so it names no \
                      extension"
                    .into(),
            );
            return registry;
        };
        if contract != u64::from(crate::machine::CONTRACT) {
            registry.stale.push(Stale {
                name: name.to_string(),
                contract: contract.try_into().unwrap_or(u32::MAX),
            });
            continue;
        }
        match crate::manifest::parse(record.manifest) {
            Ok(manifest) => registry.entries.push(Declared {
                manifest,
                path: record.path,
                declared_at: record.declared_at,
            }),
            Err(err) => {
                registry.entries.clear();
                registry.stale.clear();
                registry.unreadable = Some(err.to_string());
                return registry;
            }
        }
    }
    registry
}

/// Record a manifest, or replace the record already under that name.
///
/// A re-declaration keeps its place in the order rather than moving to the
/// end: the order is the one subscribers are fanned out in and verbs are
/// listed in, and upgrading a binary is not a reordering.
pub fn declare(shook: &Handshake) -> Result<()> {
    declare_into(&writable()?, shook)
}

/// Take a name off the list. `false` when it was not on it, which is the
/// verb's to refuse rather than this module's.
pub fn forget(name: &str) -> Result<bool> {
    forget_from(&writable()?, name)
}

/// The two writers against a named file, which is the whole of them: the
/// public pair is this pair with the path resolved.
fn declare_into(file: &Path, shook: &Handshake) -> Result<()> {
    let mut records = for_writing(file)?;
    let fresh = Record {
        path: shook.path.clone(),
        declared_at: now_secs(),
        manifest: serde_json::to_value(&shook.manifest)
            .map_err(|err| Error::msg(format!("that manifest will not serialize: {err}")))?,
    };
    match records.iter().position(|r| named(r) == shook.manifest.name) {
        Some(at) => records[at] = fresh,
        None => records.push(fresh),
    }
    write(file, &records)
}

fn forget_from(file: &Path, name: &str) -> Result<bool> {
    let mut records = for_writing(file)?;
    let before = records.len();
    records.retain(|record| named(record) != name);
    if records.len() == before {
        return Ok(false);
    }
    write(file, &records)?;
    Ok(true)
}

/// The file on disk, exactly as it stands.
///
/// [`load`] is the reader's door and drops what this fufu will not describe;
/// a write that went through it would drop those records from the file too,
/// so one upgrade and one `ff extension add` would silently unregister
/// somebody else's extension.
fn for_writing(file: &Path) -> Result<Vec<Record>> {
    raw(file).map_err(|why| {
        Error::coded(
            "extension/registry-unreadable",
            format!("{} is not a registry fufu can read: {why}", file.display()),
            vec!["ff doctor".into()],
        )
    })
}

fn writable() -> Result<PathBuf> {
    path().ok_or_else(|| {
        Error::coded(
            "extension/registry-unwritable",
            "there is nowhere to record a declaration: nothing in the environment names a \
             config directory",
            vec!["ff doctor".into()],
        )
    })
}

/// Read and shape-check the file, with the reason as text on failure. A
/// file that is not there is the empty registry, so it is `Ok` and not an
/// absence anyone below has to spell.
fn raw(file: &Path) -> std::result::Result<Vec<Record>, String> {
    let body = match std::fs::read(file) {
        Ok(body) => body,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.to_string()),
    };
    let file: File = serde_json::from_slice(&body).map_err(|err| err.to_string())?;
    Ok(file.extensions)
}

fn write(file: &Path, records: &[Record]) -> Result<()> {
    let failed = |err: std::io::Error| {
        Error::coded(
            "extension/registry-unwritable",
            format!("{} could not be written: {err}", file.display()),
            vec!["ff doctor".into()],
        )
    };

    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(failed)?;
    }
    let mut body = serde_json::to_string_pretty(&File {
        ff: crate::machine::CONTRACT,
        extensions: records.to_vec(),
    })
    .map_err(|err| Error::msg(format!("the registry will not serialize: {err}")))?;
    body.push('\n');

    // Temp file and rename, as the update check's state file is written: a
    // reader on the agent's critical path must never see half a registry.
    let tmp = file.with_extension("json.ff-tmp");
    std::fs::write(&tmp, &body).map_err(failed)?;
    std::fs::rename(&tmp, file).map_err(failed)?;
    Ok(())
}

/// The file's own shape, and the record's, which is the manifest untyped:
/// what the writers rewrite is what they read, so a record this fufu will
/// not describe still survives somebody else's `ff extension add`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct File {
    ff: u32,
    extensions: Vec<Record>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Record {
    path: PathBuf,
    declared_at: i64,
    manifest: serde_json::Value,
}

fn named(record: &Record) -> &str {
    record
        .manifest
        .get("name")
        .and_then(|name| name.as_str())
        .unwrap_or_default()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A manifest of the smallest shape, under whatever name and contract
    /// the test needs.
    fn manifest(name: &str, contract: u32) -> Manifest {
        crate::manifest::parse(serde_json::json!({
            "name": name,
            "version": "0.4.1",
            "contract": contract,
            "verbs": [{"name": "board", "read_only": true}],
            "undoable": true,
        }))
        .expect("a manifest the page types")
    }

    fn shook(name: &str) -> Handshake {
        Handshake {
            path: PathBuf::from(format!("/usr/local/bin/ff-{name}")),
            manifest: manifest(name, crate::machine::CONTRACT),
        }
    }

    /// A registry file in a fresh directory the caller keeps alive, written
    /// verbatim so a test can put something in it fufu would never write.
    fn registry_file(body: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let file = dir.path().join("fufu").join(FILE);
        std::fs::create_dir_all(file.parent().expect("parent")).expect("create fufu dir");
        std::fs::write(&file, body).expect("write registry");
        (dir, file)
    }

    /// The file a `declare` would have written for these names, in order.
    fn recorded(names: &[&str]) -> String {
        let records: Vec<Record> = names
            .iter()
            .map(|name| Record {
                path: PathBuf::from(format!("/usr/local/bin/ff-{name}")),
                declared_at: 1788462398,
                manifest: serde_json::to_value(manifest(name, crate::machine::CONTRACT))
                    .expect("serialize"),
            })
            .collect();
        serde_json::to_string_pretty(&File {
            ff: crate::machine::CONTRACT,
            extensions: records,
        })
        .expect("serialize")
    }

    /// An absent registry is the normal empty case, not trouble: most
    /// machines declare nothing, and no file is how they say so.
    #[test]
    fn an_absent_registry_is_empty_and_quiet() {
        let dir = tempfile::TempDir::new().expect("create tempdir");
        let registry = load(Some(&dir.path().join("fufu").join(FILE)));
        assert!(registry.is_empty());
        assert!(registry.stale.is_empty());
        assert_eq!(registry.unreadable, None);
        assert!(registry.path.is_some());
    }

    /// No config directory at all is the same empty answer, and not one a
    /// reader has to handle differently.
    #[test]
    fn no_config_directory_is_the_same_empty_answer() {
        let registry = load(None);
        assert!(registry.is_empty());
        assert_eq!(registry.unreadable, None);
        assert_eq!(registry.path, None);
    }

    #[test]
    fn a_populated_registry_reads_in_the_order_it_was_written() {
        let (_dir, file) = registry_file(&recorded(&["tower", "bay"]));
        let registry = load(Some(&file));
        let names: Vec<&str> = registry.declared().iter().map(Declared::name).collect();
        assert_eq!(names, ["tower", "bay"]);
        assert_eq!(registry.unreadable, None);
        assert_eq!(registry.get("bay").expect("bay").manifest.version, "0.4.1");
        assert_eq!(
            registry.get("tower").expect("tower").path,
            PathBuf::from("/usr/local/bin/ff-tower")
        );
        assert!(registry.get("nothing-declared-this").is_none());
    }

    /// A corrupt registry describes nothing, and says why. The allowlist
    /// failing open would serve verbs nobody declared.
    #[test]
    fn a_corrupt_registry_is_empty_and_says_so() {
        for body in [
            "",
            "{",
            "not json at all",
            "[]",
            r#"{"ff":1}"#,
            r#"{"ff":1,"extensions":{}}"#,
            // A record with no manifest, and one whose manifest names no
            // name — neither is a record naming an extension.
            r#"{"ff":1,"extensions":[{"path":"/bin/ff-tower","declared_at":1}]}"#,
            r#"{"ff":1,"extensions":[{"path":"/bin/ff-tower","declared_at":1,"manifest":{"contract":1}}]}"#,
            // A record claiming this contract and shaped like nothing this
            // contract types.
            r#"{"ff":1,"extensions":[{"path":"/bin/ff-tower","declared_at":1,
                "manifest":{"name":"tower","contract":1}}]}"#,
        ] {
            let (_dir, file) = registry_file(body);
            let registry = load(Some(&file));
            assert!(registry.is_empty(), "{body}");
            assert!(registry.unreadable.is_some(), "{body}");
        }
    }

    /// One bad record costs the whole file, on the rule a half-declared
    /// extension is refused whole: the good records go with it rather than
    /// leaving fufu describing some of what the file says.
    #[test]
    fn one_unreadable_record_costs_the_whole_file() {
        let good = recorded(&["tower"]);
        let mut file: serde_json::Value = serde_json::from_str(&good).expect("parse");
        file["extensions"][0]["manifest"]["verbs"] = serde_json::json!([]);
        let (_dir, path) = registry_file(&file.to_string());
        let registry = load(Some(&path));
        assert!(registry.is_empty());
        assert!(registry.unreadable.is_some());
    }

    /// A record from a contract fufu does not speak is not described and is
    /// not lost: doctor is told, and nothing else has to check.
    #[test]
    fn a_record_from_another_contract_is_stale_and_not_described() {
        let mut file: serde_json::Value =
            serde_json::from_str(&recorded(&["tower", "bay"])).expect("parse");
        file["extensions"][0]["manifest"]["contract"] = serde_json::json!(99);
        // Shaped by rules this fufu has never seen, and read anyway.
        file["extensions"][0]["manifest"]["verbs"] = serde_json::json!("board, done");
        let (_dir, path) = registry_file(&file.to_string());

        let registry = load(Some(&path));
        let names: Vec<&str> = registry.declared().iter().map(Declared::name).collect();
        assert_eq!(names, ["bay"]);
        assert!(registry.get("tower").is_none());
        assert_eq!(registry.unreadable, None);
        assert_eq!(registry.stale.len(), 1);
        assert_eq!(registry.stale[0].name, "tower");
        assert_eq!(registry.stale[0].contract, 99);
    }

    /// A binary that has left PATH is a `None` from `resolve` and nothing
    /// else: the record stays, so doctor can report it.
    #[test]
    fn a_record_whose_binary_left_path_still_reads() {
        let (_dir, file) = registry_file(&recorded(&["nothing-on-path-answers-to-this"]));
        let registry = load(Some(&file));
        let declared = registry.declared().first().expect("the record");
        assert_eq!(declared.name(), "nothing-on-path-answers-to-this");
        assert_eq!(declared.resolve(), None);
    }

    /// The writers' round trip, against a real config root.
    /// The writers, which the public pair is these two with the path
    /// resolved out of the environment.
    mod writing {
        use super::*;

        fn declared(file: &Path, name: &str) {
            declare_into(file, &shook(name)).expect("declare");
        }

        #[test]
        fn what_is_declared_is_what_reads_back() {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            let file = dir.path().join("fufu").join(FILE);

            declared(&file, "tower");
            declared(&file, "bay");

            let registry = load(Some(&file));
            let names: Vec<&str> = registry.declared().iter().map(Declared::name).collect();
            assert_eq!(names, ["tower", "bay"]);
            let tower = registry.get("tower").expect("tower");
            assert_eq!(tower.path, PathBuf::from("/usr/local/bin/ff-tower"));
            assert_eq!(tower.manifest.contract, crate::machine::CONTRACT);
            assert!(tower.declared_at > 0);
        }

        /// Re-declaring replaces the record and keeps its place, because the
        /// order is what subscribers are fanned out in.
        #[test]
        fn re_declaring_keeps_its_place_in_the_order() {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            let file = dir.path().join("fufu").join(FILE);

            declared(&file, "tower");
            declared(&file, "bay");
            let mut upgraded = shook("tower");
            upgraded.manifest.version = "0.5.0".into();
            declare_into(&file, &upgraded).expect("re-declare");

            let registry = load(Some(&file));
            let names: Vec<&str> = registry.declared().iter().map(Declared::name).collect();
            assert_eq!(names, ["tower", "bay"]);
            assert_eq!(
                registry.get("tower").expect("tower").manifest.version,
                "0.5.0"
            );
        }

        #[test]
        fn forgetting_takes_one_name_off_and_leaves_the_rest() {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            let file = dir.path().join("fufu").join(FILE);

            declared(&file, "tower");
            declared(&file, "bay");
            assert!(forget_from(&file, "tower").expect("forget"));
            assert!(!forget_from(&file, "tower").expect("forget again"));

            let registry = load(Some(&file));
            let names: Vec<&str> = registry.declared().iter().map(Declared::name).collect();
            assert_eq!(names, ["bay"]);
        }

        /// A record this fufu will not describe survives somebody else's
        /// declaration, because a write rewrites what it read rather than
        /// what the reader kept.
        #[test]
        fn a_stale_record_survives_a_write() {
            let mut seeded: serde_json::Value =
                serde_json::from_str(&recorded(&["tower"])).expect("parse");
            seeded["extensions"][0]["manifest"]["contract"] = serde_json::json!(99);
            let (_dir, file) = registry_file(&seeded.to_string());

            declared(&file, "bay");

            let registry = load(Some(&file));
            let names: Vec<&str> = registry.declared().iter().map(Declared::name).collect();
            assert_eq!(names, ["bay"]);
            assert_eq!(registry.stale.len(), 1);
            assert_eq!(registry.stale[0].name, "tower");
        }

        /// A file fufu cannot read is not a file fufu overwrites: a person
        /// hand-edited it, and clobbering it would lose whatever they meant.
        #[test]
        fn a_corrupt_registry_is_not_written_over() {
            let (_dir, file) = registry_file("{ this was hand-edited");
            let err = for_writing(&file).expect_err("corrupt");
            assert_eq!(err.id(), "extension/registry-unreadable");
            assert_eq!(
                std::fs::read_to_string(&file).expect("still there"),
                "{ this was hand-edited"
            );
        }

        /// The temp file is renamed over the registry, and nothing is left
        /// beside it.
        #[test]
        fn a_write_leaves_no_temp_file_behind() {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            let file = dir.path().join("fufu").join(FILE);
            declared(&file, "tower");

            let beside: Vec<String> = std::fs::read_dir(file.parent().expect("parent"))
                .expect("read dir")
                .map(|entry| {
                    entry
                        .expect("entry")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            assert_eq!(beside, [FILE]);
        }

        /// The file a person opens is one they can read, and it ends in a
        /// newline the way every other file fufu writes does.
        #[test]
        fn the_file_is_pretty_printed() {
            let dir = tempfile::TempDir::new().expect("create tempdir");
            let file = dir.path().join("fufu").join(FILE);
            declared(&file, "tower");

            let body = std::fs::read_to_string(&file).expect("read");
            assert!(body.ends_with("}\n"), "{body}");
            assert!(body.lines().count() > 5, "{body}");
            assert!(body.starts_with("{\n  \"ff\": 1,"), "{body}");
        }
    }
}
