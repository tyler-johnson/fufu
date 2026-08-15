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
//! Ambiguity is refused here, never ranked. `<name>` is looked up as a ref
//! and as an object prefix unconditionally, with neither winning, because the
//! silent precedence this replaces — branch first, then rev-parse — resolved
//! a name to a branch even when a commit of the same spelling existed, and
//! said nothing.

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

/// One resolved revision leaf, plus the name the resolver actually used.
pub struct Leaf {
    pub rev: Rev,
    /// The short branch name, when the base canonicalized to
    /// `refs/heads/<name>` and no suffix followed. A suffix means the token
    /// no longer names that branch's tip, so it no longer earns the name.
    pub name: Option<String>,
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
        Some(canonicalize(repo, base)?)
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

    let name = match (&canonical, suffix.is_empty()) {
        (Some(Base::Ref(full)), true) => full.strip_prefix("refs/heads/").map(str::to_string),
        _ => None,
    };
    Ok(Leaf {
        rev: Rev::Commit(ops::CommitId::new(id)),
        name,
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

/// Turn a base into one of the three shapes gix may see, refusing anything
/// that two address spaces could both claim.
fn canonicalize(repo: &gix::Repository, base: &str) -> Result<Base> {
    if base == "HEAD" {
        return Ok(Base::Head);
    }
    if base == "trunk" {
        return canonical_trunk(repo);
    }

    // Both lookups, unconditionally, with neither winning.
    let as_ref = ref_candidate(repo, base)?;
    let as_object = object_candidate(repo, base)?;
    match (as_ref, as_object) {
        (Some(full), Some(id)) => Err(ambiguous(base, &full, id)),
        (Some(full), None) => Ok(Base::Ref(full)),
        (None, Some(id)) => Ok(Base::Sha(id.to_string())),
        (None, None) => {
            // Nothing in revision space answers to it. Before saying so,
            // check the other address space — an operation id typed here is a
            // reader who has the right id and the wrong verb, and telling
            // them that is worth more than telling them nothing exists. The
            // check is second rather than first so a branch really named in
            // the letters alphabet keeps its own meaning.
            if let Some(op) = op_named(repo, base)? {
                return Err(op_in_rev_position(&op));
            }
            Err(unknown_revision(base))
        }
    }
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

/// The letters-spelled operation this base names, if it names one. Both
/// halves matter: the alphabet decodes, and the log actually holds an
/// operation at that prefix.
fn op_named(repo: &gix::Repository, base: &str) -> Result<Option<String>> {
    if base.len() < MIN_HEX_LEN || base.len() > 40 {
        return Ok(None);
    }
    let Some(hex) = crate::snapid::decode(base) else {
        return Ok(None);
    };
    for candidate in ops::index::prefix_matches(repo, &hex)? {
        if ops::is_op_commit(repo, candidate)? {
            return Ok(Some(base.to_string()));
        }
    }
    Ok(None)
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
    let mut out: Vec<gix::ObjectId> = Vec::new();
    let platform = repo.references().map_err(Error::repo)?;
    for reference in platform.all().map_err(Error::repo)? {
        // A damaged or dangling ref is skipped rather than fatal: the set a
        // revset denotes must not depend on some unrelated ref being intact.
        let Ok(mut reference) = reference else {
            continue;
        };
        let name = reference.name().as_bstr().to_string();
        if !VISIBLE_PREFIXES.iter().any(|p| name.starts_with(p)) {
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
    if let Some(id) = open_commit(repo)?
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

fn ambiguous(base: &str, full_ref: &str, id: gix::ObjectId) -> Error {
    Error::coded(
        "usage/revset-ambiguous",
        format!("`{base}` is both the ref {full_ref} and the object {id}; fufu will not pick one"),
        vec![format!("ff log -r {full_ref}"), format!("ff log -r {id}")],
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

fn op_in_rev_position(token: &str) -> Error {
    Error::coded(
        "usage/op-in-rev-position",
        format!(
            "`{token}` is an operation, and `-r` takes revisions. Operations are their own \
             address space: they are what `--at-op` and `ff op show` read"
        ),
        vec![format!("ff op show {token}"), "ff op log".into()],
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
    /// `start.rs` had. That caller is exempted by name below because a later
    /// pass routes it through here and deletes it; nothing else may join it.
    #[test]
    fn rev_parse_has_exactly_one_caller() {
        const EXEMPT: [&str; 2] = [
            // This file: the door itself, plus this test naming it.
            "revset/resolve.rs",
            // Scheduled for deletion when `start` routes through the revset.
            "ff-core/src/start.rs",
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
