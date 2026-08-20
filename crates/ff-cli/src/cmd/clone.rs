//! `ff clone` — bring a repository down, and arm it on arrival.
//!
//! Not a wrapper around `git clone`. fufu owns the whole sequence: it
//! negotiates the pack over the wire itself, resolves the remote's HEAD,
//! checks out the worktree, and then does the two things `git clone` has no
//! reason to — writes the gc guard, and takes the operation log's floor. The
//! report is in fufu's vocabulary rather than git's.
//!
//! That last part is not decoration. `ff publish` decides what to send by
//! comparing against the shared copy fufu remembers, and this is the moment
//! that memory starts out true rather than inferred.
//!
//! Like `ff init`, it captures *last*. Every other verb captures first; these
//! two cannot, because there is no repository to capture until the work is
//! done. What they take is not a pre-command snapshot but the log's first
//! entry, and an append before the clone happened would be a claim nothing
//! could falsify.

use ff_core::{Error, Result};

use crate::ctx::Ctx;

pub fn run(
    ctx: &Ctx,
    url: String,
    dir: Option<String>,
    branch: Option<String>,
    depth: Option<std::num::NonZeroU32>,
    origin: String,
) -> Result<()> {
    let dir = match dir {
        Some(dir) => std::path::PathBuf::from(dir),
        None => std::path::PathBuf::from(default_dir(&url)?),
    };
    // Refused rather than merged into, the way git refuses. gix would delete
    // the directory on a failed clone, and deleting a directory somebody
    // already put something in is not a thing to do on their behalf.
    if is_nonempty(&dir) {
        return Err(Error::coded(
            "clone/target-exists",
            format!("{} already exists and is not empty", dir.display()),
            vec![format!("ff clone {url} <dir>"), "ff init".into()],
        ));
    }

    let colored = crate::pager::color_enabled();
    let repo = crate::net::clone(crate::net::Clone {
        url: &url,
        dir: &dir,
        branch: branch.as_deref(),
        depth,
        remote: &origin,
        progress: !ctx.json,
    })?;

    crate::render::init_palette(&repo);
    let armed = crate::cmd::init::arm(&repo, &crate::provenance::pre_ff(ctx))?;
    // Counted after the checkout, from what actually landed: an empty remote
    // clones to an unborn branch and zero commits, and saying so is better
    // than a number borrowed from the wire.
    let commits = count_commits(&repo);

    if ctx.json {
        let payload = serde_json::json!({
            "path": crate::cmd::init::workdir(&repo),
            "url": url,
            "remote": origin,
            "branch": armed.branch,
            "commits": commits,
            "created": true,
            "floor": armed.floor,
        });
        crate::machine::emit("clone", &payload)?;
        return Ok(());
    }

    println!(
        "{}",
        crate::render::paint_ok(
            &format!(
                "cloned into {} — {commits} commit{} on {}",
                display_dir(&dir),
                if commits == 1 { "" } else { "s" },
                armed.branch
            ),
            colored
        )
    );
    crate::cmd::init::tail(colored);
    Ok(())
}

/// The last segment of the URL's *path*, with `.git` stripped — git's own
/// rule, and the one people already expect from typing `git clone <url>`.
///
/// Parsed rather than sliced. `https://example.com/` has a host and no path,
/// so the naive last-segment rule would name the directory after the host and
/// then go to the network to find out it was wrong; gix's parser is the same
/// one the clone itself uses, so a URL refused here is one the clone would
/// have refused anyway, refused before anything is created.
fn default_dir(url: &str) -> Result<String> {
    let parsed = gix::url::parse(url.into()).map_err(|err| {
        Error::coded(
            "clone/bad-url",
            format!("that is not a repository fufu can address: {err}"),
            vec![],
        )
    })?;
    // Both separators: a local Windows path arrives as `C:\src\thing.git`
    // and its parsed path keeps the backslashes, so splitting on `/` alone
    // makes the "last segment" the whole path — and `ff clone C:\src\thing.git`
    // would build `C:\src\thing` instead of `thing` here. git treats the two
    // interchangeably on Windows, and a backslash inside a real URL's path is
    // pathological enough that following git is the right call.
    let path = parsed.path.to_string();
    let tail = path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim_end_matches(".git");
    if tail.is_empty() || tail == "." || tail == ".." || tail == "~" {
        return Err(Error::coded(
            "clone/bad-url",
            format!("nothing in {url} names a directory to clone into"),
            vec![format!("ff clone {url} <dir>")],
        ));
    }
    Ok(tail.to_string())
}

/// `./thing` rather than a bare `thing`, so the report reads as a place on
/// disk. A path that already says where it is keeps its own spelling.
///
/// Rootedness is tested by hand as well as by `is_absolute`, because Windows
/// calls `/tmp/w` *relative* — it names no drive — and prefixing it produced
/// `.//tmp/w`. A leading separator already says "not here", whatever the
/// platform thinks of the rest of the path.
fn display_dir(dir: &std::path::Path) -> String {
    let shown = dir.display().to_string();
    let rooted = dir.is_absolute() || shown.starts_with(['/', '\\']);
    if rooted || shown.starts_with('.') {
        shown
    } else {
        format!("./{shown}")
    }
}

fn is_nonempty(dir: &std::path::Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some())
}

/// How many commits the checked-out branch carries. Best effort by design:
/// the clone has landed either way, and a walk that cannot finish must not
/// turn a successful clone into a failure.
fn count_commits(repo: &gix::Repository) -> usize {
    // An unborn head is not a failure here: it is what a clone of an empty
    // remote leaves, and zero is the true answer.
    let Ok(head) = repo.head_id() else { return 0 };
    let Ok(walk) = head.ancestors().all() else {
        return 0;
    };
    walk.take_while(|step| step.is_ok()).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_directory_is_the_last_path_segment_without_dot_git() {
        let dir = |url| default_dir(url).unwrap_or_else(|err| panic!("{url}: {err}"));
        assert_eq!(dir("git@github.com:tyler-johnson/fufu.git"), "fufu");
        assert_eq!(dir("https://github.com/tyler-johnson/fufu.git"), "fufu");
        assert_eq!(dir("https://github.com/tyler-johnson/fufu"), "fufu");
        assert_eq!(dir("https://github.com/tyler-johnson/fufu/"), "fufu");
        assert_eq!(dir("/srv/git/thing.git"), "thing");
        assert_eq!(dir("ssh://host/~/thing.git"), "thing");
        assert_eq!(dir("./thing"), "thing");
        // A local Windows path, which is what `ff clone` is handed there.
        assert_eq!(dir(r"C:\src\thing.git"), "thing");
        assert_eq!(dir(r"C:\src\thing"), "thing");
        assert_eq!(dir(r"\\server\share\thing.git"), "thing");
    }

    /// A host is not a directory. The naive rule named one after `example.com`
    /// and then went to the network to find out it was wrong.
    #[test]
    fn a_url_that_names_no_directory_is_refused() {
        for url in ["", "/", "https://example.com/", "https://example.com", "."] {
            assert_eq!(
                default_dir(url).unwrap_err().id(),
                "clone/bad-url",
                "{url:?} names no directory"
            );
        }
    }

    #[test]
    fn a_relative_target_is_shown_as_a_place() {
        assert_eq!(display_dir(std::path::Path::new("fufu")), "./fufu");
        assert_eq!(display_dir(std::path::Path::new("./w")), "./w");
        // Rooted, and left alone — including on Windows, which does not
        // consider a drive-less path absolute.
        assert_eq!(display_dir(std::path::Path::new("/tmp/w")), "/tmp/w");
        assert_eq!(display_dir(std::path::Path::new(r"\\srv\w")), r"\\srv\w");
    }
}
