//! `fufu.pull` — how `ff pull`'s base step takes a moved base in, and the
//! per-branch `fufu.<pattern>.pull` override.
//!
//! The setting is plain git config. `fufu.pull` is the standing answer for
//! every branch; a `[fufu "tyler/*"]` section's `pull` overrides it for the
//! branches its pattern matches. Patterns are refspec globs matched with
//! `*` crossing `/`, and the last matching row in git's read order wins, so
//! a repository row beats a global one with no precedence rule of its own.
//! An unreadable value is skipped the way `onconflict` skips one, so a
//! misspelling falls through to the next row or the default.
//!
//! [`resolve`] is the one function the base step, the cascade, and
//! `ff resolve` call. It answers with the policy and where it came from,
//! because the report names the source: `behind main (merge, fufu.pull in
//! this repo)` is a sentence the reader can act on, `behind main` alone is
//! not.

use gix::bstr::{BStr, ByteSlice};
use gix::config::file::Metadata;
use gix::config::source::Kind;
use serde::{Deserialize, Serialize};

/// How the base step takes a moved base in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PullPolicy {
    /// Replay a straight line; leave a branch standing once its commits
    /// hold any merge.
    Auto,
    /// Replay the branch's commits onto the base, flattening a merge of
    /// the base the way the replay engine already does.
    Replay,
    /// Leave the branch standing and report it behind.
    Merge,
}

impl PullPolicy {
    pub const DEFAULT: PullPolicy = PullPolicy::Auto;

    /// The value as git config spells it, ascii case-insensitive.
    pub fn parse(raw: &str) -> Option<PullPolicy> {
        [PullPolicy::Auto, PullPolicy::Replay, PullPolicy::Merge]
            .into_iter()
            .find(|policy| policy.as_str().eq_ignore_ascii_case(raw))
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PullPolicy::Auto => "auto",
            PullPolicy::Replay => "replay",
            PullPolicy::Merge => "merge",
        }
    }
}

/// Where the effective value came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PolicySource {
    /// Nothing set: `auto`.
    Default,
    /// `fufu.pull`, in the named scope.
    Setting { scope: String },
    /// `fufu.<pattern>.pull`, in the named scope.
    Pattern { pattern: String, scope: String },
}

/// The effective policy for one branch and the row that set it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Resolved {
    pub policy: PullPolicy,
    pub source: PolicySource,
}

/// One `fufu.<pattern>.pull` row, in git's read order. `ff config pull`
/// lists these.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PatternRow {
    pub pattern: String,
    pub value: String,
    pub scope: String,
}

/// The scope word a config source's kind reports under.
pub fn scope_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Override => "env",
        Kind::Repository => "local",
        Kind::Global => "global",
        Kind::System | Kind::GitInstallation => "system",
    }
}

fn scope_of(meta: &Metadata) -> String {
    scope_label(meta.source.kind()).to_string()
}

/// The effective policy for `branch`, a short name like `tyler/x`, read
/// from the repository's configuration as it stands.
pub fn pull_policy(repo: &gix::Repository, branch: &str) -> Resolved {
    resolve(repo.config_snapshot().plumbing(), branch)
}

/// The effective policy for `branch` in `file`. gix keeps sections in read
/// order across sources, lowest precedence first, so the last row that
/// applies is git's winner: the last matching pattern row, else the last
/// `fufu.pull`, else the default. A row whose value does not parse is
/// skipped.
pub fn resolve(file: &gix::config::File, branch: &str) -> Resolved {
    let branch: &BStr = branch.as_bytes().as_bstr();
    let mut setting: Option<Resolved> = None;
    let mut pattern: Option<Resolved> = None;
    for section in file.sections_by_name("fufu").into_iter().flatten() {
        let Some(raw) = section.value("pull") else {
            continue;
        };
        let Some(policy) = PullPolicy::parse(&raw.to_str_lossy()) else {
            continue;
        };
        let scope = scope_of(section.meta());
        match section.header().subsection_name() {
            Some(glob) => {
                if gix::glob::wildmatch(glob, branch, gix::glob::wildmatch::Mode::empty()) {
                    pattern = Some(Resolved {
                        policy,
                        source: PolicySource::Pattern {
                            pattern: glob.to_str_lossy().into_owned(),
                            scope,
                        },
                    });
                }
            }
            None => {
                setting = Some(Resolved {
                    policy,
                    source: PolicySource::Setting { scope },
                });
            }
        }
    }
    pattern.or(setting).unwrap_or(Resolved {
        policy: PullPolicy::DEFAULT,
        source: PolicySource::Default,
    })
}

/// Every `fufu.<pattern>.pull` row in `file`, in git's read order, values
/// as written: this is the listing, not the decision, so an unreadable
/// value is shown rather than hidden.
pub fn pattern_rows(file: &gix::config::File) -> Vec<PatternRow> {
    let mut rows = Vec::new();
    for section in file.sections_by_name("fufu").into_iter().flatten() {
        let Some(glob) = section.header().subsection_name() else {
            continue;
        };
        let Some(value) = section.value("pull") else {
            continue;
        };
        rows.push(PatternRow {
            pattern: glob.to_str_lossy().into_owned(),
            value: value.to_str_lossy().into_owned(),
            scope: scope_of(section.meta()),
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use gix::config::Source;

    /// A config file read from the given sources in git's order, global
    /// before local, touching no process environment.
    fn read(global: &str, local: &str) -> gix::config::File {
        let mut file = gix::config::File::from_bytes_no_includes(
            global.as_bytes(),
            Metadata::from(Source::User),
            Default::default(),
        )
        .unwrap();
        let local = gix::config::File::from_bytes_no_includes(
            local.as_bytes(),
            Metadata::from(Source::Local),
            Default::default(),
        )
        .unwrap();
        file.append(local).unwrap();
        file
    }

    fn setting(scope: &str) -> PolicySource {
        PolicySource::Setting {
            scope: scope.into(),
        }
    }

    fn pattern(pattern: &str, scope: &str) -> PolicySource {
        PolicySource::Pattern {
            pattern: pattern.into(),
            scope: scope.into(),
        }
    }

    #[test]
    fn a_pattern_row_overrides_the_setting_for_the_branches_it_matches() {
        let file = read(
            "[fufu]\n\tpull = merge\n",
            "[fufu \"tyler/*\"]\n\tpull = replay\n",
        );
        assert_eq!(
            resolve(&file, "tyler/x"),
            Resolved {
                policy: PullPolicy::Replay,
                source: pattern("tyler/*", "local"),
            }
        );
        assert_eq!(
            resolve(&file, "alice/x"),
            Resolved {
                policy: PullPolicy::Merge,
                source: setting("global"),
            }
        );
        assert_eq!(
            pattern_rows(&file),
            vec![PatternRow {
                pattern: "tyler/*".into(),
                value: "replay".into(),
                scope: "local".into(),
            }]
        );
    }

    #[test]
    fn the_last_matching_pattern_wins_and_a_star_crosses_a_slash() {
        let file = read(
            "[fufu \"tyler/*\"]\n\tpull = replay\n",
            "[fufu \"*/deep\"]\n\tpull = merge\n[fufu \"alice/*\"]\n\tpull = replay\n",
        );
        assert_eq!(
            resolve(&file, "tyler/a/deep"),
            Resolved {
                policy: PullPolicy::Merge,
                source: pattern("*/deep", "local"),
            }
        );
        assert_eq!(
            resolve(&file, "tyler/a/b"),
            Resolved {
                policy: PullPolicy::Replay,
                source: pattern("tyler/*", "global"),
            }
        );
        assert_eq!(
            resolve(&file, "main"),
            Resolved {
                policy: PullPolicy::Auto,
                source: PolicySource::Default,
            }
        );
    }

    #[test]
    fn the_local_setting_beats_the_global_one_and_case_is_ignored() {
        let file = read("[fufu]\n\tpull = merge\n", "[fufu]\n\tpull = REPLAY\n");
        assert_eq!(
            resolve(&file, "x"),
            Resolved {
                policy: PullPolicy::Replay,
                source: setting("local"),
            }
        );
    }

    #[test]
    fn nothing_set_is_the_default_and_an_invalid_value_is_skipped() {
        let file = read("", "");
        assert_eq!(
            resolve(&file, "x"),
            Resolved {
                policy: PullPolicy::Auto,
                source: PolicySource::Default,
            }
        );
        let file = read(
            "[fufu]\n\tpull = merge\n",
            "[fufu]\n\tpull = sometimes\n[fufu \"x\"]\n\tpull = never\n",
        );
        assert_eq!(
            resolve(&file, "x"),
            Resolved {
                policy: PullPolicy::Merge,
                source: setting("global"),
            }
        );
        assert_eq!(pattern_rows(&file)[0].value, "never");
    }

    #[test]
    fn parse_and_as_str_round_trip() {
        for policy in [PullPolicy::Auto, PullPolicy::Replay, PullPolicy::Merge] {
            assert_eq!(PullPolicy::parse(policy.as_str()), Some(policy));
            assert_eq!(
                PullPolicy::parse(&policy.as_str().to_uppercase()),
                Some(policy)
            );
        }
        assert_eq!(PullPolicy::parse("sometimes"), None);
        assert_eq!(
            serde_json::to_string(&PullPolicy::Merge).unwrap(),
            "\"merge\""
        );
    }
}
