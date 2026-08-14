//! The snapshot id index: a per-chain, on-disk, sorted file of snapshot ids.
//!
//! The uniqueness domain for prefix highlighting is exactly the set `ff restore
//! --at` resolves against: the chain's live and trash ids. Recomputing it means
//! decoding the whole chain on every `ff log`. So materialize it: one sorted id
//! file per chain, appended by capture, rebuilt whenever the chain tip moves out
//! from under it. Derived and disposable — deleting it costs one rebuild, never
//! data.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::snapshot::chain;

const HEADER_LEN: usize = 50;
const RECORD_LEN: usize = 41;
const MERGE_TAIL: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Live,
    Trash,
}

impl Kind {
    fn dir(self) -> &'static str {
        match self {
            Kind::Live => "live",
            Kind::Trash => "trash",
        }
    }

    fn ref_name(self, chain_name: &str) -> String {
        match self {
            Kind::Live => format!("{}{chain_name}", chain::SNAP_PREFIX),
            Kind::Trash => chain::trash_ref(chain_name),
        }
    }
}

/// A materialized domain, open for reading: the file when there is one,
/// an in-memory sorted list when the file could not be written.
#[derive(Default)]
struct Index {
    file: Option<File>,
    mem: Vec<String>,
    base: usize,
    total: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    InSync { ids: usize },
    Stale,
    Absent,
}

/// The on-disk path for a chain's id index file.
pub fn path(repo: &gix::Repository, chain_name: &str, kind: Kind) -> PathBuf {
    repo.common_dir()
        .join("fufu/ids")
        .join(kind.dir())
        .join(chain_name)
}

/// Open or rebuild the index for a chain ref. IO errors are swallowed — the
/// worst case is an in-memory index, which is today's behavior.
fn ensure(repo: &gix::Repository, chain_name: &str, kind: Kind) -> Result<Index> {
    let ref_name = kind.ref_name(chain_name);

    let tip = chain::tip(repo, &ref_name)?;
    let Some(tip) = tip else {
        return Ok(Index::default());
    };
    let tip_str = tip.to_string();

    let file_path = path(repo, chain_name, kind);

    // Try to open and verify the existing file.
    if let Ok(idx) = try_open_verified(&file_path, &tip_str) {
        return Ok(merge_if_due(&file_path, &tip_str, idx));
    }

    // Rebuild from the chain walk.
    let ids = crate::evolog::ref_ids(repo, &ref_name)?;
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    let _ = write_index(&file_path, &tip_str, &sorted);
    let len = sorted.len();
    Ok(Index {
        file: None,
        mem: sorted,
        base: len,
        total: len,
    })
}

/// The one verification rule, in one place: size alignment, header sentinels,
/// the tip, and the base count. It reads 50 bytes and a stat — never the body,
/// which is the difference between a seek and a scan on a 30k-snapshot chain.
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
/// means the file is unusable and the caller rebuilds.
fn try_open_verified(file_path: &Path, expected_tip: &str) -> std::io::Result<Index> {
    let mut file = File::open(file_path)?;
    let (base, total) = verify(&mut file, expected_tip)?;
    Ok(Index {
        file: Some(file),
        mem: Vec::new(),
        base,
        total,
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

/// If the tail has grown beyond MERGE_TAIL, read all records, sort, rewrite,
/// and return an in-memory index. Otherwise return the index unchanged.
fn merge_if_due(file_path: &Path, tip: &str, mut index: Index) -> Index {
    if index.total - index.base <= MERGE_TAIL {
        return index;
    }

    let mut records: Vec<String> = (0..index.total)
        .map(|i| index.at(i))
        .filter(|s| !s.is_empty())
        .collect();
    records.sort_unstable();
    let _ = write_index(file_path, tip, &records);
    let len = records.len();
    Index {
        file: None,
        mem: records,
        base: len,
        total: len,
    }
}

impl Index {
    /// Read record i as a 40-char String. Any failure yields an empty string,
    /// which compares unequal to every real id and so degrades one row, never the command.
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

    /// The index of the first base record that is not less than `key` — the
    /// binary search every lookup starts from. ~15 seeks on a 30k-id chain.
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
    /// same commit can sit in both the live and trash files between trims.
    fn longest_common(&mut self, id: &str) -> usize {
        if self.total == 0 {
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

        // The tail is unsorted, so it is scanned — but it is bounded by
        // MERGE_TAIL, which is what keeps that scan a constant.
        for i in self.base..self.total {
            let rec = self.at(i);
            if rec != id {
                best = best.max(common_prefix(&rec, id));
            }
        }
        best
    }
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

/// The shortest-unique-prefix length for each id in `ids` — one more than the
/// longest prefix it shares with anything else in the domain, clamped to the
/// id's own length. Identical to `render::unique_prefix_lens` computed over
/// the fully walked live+trash domain, and the tests pin that equivalence.
///
/// The cost is what changed: this answers per id, with a binary search into
/// each of the two indexes, so it is priced by the rows on screen rather than
/// by the length of the chain. Taking the max across the two indexes is the
/// same answer as sorting their union — the nearest neighbour in the union is
/// whichever of the two per-file neighbours is closer.
pub fn prefix_lens(
    repo: &gix::Repository,
    chain_name: &str,
    ids: &[String],
) -> Result<HashMap<String, usize>> {
    let mut live = ensure(repo, chain_name, Kind::Live)?;
    let mut trash = ensure(repo, chain_name, Kind::Trash)?;

    let mut lens = HashMap::with_capacity(ids.len());
    for id in ids {
        if lens.contains_key(id) {
            continue;
        }
        let shared = live.longest_common(id).max(trash.longest_common(id));
        lens.insert(id.clone(), (shared + 1).min(id.len().max(1)));
    }
    Ok(lens)
}

/// Find every snapshot id (live and trash) starting with `prefix`.
/// Returns None when the index could not be ensured (caller falls back to a walk).
///
/// Candidates come out in sorted order rather than newest-first walk order — the
/// caller's identity guard is what makes any candidate safe, and a stale index
/// can only ever produce a candidate that then fails that guard.
pub fn prefix_matches(
    repo: &gix::Repository,
    chain_name: &str,
    prefix: &str,
) -> Result<Option<Vec<gix::ObjectId>>> {
    let mut live = match ensure(repo, chain_name, Kind::Live) {
        Ok(idx) => idx,
        Err(_) => return Ok(None),
    };
    let mut trash = match ensure(repo, chain_name, Kind::Trash) {
        Ok(idx) => idx,
        Err(_) => return Ok(None),
    };

    let mut seen = Vec::new();

    // Collect from live index: binary-search base, scan tail.
    collect_matches(&mut live, prefix, &mut seen);

    // Collect from trash index, skipping ids already collected.
    collect_matches(&mut trash, prefix, &mut seen);

    // Parse collected ids.
    let mut candidates = Vec::new();
    for id_str in seen {
        if let Ok(id) = gix::ObjectId::from_hex(id_str.as_bytes()) {
            candidates.push(id);
        }
    }
    Ok(Some(candidates))
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

    for j in index.base..index.total {
        let rec = index.at(j);
        if rec.starts_with(prefix) && !out.contains(&rec) {
            out.push(rec);
        }
    }
}

/// Append one id to the live index. Returns nothing; every failure is silent,
/// because capture must not gain a failure mode from a derived cache.
pub fn record(
    repo: &gix::Repository,
    chain_name: &str,
    prev: Option<gix::ObjectId>,
    new: gix::ObjectId,
) {
    let file_path = path(repo, chain_name, Kind::Live);
    let new_str = new.to_string();

    match prev {
        None => {
            // Chain was just created; the whole domain is one id.
            let _ = write_index(&file_path, &new_str, std::slice::from_ref(&new_str));
        }
        Some(prev_id) => {
            let prev_str = prev_id.to_string();

            // Open the live file read-write and verify it against the PREVIOUS
            // tip. Verifying against the new one would stamp a fresh header
            // onto a body that was already stale — a wrong index that passes
            // every check. If it does not verify we do nothing at all, and
            // the next read rebuilds.
            let Ok(mut file) = File::options().read(true).write(true).open(&file_path) else {
                return;
            };
            let Ok((base, _)) = verify(&mut file, &prev_str) else {
                return;
            };

            // Record first, header second: a crash between them leaves a stale
            // header (self-heals on next read); the reverse order would not.
            if file.seek(SeekFrom::End(0)).is_err() {
                return;
            }
            if file.write_all(new_str.as_bytes()).is_err() {
                return;
            }
            if file.write_all(b"\n").is_err() {
                return;
            }

            // Rewrite the header with the new tip and the unchanged base count.
            let header = format!("{new_str} {:08x}\n", base);
            if file.seek(SeekFrom::Start(0)).is_err() {
                return;
            }
            let _ = file.write_all(header.as_bytes());
        }
    }
}

/// Read-only status of the live index file. Never rebuilds, never writes.
pub fn status(repo: &gix::Repository, chain_name: &str) -> Result<Status> {
    let Ok(mut file) = File::open(path(repo, chain_name, Kind::Live)) else {
        return Ok(Status::Absent);
    };
    // A file whose chain is gone is stale by the same rule that governs
    // everything else here: it does not describe the ref it names.
    let Some(tip) = chain::tip(repo, &Kind::Live.ref_name(chain_name))? else {
        return Ok(Status::Stale);
    };
    Ok(match verify(&mut file, &tip.to_string()) {
        Ok((_, total)) => Status::InSync { ids: total },
        Err(_) => Status::Stale,
    })
}
