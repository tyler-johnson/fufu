//! The operation id index: one on-disk, sorted file of op ids per domain.
//!
//! The uniqueness domain for prefix highlighting is exactly the set `ff op`
//! resolves against — the log, live and trashed. Recomputing it means
//! decoding the whole log on every `ff log`. So materialize it: a sorted id
//! file, appended by every append, caught up whenever the tip has moved out
//! from under it. Derived and disposable — deleting it costs one rebuild,
//! never data.
//!
//! Two things differ from the per-chain index this was lifted from, and both
//! are principle 13. A tip mismatch used to mean a full rebuild: a
//! multi-second walk of the whole log, inside `ff log`. It is now a bounded
//! catch-up that walks *back* for the recorded tip and holds the difference
//! in memory, falling back to a rebuild only when the recorded tip is not
//! within reach. And the tail merge — a sequential rewrite of the entire
//! file, 22 MB at half a million ids — moved off the read path onto
//! [`record`], where a write was happening anyway.
//!
//! `MERGE_TAIL` stays 512 regardless of how large the domain grows: making
//! it proportional would turn the constant tail scan into a linear one, and
//! the constant is the whole point. The accepted consequence of one global
//! id space is that unique prefixes lengthen from three or four letters to
//! about five.
//!
//! **One file per chain, one resolution domain across all of them.** A
//! worktree writes only its own chain's file, which is what keeps a bay's
//! appends from invalidating main's index and sending every `ff log` there
//! into a rebuild. Resolution reads every chain's file and takes the union,
//! so ids stay unique across the repository and a removed worktree's
//! operations still resolve — `ff op show` and `ff restore --at-op` reach a
//! dead bay's captures with no new grammar, which is the whole reason the
//! domain is global rather than per-chain.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error::Result;

const HEADER_LEN: usize = 50;
const RECORD_LEN: usize = 41;
const MERGE_TAIL: usize = 512;

/// How far back the catch-up walk will look for the recorded tip before
/// giving up and rebuilding. Generous next to any plausible gap (a few ops
/// per command, a burst of captures from an agent) and still a bound.
const CATCHUP_CAP: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Live,
    Trash,
}

impl Kind {
    fn file(self) -> &'static str {
        match self {
            Kind::Live => "live",
            Kind::Trash => "trash",
        }
    }

    /// The ref one chain keeps this domain's tip on.
    fn ref_name(self, chain: &str) -> String {
        match self {
            Kind::Live => crate::ops::ops_ref(chain),
            Kind::Trash => crate::ops::ops_trash_ref(chain),
        }
    }
}

/// A materialized domain, open for reading: the file when there is one, an
/// in-memory sorted list when the file could not be written, plus whatever
/// the catch-up walk found beyond the file's recorded tip.
#[derive(Default)]
struct Index {
    file: Option<File>,
    mem: Vec<String>,
    base: usize,
    total: usize,
    /// Ops appended since the file's header was written. Unsorted, bounded
    /// by `CATCHUP_CAP`, scanned exactly like the file's own tail.
    extra: Vec<String>,
    /// The file's own unsorted tail, read whole on first use.
    ///
    /// Bounded by `MERGE_TAIL`, so this is at most ~21 KiB — and reading it
    /// in one go is the difference between a constant *count* and a constant
    /// *cost*. Scanning it through [`Index::at`] meant a seek and a read per
    /// record, which is five hundred syscalls per id and twelve thousand for
    /// a screen of twenty-five: `ff evolog` measured 19ms against a 4ms
    /// floor, almost all of it system time, and grew with the log right up
    /// to the point the tail merges.
    tail: Option<Vec<String>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    InSync { ids: usize },
    Stale,
    Absent,
}

/// The on-disk path for this worktree's chain's id index file.
pub fn path(repo: &gix::Repository, kind: Kind) -> PathBuf {
    chain_path(repo, &crate::ops::chain_id(repo), kind)
}

/// The on-disk path for any chain's id index file.
pub fn chain_path(repo: &gix::Repository, chain: &str, kind: Kind) -> PathBuf {
    repo.common_dir()
        .join("fufu/ops")
        .join(chain)
        .join(kind.file())
}

/// Open the index for a domain, catching it up in memory if the tip moved.
/// IO errors are swallowed — the worst case is an in-memory index, which is
/// the behavior of a repository that has never written one.
fn ensure(repo: &gix::Repository, chain: &str, kind: Kind) -> Result<Index> {
    let Some(tip) = crate::refs::ref_target(repo, &kind.ref_name(chain))? else {
        return Ok(Index::default());
    };
    let tip_str = tip.to_string();
    let file_path = chain_path(repo, chain, kind);

    // In sync: the common case, and it costs 50 bytes and a stat.
    if let Ok(idx) = try_open_verified(&file_path, &tip_str) {
        return Ok(idx);
    }

    // Behind: walk back from the current tip for the recorded one. What the
    // walk finds stays in memory — a read that is merely behind has no
    // business rewriting the file, and the next `record` lands it anyway.
    if let Ok(recorded) = recorded_tip(&file_path)
        && let Some(extra) = catch_up(repo, tip, &recorded)
        && let Ok(mut idx) = try_open_verified(&file_path, &recorded)
    {
        idx.extra = extra;
        return Ok(idx);
    }

    // Out of reach: rebuild from the log walk.
    let mut sorted = domain_ids(repo, chain, kind)?;
    sorted.sort_unstable();
    let _ = write_index(&file_path, &tip_str, &sorted);
    let len = sorted.len();
    Ok(Index {
        file: None,
        mem: sorted,
        base: len,
        total: len,
        extra: Vec::new(),
        tail: None,
    })
}

/// Recompute a domain and write it, replacing whatever was there.
///
/// The one caller is a pointer move. A rewind puts the tip somewhere
/// [`catch_up`] cannot reach forward from, and nothing appends afterwards to
/// land the file, so every later read would rebuild in line — a linear cost
/// on the read path, which is the one place that can never afford one. Paying
/// it once here keeps the flat rows flat. Best effort and silent, like every
/// other write to a derived cache.
pub fn refresh(repo: &gix::Repository, kind: Kind) {
    let chain = crate::ops::chain_id(repo);
    let Ok(Some(tip)) = crate::refs::ref_target(repo, &kind.ref_name(&chain)) else {
        return;
    };
    let Ok(mut ids) = domain_ids(repo, &chain, kind) else {
        return;
    };
    ids.sort_unstable();
    let _ = write_index(&chain_path(repo, &chain, kind), &tip.to_string(), &ids);
}

/// Every op id a domain resolves — the materialized form of exactly the set
/// `ff op` accepts, ids only and no abbreviation, which is what makes it
/// affordable over a whole log.
///
/// For the live log that is the walk from the tip *plus* what the ref's own
/// reflog still reaches. An undo steps the pointer back rather than appending,
/// so the operations it stepped off hang off a position only the reflog
/// records — and DESIGN keeps those addressable (`ff op restore` accepts an
/// abandoned id until trim ages it out), which makes them part of the domain
/// by definition rather than by kindness. Trim is what removes an operation
/// from it, and trim removes it honestly: the ref goes and its reflog with it,
/// so a dropped operation leaves the domain as well as the walk. Each seed
/// walk stops the moment it meets an id already collected, so the total work
/// is the size of the domain and not the number of seeds times its depth.
fn domain_ids(repo: &gix::Repository, chain: &str, kind: Kind) -> Result<Vec<String>> {
    let name = kind.ref_name(chain);
    let mut seeds: Vec<gix::ObjectId> = Vec::new();
    if let Some(tip) = crate::refs::ref_target(repo, &name)? {
        seeds.push(tip);
    }
    if kind == Kind::Live {
        for line in crate::refs::read_ref_log(repo, &name)? {
            seeds.push(line.new);
        }
    }

    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out = Vec::new();
    for seed in seeds {
        let mut cur = Some(seed);
        while let Some(id) = cur {
            let Ok(op) = crate::ops::walk::decode(repo, id) else {
                break; // damaged history: index what is legible
            };
            let id_str = id.to_string();
            if !seen.insert(id_str.clone()) {
                break; // this tail is already in the domain
            }
            out.push(id_str);
            cur = op.prev().map(|p| p.object_id());
        }
    }
    Ok(out)
}

/// Walk back from `tip` looking for `recorded`, collecting what lies between.
/// `None` when the recorded tip is not within `CATCHUP_CAP` headers — which
/// is a trim, an undo, or a hand-moved ref, all of which mean rebuild.
fn catch_up(repo: &gix::Repository, tip: gix::ObjectId, recorded: &str) -> Option<Vec<String>> {
    let mut extra = Vec::new();
    let mut cur = Some(tip);
    while let Some(id) = cur {
        let id_str = id.to_string();
        if id_str == recorded {
            return Some(extra);
        }
        if extra.len() >= CATCHUP_CAP {
            return None;
        }
        extra.push(id_str);
        cur = crate::ops::walk::decode(repo, id)
            .ok()?
            .prev()
            .map(|p| p.object_id());
    }
    None
}

/// The tip an existing index file claims, without reading its body.
fn recorded_tip(file_path: &Path) -> std::io::Result<String> {
    let mut file = File::open(file_path)?;
    let mut header = [0u8; HEADER_LEN];
    file.read_exact(&mut header)?;
    if header[40] != b' ' || header[49] != b'\n' {
        return Err(std::io::Error::other("index header sentinel corrupt"));
    }
    Ok(String::from_utf8_lossy(&header[..40]).into_owned())
}

/// The one verification rule, in one place: size alignment, header
/// sentinels, the tip, and the base count. It reads 50 bytes and a stat —
/// never the body, which is the difference between a seek and a scan.
///
/// The tip check is the load-bearing one. It catches a trim (which relinks
/// parents, so every surviving id changes), an undo, a hand-moved ref, and a
/// crash between the record write and the header write. The alignment checks
/// catch a torn append. Together they are why this file needs no lock, no
/// journal, and no format version: anything unexpected simply rebuilds.
fn verify(file: &mut File, expected_tip: &str) -> std::io::Result<(usize, usize)> {
    let len = file.metadata()?.len() as usize;
    if len < HEADER_LEN || !(len - HEADER_LEN).is_multiple_of(RECORD_LEN) {
        return Err(std::io::Error::other("index file misaligned"));
    }
    let total = (len - HEADER_LEN) / RECORD_LEN;

    let mut header = [0u8; HEADER_LEN];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut header)?;
    if header[40] != b' ' || header[49] != b'\n' {
        return Err(std::io::Error::other("index header sentinel corrupt"));
    }
    if &header[..40] != expected_tip.as_bytes() {
        return Err(std::io::Error::other("index tip mismatch"));
    }

    let base = usize::from_str_radix(&String::from_utf8_lossy(&header[41..49]), 16)
        .map_err(std::io::Error::other)?;
    if base > total {
        return Err(std::io::Error::other("index base exceeds total"));
    }
    Ok((base, total))
}

/// Open the index file and verify it against the expected tip. Any failure
/// means the file is unusable and the caller catches up or rebuilds.
fn try_open_verified(file_path: &Path, expected_tip: &str) -> std::io::Result<Index> {
    let mut file = File::open(file_path)?;
    let (base, total) = verify(&mut file, expected_tip)?;
    Ok(Index {
        file: Some(file),
        mem: Vec::new(),
        base,
        total,
        extra: Vec::new(),
        tail: None,
    })
}

/// Write the index file durably (tmp + sync + rename). Refuses if any id is
/// not exactly 40 bytes.
fn write_index(file_path: &Path, tip: &str, sorted: &[String]) -> std::io::Result<()> {
    for id in sorted {
        if id.len() != 40 {
            return Err(std::io::Error::other("id length is not 40"));
        }
    }

    let parent = file_path
        .parent()
        .ok_or_else(|| std::io::Error::other("index path has no parent"))?;
    std::fs::create_dir_all(parent)?;

    let mut body = String::with_capacity(HEADER_LEN + sorted.len() * RECORD_LEN);
    body.push_str(tip);
    body.push(' ');
    body.push_str(&format!("{:08x}", sorted.len()));
    body.push('\n');
    for id in sorted {
        body.push_str(id);
        body.push('\n');
    }

    let tmp = parent.join(format!(
        ".tmp-{}-{}",
        std::process::id(),
        file_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    ));

    let mut file = File::create(&tmp)?;
    file.write_all(body.as_bytes())?;
    file.sync_all()?;
    drop(file);
    std::fs::rename(&tmp, file_path)?;
    Ok(())
}

impl Index {
    /// Read record i as a 40-char String. Any failure yields an empty
    /// string, which compares unequal to every real id and so degrades one
    /// row, never the command.
    fn at(&mut self, i: usize) -> String {
        if self.file.is_none() {
            return self.mem.get(i).cloned().unwrap_or_default();
        }

        let file = self.file.as_mut().unwrap();
        let offset = HEADER_LEN + i * RECORD_LEN;
        if file.seek(SeekFrom::Start(offset as u64)).is_err() {
            return String::new();
        }
        let mut buf = [0u8; 40];
        if file.read_exact(&mut buf).is_err() {
            return String::new();
        }
        String::from_utf8_lossy(&buf).to_string()
    }

    /// The unsorted tail, read whole and cached for the life of this handle.
    fn tail(&mut self) -> &[String] {
        if self.tail.is_none() {
            let mut records = Vec::new();
            if self.total > self.base {
                if let Some(file) = self.file.as_mut() {
                    let span = self.total - self.base;
                    let mut buf = vec![0u8; span * RECORD_LEN];
                    if file
                        .seek(SeekFrom::Start(
                            (HEADER_LEN + self.base * RECORD_LEN) as u64,
                        ))
                        .is_ok()
                        && file.read_exact(&mut buf).is_ok()
                    {
                        records = buf
                            .as_chunks::<RECORD_LEN>()
                            .0
                            .iter()
                            .map(|rec| String::from_utf8_lossy(&rec[..40]).to_string())
                            .collect();
                    }
                } else {
                    records = self
                        .mem
                        .get(self.base..self.total)
                        .map(<[String]>::to_vec)
                        .unwrap_or_default();
                }
            }
            self.tail = Some(records);
        }
        self.tail.as_deref().unwrap_or_default()
    }

    /// The index of the first base record that is not less than `key` — the
    /// binary search every lookup starts from.
    fn lower_bound(&mut self, key: &str) -> usize {
        let (mut lo, mut hi) = (0usize, self.base);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.at(mid).as_str() < key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    }

    /// The longest prefix `id` shares with any *other* entry in this index.
    ///
    /// Skipping entries equal to `id` is what makes this correct: `id` is
    /// normally in the index — it is an id we are about to print — and the
    /// same op can sit in both the live and trash files between trims.
    fn longest_common(&mut self, id: &str) -> usize {
        if self.total == 0 && self.extra.is_empty() {
            return 0;
        }
        let mut best = 0;
        let bound = self.lower_bound(id);

        // The base is sorted, so the nearest distinct neighbour on each side
        // is the maximum over that whole side: each walk stops at the first
        // record that is not `id` itself.
        for i in (0..bound).rev() {
            let rec = self.at(i);
            if rec != id {
                best = best.max(common_prefix(&rec, id));
                break;
            }
        }
        for i in bound..self.base {
            let rec = self.at(i);
            if rec != id {
                best = best.max(common_prefix(&rec, id));
                break;
            }
        }

        // The tail and the catch-up are unsorted, so both are scanned — and
        // both are bounded, which is what keeps that scan a constant.
        for rec in self.tail() {
            if rec != id {
                best = best.max(common_prefix(rec, id));
            }
        }
        for rec in &self.extra {
            if rec != id {
                best = best.max(common_prefix(rec, id));
            }
        }
        best
    }
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

/// The shortest-unique-prefix length for each id in `ids` — one more than
/// the longest prefix it shares with anything else in the domain, clamped to
/// the id's own length.
///
/// Priced by the rows on screen rather than by the length of the log: a
/// binary search into each of the two indexes per id. Taking the max across
/// the two is the same answer as sorting their union — the nearest
/// neighbour in the union is whichever per-file neighbour is closer.
///
/// Ids are raw hex here, not letters: the alphabet is an order-preserving
/// per-character map, so a prefix length computed over hex is the same
/// number of letters, and the file stays greppable with git's own ids.
pub fn prefix_lens(repo: &gix::Repository, ids: &[String]) -> Result<HashMap<String, usize>> {
    let mut open = domain(repo)?;

    let mut lens = HashMap::with_capacity(ids.len());
    for id in ids {
        if lens.contains_key(id) {
            continue;
        }
        let shared = open
            .iter_mut()
            .map(|index| index.longest_common(id))
            .max()
            .unwrap_or(0);
        lens.insert(id.clone(), (shared + 1).min(id.len().max(1)));
    }
    Ok(lens)
}

/// Every chain's index files, live and trash, opened once.
///
/// The union of these is the resolution domain, and taking the maximum
/// shared prefix across them is the same answer as sorting that union — the
/// nearest neighbour in a union is whichever per-file neighbour is closer.
/// Bounded by the number of chains the repository has ever had, which is the
/// number of worktrees, so this is a handful of binary searches rather than
/// anything that grows with history.
fn domain(repo: &gix::Repository) -> Result<Vec<Index>> {
    let mut chains = crate::ops::chain_ids(repo)?;
    // A chain with no refs yet is still the one we are standing in, and
    // `ensure` on an absent ref is free.
    let me = crate::ops::chain_id(repo);
    if !chains.iter().any(|c| c == &me) {
        chains.push(me);
    }
    let mut out = Vec::with_capacity(chains.len() * 2);
    for chain in &chains {
        out.push(ensure(repo, chain, Kind::Live)?);
        out.push(ensure(repo, chain, Kind::Trash)?);
    }
    Ok(out)
}

/// Every op id (live and trashed) starting with `prefix`, as raw hex.
///
/// Candidates come out in sorted order rather than newest-first walk order —
/// the caller's identity guard is what makes any candidate safe, and a stale
/// index can only ever produce a candidate that then fails that guard.
pub fn prefix_matches(repo: &gix::Repository, prefix: &str) -> Result<Vec<gix::ObjectId>> {
    let mut seen = Vec::new();
    for index in domain(repo)?.iter_mut() {
        collect_matches(index, prefix, &mut seen);
    }

    let mut candidates = Vec::new();
    for id_str in seen {
        if let Ok(id) = gix::ObjectId::from_hex(id_str.as_bytes()) {
            candidates.push(id);
        }
    }
    Ok(candidates)
}

/// Whether one exact id is in a domain. The live answer is what separates
/// "old" from "trimmed", and it costs a binary search rather than a walk.
pub fn contains(repo: &gix::Repository, kind: Kind, id: gix::ObjectId) -> Result<bool> {
    let hex = id.to_string();
    let mut chains = crate::ops::chain_ids(repo)?;
    let me = crate::ops::chain_id(repo);
    if !chains.iter().any(|c| c == &me) {
        chains.push(me);
    }
    for chain in &chains {
        let mut index = ensure(repo, chain, kind)?;
        let mut found = Vec::new();
        collect_matches(&mut index, &hex, &mut found);
        if found.contains(&hex) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn collect_matches(index: &mut Index, prefix: &str, out: &mut Vec<String>) {
    // Every record sharing a prefix is contiguous in a sorted file, so the
    // lower bound plus a forward scan is the whole of the base's answer.
    let mut i = index.lower_bound(prefix);
    while i < index.base {
        let rec = index.at(i);
        if !rec.starts_with(prefix) {
            break;
        }
        if !out.contains(&rec) {
            out.push(rec);
        }
        i += 1;
    }

    for rec in index.tail() {
        if rec.starts_with(prefix) && !out.contains(rec) {
            out.push(rec.clone());
        }
    }
    for rec in &index.extra {
        if rec.starts_with(prefix) && !out.contains(rec) {
            out.push(rec.clone());
        }
    }
}

/// Append one id to the live index, merging the tail when it has grown past
/// `MERGE_TAIL`. Returns nothing; every failure is silent, because the write
/// path must not gain a failure mode from a derived cache.
///
/// The merge lives here rather than on the read path deliberately: it is a
/// full sequential rewrite of the file, and a read is the one place that can
/// never afford one.
pub fn record(repo: &gix::Repository, prev: Option<gix::ObjectId>, new: gix::ObjectId) {
    let file_path = path(repo, Kind::Live);
    let new_str = new.to_string();

    let Some(prev_id) = prev else {
        // The log was just created; the whole domain is one id.
        let _ = write_index(&file_path, &new_str, std::slice::from_ref(&new_str));
        return;
    };
    let prev_str = prev_id.to_string();

    // Open the live file read-write and verify it against the PREVIOUS tip.
    // Verifying against the new one would stamp a fresh header onto a body
    // that was already stale — a wrong index that passes every check. If it
    // does not verify we do nothing at all, and the next read catches up.
    let Ok(mut file) = File::options().read(true).write(true).open(&file_path) else {
        return;
    };
    let Ok((base, total)) = verify(&mut file, &prev_str) else {
        return;
    };

    // Record first, header second: a crash between them leaves a stale
    // header (self-heals on the next read); the reverse order would not.
    if file.seek(SeekFrom::End(0)).is_err() {
        return;
    }
    if file.write_all(new_str.as_bytes()).is_err() {
        return;
    }
    if file.write_all(b"\n").is_err() {
        return;
    }
    let header = format!("{new_str} {base:08x}\n");
    if file.seek(SeekFrom::Start(0)).is_err() {
        return;
    }
    if file.write_all(header.as_bytes()).is_err() {
        return;
    }

    if total + 1 - base > MERGE_TAIL {
        merge(&file_path, &new_str, file, base, total + 1);
    }
}

/// Fold the unsorted tail back into the sorted base. Best effort: a failure
/// leaves the file exactly as it was, tail and all, and the next append
/// tries again.
fn merge(file_path: &Path, tip: &str, file: File, base: usize, total: usize) {
    let mut index = Index {
        file: Some(file),
        mem: Vec::new(),
        base,
        total,
        extra: Vec::new(),
        tail: None,
    };
    let mut records: Vec<String> = (0..total)
        .map(|i| index.at(i))
        .filter(|s| s.len() == 40)
        .collect();
    records.sort_unstable();
    let _ = write_index(file_path, tip, &records);
}

/// Read-only status of the live index file. Never rebuilds, never writes.
pub fn status(repo: &gix::Repository) -> Result<Status> {
    let Ok(mut file) = File::open(path(repo, Kind::Live)) else {
        return Ok(Status::Absent);
    };
    // A file whose log is gone is stale by the same rule that governs
    // everything else here: it does not describe the ref it names.
    let Some(tip) = crate::refs::ref_target(repo, &crate::ops::ops_ref_of(repo))? else {
        return Ok(Status::Stale);
    };
    Ok(match verify(&mut file, &tip.to_string()) {
        Ok((_, total)) => Status::InSync { ids: total },
        Err(_) => Status::Stale,
    })
}
