//! The change id: the identity a commit keeps through rewrites, minted for
//! the open change and written into the commit as a `change-id` header.
//! Every rewrite fufu makes carries it; the open change's id moves with the
//! close and comes back with `ff undo`.

use std::path::Path;
use std::process::{Command, Output};

use ff_testsupport::Fixture;
use ff_testsupport::fixtures::null_device;

fn ff_at(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ff"))
        .current_dir(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", null_device())
        .env("GIT_CONFIG_SYSTEM", null_device())
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env_remove("GIT_AUTHOR_NAME")
        .env_remove("GIT_AUTHOR_EMAIL")
        .env_remove("GIT_AUTHOR_DATE")
        .env_remove("GIT_COMMITTER_NAME")
        .env_remove("GIT_COMMITTER_EMAIL")
        .env_remove("GIT_COMMITTER_DATE")
        .env_remove("EMAIL")
        .output()
        .expect("spawn ff")
}

fn ff(fx: &Fixture, args: &[&str]) -> Output {
    ff_at(&fx.path(), args)
}

fn stdout(out: &Output) -> String {
    String::from_utf8(out.stdout.clone()).expect("utf-8 stdout")
}

fn stderr(out: &Output) -> String {
    String::from_utf8(out.stderr.clone()).expect("utf-8 stderr")
}

fn out(output: &Output) -> String {
    format!("{}{}", stdout(output), stderr(output))
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout(output)).expect("valid json")
}

fn ok(output: Output) -> Output {
    assert!(output.status.success(), "{}", out(&output));
    output
}

fn repo() -> Fixture {
    let fx = Fixture::new();
    fx.set_config("user.name", "Change Tester");
    fx.set_config("user.email", "change@test.test");
    fx
}

fn head(fx: &Fixture) -> String {
    fx.git(&["rev-parse", "HEAD"]).trim().to_string()
}

/// A commit's change id as `ff show --json` reports it.
fn id_of(fx: &Fixture, rev: &str) -> String {
    json(&ok(ff(fx, &["--json", "show", rev])))["data"]["change_id"]
        .as_str()
        .expect("change_id on ff show --json")
        .to_string()
}

/// The `change-id` header on a commit, raw off the object.
fn header_of(fx: &Fixture, rev: &str) -> Option<String> {
    fx.git(&["cat-file", "-p", rev])
        .lines()
        .take_while(|line| !line.is_empty())
        .find_map(|line| line.strip_prefix("change-id ").map(str::to_string))
}

/// The open change's id as the branch metadata records it, read without
/// running a verb, so the read itself cannot mint one.
fn open_id_on_disk(fx: &Fixture, branch: &str) -> Option<String> {
    let path = fx.path().join(".git/fufu/branch").join(branch);
    let text = std::fs::read_to_string(path).ok()?;
    let meta: serde_json::Value = serde_json::from_str(&text).expect("branch metadata is json");
    meta["change_id"].as_str().map(str::to_string)
}

fn is_letters(s: &str) -> bool {
    s.len() == 32 && s.chars().all(|c| ('k'..='z').contains(&c))
}

/// The close writes the id as a header, and `ff show` reads the header.
#[test]
fn a_close_writes_the_header_and_show_reads_it() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("root");
    fx.write("a.txt", "b\n");
    ok(ff(&fx, &["commit", "-m", "closed"]));

    let header = header_of(&fx, "HEAD").expect("a change-id header");
    assert!(is_letters(&header), "{header}");
    assert_eq!(id_of(&fx, "HEAD"), header);

    // The human header line carries the whole id after the sha.
    let text = stdout(&ok(ff(&fx, &["show", "HEAD"])));
    let first = text.lines().next().unwrap();
    let tokens: Vec<&str> = first.split_whitespace().collect();
    assert_eq!(tokens[1], header, "{first:?}");

    // A commit git made has no header and a derived id, the same one on
    // every read.
    assert_eq!(header_of(&fx, "HEAD^"), None);
    let derived = id_of(&fx, "HEAD^");
    assert!(is_letters(&derived), "{derived}");
    assert_eq!(id_of(&fx, "HEAD^"), derived);
    assert_ne!(derived, header);
}

/// `ff describe <rev>` rewrites the commit and the id rides along.
#[test]
fn the_id_survives_a_reword() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("root");
    fx.write("a.txt", "b\n");
    ok(ff(&fx, &["commit", "-m", "first words"]));
    let before = head(&fx);
    let id = id_of(&fx, &before);

    ok(ff(&fx, &["describe", &before, "-m", "second words"]));
    let after = head(&fx);
    assert_ne!(before, after, "the reword rewrote the commit");
    assert_eq!(id_of(&fx, &after), id, "and the id stayed");
    assert_eq!(header_of(&fx, &after).as_deref(), Some(id.as_str()));
}

/// `ff restack` replays the branch's commits and each keeps its id.
#[test]
fn the_id_survives_a_restack() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    ok(ff(&fx, &["commit", "-m", "f1"]));
    let f1 = head(&fx);
    let id = id_of(&fx, &f1);

    fx.git(&["switch", "-q", "main"]);
    fx.write("m.txt", "m\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);

    ok(ff(&fx, &["restack"]));
    let replayed = head(&fx);
    assert_ne!(f1, replayed, "the restack replayed f1");
    assert_eq!(id_of(&fx, &replayed), id);
}

/// `ff absorb` rewrites the commit beneath the open change; the rewritten
/// commit is the same change, so it keeps the id.
#[test]
fn the_id_survives_an_absorb() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("root");
    fx.write("a.txt", "b\n");
    ok(ff(&fx, &["commit", "-m", "work"]));
    let before = head(&fx);
    let id = id_of(&fx, &before);

    fx.write("a.txt", "c\n");
    ok(ff(&fx, &["absorb"]));
    let after = head(&fx);
    assert_ne!(before, after, "the absorb rewrote the commit");
    assert_eq!(id_of(&fx, &after), id);
}

/// `ff pull`'s replay onto the moved base carries the id too.
#[test]
fn the_id_survives_a_pull_replay() {
    let fx = repo();
    fx.write("root.txt", "root\n");
    fx.commit("root");
    fx.git(&["switch", "-q", "-c", "feature"]);
    fx.write("f.txt", "f\n");
    ok(ff(&fx, &["commit", "-m", "f1"]));
    let f1 = head(&fx);
    let id = id_of(&fx, &f1);

    fx.git(&["switch", "-q", "main"]);
    fx.write("m.txt", "m\n");
    fx.commit("m1");
    fx.git(&["switch", "-q", "feature"]);

    ok(ff(&fx, &["pull", "--no-fetch"]));
    let replayed = head(&fx);
    assert_ne!(f1, replayed, "the pull replayed f1");
    assert_eq!(id_of(&fx, &replayed), id);
}

/// `ff undo` of a close puts the id back on the open change, and `ff redo`
/// takes it off again: the identity moves with the close both ways.
#[test]
fn undo_of_a_close_puts_the_id_back_on_the_open_change() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("root");
    fx.write("a.txt", "b\n");
    let open = json(&ok(ff(&fx, &["--json", "log"])))["data"]["open"]["change_id"]
        .as_str()
        .expect("the open change has an id once captured")
        .to_string();
    assert!(is_letters(&open), "{open}");

    ok(ff(&fx, &["commit", "-m", "closed"]));
    let log = json(&ok(ff(&fx, &["--json", "log"])));
    assert_eq!(log["data"]["commits"][0]["change_id"], open);
    assert!(
        log["data"]["open"]["change_id"].is_null(),
        "the id left with the close: {log}"
    );

    ok(ff(&fx, &["undo"]));
    let log = json(&ok(ff(&fx, &["--json", "log"])));
    assert_eq!(
        log["data"]["open"]["change_id"], open,
        "undo put the id back on @: {log}"
    );
    assert_ne!(log["data"]["commits"][0]["change_id"], open);

    ok(ff(&fx, &["redo"]));
    let log = json(&ok(ff(&fx, &["--json", "log"])));
    assert_eq!(log["data"]["commits"][0]["change_id"], open);
    assert!(
        log["data"]["open"]["change_id"].is_null(),
        "redo took it off again: {log}"
    );
}

/// A partial close leaves a remainder on disk, and the remainder is a change
/// with an id of its own the moment the close lands — before any capture.
#[test]
fn a_partial_close_gives_the_remainder_a_fresh_id_at_once() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.write("b.txt", "b\n");
    fx.commit("root");
    fx.write("a.txt", "a2\n");
    fx.write("b.txt", "b2\n");
    ok(ff(&fx, &[]));
    let open = open_id_on_disk(&fx, "main").expect("the capture minted an id");

    ok(ff(&fx, &["commit", "a.txt", "-m", "just a"]));
    assert_eq!(id_of(&fx, "HEAD"), open, "the slice wears the open id");
    let remainder = open_id_on_disk(&fx, "main").expect("the remainder has an id already");
    assert!(is_letters(&remainder), "{remainder}");
    assert_ne!(remainder, open, "and it is a fresh one");

    // The @ row wears it, and the close of the remainder writes it.
    let status = json(&ok(ff(&fx, &["--json", "status"])));
    assert_eq!(status["data"]["open"]["change_id"], remainder);
    ok(ff(&fx, &["commit", "-m", "the rest"]));
    assert_eq!(id_of(&fx, "HEAD"), remainder);
    assert_eq!(open_id_on_disk(&fx, "main"), None);
}

/// `ff commit -b <fresh>` from a named branch lands on the new branch and
/// leaves the old one behind: the id and the pending description both go
/// with the commit, and the branch left behind keeps neither.
#[test]
fn a_fresh_branch_close_clears_the_branch_left_behind() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("root");
    fx.write("a.txt", "b\n");
    ok(ff(&fx, &["describe", "-m", "planned"]));
    let open = open_id_on_disk(&fx, "main").expect("describe minted an id");

    ok(ff(&fx, &["commit", "-b", "fresh"]));
    assert_eq!(
        fx.git(&["rev-parse", "--abbrev-ref", "HEAD"]).trim(),
        "fresh"
    );
    assert_eq!(id_of(&fx, "HEAD"), open);
    assert_eq!(fx.git(&["log", "-1", "--format=%s"]).trim(), "planned");
    assert!(
        !fx.path().join(".git/fufu/branch/main").exists(),
        "main keeps neither the id nor the description"
    );
    assert_eq!(open_id_on_disk(&fx, "fresh"), None);
}

/// A detached tree closes nothing and an editing session amends a commit
/// that already has an id, so a capture mints nothing for either.
#[test]
fn no_id_is_minted_for_a_detached_tree_or_a_session() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    let c0 = fx.commit("c0");
    fx.write("a.txt", "b\n");
    fx.commit("c1");

    fx.git(&["checkout", "-q", "--detach"]);
    fx.write("a.txt", "detached\n");
    ok(ff(&fx, &[]));
    let entries: Vec<String> = std::fs::read_dir(fx.path().join(".git/fufu/branch"))
        .map(|dir| {
            dir.map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        entries.is_empty(),
        "a detached capture wrote branch metadata: {entries:?}"
    );
    fx.git(&["checkout", "-q", "--", "a.txt"]);
    fx.git(&["switch", "-q", "main"]);

    ok(ff(&fx, &["edit", &c0]));
    let session = fx
        .git(&["rev-parse", "--abbrev-ref", "HEAD"])
        .trim()
        .to_string();
    fx.write("a.txt", "amended\n");
    ok(ff(&fx, &[]));
    assert_eq!(
        open_id_on_disk(&fx, &session),
        None,
        "a session's capture minted an id"
    );
    // The @ row wears the amended commit's id instead.
    let status = json(&ok(ff(&fx, &["--json", "status"])));
    assert_eq!(status["data"]["open"]["change_id"], id_of(&fx, &c0));
}

/// The map's `@` row and `ff log`'s agree on the open change's id, and it is
/// its own, not the tip's.
#[test]
fn the_map_shows_the_open_id_beside_the_tip() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("root");
    fx.write("a.txt", "b\n");
    ok(ff(&fx, &["commit", "-m", "tip"]));
    fx.write("a.txt", "c\n");

    let open = json(&ok(ff(&fx, &["--json", "status"])))["data"]["open"]["change_id"]
        .as_str()
        .unwrap()
        .to_string();
    let tip = id_of(&fx, "HEAD");
    let map = stdout(&ok(ff(&fx, &[])));
    let at = map.lines().next().unwrap();
    assert_eq!(at.split_whitespace().nth(1).unwrap(), &open[..8], "{map}");
    let bullet = map
        .lines()
        .find(|l| l.contains(&tip[..8]))
        .expect("the tip's row");
    assert!(bullet.starts_with('●'), "{bullet}");
    assert_ne!(&open[..8], &tip[..8]);

    let json_map = json(&ok(ff(&fx, &["--json"])));
    let rows = json_map["data"]["rows"].as_array().unwrap();
    assert_eq!(rows[0]["node"]["change_id"], open, "{json_map}");
    assert_eq!(rows[1]["node"]["change_id"], tip, "{json_map}");
}

// --- revision slots ---

/// A repository with one commit fufu closed, standing on `main` with a clean
/// tree. Returns the commit's sha and its change id.
fn closed(fx: &Fixture) -> (String, String) {
    fx.write("a.txt", "a\n");
    fx.commit("root");
    fx.write("a.txt", "b\n");
    ok(ff(fx, &["commit", "-m", "closed"]));
    let sha = head(fx);
    let id = id_of(fx, &sha);
    (sha, id)
}

/// A letters token in a revision slot is a change id: whole, or any prefix
/// unique in the repository.
#[test]
fn a_change_id_names_its_commit_in_a_revision_slot() {
    let fx = repo();
    let (sha, id) = closed(&fx);

    let shown = json(&ok(ff(&fx, &["--json", "show", &id])));
    assert_eq!(shown["data"]["id"], sha, "{shown}");
    let shown = json(&ok(ff(&fx, &["--json", "show", &id[..8]])));
    assert_eq!(shown["data"]["id"], sha, "a prefix: {shown}");

    let log = json(&ok(ff(&fx, &["--json", "log", "-r", &id[..8]])));
    let rows = log["data"]["commits"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{log}");
    assert_eq!(rows[0]["id"], sha);
    assert!(log["data"]["open"].is_null(), "@ is not in the set: {log}");

    ok(ff(&fx, &["describe", &id[..8], "-m", "reworded by id"]));
    assert_eq!(
        fx.git(&["log", "-1", "--format=%s"]).trim(),
        "reworded by id"
    );
    assert_eq!(id_of(&fx, "HEAD"), id, "and the reword kept the id");
}

/// A prefix of the open change's own id is `@`, and takes what `@` takes.
#[test]
fn a_prefix_of_the_open_id_is_the_open_change() {
    let fx = repo();
    closed(&fx);
    fx.write("a.txt", "c\n");
    ok(ff(&fx, &[]));
    let open = open_id_on_disk(&fx, "main").expect("the capture minted an id");

    let by_at = stdout(&ok(ff(&fx, &["show", "@"])));
    let by_id = stdout(&ok(ff(&fx, &["show", &open[..8]])));
    assert_eq!(by_id, by_at, "the open prefix is @");
    assert!(by_id.contains("the open change on main"), "{by_id}");

    // Its suffixes are `@`'s: `^` steps onto HEAD, and it has no reflog.
    let head = fx.git(&["rev-parse", "HEAD"]).trim().to_string();
    let shown = json(&ok(ff(
        &fx,
        &["--json", "show", &format!("{}^", &open[..8])],
    )));
    assert_eq!(shown["data"]["id"], head, "{shown}");
    let out = ff(&fx, &["--json", "show", &format!("{}@{{1}}", &open[..8])]);
    assert!(!out.status.success());
    assert_eq!(json(&out)["error"]["id"], "usage/revset-open-suffix");
}

/// An operation id in a revision slot is refused toward `ff op show`: it is
/// hex like a commit, and the slot is what says it is an operation. Whole
/// at `ff show`, and as the prefix `ff op log` prints at `ff log -r`.
#[test]
fn an_operation_id_is_refused_in_a_revision_slot() {
    let fx = repo();
    closed(&fx);
    let row = json(&ok(ff(&fx, &["--json", "op", "log", "-n", "1"])))["data"]["ops"][0].clone();
    let op = row["id"].as_str().expect("an op id").to_string();
    assert_eq!(op.len(), 40, "{op}");
    assert!(op.chars().all(|c| c.is_ascii_hexdigit()), "{op}");

    let out = ff(&fx, &["--json", "show", &op]);
    assert!(!out.status.success());
    let v = json(&out);
    assert_eq!(v["error"]["id"], "usage/op-in-rev-position", "{v}");
    assert!(
        v["error"]["message"]
            .as_str()
            .unwrap()
            .contains("hex like commits"),
        "{v}"
    );

    let short = row["short_id"].as_str().expect("a short id");
    let out = ff(&fx, &["--json", "log", "-r", short]);
    assert!(!out.status.success());
    let v = json(&out);
    assert_eq!(v["error"]["id"], "usage/op-in-rev-position", "{v}");
    assert!(
        v["error"]["exits"].to_string().contains("ff op show"),
        "{v}"
    );
}

/// One change id on two visible commits — a rewrite beside a ref that still
/// holds the copy it rewrote — is divergent, and the prefix is refused by
/// name rather than resolved to either.
#[test]
fn a_divergent_change_is_refused_by_name() {
    let fx = repo();
    let (old, id) = closed(&fx);
    ok(ff(&fx, &["describe", &old, "-m", "rewritten"]));
    let new = head(&fx);
    assert_ne!(old, new);
    // A ref back at the copy the reword replaced: the same change, twice.
    fx.git(&["branch", "stale", &old]);

    let out = ff(&fx, &["--json", "show", &id]);
    assert!(!out.status.success(), "{}", stdout(&out));
    let v = json(&out);
    assert_eq!(v["error"]["id"], "usage/revset-divergent", "{v}");
    let message = v["error"]["message"].as_str().unwrap();
    assert!(
        message.contains(&old[..8]) && message.contains(&new[..8]),
        "{message}"
    );
    let exits = v["error"]["exits"].to_string();
    assert!(
        exits.contains(&format!("ff show {}", &old[..8]))
            && exits.contains(&format!("ff show {}", &new[..8])),
        "{exits}"
    );

    // Each commit is still reachable by its sha, and ff explain knows the id.
    assert!(ff(&fx, &["show", &new]).status.success());
    let explained = ff(&fx, &["explain", "usage/revset-divergent"]);
    assert!(explained.status.success(), "{}", stderr(&explained));
    assert!(stdout(&explained).contains("more than one visible commit"));

    // Move the stale ref away and the id resolves again.
    fx.git(&["branch", "-D", "stale"]);
    let shown = json(&ok(ff(&fx, &["--json", "show", &id])));
    assert_eq!(shown["data"]["id"], new, "{shown}");
}

/// A branch really named in the alphabet keeps its meaning, and colliding
/// with a change prefix is refused rather than ranked.
#[test]
fn a_letters_branch_colliding_with_a_change_prefix_is_ambiguous() {
    let fx = repo();
    let (sha, id) = closed(&fx);
    let prefix = &id[..4];
    fx.git(&["branch", prefix, "HEAD^"]);

    let out = ff(&fx, &["--json", "show", prefix]);
    assert!(!out.status.success());
    let v = json(&out);
    assert_eq!(v["error"]["id"], "usage/revset-ambiguous", "{v}");
    let message = v["error"]["message"].as_str().unwrap();
    assert!(
        message.contains(&format!("refs/heads/{prefix}")) && message.contains("the change"),
        "{message}"
    );
    // Spelled whole, the id is only the change.
    assert_eq!(
        json(&ok(ff(&fx, &["--json", "show", &id])))["data"]["id"],
        sha
    );
}

/// A change id in an operation slot is the mirror refusal, worded for an id
/// rather than for a branch name.
#[test]
fn a_change_id_in_an_op_log_expression_is_refused_toward_revisions() {
    let fx = repo();
    let (_, id) = closed(&fx);
    let out = ff(&fx, &["--json", "op", "log", &id[..8]]);
    assert!(!out.status.success());
    let v = json(&out);
    assert_eq!(v["error"]["id"], "usage/rev-in-op-position", "{v}");
    let message = v["error"]["message"].as_str().unwrap();
    assert!(message.contains("is a change id"), "{message}");
    assert!(!message.contains("on_branch"), "{message}");
    assert!(v["error"]["exits"].to_string().contains("ff log -r"), "{v}");

    // The same refusal at `--at-op`, the other slot that reads an operation.
    let out = ff(&fx, &["--json", "restore", "--all", "--at-op", &id[..8]]);
    assert!(!out.status.success());
    let v = json(&out);
    assert_eq!(v["error"]["id"], "usage/rev-in-op-position", "{v}");
    assert!(v["error"]["exits"].to_string().contains("ff log -r"), "{v}");
}

/// `ff switch <change id>` is `ff switch <sha>`: a redirect to `ff start`
/// at that commit, where it used to be `branch/not-found`.
#[test]
fn switch_to_a_change_id_redirects_to_start() {
    let fx = repo();
    let (sha, id) = closed(&fx);
    fx.write("b.txt", "b\n");
    fx.commit("above");

    let out = ok(ff(&fx, &["switch", &id[..8]]));
    let text = stdout(&out);
    assert!(text.contains("minted"), "{text}");
    assert_eq!(head(&fx), sha, "standing on the change's commit");
    assert!(
        fx.git(&["rev-parse", "--abbrev-ref", "HEAD"])
            .starts_with("ff/"),
        "on an anonymous branch"
    );
}

// --- ff evolog <rev> ---

/// `ff evolog <rev>` is the change's history across chains: the close on
/// this worktree's chain, and a reword made from another worktree's, each
/// with the commit it produced, then the captures behind the close.
#[test]
fn evolog_of_a_change_lists_its_operations_across_chains() {
    let fx = repo();
    let (closed_sha, id) = closed(&fx);

    let bay = fx.root().join("bay");
    fx.git(&["worktree", "add", "-q", "-b", "side", bay.to_str().unwrap()]);
    let out = ff_at(
        &bay,
        &["describe", &closed_sha, "-m", "reworded in the bay"],
    );
    assert!(out.status.success(), "{}", self::out(&out));
    let reworded = fx.git_in(&bay, &["rev-parse", "HEAD"]).trim().to_string();
    assert_ne!(reworded, closed_sha);

    // Both copies are visible, so the id itself is divergent; the sha names
    // the one asked about, and the history is the change's either way.
    let v = json(&ok(ff(&fx, &["--json", "evolog", &reworded])));
    let data = &v["data"];
    assert_eq!(data["change_id"], id, "{v}");
    assert_eq!(data["commit"], reworded, "{v}");
    let ops = data["operations"].as_array().expect("operations");
    let verbs: Vec<&str> = ops.iter().map(|op| op["verb"].as_str().unwrap()).collect();
    assert_eq!(verbs, ["describe", "commit"], "newest first: {v}");
    assert_eq!(
        ops[0]["commit"], reworded,
        "the reword produced the new copy"
    );
    assert_eq!(
        ops[1]["commit"], closed_sha,
        "the close produced the old one"
    );
    assert_ne!(
        ops[0]["chain"], ops[1]["chain"],
        "the reword is on the bay's chain: {v}"
    );
    for op in ops {
        assert_eq!(op["id"].as_str().unwrap().len(), 40, "{op}");
        assert!(op["short_id"].as_str().unwrap().len() >= 4, "{op}");
        assert!(op.get("session").is_some(), "{op}");
    }
    let snaps = data["snapshots"].as_array().expect("snapshots");
    assert!(
        snaps
            .iter()
            .any(|s| s["subject"].as_str().unwrap().starts_with("pre: ff commit")),
        "the close's own capture is behind it: {v}"
    );

    // The human view: operation rows, a divider, capture rows.
    let text = stdout(&ok(ff(&fx, &["evolog", &reworded])));
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0].contains("describe") && lines[0].contains(&reworded[..8]),
        "{text}"
    );
    assert!(
        lines[1].contains("commit") && lines[1].contains(&closed_sha[..8]),
        "{text}"
    );
    assert_eq!(lines[2], "captures", "{text}");
    assert!(lines.len() > 3, "capture rows follow: {text}");
}

/// A commit fufu did not close has no header and no operations: the
/// fallback is the captures on this chain that match it.
#[test]
fn evolog_of_a_headerless_commit_falls_back_to_its_captures() {
    let fx = repo();
    fx.write("a.txt", "a\n");
    fx.commit("root");
    fx.write("a.txt", "b\n");
    ok(ff(&fx, &[]));
    fx.git(&["add", "-A"]);
    fx.git(&["commit", "-q", "-m", "made by git"]);
    let sha = head(&fx);

    let v = json(&ok(ff(&fx, &["--json", "evolog", &sha])));
    let data = &v["data"];
    assert_eq!(data["operations"].as_array().unwrap().len(), 0, "{v}");
    let snaps = data["snapshots"].as_array().unwrap();
    assert!(!snaps.is_empty(), "the capture matching the commit: {v}");
    assert_eq!(data["change_id"], id_of(&fx, &sha));

    let text = stdout(&ok(ff(&fx, &["evolog", &sha])));
    assert!(
        !text.contains("captures"),
        "no divider without operations: {text}"
    );
    assert!(text.lines().count() >= 1, "{text}");

    // -p still prints the capture's patch.
    let text = stdout(&ok(ff(&fx, &["evolog", "-p", &sha])));
    assert!(text.contains("diff --git"), "{text}");

    // A commit nothing captured has nothing to show, and says so.
    let text = stdout(&ok(ff(&fx, &["evolog", "HEAD^"])));
    assert!(text.starts_with("no operations recorded for "), "{text}");
}

/// `ff evolog @` is bare `ff evolog`, and both carry the open change's id.
#[test]
fn evolog_of_the_open_change_is_the_bare_form() {
    let fx = repo();
    closed(&fx);
    fx.write("a.txt", "c\n");
    ok(ff(&fx, &[]));

    // Each run prints its own relative age, and a second can tick over
    // between two, so the rows are compared without that column.
    let bare = ageless(&stdout(&ok(ff(&fx, &["evolog"]))));
    let at = ageless(&stdout(&ok(ff(&fx, &["evolog", "@"]))));
    assert_eq!(bare, at);
    assert!(!bare.contains("captures"), "{bare}");

    let v = json(&ok(ff(&fx, &["--json", "evolog"])));
    assert_eq!(
        v["data"]["change_id"],
        open_id_on_disk(&fx, "main").unwrap(),
        "{v}"
    );
    assert!(v["data"].get("operations").is_none(), "{v}");
    let open = open_id_on_disk(&fx, "main").unwrap();
    let by_prefix = ageless(&stdout(&ok(ff(&fx, &["evolog", &open[..8]]))));
    assert_eq!(by_prefix, bare, "a prefix of the open id is @");
}

/// The text with every `<n><unit> ago` token dropped.
fn ageless(text: &str) -> String {
    text.lines()
        .map(|line| {
            let words: Vec<&str> = line.split_whitespace().collect();
            let mut kept = Vec::with_capacity(words.len());
            let mut i = 0;
            while i < words.len() {
                let is_age = words.get(i + 1) == Some(&"ago")
                    && words[i].len() >= 2
                    && words[i][..words[i].len() - 1]
                        .bytes()
                        .all(|b| b.is_ascii_digit());
                if is_age {
                    i += 2;
                } else {
                    kept.push(words[i]);
                    i += 1;
                }
            }
            kept.join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
