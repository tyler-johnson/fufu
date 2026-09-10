//! What a revision token denotes — and the one module in the workspace
//! allowed to hand text to gix.
//!
//! The invariant the guard test at the bottom pins: user text never reaches
//! `rev_parse` unmodified. What reaches it is `<canonical-base><suffixes>`,
//! where the base is a full ref path (`refs/heads/main`), a 40-character hex
//! sha, or the literal `HEAD` — and nothing else. All three are unambiguous
//! by construction, so gix's `RefsHint` never has a decision to make and its
//! documented deviation, "`@` actually stands for `HEAD`", is structurally
//! unreachable. That last part is the point rather than a side effect: fufu's
//! `@` is the open change, and a resolver that let gix see a bare `@` would
//! have shipped two meanings for one symbol.
//!
//! Handing gix a ref *name* rather than the sha it holds is deliberate too.
//! `@{1}` and `@{upstream}` navigate from a ref, so a resolver that peeled
//! first would have deleted half of gitrevisions on its way to a cleaner
//! intermediate value.
//!
//! Ambiguity is refused here, never ranked. `<name>` is looked up as a ref,
//! as an object prefix, as a change id prefix, and as the open change's id
//! unconditionally, with none winning, because the silent precedence this
//! replaces — branch first, then rev-parse — resolved a name to a branch
//! even when a commit of the same spelling existed, and said nothing.
//!
//! Letters are a change id, and nothing else is spelled in them. An
//! operation id is hex like a commit's, so an operation typed here resolves
//! as an object and is then refused by name, redirected to the verbs that
//! read one. A change id that stands on more than one visible commit — a
//! rewrite beside a ref still holding the copy it rewrote — is divergent,
//! and refused by name rather than drawn.

use std::collections::HashMap;

use crate::changeid::{self, ChangeId};
use crate::error::{Error, Result};
use crate::model::HeadState;
use crate::ops;

use super::Rev;

/// Git's own shortest-accepted object prefix. Borrowed rather than restated:
/// it is what separates an object lookup from an ordinary name below.
const MIN_HEX_LEN: usize = gix::hash::Prefix::MIN_HEX_LEN;

/// Which namespaces the language can see. Everything under `refs/fufu/` is
/// machinery — the op log, parked trees, trash — and a revset that swept it
/// in would answer `~main` with fufu's own commits.
const VISIBLE_PREFIXES: [&str; 3] = ["refs/heads/", "refs/tags/", "refs/remotes/"];

/// The namespaces a change id is looked for in first: what is local. The
/// remotes join only when nothing local answers, so an unpushed rewrite is
/// not divergent against its own pre-rewrite copy on the remote.
const LOCAL_PREFIXES: [&str; 2] = ["refs/heads/", "refs/tags/"];
const REMOTE_PREFIXES: [&str; 1] = ["refs/remotes/"];

/// How many commits a change-id lookup reads before it stops. There is no
/// index over change ids; the walk is newest-first from every visible tip,
/// and a prefix deeper than this is one to spell as a sha.
pub const CHANGE_SCAN_CAP: usize = 10_000;

/// One resolved revision leaf, plus the name the resolver actually used.
pub struct Leaf {
    pub rev: Rev,
    /// The short branch name, when the base canonicalized to
    /// `refs/heads/<name>` and no suffix followed. A suffix means the token
    /// no longer names that branch's tip, so it no longer earns the name.
    ///
    /// Local branches only, and deliberately: callers read this to mean "this
    /// token names a branch that exists here", and a tracking ref does not.
    pub name: Option<String>,
    /// The canonical ref the base resolved to, under the same no-suffix rule.
    /// A superset of `name` — a tracking ref earns this and never earns that
    /// — for the callers that want the ref rather than the local branch.
    pub full_ref: Option<String>,
}

/// What a base canonicalized to: one of the three shapes gix may see, or the
/// open change, which gix never sees at all.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Canonical {
    Base(Base),
    Open,
}

/// The canonical base — the only three shapes gix is ever shown.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Base {
    /// A full ref path, kept as a name so `@{n}` still has one to navigate.
    Ref(String),
    /// A full 40-character hex sha.
    Sha(String),
    Head,
}

impl Base {
    fn spec(&self, suffix: &str) -> String {
        match self {
            Base::Ref(name) => format!("{name}{suffix}"),
            Base::Sha(hex) => format!("{hex}{suffix}"),
            Base::Head => format!("HEAD{suffix}"),
        }
    }
}

/// Resolve one revision token. Every refusal in the language that concerns a
/// single revision is raised here, which is what keeps a bad revset priced by
/// its leaves rather than by the repository's history.
pub fn leaf(repo: &gix::Repository, token: &str) -> Result<Leaf> {
    // `@` is fufu's, and it takes no suffixes — see `open_suffix`.
    if token == "@" {
        return Ok(Leaf {
            rev: Rev::Open,
            name: None,
            full_ref: None,
        });
    }
    // `@{…}` is gitrevisions, not fufu's `@`: `@{` is not a legal ref name,
    // so the two can never be confused, and the whole token goes to gix
    // verbatim — there is no base to canonicalize, and the implied name gix
    // navigates from is `HEAD`, which is one of the three shapes anyway.
    let verbatim = token.starts_with("@{");
    if !verbatim && let Some(rest) = token.strip_prefix('@') {
        return Err(open_suffix(rest));
    }

    let (base, suffix) = if verbatim {
        (token, token)
    } else {
        split(token)
    };
    if let Some(shorthand) = range_suffix(suffix) {
        return Err(range_shorthand(base, shorthand));
    }

    let canonical = if verbatim {
        None
    } else {
        match canonicalize(repo, base)? {
            Canonical::Base(base) => Some(base),
            // A prefix of the open change's id is `@`, and takes what `@`
            // takes: no suffixes.
            Canonical::Open if suffix.is_empty() => {
                return Ok(Leaf {
                    rev: Rev::Open,
                    name: None,
                    full_ref: None,
                });
            }
            Canonical::Open => return Err(open_suffix(suffix)),
        }
    };
    let spec = match &canonical {
        Some(base) => base.spec(suffix),
        None => token.to_string(),
    };
    let id = parse_single(repo, &spec, token)?;

    // One object read per leaf, and leaves are few. An operation reached
    // through `refs/fufu/ops` or through its raw sha is still an operation.
    if ops::is_op_commit(repo, id)? {
        return Err(op_in_rev_position(token));
    }

    let full_ref = match (&canonical, suffix.is_empty()) {
        (Some(Base::Ref(full)), true) => Some(full.clone()),
        _ => None,
    };
    let name = full_ref
        .as_deref()
        .and_then(|full| full.strip_prefix("refs/heads/"))
        .map(str::to_string);
    Ok(Leaf {
        rev: Rev::Commit(ops::CommitId::new(id)),
        name,
        full_ref,
    })
}

/// The one call. Everything above exists to make its argument safe.
fn parse_single(repo: &gix::Repository, spec: &str, token: &str) -> Result<gix::ObjectId> {
    let id = repo
        .rev_parse_single(spec)
        .map_err(|_| unknown_revision(token))?;
    let object = id.object().map_err(Error::repo)?;
    Ok(object
        .peel_to_kind(gix::objs::Kind::Commit)
        .map_err(|_| not_a_commit(token))?
        .id)
}

/// Split a revision token into its base and its suffixes, at the first `^`,
/// `~`, or `@{`. The scanner already proved the token is well formed, so this
/// only has to find the seam.
fn split(token: &str) -> (&str, &str) {
    let b = token.as_bytes();
    for (i, c) in b.iter().enumerate() {
        match c {
            b'^' | b'~' => return token.split_at(i),
            b'@' if b.get(i + 1) == Some(&b'{') => return token.split_at(i),
            _ => {}
        }
    }
    (token, "")
}

/// `^!` and `^@` if either appears as a suffix in its own right. Brace groups
/// are stepped over rather than searched, because `^{/fix^!}` carries those
/// bytes inside a message pattern and means nothing by them.
fn range_suffix(suffix: &str) -> Option<&'static str> {
    let b = suffix.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'^' => {
                i += 1;
                match b.get(i) {
                    Some(b'{') => i = skip_braces(b, i),
                    Some(b'!') => return Some("^!"),
                    Some(b'@') => return Some("^@"),
                    _ => {
                        if b.get(i) == Some(&b'-') {
                            i += 1;
                        }
                        while b.get(i).is_some_and(u8::is_ascii_digit) {
                            i += 1;
                        }
                    }
                }
            }
            b'~' => {
                i += 1;
                while b.get(i).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                }
            }
            b'@' => {
                i += 1;
                i = skip_braces(b, i);
            }
            _ => i += 1,
        }
    }
    None
}

/// Past a brace group, by nesting depth, matching the scanner's own rule.
fn skip_braces(b: &[u8], mut i: usize) -> usize {
    if b.get(i) != Some(&b'{') {
        return i;
    }
    let mut depth = 0usize;
    while i < b.len() {
        match b[i] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    i
}

/// Turn a base into one of the shapes gix may see, or the open change,
/// refusing anything that two readings could both claim.
fn canonicalize(repo: &gix::Repository, base: &str) -> Result<Canonical> {
    if base == "HEAD" {
        return Ok(Canonical::Base(Base::Head));
    }
    if base == "trunk" {
        return canonical_trunk(repo).map(Canonical::Base);
    }

    // Every lookup, unconditionally, with none winning.
    let as_ref = ref_candidate(repo, base)?;
    let as_object = object_candidate(repo, base)?;
    let as_change = change_named(repo, base)?;
    let as_open = open_named(repo, base)?;

    // One reading per kind that answered, spelled the way its exit takes
    // it, so the refusal can name every one.
    let mut readings: Vec<(String, String)> = Vec::new();
    if let Some(full) = &as_ref {
        readings.push((format!("the ref {full}"), format!("ff log -r {full}")));
    }
    if let Some(id) = &as_object {
        readings.push((format!("the object {id}"), format!("ff log -r {id}")));
    }
    if let Some((id, _)) = as_change.first() {
        let what = if as_change.iter().all(|(other, _)| other == id) {
            format!("the change {id}")
        } else {
            "a prefix of more than one change".to_string()
        };
        readings.push((what, format!("ff log -r {id}")));
    }
    if as_open {
        readings.push(("the open change".to_string(), "ff log -r @".to_string()));
    }
    if readings.len() > 1 {
        return Err(ambiguous(base, &readings));
    }

    if let Some(full) = as_ref {
        return Ok(Canonical::Base(Base::Ref(full)));
    }
    if let Some(id) = as_object {
        return Ok(Canonical::Base(Base::Sha(id.to_string())));
    }
    if as_open {
        return Ok(Canonical::Open);
    }
    if let Some((id, _)) = as_change.first() {
        if as_change.iter().any(|(other, _)| other != id) {
            return Err(ambiguous_change(base));
        }
        let commits: Vec<gix::ObjectId> = as_change.iter().map(|(_, sha)| *sha).collect();
        return match commits.as_slice() {
            [one] => Ok(Canonical::Base(Base::Sha(one.to_string()))),
            many => Err(divergent(repo, base, id, many)),
        };
    }

    Err(unknown_revision(base))
}

/// Every visible commit whose change id the base is a prefix of, paired with
/// that id, newest first. Two tiers: what is local — HEAD, the branches, the
/// tags — and, only when that answers nothing, the remotes too. Each commit
/// is read once, and the walk stops at `CHANGE_SCAN_CAP`.
fn change_named(repo: &gix::Repository, base: &str) -> Result<Vec<(ChangeId, gix::ObjectId)>> {
    if base.len() < MIN_HEX_LEN
        || base.len() > changeid::LETTERS
        || crate::letters::decode(base).is_none()
    {
        return Ok(Vec::new());
    }
    let mut seen: HashMap<gix::ObjectId, ()> = HashMap::new();
    let local = tips_under(repo, &LOCAL_PREFIXES, true)?;
    let mut hits = scan_for_change(repo, base, local, &mut seen)?;
    if hits.is_empty() {
        let remote = tips_under(repo, &REMOTE_PREFIXES, false)?;
        hits = scan_for_change(repo, base, remote, &mut seen)?;
    }
    Ok(hits)
}

/// One tier of the change-id walk. `seen` carries across tiers, so a commit
/// the local tier already read is not read again from a remote tip.
fn scan_for_change(
    repo: &gix::Repository,
    base: &str,
    tips: Vec<gix::ObjectId>,
    seen: &mut HashMap<gix::ObjectId, ()>,
) -> Result<Vec<(ChangeId, gix::ObjectId)>> {
    use gix::revision::walk::Sorting;
    use gix::traverse::commit::simple::CommitTimeOrder;
    let mut hits = Vec::new();
    if tips.is_empty() {
        return Ok(hits);
    }
    let walk = repo
        .rev_walk(tips)
        .sorting(Sorting::ByCommitTime(CommitTimeOrder::NewestFirst))
        .all()
        .map_err(Error::repo)?;
    for info in walk {
        if seen.len() >= CHANGE_SCAN_CAP {
            break;
        }
        // A damaged commit ends the walk rather than failing the lookup:
        // what was read is still an answer.
        let Ok(info) = info else { break };
        if seen.insert(info.id, ()).is_some() {
            continue;
        }
        let Ok(commit) = repo.find_commit(info.id) else {
            continue;
        };
        let id = changeid::of_commit(&commit.data, &info.id);
        if id.has_prefix(base) {
            hits.push((id, info.id));
        }
    }
    Ok(hits)
}

/// Whether the base is a prefix of the open change's own id — the id on the
/// current branch's metadata, which no commit carries yet. Inside a session
/// the open change wears the amended commit's id, and the walk finds that.
fn open_named(repo: &gix::Repository, base: &str) -> Result<bool> {
    if base.len() < MIN_HEX_LEN || crate::letters::decode(base).is_none() {
        return Ok(false);
    }
    let branch = match crate::head::head_state(repo)? {
        HeadState::Detached { .. } => return Ok(false),
        head => crate::snapshot::chain::chain_name(&head),
    };
    let meta = crate::branchmeta::read(repo, &branch)?;
    Ok(meta
        .change_id
        .as_deref()
        .and_then(ChangeId::parse)
        .is_some_and(|id| id.has_prefix(base)))
}

/// `trunk` is a revision, resolved through fufu's own ladder. A literal ref
/// of that name pointing elsewhere is two answers to one word, so it is
/// refused by the same rule that governs every other base.
fn canonical_trunk(repo: &gix::Repository) -> Result<Base> {
    let literal = ref_candidate(repo, "trunk")?;
    match crate::trunk::trunk(repo) {
        Ok(t) => {
            if let Some(full) = literal
                && full != t.full_ref
                && peeled(repo, &full)? != peeled(repo, &t.full_ref)?
            {
                return Err(ambiguous_trunk(&full, &t.full_ref));
            }
            Ok(Base::Ref(t.full_ref))
        }
        // No trunk to resolve, but something is literally named `trunk`: the
        // word still denotes, so use it rather than reporting a ladder the
        // user never invoked.
        Err(err) => match literal {
            Some(full) => Ok(Base::Ref(full)),
            None => Err(err),
        },
    }
}

/// The full ref name a base denotes, by git's own precedence ladder —
/// `<name>`, `refs/<name>`, `refs/tags/<name>`, `refs/heads/<name>`,
/// `refs/remotes/<name>`, `refs/remotes/<name>/HEAD`. gix walks exactly that
/// ladder for a partial name, so borrowing it beats restating it and then
/// drifting from it.
fn ref_candidate(repo: &gix::Repository, base: &str) -> Result<Option<String>> {
    match repo.try_find_reference(base) {
        Ok(Some(r)) => Ok(Some(r.name().as_bstr().to_string())),
        // A base that cannot even be spelled as a partial ref name is not a
        // ref; that is an answer, not a failure.
        Ok(None) | Err(_) => Ok(None),
    }
}

/// The object a base denotes, when it is hex-shaped and long enough to be an
/// abbreviation git would accept.
fn object_candidate(repo: &gix::Repository, base: &str) -> Result<Option<gix::ObjectId>> {
    if base.len() < MIN_HEX_LEN || base.len() > 40 || !base.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(None);
    }
    let lowered = base.to_ascii_lowercase();
    let Ok(prefix) = gix::hash::Prefix::from_hex(&lowered) else {
        return Ok(None);
    };
    match repo.objects.lookup_prefix(prefix, None) {
        Ok(Some(Ok(id))) => Ok(Some(id)),
        Ok(Some(Err(()))) => Err(ambiguous_object(base)),
        Ok(None) => Ok(None),
        Err(err) => Err(Error::repo(err)),
    }
}

/// A ref's peeled target, for comparing two names that may hold one commit.
fn peeled(repo: &gix::Repository, full: &str) -> Result<Option<gix::ObjectId>> {
    match repo.try_find_reference(full).map_err(Error::repo)? {
        Some(mut r) => Ok(Some(r.peel_to_id_in_place().map_err(Error::repo)?.detach())),
        None => Ok(None),
    }
}

/// Every commit the language can see: the tips of the visible namespaces,
/// plus whatever HEAD is on. This is the universe a complement is taken
/// against and the ceiling an open-ended forward walk stops at.
pub fn universe_tips(repo: &gix::Repository) -> Result<Vec<gix::ObjectId>> {
    tips_under(repo, &VISIBLE_PREFIXES, true)
}

/// The commits at the tips of the given namespaces, plus HEAD's when asked.
fn tips_under(
    repo: &gix::Repository,
    prefixes: &[&str],
    with_head: bool,
) -> Result<Vec<gix::ObjectId>> {
    let mut out: Vec<gix::ObjectId> = Vec::new();
    let platform = repo.references().map_err(Error::repo)?;
    for reference in platform.all().map_err(Error::repo)? {
        // A damaged or dangling ref is skipped rather than fatal: the set a
        // revset denotes must not depend on some unrelated ref being intact.
        let Ok(mut reference) = reference else {
            continue;
        };
        let name = reference.name().as_bstr().to_string();
        if !prefixes.iter().any(|p| name.starts_with(p)) {
            continue;
        }
        let Ok(id) = reference.peel_to_id_in_place() else {
            continue;
        };
        let id = id.detach();
        if is_commit(repo, id) && !out.contains(&id) {
            out.push(id);
        }
    }
    if with_head
        && let Some(id) = open_commit(repo)?
        && !out.contains(&id)
    {
        out.push(id);
    }
    Ok(out)
}

/// The commit the open change sits on — `HEAD`'s, which is what git already
/// says. `None` on an unborn HEAD, where there is no commit yet.
pub fn open_commit(repo: &gix::Repository) -> Result<Option<gix::ObjectId>> {
    let hex = match crate::head::head_state(repo)? {
        HeadState::Unborn { .. } => return Ok(None),
        HeadState::Branch { commit, .. } | HeadState::Detached { commit } => commit,
    };
    Ok(Some(
        gix::ObjectId::from_hex(hex.as_bytes()).map_err(Error::repo)?,
    ))
}

fn is_commit(repo: &gix::Repository, id: gix::ObjectId) -> bool {
    matches!(
        repo.try_find_header(id),
        Ok(Some(header)) if header.kind() == gix::objs::Kind::Commit
    )
}

// --- refusals ---

/// We own `@`'s suffix rule precisely because we deviated on the symbol. The
/// translation is off by one — the open change sits *on* HEAD's commit — and
/// shipping a layer that quietly performed it would be a bug factory.
fn open_suffix(rest: &str) -> Error {
    Error::coded(
        "usage/revset-open-suffix",
        format!(
            "no `@{rest}`: `@` is the open change and takes no suffixes. The commit under \
             it is `HEAD`, so `@^` is `HEAD` and `@~2` is `HEAD~`"
        ),
        vec!["ff log -r HEAD".into(), "ff log -r \"HEAD~\"".into()],
    )
}

/// `x^!` and `x^@` are rev-list's range shorthands wearing a suffix's
/// clothes: neither names one revision, so neither survives into a language
/// whose ranges are its own.
fn range_shorthand(base: &str, shorthand: &'static str) -> Error {
    let exits = if shorthand == "^!" {
        vec![format!("ff log -r \"{base}^..{base}\"")]
    } else {
        vec![format!("ff log -r \"{base}^ | {base}^2\"")]
    };
    Error::coded(
        "usage/revset-range-suffix",
        format!(
            "`{base}{shorthand}` is a rev-list range, not a revision; fufu spells ranges \
             in its own set algebra"
        ),
        exits,
    )
}

/// More than one kind of thing answered to the base. Every reading is named,
/// with the spelling that means only it.
fn ambiguous(base: &str, readings: &[(String, String)]) -> Error {
    let names: Vec<&str> = readings.iter().map(|(what, _)| what.as_str()).collect();
    let listed = match names.as_slice() {
        [a, b] => format!("both {a} and {b}"),
        many => {
            let (last, rest) = many.split_last().expect("at least two readings");
            format!("{}, and {last}", rest.join(", "))
        }
    };
    Error::coded(
        "usage/revset-ambiguous",
        format!("`{base}` is {listed}; fufu will not pick one"),
        readings.iter().map(|(_, exit)| exit.clone()).collect(),
    )
}

fn ambiguous_change(base: &str) -> Error {
    Error::coded(
        "usage/revset-ambiguous",
        format!("`{base}` is a prefix of more than one change; spell more of it"),
        vec!["ff log".into()],
    )
}

/// One change id on more than one visible commit. jj draws this as `??` and
/// refuses the bare prefix; fufu refuses it by name, since the column is
/// drawn without the walk that would find the twin.
fn divergent(
    repo: &gix::Repository,
    base: &str,
    id: &ChangeId,
    commits: &[gix::ObjectId],
) -> Error {
    let named: Vec<String> = commits
        .iter()
        .map(|sha| {
            let subject = repo
                .find_commit(*sha)
                .ok()
                .and_then(|c| c.message().ok().map(|m| m.summary().to_string()))
                .unwrap_or_default();
            format!("{} \"{subject}\"", crate::sha::short_oid(*sha))
        })
        .collect();
    Error::coded(
        "usage/revset-divergent",
        format!(
            "`{base}` is the change {id}, and that change stands on {} visible commits: {}; \
             name the commit instead",
            commits.len(),
            named.join(", ")
        ),
        commits
            .iter()
            .map(|sha| format!("ff show {}", crate::sha::short_oid(*sha)))
            .collect(),
    )
}

fn ambiguous_trunk(literal: &str, resolved: &str) -> Error {
    Error::coded(
        "usage/revset-ambiguous",
        format!(
            "`trunk` is both the ref {literal} and this repository's trunk {resolved}; \
             fufu will not pick one"
        ),
        vec![
            format!("ff log -r {literal}"),
            format!("ff log -r {resolved}"),
        ],
    )
}

fn ambiguous_object(base: &str) -> Error {
    Error::coded(
        "usage/revset-ambiguous",
        format!("`{base}` is a prefix of more than one object; spell more of it"),
        vec!["ff log".into()],
    )
}

fn unknown_revision(token: &str) -> Error {
    Error::coded(
        "usage/revset-unknown-revision",
        format!("no revision here answers to `{token}`"),
        vec!["ff log".into(), "ff branch".into()],
    )
}

fn not_a_commit(token: &str) -> Error {
    Error::coded(
        "usage/revset-not-a-commit",
        format!("`{token}` names an object that is not a commit; a revset is a set of commits"),
        vec!["ff log".into()],
    )
}

/// The position is named generically on purpose. This resolver is what
/// `ff log -r`, `ff restore --from` and `ff show` all go through, so a
/// message that named `-r` was already wrong on two of the three; `ff log
/// -r` earns a place in the exits instead, which is where a spelling
/// belongs.
fn op_in_rev_position(token: &str) -> Error {
    Error::coded(
        "usage/op-in-rev-position",
        format!(
            "`{token}` is an operation, and this position takes revisions. Operations are their \
             own address space, hex like commits, and the slot decides: they are what `--at-op` \
             and `ff op show` read"
        ),
        vec![
            format!("ff op show {token}"),
            "ff op log".into(),
            "ff log -r <rev>".into(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitting_finds_the_first_seam() {
        for (token, base, suffix) in [
            ("main", "main", ""),
            ("main~2", "main", "~2"),
            ("main^", "main", "^"),
            ("refs/heads/x@{1}", "refs/heads/x", "@{1}"),
            ("x^{tree}", "x", "^{tree}"),
            ("origin/main", "origin/main", ""),
            ("origin/main@{1}", "origin/main", "@{1}"),
            ("@{upstream}", "", "@{upstream}"),
            ("a^2~3^{tree}", "a", "^2~3^{tree}"),
            // An `@` with no brace after it belongs to the name.
            ("user@host", "user@host", ""),
        ] {
            assert_eq!(split(token), (base, suffix), "{token}");
        }
    }

    #[test]
    fn range_shorthands_are_found_past_brace_groups() {
        assert_eq!(range_suffix("^!"), Some("^!"));
        assert_eq!(range_suffix("^@"), Some("^@"));
        assert_eq!(range_suffix("~2^!"), Some("^!"));
        assert_eq!(range_suffix("^2~3^{tree}"), None);
        assert_eq!(range_suffix("@{upstream}"), None);
        assert_eq!(range_suffix("^-1"), None);
        // The bytes inside a message pattern mean nothing by themselves.
        assert_eq!(range_suffix("^{/fix^!}"), None);
    }

    #[test]
    fn a_canonical_base_is_one_of_three_shapes() {
        assert_eq!(Base::Head.spec("~2"), "HEAD~2");
        assert_eq!(
            Base::Ref("refs/heads/main".into()).spec("@{1}"),
            "refs/heads/main@{1}"
        );
        assert_eq!(Base::Sha("ab".repeat(20)).spec(""), "ab".repeat(20));
    }

    #[test]
    fn the_open_change_takes_no_suffixes() {
        for rest in ["^", "~2", "@{1}"] {
            let err = open_suffix(rest);
            assert_eq!(err.id(), "usage/revset-open-suffix");
            assert!(err.to_string().contains("HEAD"), "must teach HEAD");
        }
    }

    /// Rule one, made mechanical. `rev_parse` is the door between fufu's
    /// grammar and git's, and a second caller would be a second door with no
    /// lock on it — which is exactly the shape the silent precedence in
    /// `start.rs` had before it was routed through here. Nothing is exempt
    /// now but this file, so the door has one caller and the test proves it.
    #[test]
    fn rev_parse_has_exactly_one_caller() {
        const EXEMPT: [&str; 1] = [
            // This file: the door itself, plus this test naming it.
            "revset/resolve.rs",
        ];
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/ is the manifest dir's parent")
            .to_path_buf();

        let mut offenders: Vec<String> = Vec::new();
        let mut scanned = 0usize;
        for file in rust_sources(&crates) {
            let shown = file.display().to_string().replace('\\', "/");
            // Production sources only: a test may quote the name it guards.
            if !shown.contains("/src/") {
                continue;
            }
            scanned += 1;
            if EXEMPT.iter().any(|e| shown.ends_with(e)) {
                continue;
            }
            let text = std::fs::read_to_string(&file).expect("read source");
            if text.contains("rev_parse") {
                offenders.push(shown);
            }
        }
        // A walker that silently found nothing would pass while checking
        // nothing, so it has to prove it read the tree.
        assert!(
            scanned > 20,
            "only {scanned} sources walked — the walk is broken"
        );
        assert!(
            offenders.is_empty(),
            "rev_parse belongs to revset/resolve.rs alone; also found in {offenders:#?}"
        );
    }

    /// Every `.rs` file under `dir`, recursively.
    fn rust_sources(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut found = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return found;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                found.extend(rust_sources(&path));
            } else if path.extension().is_some_and(|e| e == "rs") {
                found.push(path);
            }
        }
        found
    }
}
