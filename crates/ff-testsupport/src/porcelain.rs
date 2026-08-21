//! The differential contract: parse real git's `status --porcelain=v2 --branch`
//! and `git log` output into a normalized shape, convert `ff_core` results into
//! the same shape, and assert equality — plus the index byte-identity tripwire.

use std::path::Path;

use crate::fixtures::{Fixture, index_bytes_at};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NormHead {
    /// Short branch name of the unborn branch.
    Unborn(String),
    Branch {
        name: String,
        commit: String,
    },
    Detached {
        commit: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormUpstream {
    /// Short tracking name, e.g. `origin/main`.
    pub name: String,
    /// `Some((ahead, behind))`, or `None` when the upstream ref is gone.
    pub ab: Option<(usize, usize)>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct NormEntry {
    pub path: String,
    pub from: Option<String>,
    pub kind: char,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Norm {
    pub head: NormHead,
    pub upstream: Option<NormUpstream>,
    pub staged: Vec<NormEntry>,
    pub unstaged: Vec<NormEntry>,
    pub untracked: Vec<String>,
    pub conflicts: Vec<String>,
}

/// Parse `git status --porcelain=v2 --branch` output.
pub fn parse_porcelain_v2(out: &str) -> Norm {
    let mut oid = String::new();
    let mut head_name = String::new();
    let mut upstream_name: Option<String> = None;
    let mut ab: Option<(usize, usize)> = None;
    let mut staged = Vec::new();
    let mut unstaged = Vec::new();
    let mut untracked = Vec::new();
    let mut conflicts = Vec::new();

    for line in out.lines() {
        if let Some(rest) = line.strip_prefix("# branch.oid ") {
            oid = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("# branch.head ") {
            head_name = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            upstream_name = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            let mut parts = rest.split(' ');
            let ahead: usize = parts
                .next()
                .unwrap()
                .strip_prefix('+')
                .unwrap()
                .parse()
                .unwrap();
            let behind: usize = parts
                .next()
                .unwrap()
                .strip_prefix('-')
                .unwrap()
                .parse()
                .unwrap();
            ab = Some((ahead, behind));
        } else if let Some(rest) = line.strip_prefix("1 ") {
            let fields: Vec<&str> = rest.splitn(8, ' ').collect();
            let xy = fields[0];
            let path = fields[7].to_string();
            push_xy(xy, path, None, &mut staged, &mut unstaged);
        } else if let Some(rest) = line.strip_prefix("2 ") {
            let fields: Vec<&str> = rest.splitn(9, ' ').collect();
            let xy = fields[0];
            let (path, orig) = fields[8]
                .split_once('\t')
                .expect("rename line has tab-separated orig path");
            push_xy(
                xy,
                path.to_string(),
                Some(orig.to_string()),
                &mut staged,
                &mut unstaged,
            );
        } else if let Some(rest) = line.strip_prefix("u ") {
            let fields: Vec<&str> = rest.splitn(10, ' ').collect();
            conflicts.push(fields[9].to_string());
        } else if let Some(rest) = line.strip_prefix("? ") {
            untracked.push(rest.to_string());
        }
        // `! ` (ignored) is never requested; anything else is a header we don't need.
    }

    let head = if oid == "(initial)" {
        NormHead::Unborn(head_name)
    } else if head_name == "(detached)" {
        NormHead::Detached { commit: oid }
    } else {
        NormHead::Branch {
            name: head_name,
            commit: oid,
        }
    };
    let upstream = upstream_name.map(|name| NormUpstream { name, ab });

    staged.sort();
    unstaged.sort();
    untracked.sort();
    conflicts.sort();

    Norm {
        head,
        upstream,
        staged,
        unstaged,
        untracked,
        conflicts,
    }
}

fn push_xy(
    xy: &str,
    path: String,
    from: Option<String>,
    staged: &mut Vec<NormEntry>,
    unstaged: &mut Vec<NormEntry>,
) {
    let mut chars = xy.chars();
    let x = chars.next().unwrap();
    let y = chars.next().unwrap();
    if x != '.' {
        staged.push(NormEntry {
            path: path.clone(),
            from: from.clone(),
            kind: x,
        });
    }
    if y != '.' {
        // Worktree-added only happens for intent-to-add entries; normalize to 'I'.
        let kind = if y == 'A' { 'I' } else { y };
        unstaged.push(NormEntry {
            path,
            from: None,
            kind,
        });
    }
}

/// Convert a native `ff_core::Status` into the normalized shape.
pub fn norm_from_native(status: &ff_core::Status) -> Norm {
    use ff_core::{ChangeKind, HeadState};

    let head = match &status.head {
        HeadState::Unborn { r#ref } => NormHead::Unborn(
            r#ref
                .strip_prefix("refs/heads/")
                .unwrap_or(r#ref)
                .to_string(),
        ),
        HeadState::Branch { name, commit, .. } => NormHead::Branch {
            name: name.clone(),
            commit: commit.clone(),
        },
        HeadState::Detached { commit } => NormHead::Detached {
            commit: commit.clone(),
        },
    };

    let kind_char = |kind: ChangeKind| match kind {
        ChangeKind::Added => 'A',
        ChangeKind::Modified => 'M',
        ChangeKind::Deleted => 'D',
        ChangeKind::TypeChange => 'T',
        ChangeKind::Renamed => 'R',
        ChangeKind::Copied => 'C',
        ChangeKind::IntentToAdd => 'I',
    };
    let entries = |list: &[ff_core::StatusEntry]| {
        let mut v: Vec<NormEntry> = list
            .iter()
            .map(|e| NormEntry {
                path: e.path.clone(),
                from: e.from.clone(),
                kind: kind_char(e.kind),
            })
            .collect();
        v.sort();
        v
    };

    Norm {
        head,
        upstream: status.upstream.as_ref().map(|u| NormUpstream {
            name: u.r#ref.clone(),
            ab: if u.gone {
                None
            } else {
                Some((u.ahead, u.behind))
            },
        }),
        staged: entries(&status.staged),
        unstaged: entries(&status.unstaged),
        untracked: status.untracked.clone(),
        conflicts: status.conflicts.clone(),
    }
}

/// The core differential assertion: ff-core's status must match real git's
/// porcelain v2 output, and must leave `.git/index` byte-identical.
pub fn assert_status_matches(fx: &Fixture) {
    assert_status_matches_at(fx, &fx.path());
}

pub fn assert_status_matches_at(fx: &Fixture, dir: &Path) {
    let before = index_bytes_at(dir);
    let repo = ff_core::discover_isolated(dir).expect("discover repo");
    let native = ff_core::status(&repo).expect("ff-core status");
    let after = index_bytes_at(dir);
    assert_eq!(
        before, after,
        "ff-core::status must leave .git/index byte-identical"
    );

    let out = fx.git_in(
        dir,
        &[
            "--no-optional-locks",
            "status",
            "--porcelain=v2",
            "--branch",
        ],
    );
    let git = parse_porcelain_v2(&out);
    let native = norm_from_native(&native);
    assert_eq!(
        git, native,
        "differential mismatch vs git status --porcelain=v2"
    );
}

/// One commit as printed by real `git log`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommit {
    pub id: String,
    pub short: String,
    pub author_name: String,
    pub author_email: String,
    pub time: i64,
    pub subject: String,
}

const LOG_FORMAT: &str = "--format=%H%x1f%h%x1f%an%x1f%ae%x1f%at%x1f%s";

/// Parse `git log` in our field-separated format, optionally limited.
pub fn git_log(fx: &Fixture, limit: Option<usize>) -> Vec<GitCommit> {
    let mut args = vec!["log", LOG_FORMAT];
    let n;
    if let Some(limit) = limit {
        n = format!("-n{limit}");
        args.push(&n);
    }
    let out = fx.git(&args);
    out.lines()
        .map(|line| {
            let f: Vec<&str> = line.split('\u{1f}').collect();
            GitCommit {
                id: f[0].to_string(),
                short: f[1].to_string(),
                author_name: f[2].to_string(),
                author_email: f[3].to_string(),
                time: f[4].parse().expect("author time"),
                subject: f[5].to_string(),
            }
        })
        .collect()
}

/// Differential assertion for `ff log`: entries equal git's, and `short_id` is
/// a prefix of `id` that git can resolve unambiguously. Short ids are *not*
/// required to have git's exact length — abbreviation floors differ legitimately.
pub fn assert_log_matches(fx: &Fixture, limit: Option<usize>) {
    let mut repo = fx.repo();
    let native: Vec<ff_core::LogEntry> = ff_core::log(
        &mut repo,
        &ff_core::LogOptions {
            limit,
            revs: None,
            paths: Vec::new(),
        },
    )
    .expect("ff-core log")
    .entries
    .collect::<ff_core::Result<_>>()
    .expect("ff-core log entries");
    let expected = git_log(fx, limit);

    assert_eq!(
        native.len(),
        expected.len(),
        "log length mismatch: native {native:#?} vs git {expected:#?}"
    );
    for (n, g) in native.iter().zip(&expected) {
        assert_eq!(n.id, g.id, "commit id");
        assert_eq!(n.author_name, g.author_name, "author name for {}", g.id);
        assert_eq!(n.author_email, g.author_email, "author email for {}", g.id);
        assert_eq!(n.time, g.time, "author time for {}", g.id);
        assert_eq!(n.subject, g.subject, "subject for {}", g.id);
        assert!(
            n.id.starts_with(&n.short_id),
            "short id {} must be a prefix of {}",
            n.short_id,
            n.id
        );
        let spec = format!("{}^{{commit}}", n.short_id);
        let resolved = fx.git(&["rev-parse", "--verify", &spec]);
        assert_eq!(
            resolved.trim(),
            n.id,
            "short id {} must resolve unambiguously",
            n.short_id
        );
    }
}
