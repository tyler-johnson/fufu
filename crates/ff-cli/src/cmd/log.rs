use ff_core::{ChangeStat, Error, LogOptions, Result, revset::Revset};

use crate::cmd::fileview::{self, Depth, FileView, Flags};
use crate::ctx::Ctx;

/// `revisions` is `-r`: the set the rows come from. It replaces the source of
/// the rows and nothing else, so it composes with `--commits`.
///
/// `paths` is the positional: the files or directories the rows must touch,
/// and the open change's row must touch them too.
///
/// `view` is the flags that pick what the rows say.
pub fn run(
    ctx: &Ctx,
    count: usize,
    revisions: Option<String>,
    view: View,
    paths: Vec<String>,
) -> Result<()> {
    if view.ops {
        return Err(ops_retired());
    }
    // The past-state view is what `--at-op` would need here, and it does not
    // exist yet.
    ctx.refuse_past("ff log")?;
    // Nothing under a row unless asked: the compact log is the default.
    let files = FileView::resolve(view.files, Depth::Skip)?;
    run_inner(ctx, count, revisions, &view, files, paths)
}

/// The view flags, gathered so the entry point stays under clippy's argument
/// count as they accumulate.
pub struct View {
    /// `--commits`: the plain commits view.
    pub commits: bool,
    /// The retired `--ops`, kept as a hidden flag only so typing it is
    /// answered rather than met with a bare "unexpected argument".
    pub ops: bool,
    /// `--signatures`: verify each signed row.
    pub signatures: bool,
    /// `--body`: each row's message body under its subject.
    pub body: bool,
    /// `-p`, `--stat`, `--name-only`, and `-U`: what hangs under each row
    /// for the files it changed.
    pub files: Flags,
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

/// Default view, jj-style: the open change (`@`) as the spine's head, then
/// the commit walk (`●` rows) with each commit's chain-segment tip beside
/// it. `--commits` forces the plain commits view and keeps Phase 0's exact
/// JSON shape.
pub fn run_inner(
    ctx: &Ctx,
    count: usize,
    revisions: Option<String>,
    view: &View,
    files: FileView,
    paths: Vec<String>,
) -> Result<()> {
    let View {
        commits: commits_only,
        signatures,
        body,
        ..
    } = *view;
    // Parsed before the repository is even opened: the grammar is pure, so a
    // misspelled revset fails the same way in a repo and out of one.
    let revs = match &revisions {
        Some(src) => Some(Revset::parse(src)?),
        None => None,
    };
    let mut repo = ff_core::discover(".")?;
    let limit = if count == 0 { None } else { Some(count) };

    // A selector that names nothing is refused, not answered with an empty
    // log — before either view walks. A sentence in the path slot is almost
    // always a missing flag, so the exits then lead with the two
    // flag-shaped ones.
    crate::cmd::paths::require(
        &repo,
        "log",
        "takes paths in its positional, and revisions behind -r",
        &paths,
        |token| {
            if token.chars().any(char::is_whitespace) {
                vec![
                    "ff log -r <revset>".into(),
                    format!("ff commit -m {token:?}"),
                    "ff status".into(),
                ]
            } else {
                vec!["ff status".into(), "ff log".into()]
            }
        },
    )?;

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
                    ..Default::default()
                },
            )?;
            !stat.files.is_empty()
        });
    // What hangs under each row for its files, in the view's depth: each
    // commit measured against its first parent the way `ff show` measures
    // it, `None` for a merge, and the open change against HEAD. Nothing is
    // read under the default depth, so the compact log costs what it did.
    // The path-membership read above stays its own stat read rather than
    // being folded in here: under a view it is one extra pass, and not
    // worth entangling the membership rule with the depth.
    let row_stats: Vec<Option<ChangeStat>> = if files.depth == Depth::Skip {
        vec![None; commits.len()]
    } else {
        let opts = files.diff_options(paths.clone());
        commits
            .iter()
            .map(|entry| {
                let id =
                    ff_core::gix::ObjectId::from_hex(entry.id.as_bytes()).map_err(Error::repo)?;
                ff_core::commit_diff(&repo, id, &opts)
            })
            .collect::<Result<_>>()?
    };
    let open_stat: Option<ChangeStat> = if open_in_set && files.depth != Depth::Skip {
        Some(ff_core::change_diff(
            &repo,
            &files.diff_options(paths.clone()),
        )?)
    } else {
        None
    };
    // The op anchor survives only on the machine surface, as each row's
    // `session`: the human render's letters column is the change id, read
    // off the commit, so it no longer walks the chain.
    let segments = if ctx.json {
        let ids: Vec<String> = commits.iter().map(|entry| entry.id.clone()).collect();
        ff_core::segment_anchors(&repo, &ids)?
    } else {
        std::collections::HashMap::new()
    };

    // Whether a row is signed is already known — it came off the object with
    // the subject. `--signatures` buys the other question, whether the
    // signature is any good, and that one costs a signer run apiece. The
    // batch verifies them in parallel, since a page of them is a page of
    // process startup; unsigned rows are skipped, so the cost stays
    // proportional to how much there is to check.
    let row_signatures: Vec<Option<ff_core::sign::verify::SigStatus>> = if signatures {
        let ids: Vec<ff_core::gix::ObjectId> = commits
            .iter()
            .map(|entry| ff_core::gix::ObjectId::from_hex(entry.id.as_bytes()).map_err(Error::repo))
            .collect::<Result<_>>()?;
        ff_core::sign::verify::verify_many(&repo, &ids)?
            .into_iter()
            .map(|status| status.present.then_some(status))
            .collect()
    } else {
        vec![None; commits.len()]
    };

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
        // `commits` key contract preserved. Every row also carries
        // `session`, null when the anchor operation wore no tag.
        let mut commit_values = Vec::with_capacity(commits.len());
        for (((entry, sess), sig), stat) in commits
            .iter()
            .zip(&row_sessions)
            .zip(&row_signatures)
            .zip(&row_stats)
        {
            let mut value = serde_json::to_value(entry).map_err(Error::repo)?;
            if let serde_json::Value::Object(ref mut map) = value {
                map.insert("session".into(), serde_json::json!(sess));
                // Only under `--signatures`: the key's absence is what says
                // nothing was verified, which is not the same claim as null.
                if let Some(sig) = sig {
                    map.insert(
                        "signature".into(),
                        serde_json::to_value(sig).map_err(Error::repo)?,
                    );
                }
                // Only under a file view, and null on a merge row: the
                // statement `ff show` makes in prose, that a merge has no
                // single patch. Absent would say the view was never asked.
                match stat {
                    Some(stat) => {
                        for (key, value) in fileview::json_keys(stat, files.depth) {
                            map.insert(key.into(), value);
                        }
                    }
                    None if files.depth != Depth::Skip => {
                        for key in ["changes", "insertions", "deletions"] {
                            map.insert(key.into(), serde_json::Value::Null);
                        }
                    }
                    None => {}
                }
            }
            commit_values.push(value);
        }
        // The `open` key is always present; under `-r` it is null when the
        // set does not contain the open change. Dropping the key instead
        // would make a consumer's `data.open` mean "old fufu" one moment and
        // "@ is not in this set" the next.
        let open_value = if open_in_set {
            let mut value = serde_json::json!({
                "branch": open.branch,
                "id": open.id,
                "change_id": open.change_id,
                "base": open.base,
                "subject": open.subject,
                "body": open.body,
                "time": open.time,
                "clean": open.clean,
                "pending": open.pending,
                "pending_short": open.pending.as_deref().map(ff_core::sha::short),
            });
            if let (Some(stat), serde_json::Value::Object(map)) = (&open_stat, &mut value) {
                for (key, value) in fileview::json_keys(stat, files.depth) {
                    map.insert(key.into(), value);
                }
            }
            value
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
    // The bold prefix is unique among the ids on this page plus the open
    // change's; the resolver says when a page-unique prefix is not
    // repository-unique.
    let lens = ff_core::changeid::prefix_lens(
        commits
            .iter()
            .map(|entry| entry.change_id.as_str())
            .chain(open.change_id.as_deref()),
    );
    let now = now_secs();
    let mut out = crate::pager::LogOut::new(&repo, false);
    let colored = out.colored();
    // The files under a row. A patch ends on a blank line, evolog's layout:
    // git's format has no rail, so the gap is what hands the eye back to the
    // next row. The diffstat and name-only rows carry the rail and hang
    // directly under the row, the way `ff status` hangs them under `@`. A
    // merge row and an empty stat print nothing.
    let write_files =
        |out: &mut dyn std::io::Write, stat: Option<&ChangeStat>| -> std::io::Result<()> {
            let Some(stat) = stat else { return Ok(()) };
            let block = fileview::text(stat, files.depth, colored);
            if block.is_empty() {
                return Ok(());
            }
            write!(out, "{block}")?;
            if files.depth == Depth::Patch {
                writeln!(out)?;
            }
            Ok(())
        };
    let result = (|| -> std::io::Result<()> {
        // The `@` row is printed iff the open change is a member of the set.
        // Without `-r` that is always, exactly as before; with `-r` it is the
        // honest reading — `ff log -r main` is a question about `main`, and
        // an `@` row on the answer would be a row nobody asked for.
        if open_in_set {
            let change_display = crate::render::ChangeRowDisplay {
                subject: open.subject.as_deref(),
                body: body.then_some(open.body.as_str()),
                born: open.base.is_some(),
                clean: open.clean,
                change_id: open.change_id.as_deref(),
                pending: open.pending.as_deref(),
                time: open.time,
            };
            writeln!(
                out,
                "{}",
                crate::render::change_row(&change_display, &lens, now, colored)
            )?;
            write_files(&mut out, open_stat.as_ref())?;
        }

        for ((entry, sig), stat) in commits.iter().zip(&row_signatures).zip(&row_stats) {
            let commit_display = crate::render::CommitRowDisplay {
                id: &entry.id,
                change_id: &entry.change_id,
                subject: &entry.subject,
                body: body.then_some(entry.body.as_str()),
                time: entry.time,
                // Verified, so say the verdict; otherwise the free fact.
                signature: match sig {
                    Some(status) => crate::render::SigMark::Verdict {
                        word: status.word(),
                        detail: status
                            .short_key()
                            .map(|key| format!("{} {key}", status.tool())),
                        good: status.code == 'G',
                    },
                    None if entry.signed => crate::render::SigMark::Signed,
                    None => crate::render::SigMark::None,
                },
            };
            writeln!(
                out,
                "{}",
                crate::render::commit_row(&commit_display, &lens, now, colored)
            )?;
            write_files(&mut out, stat.as_ref())?;
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
