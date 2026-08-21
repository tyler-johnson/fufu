use ff_core::{Error, LogOptions, Result, revset::Revset};

use crate::ctx::Ctx;

/// `revisions` is `-r`: the set the rows come from. It replaces the source of
/// the rows and nothing else, so it composes with `--commits`.
///
/// `paths` is the positional: the files or directories the rows must touch,
/// and the open change's row must touch them too.
///
/// `ops` is the retired `--ops`, kept as a hidden flag only so typing it is
/// answered rather than met with a bare "unexpected argument".
pub fn run(
    ctx: &Ctx,
    count: usize,
    revisions: Option<String>,
    commits: bool,
    ops: bool,
    paths: Vec<String>,
) -> Result<()> {
    if ops {
        return Err(ops_retired());
    }
    // The past-state view is what `--at-op` would need here, and it does not
    // exist yet; refuse before capturing, so a refused command writes nothing.
    ctx.refuse_past("ff log")?;
    crate::capture::pre_best_effort(&crate::provenance::pre_ff(ctx));
    run_inner(ctx, count, revisions, commits, paths)
}

/// A removal, not a rename: `ff op log` is a different command with a
/// different output shape and its own `-r`, so the redirect names it rather
/// than translating the invocation and pretending nothing moved.
fn ops_retired() -> Error {
    Error::coded(
        "usage/bad-flags",
        "--ops is gone: the operation log is its own verb, with the ids the `ff op` \
         family takes and a set language of its own",
        vec!["ff op log".into(), "ff op log 'kind(op)'".into()],
    )
}

/// A path that names nothing: refused, not answered with an empty log. A
/// sentence in the path slot is almost always a missing flag, so the exits
/// then lead with the two flag-shaped ones.
fn no_such_path(token: &str) -> Error {
    let exits = if token.chars().any(char::is_whitespace) {
        vec![
            "ff log -r <revset>".into(),
            format!("ff commit -m {token:?}"),
            "ff status".into(),
        ]
    } else {
        vec!["ff status".into(), "ff log".into()]
    };
    Error::coded(
        "usage/no-such-path",
        format!(
            "no path here matches {token:?}: `ff log` takes paths in its positional, and revisions behind -r"
        ),
        exits,
    )
}

/// Default view, jj-style: the open change (`@`) as the spine's head, then
/// the commit walk (`●` rows) with each commit's chain-segment tip beside
/// it. `--commits` forces the plain commits view and keeps Phase 0's exact
/// JSON shape.
pub fn run_inner(
    ctx: &Ctx,
    count: usize,
    revisions: Option<String>,
    commits_only: bool,
    paths: Vec<String>,
) -> Result<()> {
    // Parsed before the repository is even opened: the grammar is pure, so a
    // misspelled revset fails the same way in a repo and out of one.
    let revs = match &revisions {
        Some(src) => Some(Revset::parse(src)?),
        None => None,
    };
    let mut repo = ff_core::discover(".")?;
    let limit = if count == 0 { None } else { Some(count) };

    // A selector that names nothing is refused, not answered with an empty
    // log — before either view walks.
    for path in &paths {
        if !ff_core::path_exists(&repo, path)? {
            return Err(no_such_path(path));
        }
    }

    if commits_only {
        return commits_view(&mut repo, ctx.json, limit, revs, paths);
    }

    let open = ff_core::open_change(&repo)?;
    // Destructured rather than held: the `Log` borrows the repository, and
    // `segment_anchors` below needs it back.
    let ff_core::Log {
        open: open_in_set,
        entries,
    } = ff_core::log(
        &mut repo,
        &LogOptions {
            limit,
            revs,
            paths: paths.clone(),
        },
    )?;
    let commits: Vec<ff_core::LogEntry> = entries.collect::<Result<_>>()?;
    // The `@` row appears iff the open change touches the paths — the same
    // membership rule `-r` already has, narrowed by path, and it composes:
    // with both `-r` and paths the open change must be in the set *and*
    // touch them.
    let open_in_set = open_in_set
        && (paths.is_empty() || {
            let stat = ff_core::change_diff(
                &repo,
                &ff_core::DiffOptions {
                    hunks: false,
                    paths: paths.clone(),
                },
            )?;
            !stat.files.is_empty()
        });
    let ids: Vec<String> = commits.iter().map(|entry| entry.id.clone()).collect();
    let segments = ff_core::segment_anchors(&repo, &ids)?;

    // Each displayed commit's session is the tag (if any) its own
    // chain-segment anchor operation carried — "the operation" a commit row
    // corresponds to, per `segment_anchors`. One targeted message read per
    // anchor already found, bounded by the commits already fetched: no
    // second chain walk. Only the machine surface spends it: a tag is a
    // property of the operation rather than a view over the rows, so nothing
    // groups by it here.
    let row_sessions: Vec<Option<String>> = if ctx.json {
        commits
            .iter()
            .map(|entry| match segments.get(&entry.id) {
                Some(anchor) => crate::session::tag_of(&repo, anchor),
                None => Ok(None),
            })
            .collect::<Result<_>>()?
    } else {
        vec![None; commits.len()]
    };

    if ctx.json {
        // `commits` key contract preserved; `id_letters` is composed at this
        // edge — the model stays hex. Every row also carries `session`, null
        // when the anchor operation wore no tag.
        let mut commit_values = Vec::with_capacity(commits.len());
        for (entry, sess) in commits.iter().zip(&row_sessions) {
            let mut value = serde_json::to_value(entry).map_err(Error::repo)?;
            if let serde_json::Value::Object(ref mut map) = value {
                map.insert("session".into(), serde_json::json!(sess));
            }
            commit_values.push(value);
        }
        // The `open` key is always present; under `-r` it is null when the
        // set does not contain the open change. Dropping the key instead
        // would make a consumer's `data.open` mean "old fufu" one moment and
        // "@ is not in this set" the next.
        let open_value = if open_in_set {
            serde_json::json!({
                "branch": open.branch,
                "id": open.id,
                "id_letters": open.id.as_deref().map(ff_core::snapid::encode),
                "base": open.base,
                "subject": open.subject,
                "time": open.time,
                "clean": open.clean,
                "pending": open.pending,
                "pending_short": open.pending.as_deref().map(ff_core::sha::short),
            })
        } else {
            serde_json::Value::Null
        };
        let payload = serde_json::json!({
            "commits": commit_values,
            "open": open_value,
        });
        crate::machine::emit("log", &payload)?;
        return Ok(());
    }

    use std::io::Write as _;
    crate::render::init_palette(&repo);
    let mut ids: Vec<String> = segments.values().cloned().collect();
    ids.extend(open.id.clone());
    let lens = crate::cmd::evolog::displayed_prefix_lens(&repo, &ids)?;
    let now = now_secs();
    let mut out = crate::pager::LogOut::new(&repo, false);
    let colored = out.colored();
    let result = (|| -> std::io::Result<()> {
        // The `@` row is printed iff the open change is a member of the set.
        // Without `-r` that is always, exactly as before; with `-r` it is the
        // honest reading — `ff log -r main` is a question about `main`, and
        // an `@` row on the answer would be a row nobody asked for.
        if open_in_set {
            let change_display = crate::render::ChangeRowDisplay {
                subject: open.subject.as_deref(),
                born: open.base.is_some(),
                clean: open.clean,
                id: open.id.as_deref(),
                pending: open.pending.as_deref(),
                time: open.time,
            };
            writeln!(
                out,
                "{}",
                crate::render::change_row(&change_display, &lens, now, colored)
            )?;
        }

        for entry in &commits {
            let segment = segments.get(&entry.id).map(String::as_str);
            let commit_display = crate::render::CommitRowDisplay {
                id: &entry.id,
                subject: &entry.subject,
                time: entry.time,
            };
            writeln!(
                out,
                "{}",
                crate::render::commit_row(&commit_display, segment, &lens, now, colored)
            )?;
        }
        Ok(())
    })();
    out.finish();
    result.map_err(Error::repo)
}

/// Phase 0's commits view, byte-stable: `{"commits":[...]}`.
fn commits_view(
    repo: &mut ff_core::gix::Repository,
    json: bool,
    limit: Option<usize>,
    revs: Option<Revset>,
    paths: Vec<String>,
) -> Result<()> {
    // Commits only, so the set's `open` membership has nothing to render
    // here — `--commits` is the plain history view of whatever set it is
    // given, and the open change has no commit to put in it.
    if json {
        let entries = ff_core::log(repo, &LogOptions { limit, revs, paths })?.entries;
        let commits: Vec<_> = entries.collect::<Result<_>>()?;
        // Envelope object so future fields can be added without breaking consumers.
        let payload = serde_json::json!({ "commits": commits });
        return crate::machine::emit("log", &payload);
    }

    // Through the log family's writer, not `println!`, for the two reasons
    // the other four views already have one. It pages on a TTY, which
    // `ff log` promises and this view was quietly not doing; and a closed
    // pipe comes back as an error instead of a panic, so
    // `ff log --commits | head` ends the way every other log does. The bytes
    // are unchanged — `log_row` takes no color, so paged, piped and direct
    // all render identically.
    //
    // The writer is built before the walk because `entries` borrows the
    // repository for its whole lifetime, and the walk stays lazy: collecting
    // it here to dodge that would make `--commits -n 0` read every commit in
    // the repository before printing the first row.
    use std::io::Write as _;
    let now = now_secs();
    let mut out = crate::pager::LogOut::new(repo, false);
    let entries = ff_core::log(repo, &LogOptions { limit, revs, paths })?.entries;

    // The row error and the write error stay separate kinds: a bad row is
    // this command's failure and keeps its code, while a closed pipe is the
    // reader's business and is not a failure at all.
    let mut wrote: std::io::Result<()> = Ok(());
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) => {
                out.finish();
                return Err(err);
            }
        };
        if let Err(err) = writeln!(out, "{}", crate::render::log_row(&entry, now)) {
            wrote = Err(err);
            break;
        }
    }
    out.finish();
    // `head` closing the pipe is the ordinary end of a piped log, so it exits
    // clean the way git does; any other write error still is one.
    match wrote {
        Err(err) if err.kind() != std::io::ErrorKind::BrokenPipe => Err(Error::repo(err)),
        _ => Ok(()),
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
