//! The view set `ff diff`, `ff show`, and `ff log` share: how deep each
//! reads the files it names, and how it says what it read.
//!
//! One type and two emitters rather than a flag per verb, so the three
//! cannot drift: `--stat` prints the same block on all of them, `--name-only`
//! drops the same keys, and `-U` reaches the same dial. A verb's default
//! depth is the one thing that differs, and it is a parameter.

use ff_core::{ChangeStat, DiffOptions, Error, Result};

/// How deep a verb reads the files it names: the patch, the diffstat, the
/// paths alone, or nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Depth {
    Patch,
    Stat,
    Names,
    Skip,
}

/// The resolved view: a depth, and the context width a patch prints.
#[derive(Clone, Copy, Debug)]
pub struct FileView {
    pub depth: Depth,
    /// Context lines around each change. Inert without a patch.
    pub context: u32,
}

/// The flags as clap parsed them; a verb passes `false` for one it lacks.
#[derive(Clone, Copy, Debug, Default)]
pub struct Flags {
    pub patch: bool,
    pub stat: bool,
    pub name_only: bool,
    pub no_patch: bool,
    pub unified: Option<u32>,
}

impl FileView {
    /// `default` is the verb's depth with no flag: `Patch` on diff and show,
    /// `Skip` on log.
    ///
    /// `--stat`, `--name-only`, and `--no-patch` each pick one view, so more
    /// than one is refused. `--stat` outranks `-p`, git's rule. `-U` is taken
    /// whatever the depth: it is a dial on the patch, and a dial on a patch
    /// nobody printed turns nothing.
    pub fn resolve(flags: Flags, default: Depth) -> Result<Self> {
        let picked = [flags.stat, flags.name_only, flags.no_patch]
            .iter()
            .filter(|on| **on)
            .count();
        if picked > 1 {
            return Err(Error::coded(
                "usage/bad-flags",
                "--stat, --name-only, and --no-patch each pick one view of the files; choose one",
                vec![
                    "ff diff --stat".into(),
                    "ff show --name-only HEAD".into(),
                    "ff log -p".into(),
                ],
            ));
        }
        let depth = if flags.stat {
            Depth::Stat
        } else if flags.name_only {
            Depth::Names
        } else if flags.no_patch {
            Depth::Skip
        } else if flags.patch {
            Depth::Patch
        } else {
            default
        };
        Ok(Self {
            depth,
            context: flags.unified.unwrap_or(ff_core::patch::DEFAULT_CONTEXT),
        })
    }

    /// The options the tree diff takes for this view: hunks only under
    /// `Patch`, since the other depths never open a blob past its counts.
    pub fn diff_options(&self, paths: Vec<String>) -> DiffOptions {
        DiffOptions {
            hunks: self.depth == Depth::Patch,
            context: self.context,
            paths,
        }
    }
}

/// The JSON keys a view carries, as pairs for the caller to insert into its
/// payload map. Nothing under `Skip`; `changes` alone under `Names`, each
/// file cut to `path`, `from`, `kind`, and `binary`; `changes` and the two
/// totals otherwise. A key a view drops is absent, never null, so a
/// consumer's `changes[].hunks` under `--stat` reads as missing rather than
/// as a patch with nothing in it.
pub fn json_keys(stat: &ChangeStat, depth: Depth) -> Vec<(&'static str, serde_json::Value)> {
    match depth {
        Depth::Skip => Vec::new(),
        Depth::Names => {
            let files: Vec<serde_json::Value> = stat
                .files
                .iter()
                .map(|f| {
                    serde_json::json!({
                        "path": f.path,
                        "from": f.from,
                        "kind": f.kind,
                        "binary": f.binary,
                    })
                })
                .collect();
            vec![("changes", serde_json::Value::Array(files))]
        }
        Depth::Stat | Depth::Patch => vec![
            ("changes", serde_json::json!(stat.files)),
            ("insertions", serde_json::json!(stat.insertions)),
            ("deletions", serde_json::json!(stat.deletions)),
        ],
    }
}

/// The text a view prints for a stat: empty when there are no files or the
/// depth is `Skip`, otherwise the block, ending in a newline. The patch
/// block is git's format; the other two are the diffstat's rail rows.
pub fn text(stat: &ChangeStat, depth: Depth, colored: bool) -> String {
    if stat.files.is_empty() {
        return String::new();
    }
    match depth {
        Depth::Skip => String::new(),
        Depth::Patch => crate::render::patch_block(&stat.files, colored),
        Depth::Stat => format!("{}\n", crate::render::render_diffstat(stat, colored)),
        Depth::Names => format!("{}\n", crate::render::name_only_block(&stat.files, colored)),
    }
}
