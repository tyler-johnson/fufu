//! Floor 2 — the op journal. One commit per fufu mutation on
//! `refs/fufu/journal`: parent 1 is the previous entry (first-parent walk =
//! op log, `git log --first-parent` legible), parents 2..n are every commit
//! the entry references — reachability IS the gc pin. The entry tree holds
//! `op.json` (the machine record), `refs` (the full last-seen ref table,
//! packed-refs-shaped), and `index` (the index tree at that moment, pinned
//! by containment).
//!
//! Write-ahead discipline: a mutating verb appends its entry — refs table =
//! the PLANNED post-op state — before touching a single ref. The CAS append
//! doubles as the op serialization lock. A crash between append and mutation
//! leaves reality behind the plan; the next reconcile absorbs the difference
//! loudly ("op did not complete") instead of losing it.
//!
//! Reconciliation is lazy: at every invocation the current refs are diffed
//! against the tip entry's table. Foreign motion (the user ran real git)
//! becomes ONE `foreign` entry — states compared, never reconstructed. The
//! clean path writes nothing.

use std::collections::BTreeMap;

use gix::prelude::ObjectIdExt;
use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::{ForeignChange, OpEntry, ReconcileReport};
use crate::refs::{self, EditOutcome};
use crate::snapshot::chain;

pub const JOURNAL_REF: &str = "refs/fufu/journal";
/// Where a corrupt journal chain is parked before re-init.
pub const JOURNAL_TRASH_REF: &str = "refs/fufu/trash/@journal";
const RECORD_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpKind {
    /// A fufu mutation, journaled write-ahead.
    Op,
    /// External motion absorbed by reconciliation.
    Foreign,
    /// A non-pinning marker (init, trim).
    Note,
}

/// One ref's transition, full shas (or `None` for created/deleted ends).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefTransition {
    pub name: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// A stash-stack effect performed by the op.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StashEffect {
    Push { branch: String, stash: String },
    Drop { branch: String, stash: String },
}

/// A pending-description change (old/new text, `None` = absent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DescriptionTransition {
    pub branch: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// The machine record inside every journal entry (`op.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpRecord {
    pub version: u32,
    pub kind: OpKind,
    /// The fufu verb (`commit`, `switch`, …) or `reconcile`/`init`.
    pub verb: String,
    /// Human summary, doubling as the commit subject.
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argv: Vec<String>,
    pub time: i64,
    /// Branch (chain name) the op ran on, if on one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The capture-first snapshot taken before the op.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_snapshot: Option<String>,
    /// The pre-op index tree (also pinned as the entry's `index` subtree).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_tree: Option<String>,
    /// HEAD's transition: `ref:<name>` for symbolic, sha for detached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<(String, String)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<RefTransition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stash: Vec<StashEffect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<DescriptionTransition>,
    /// The journal entry this op undoes, when the verb is `undo`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub undo_of: Option<String>,
    /// The previous journal entry. Recorded explicitly because parent slot 1
    /// only holds the previous entry when one exists — the first entry's
    /// parent 1 is a pin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
}

impl OpRecord {
    pub fn new(
        kind: OpKind,
        verb: impl Into<String>,
        summary: impl Into<String>,
        time: i64,
    ) -> Self {
        OpRecord {
            version: RECORD_VERSION,
            kind,
            verb: verb.into(),
            summary: summary.into(),
            argv: Vec::new(),
            time,
            branch: None,
            pre_snapshot: None,
            index_tree: None,
            head: None,
            refs: Vec::new(),
            stash: Vec::new(),
            description: None,
            undo_of: None,
            prev: None,
        }
    }
}

/// The full last-seen ref table: HEAD plus every journal-tracked ref.
/// Remotes are deliberately excluded in Phase 2 (their churn stays silent).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RefsTable {
    /// `ref:<full-name>` while on a branch (born or not), sha when detached.
    pub head: String,
    /// Full ref name → sha, sorted by name.
    pub refs: BTreeMap<String, String>,
}

const TRACKED_PREFIXES: [&str; 3] = ["refs/heads/", "refs/tags/", "refs/fufu/parked/"];

/// Read the current tracked-ref state. Symbolic branch refs (rare) and
/// tag refs are recorded by their direct target.
pub fn observe_refs(repo: &gix::Repository) -> Result<RefsTable> {
    let mut table = RefsTable::default();
    let head = repo.head().map_err(Error::repo)?;
    table.head = match head.kind {
        gix::head::Kind::Unborn(name) => format!("ref:{}", name.as_bstr()),
        gix::head::Kind::Detached { target, peeled } => peeled.unwrap_or(target).to_string(),
        gix::head::Kind::Symbolic(reference) => format!("ref:{}", reference.name.as_bstr()),
    };
    let platform = repo.references().map_err(Error::repo)?;
    for prefix in TRACKED_PREFIXES {
        let iter = platform.prefixed(prefix).map_err(Error::repo)?;
        for reference in iter {
            let reference =
                reference.map_err(|err| Error::msg(format!("ref iteration failed: {err}")))?;
            let name = reference.name().as_bstr().to_string();
            if let Some(id) = reference.target().try_id() {
                table.refs.insert(name, id.to_string());
            }
        }
    }
    if let Some(stash) = refs::ref_target(repo, "refs/stash")? {
        table.refs.insert("refs/stash".into(), stash.to_string());
    }
    Ok(table)
}

impl RefsTable {
    /// Serialize as the `refs` blob: `HEAD` line first, then `<sha> <name>`
    /// sorted by name — packed-refs-shaped, one state per line.
    pub fn to_blob(&self) -> String {
        let mut out = format!("{} HEAD\n", self.head);
        for (name, sha) in &self.refs {
            out.push_str(&format!("{sha} {name}\n"));
        }
        out
    }

    pub fn from_blob(text: &str) -> Result<Self> {
        let mut table = RefsTable::default();
        for line in text.lines() {
            let Some((value, name)) = line.split_once(' ') else {
                return Err(Error::msg(format!("malformed refs line: {line:?}")));
            };
            if name == "HEAD" {
                table.head = value.to_string();
            } else {
                table.refs.insert(name.to_string(), value.to_string());
            }
        }
        if table.head.is_empty() {
            return Err(Error::msg("refs table has no HEAD line"));
        }
        Ok(table)
    }

    /// The differences carrying `self` (expected) to `current` (observed).
    pub fn diff(&self, current: &RefsTable) -> Vec<ForeignChange> {
        let mut out = Vec::new();
        if self.head != current.head {
            out.push(ForeignChange {
                name: "HEAD".into(),
                old: Some(self.head.clone()),
                new: Some(current.head.clone()),
                hint: None,
            });
        }
        let names: std::collections::BTreeSet<&String> =
            self.refs.keys().chain(current.refs.keys()).collect();
        for name in names {
            let old = self.refs.get(name);
            let new = current.refs.get(name);
            if old != new {
                out.push(ForeignChange {
                    name: name.to_string(),
                    old: old.cloned(),
                    new: new.cloned(),
                    hint: None,
                });
            }
        }
        out
    }
}

/// A decoded journal entry.
#[derive(Debug, Clone)]
pub struct Entry {
    pub id: gix::ObjectId,
    pub record: OpRecord,
    pub refs: RefsTable,
    pub index_tree: gix::ObjectId,
    /// Parent 1 — the previous entry, if any.
    pub prev: Option<gix::ObjectId>,
}

/// The current journal tip, if the journal exists.
pub fn tip(repo: &gix::Repository) -> Result<Option<gix::ObjectId>> {
    refs::ref_target(repo, JOURNAL_REF)
}

/// Decode one journal entry commit.
pub fn read_entry(repo: &gix::Repository, id: gix::ObjectId) -> Result<Entry> {
    let obj = repo.find_object(id).map_err(Error::repo)?;
    if obj.kind != gix::objs::Kind::Commit {
        return Err(Error::msg(format!("{id} is not a commit")));
    }
    let commit = gix::objs::CommitRef::from_bytes(&obj.data).map_err(Error::repo)?;
    if !chain::is_snapshot_commit(&commit) {
        return Err(Error::msg(format!("{id} does not bear the fufu identity")));
    }
    let tree_id = commit.tree();
    drop(commit);
    drop(obj);

    let tree = repo.find_tree(tree_id).map_err(Error::repo)?;
    let mut op_blob = None;
    let mut refs_blob = None;
    let mut index_tree = None;
    for entry in tree.iter() {
        let entry = entry.map_err(Error::repo)?;
        match entry.filename().to_string().as_str() {
            "op.json" => op_blob = Some(entry.object_id()),
            "refs" => refs_blob = Some(entry.object_id()),
            "index" => index_tree = Some(entry.object_id()),
            _ => {}
        }
    }
    let op_blob =
        op_blob.ok_or_else(|| Error::msg(format!("journal entry {id} has no op.json")))?;
    let refs_blob =
        refs_blob.ok_or_else(|| Error::msg(format!("journal entry {id} has no refs blob")))?;
    let index_tree =
        index_tree.ok_or_else(|| Error::msg(format!("journal entry {id} has no index tree")))?;

    let op_data = repo.find_object(op_blob).map_err(Error::repo)?.detach();
    let record: OpRecord = serde_json::from_slice(&op_data.data)
        .map_err(|err| Error::msg(format!("journal entry {id}: bad op.json: {err}")))?;
    let refs_data = repo.find_object(refs_blob).map_err(Error::repo)?.detach();
    let refs_text = std::str::from_utf8(&refs_data.data)
        .map_err(|err| Error::msg(format!("journal entry {id}: refs blob not utf-8: {err}")))?;
    let refs = RefsTable::from_blob(refs_text)?;

    let prev = record
        .prev
        .as_deref()
        .map(|hex| gix::ObjectId::from_hex(hex.as_bytes()).map_err(Error::repo))
        .transpose()?;
    Ok(Entry {
        id,
        record,
        refs,
        index_tree,
        prev,
    })
}

/// Append one entry. `table` is the state the world SHOULD be in once the op
/// completes (for `foreign`/`note` entries: the observed present). `pins`
/// are commit shas to keep reachable; non-commits are peeled or dropped with
/// a warning. CAS against the current tip; contention re-reads and retries
/// (≤3) — the append is the op lock, and losing it twice in a row means
/// something else is journaling faster than we can observe.
pub fn append(
    repo: &gix::Repository,
    record: &OpRecord,
    table: &RefsTable,
    index_tree: gix::ObjectId,
    pins: &[gix::ObjectId],
    now: i64,
) -> Result<gix::ObjectId> {
    let refs_blob = repo
        .write_blob(table.to_blob().as_bytes())
        .map_err(Error::repo)?
        .detach();

    let mut pin_parents: Vec<gix::ObjectId> = Vec::new();
    for pin in pins {
        if let Some(commit) = peel_to_commit(repo, *pin)
            && !pin_parents.contains(&commit)
        {
            pin_parents.push(commit);
        }
    }

    let sig = gix::actor::Signature {
        name: chain::FUFU_NAME.into(),
        email: chain::FUFU_EMAIL.into(),
        time: gix::date::Time {
            seconds: now,
            offset: 0,
        },
    };
    let subject = &record.summary;
    let kind = match record.kind {
        OpKind::Op => "op",
        OpKind::Foreign => "foreign",
        OpKind::Note => "note",
    };
    let message = format!("{kind}: {subject}\n");

    for attempt in 0..3 {
        let prev = tip(repo)?;

        // op.json carries the explicit prev link, so it is serialized per
        // attempt — the tip may have moved between retries.
        let mut attempt_record = record.clone();
        attempt_record.prev = prev.map(|p| p.to_string());
        let op_json = serde_json::to_vec_pretty(&attempt_record)
            .map_err(|err| Error::msg(err.to_string()))?;
        let op_blob = repo.write_blob(&op_json).map_err(Error::repo)?.detach();
        use gix::objs::tree::{Entry as TreeEntry, EntryKind};
        let tree = gix::objs::Tree {
            entries: vec![
                TreeEntry {
                    mode: EntryKind::Tree.into(),
                    filename: "index".into(),
                    oid: index_tree,
                },
                TreeEntry {
                    mode: EntryKind::Blob.into(),
                    filename: "op.json".into(),
                    oid: op_blob,
                },
                TreeEntry {
                    mode: EntryKind::Blob.into(),
                    filename: "refs".into(),
                    oid: refs_blob,
                },
            ],
        };
        let tree_id = repo.write_object(&tree).map_err(Error::repo)?.detach();

        let mut parents: Vec<gix::ObjectId> = Vec::new();
        parents.extend(prev);
        for pin in &pin_parents {
            if !parents.contains(pin) {
                parents.push(*pin);
            }
        }
        let commit = gix::objs::Commit {
            tree: tree_id,
            parents: parents.into(),
            author: sig.clone(),
            committer: sig.clone(),
            encoding: None,
            message: message.clone().into(),
            extra_headers: Vec::new(),
        };
        let commit_id = repo.write_object(&commit).map_err(Error::repo)?.detach();
        let expected = match prev {
            Some(p) => gix::refs::transaction::PreviousValue::MustExistAndMatch(
                gix::refs::Target::Object(p),
            ),
            None => gix::refs::transaction::PreviousValue::MustNotExist,
        };
        let edit = refs::update_edit(JOURNAL_REF, commit_id, expected, &record.summary)?;
        match refs::commit_edits(repo, Some(edit), now)? {
            EditOutcome::Applied => return Ok(commit_id),
            EditOutcome::Contended if attempt < 2 => continue,
            EditOutcome::Contended => {
                return Err(Error::msg(
                    "journal is contended: another fufu operation is in progress",
                ));
            }
        }
    }
    unreachable!("loop returns on every path");
}

/// Pin candidates must be commits: tags peel, everything else drops.
fn peel_to_commit(repo: &gix::Repository, id: gix::ObjectId) -> Option<gix::ObjectId> {
    let obj = repo.find_object(id).ok()?;
    match obj.kind {
        gix::objs::Kind::Commit => Some(id),
        gix::objs::Kind::Tag => obj.peel_to_kind(gix::objs::Kind::Commit).ok().map(|o| o.id),
        _ => None,
    }
}

/// Reconcile the journal with reality. Returns the report; appends at most
/// one entry (`foreign` on divergence, `init` note on bootstrap). The clean
/// path writes nothing.
pub fn reconcile(repo: &gix::Repository, now: i64) -> Result<ReconcileReport> {
    let observed = observe_refs(repo)?;
    let mut report = ReconcileReport {
        bootstrapped: false,
        reinitialized: false,
        foreign: Vec::new(),
        entry: None,
        warnings: Vec::new(),
    };

    let tip_entry = match tip(repo)? {
        None => None,
        Some(id) => match read_entry(repo, id) {
            Ok(entry) => Some(entry),
            Err(err) => {
                // Corrupt tip: park the whole chain, then re-init. The old
                // chain stays reachable (and inspectable) from trash.
                report.reinitialized = true;
                report.warnings.push(format!(
                    "journal tip unreadable ({err}); chain parked at {JOURNAL_TRASH_REF}"
                ));
                refs::write_ref(
                    repo,
                    JOURNAL_TRASH_REF,
                    id,
                    gix::refs::transaction::PreviousValue::Any,
                    now,
                    "reconcile: parked corrupt journal",
                )?;
                refs::delete_ref(repo, JOURNAL_REF, id, now)?;
                None
            }
        },
    };

    match tip_entry {
        None => {
            // Bootstrap: journal the observed state as the new floor.
            // Anything before this moment is no longer undoable.
            report.bootstrapped = true;
            let mut record = OpRecord::new(
                OpKind::Note,
                "init",
                "journal initialized from observed state; earlier operations not undoable",
                now,
            );
            record.branch = current_branch(&observed);
            let index_tree = crate::index::tree_from_index(repo)?;
            let pins = table_pins(&observed);
            let id = append(repo, &record, &observed, index_tree, &pins, now)?;
            report.entry = Some(id.to_string());
            Ok(report)
        }
        Some(entry) => {
            let mut foreign = entry.refs.diff(&observed);
            if foreign.is_empty() {
                return Ok(report); // clean: write nothing
            }
            // Divergence. Quote git's own reflog messages as hints, and
            // flag the write-ahead crash case: an op entry whose planned
            // state never materialized.
            let incomplete = entry.record.kind == OpKind::Op
                && foreign.iter().all(|change| {
                    // Reality still holds the op's OLD value: it never ran.
                    change.name == "HEAD"
                        || entry
                            .record
                            .refs
                            .iter()
                            .any(|t| t.name == change.name && t.old == change.new)
                });
            for change in &mut foreign {
                change.hint = reflog_hint(repo, &change.name, change.new.as_deref());
            }
            let summary = if incomplete {
                format!(
                    "absorbed {} ref change(s); previous op may not have completed",
                    foreign.len()
                )
            } else {
                format!("absorbed {} foreign ref change(s)", foreign.len())
            };
            let mut record = OpRecord::new(OpKind::Foreign, "reconcile", summary, now);
            record.branch = current_branch(&observed);
            record.refs = foreign
                .iter()
                .map(|change| RefTransition {
                    name: change.name.clone(),
                    old: change.old.clone(),
                    new: change.new.clone(),
                })
                .collect();
            record.head = head_transition(&entry.refs, &observed);
            let index_tree = crate::index::tree_from_index(repo)?;
            let mut pins: Vec<gix::ObjectId> = Vec::new();
            for change in &foreign {
                for sha in [&change.old, &change.new].into_iter().flatten() {
                    match gix::ObjectId::from_hex(sha.as_bytes()) {
                        Ok(id) if repo.try_find_object(id).ok().flatten().is_some() => {
                            pins.push(id);
                        }
                        Ok(_) => report.warnings.push(format!(
                            "{}: {sha} is gone from the object store; not pinnable",
                            change.name
                        )),
                        Err(_) => {}
                    }
                }
            }
            let id = append(repo, &record, &observed, index_tree, &pins, now)?;
            report.entry = Some(id.to_string());
            report.foreign = foreign;
            Ok(report)
        }
    }
}

fn current_branch(table: &RefsTable) -> Option<String> {
    table
        .head
        .strip_prefix("ref:refs/heads/")
        .map(|s| s.to_string())
}

fn head_transition(old: &RefsTable, new: &RefsTable) -> Option<(String, String)> {
    (old.head != new.head).then(|| (old.head.clone(), new.head.clone()))
}

/// Every sha a table references, as pin candidates.
fn table_pins(table: &RefsTable) -> Vec<gix::ObjectId> {
    let mut pins = Vec::new();
    for sha in table.refs.values() {
        if let Ok(id) = gix::ObjectId::from_hex(sha.as_bytes()) {
            pins.push(id);
        }
    }
    if let Ok(id) = gix::ObjectId::from_hex(table.head.as_bytes()) {
        pins.push(id);
    }
    pins
}

/// The newest reflog message on `name` whose new value matches `target` —
/// git's own words about what happened, best effort.
fn reflog_hint(repo: &gix::Repository, name: &str, target: Option<&str>) -> Option<String> {
    let target = target?;
    let reference = repo.try_find_reference(name).ok()??;
    let mut platform = reference.log_iter();
    let iter = platform.rev().ok()??;
    for line in iter.flatten() {
        if line.new_oid.to_string() == target {
            let msg = line.message.to_string();
            if !msg.is_empty() {
                return Some(msg);
            }
            break;
        }
    }
    None
}

/// First-parent walk of the journal, newest first, as display entries.
pub fn read_ops(repo: &gix::Repository, limit: usize) -> Result<Vec<OpEntry>> {
    let mut out = Vec::new();
    let mut cur = tip(repo)?;
    while let Some(id) = cur {
        if limit != 0 && out.len() >= limit {
            break;
        }
        let entry = match read_entry(repo, id) {
            Ok(entry) => entry,
            Err(_) => break, // damaged history: show what is legible
        };
        let short_id = id
            .attach(repo)
            .shorten()
            .map(|p| p.to_string())
            .unwrap_or_else(|_| id.to_string()[..7].to_string());
        out.push(OpEntry {
            id: id.to_string(),
            short_id,
            kind: match entry.record.kind {
                OpKind::Op => "op".into(),
                OpKind::Foreign => "foreign".into(),
                OpKind::Note => "note".into(),
            },
            verb: entry.record.verb.clone(),
            summary: entry.record.summary.clone(),
            time: entry.record.time,
            branch: entry.record.branch.clone(),
            undo_of: entry.record.undo_of.clone(),
        });
        cur = entry.prev;
    }
    Ok(out)
}

/// The shared preamble of every mutating verb: capture-first snapshot
/// (mandatory — contention aborts), then reconcile. Nothing the verb does
/// afterwards can orphan state that isn't already on the timeline and
/// journal-pinned.
pub struct VerbContext {
    pub now: i64,
    /// The pre-verb snapshot, when one was created (clean tree = none).
    pub pre_snapshot: Option<String>,
    pub reconcile: ReconcileReport,
}

pub fn begin_verb(
    repo: &gix::Repository,
    prov: &crate::snapshot::Provenance,
    now: Option<i64>,
) -> Result<VerbContext> {
    let now = now.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });
    let pre = crate::snapshot::take_with(
        repo,
        prov,
        &crate::snapshot::TakeOptions {
            now: Some(now),
            max_file_size: None,
        },
    )?;
    let pre_snapshot = match pre {
        crate::model::SnapOutcome::Created { id, .. } => Some(id),
        crate::model::SnapOutcome::NoOp { .. } => None,
        crate::model::SnapOutcome::Contended { .. } => {
            return Err(Error::msg(
                "a concurrent ff snapshot is in progress; aborted (nothing was written)",
            ));
        }
    };
    let reconcile = reconcile(repo, now)?;
    Ok(VerbContext {
        now,
        pre_snapshot,
        reconcile,
    })
}

/// Resolve an op-id prefix against the journal chain.
pub fn resolve_op_prefix(repo: &gix::Repository, prefix: &str) -> Result<gix::ObjectId> {
    let mut matches = Vec::new();
    let mut cur = tip(repo)?;
    while let Some(id) = cur {
        if id.to_string().starts_with(prefix) {
            matches.push(id);
        }
        let Ok(entry) = read_entry(repo, id) else {
            break;
        };
        cur = entry.prev;
    }
    match matches.as_slice() {
        [] => Err(Error::msg(format!("no journal entry matching {prefix}"))),
        [one] => Ok(*one),
        many => {
            let list: Vec<String> = many.iter().map(|id| id.to_string()).collect();
            Err(Error::msg(format!(
                "ambiguous op prefix {prefix}: {}",
                list.join(", ")
            )))
        }
    }
}
