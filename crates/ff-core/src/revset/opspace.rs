//! The set language over operations — the same grammar as revision space,
//! reading the other address space.
//!
//! One front end serves both: [`lex`](super::lex) and [`parse`](super::parse)
//! never learn which space they are in, because the difference is what a leaf
//! *denotes* and what functions exist, never how the text is shaped. What
//! changes here is everything below that.
//!
//! **Ancestry is the log link, not the parents.** An operation's parents carry
//! three unrelated relations at once — the chain in slot one, the base commit
//! in slot two, pins after that — because git has one edge type and nowhere
//! else to put them. So `~`, `::` and `..` follow `fufu-prev`, and the base is
//! reached only by naming it: `base(<op>)`, which is also the one crossing
//! back to history.
//!
//! **The live log is a chain**, which is why this is a fraction of the size of
//! the commit evaluator. There is no DAG to sort, no merge-base to find, no
//! child map to build — the walk from the tip is already newest-first and
//! already total. Two consequences fall out of that and are worth naming
//! rather than leaving to be discovered:
//!
//! - Hiding a set is hiding its newest member, since on a chain everything
//!   below a hidden operation is that operation's ancestor and hidden too.
//! - `x::` costs a bounded walk rather than a reverse index, because the
//!   descendants of `x` are exactly the operations between the tip and `x`.
//!   Principle 13 keeps that form expensive in *revision* space, where git
//!   stores no child edges; the objection has no force on a chain there is
//!   somewhere to walk down from. (`x+` stays out of both spaces, but for the
//!   other reason: it is not in the language at all, and one grammar means
//!   one grammar.)
//!
//! The universe is the live log — the walk from `refs/fufu/ops`. Operations a
//! rewind stepped off are still addressable *by id* (that is what keeps `ff op
//! restore <abandoned-id>` honest), but they are not members of any set: a
//! forked-off branch of the log is not somewhere the log is, and a complement
//! that swept it in would answer `~@` with operations the log cannot walk to.

use std::collections::HashSet;

use crate::error::{Error, Result};
use crate::ops::{OpId, OpKind, OpLog};

use super::parse::{Arg, Expr, PatternKind};
use super::pattern::Pattern;

/// One member of an operation set, carrying the key the language orders by.
/// The time travels with the member because the walk already knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpMember {
    pub id: OpId,
    pub time: i64,
}

/// Where a walk starts: whatever a sub-plan denotes, or the log's tip, which
/// is the ceiling every unbounded form stops at.
enum Tips {
    Of(Box<Plan>),
    Everything,
}

/// The plan IR. Every node is either a bounded walk of the chain or a shape
/// that genuinely has to materialize one.
enum Plan {
    /// Members already in hand, newest first.
    Set(Vec<OpMember>),
    /// The chain from `tips` backwards, stopping before `hidden`.
    Ancestors {
        tips: Tips,
        hidden: Option<Box<Plan>>,
    },
    /// Everything from the tip down to and including the oldest seed.
    Descend {
        seeds: Box<Plan>,
    },
    Union(Box<Plan>, Box<Plan>),
    /// `keep`, filtered by whether a member is (or is not) in `probe`.
    Filter {
        keep: Box<Plan>,
        probe: Box<Plan>,
        want: bool,
    },
    Latest(Box<Plan>),
    /// `heads(x)` when `heads`, `roots(x)` otherwise. On a chain these are the
    /// newest and the oldest member, which is what makes them cheap.
    Extremes {
        of: Box<Plan>,
        heads: bool,
    },
    /// A predicate over an operation's own fields, scanned across the log.
    Scan(Predicate),
}

/// What the three op-space predicates read. Each exists because captures
/// outnumber verb operations by more than an order of magnitude and an
/// unfiltered log is mostly machine noise.
enum Predicate {
    OnBranch(Pattern),
    Session(Pattern),
    Kind(OpKind),
}

/// Evaluate an expression against the operation log, newest first, lazily.
pub fn evaluate<'r>(
    repo: &'r gix::Repository,
    expr: &Expr,
) -> Result<Box<dyn Iterator<Item = Result<OpMember>> + 'r>> {
    run(repo, plan_of(repo, expr)?)
}

/// The operations an expression denotes, materialized. Used by `base()`,
/// which has to cross into history and so cannot stay lazy.
pub fn members(repo: &gix::Repository, expr: &Expr) -> Result<Vec<OpMember>> {
    evaluate(repo, expr)?.collect()
}

// ── binding ───────────────────────────────────────────────────────────────

/// Stage two, and the whole error budget: every leaf resolves and every
/// refusal is raised here, before a single operation is decoded.
fn plan_of(repo: &gix::Repository, expr: &Expr) -> Result<Plan> {
    Ok(match expr {
        Expr::Revision(token) => Plan::Set(vec![member(repo, leaf(repo, token)?)?]),
        Expr::Complement(inner) => Plan::Filter {
            keep: Box::new(universe()),
            probe: Box::new(plan_of(repo, inner)?),
            want: false,
        },
        Expr::Intersection(a, b) => Plan::Filter {
            keep: Box::new(plan_of(repo, a)?),
            probe: Box::new(plan_of(repo, b)?),
            want: true,
        },
        Expr::Union(a, b) => Plan::Union(Box::new(plan_of(repo, a)?), Box::new(plan_of(repo, b)?)),
        // `::x` is the chain below x. `x::` and `x::y` walk down from the
        // tip, which on a chain is a bounded walk rather than an index.
        Expr::DagRange { from, to } => match (from, to) {
            (None, Some(to)) => Plan::Ancestors {
                tips: Tips::Of(Box::new(plan_of(repo, to)?)),
                hidden: None,
            },
            (Some(from), None) => Plan::Descend {
                seeds: Box::new(plan_of(repo, from)?),
            },
            (Some(from), Some(to)) => Plan::Filter {
                keep: Box::new(Plan::Ancestors {
                    tips: Tips::Of(Box::new(plan_of(repo, to)?)),
                    hidden: None,
                }),
                probe: Box::new(Plan::Descend {
                    seeds: Box::new(plan_of(repo, from)?),
                }),
                want: true,
            },
            (None, None) => universe(),
        },
        // `x..y` is the chain below y with the chain below x removed — which
        // on a chain is one walk that stops when it reaches x.
        Expr::Range { from, to } => match (from, to) {
            (None, Some(to)) => Plan::Ancestors {
                tips: Tips::Of(Box::new(plan_of(repo, to)?)),
                hidden: None,
            },
            (Some(from), Some(to)) => Plan::Ancestors {
                tips: Tips::Of(Box::new(plan_of(repo, to)?)),
                hidden: Some(Box::new(plan_of(repo, from)?)),
            },
            (Some(from), None) => Plan::Ancestors {
                tips: Tips::Everything,
                hidden: Some(Box::new(plan_of(repo, from)?)),
            },
            (None, None) => universe(),
        },
        Expr::Function { name, args } => function(repo, name, args)?,
    })
}

fn universe() -> Plan {
    Plan::Ancestors {
        tips: Tips::Everything,
        hidden: None,
    }
}

/// Resolve one operation leaf.
///
/// [`OpLog::resolve`] is the whole of it, which is the point: `@`, a
/// letters-spelled id or prefix, and git's own first-parent suffixes all mean
/// here exactly what they mean at `ff op show`, and hex is refused there
/// rather than a second time here.
fn leaf(repo: &gix::Repository, token: &str) -> Result<OpId> {
    let log = OpLog::open(repo)?;
    match log.resolve(token) {
        Ok(id) => Ok(id),
        // Nothing in operation space answers to it. Before saying so, check
        // the other space — a branch name typed here is a reader with the
        // right name and the wrong verb, and the mirror of that refusal
        // already exists for an op id typed among revisions.
        Err(err) if err.id() == "op/not-found" && names_a_revision(repo, token) => {
            Err(rev_in_op_position(token))
        }
        Err(err) => Err(err),
    }
}

/// Whether a token denotes something in revision space. Best effort and
/// quiet: it only decides which of two refusals to raise.
fn names_a_revision(repo: &gix::Repository, token: &str) -> bool {
    super::resolve::leaf(repo, token).is_ok()
}

fn member(repo: &gix::Repository, id: OpId) -> Result<OpMember> {
    Ok(OpMember {
        id,
        time: OpLog::open(repo)?.get(id)?.time(),
    })
}

/// The functions operations have. Four, and each earns its place against a
/// caller that already exists: `base` because it is the only crossing back to
/// history, `on_branch` because one log spans every branch, and `session` and
/// `kind` because an unfiltered log is mostly the capture floor.
///
/// `latest`, `heads` and `roots` are not op-space functions so much as set
/// functions — they read no field either space owns, so they work in both.
fn function(repo: &gix::Repository, name: &str, args: &[Arg]) -> Result<Plan> {
    let one = |args: &[Arg]| -> Result<()> {
        if args.len() == 1 {
            Ok(())
        } else {
            Err(arity(name, 1, args.len()))
        }
    };
    Ok(match name {
        "latest" => {
            one(args)?;
            Plan::Latest(Box::new(set_arg(repo, name, &args[0])?))
        }
        "heads" | "roots" => {
            one(args)?;
            Plan::Extremes {
                of: Box::new(set_arg(repo, name, &args[0])?),
                heads: name == "heads",
            }
        }
        "on_branch" => {
            one(args)?;
            Plan::Scan(Predicate::OnBranch(pattern_arg(name, &args[0])?))
        }
        "session" => {
            one(args)?;
            Plan::Scan(Predicate::Session(pattern_arg(name, &args[0])?))
        }
        // A kind is an enum with four members, not free text, so it is matched
        // by name and a misspelling lists them. A pattern here would make
        // `kind(o)` quietly mean foreign and note as well.
        "kind" => {
            one(args)?;
            Plan::Scan(Predicate::Kind(kind_arg(&args[0])?))
        }
        // `base()` returns commits, so it is a revision-space function whose
        // *argument* is an operation set. Calling it here would be asking for
        // history in a position that takes operations.
        "base" => return Err(base_here()),
        _ => return Err(unknown_function(name)),
    })
}

fn set_arg(repo: &gix::Repository, name: &str, arg: &Arg) -> Result<Plan> {
    match arg {
        Arg::Set(expr) => plan_of(repo, expr),
        Arg::Pattern { .. } => Err(arity_kind(name, "<set>")),
    }
}

/// A bare argument's implicit kind is `substring:`, matching what the
/// revision-space predicates already do.
fn pattern_arg(name: &str, arg: &Arg) -> Result<Pattern> {
    match arg {
        Arg::Pattern { kind, value } => Pattern::new(*kind, value.clone()),
        Arg::Set(Expr::Revision(text)) => Pattern::new(PatternKind::Substring, text.clone()),
        Arg::Set(_) => Err(arity_kind(name, "<pattern>")),
    }
}

fn kind_arg(arg: &Arg) -> Result<OpKind> {
    let word = match arg {
        Arg::Set(Expr::Revision(text)) => text.clone(),
        Arg::Pattern { value, .. } => value.clone(),
        Arg::Set(_) => return Err(arity_kind("kind", "<kind>")),
    };
    OpKind::from_str(&word).ok_or_else(|| bad_kind(&word))
}

// ── evaluation ────────────────────────────────────────────────────────────

fn run<'r>(
    repo: &'r gix::Repository,
    plan: Plan,
) -> Result<Box<dyn Iterator<Item = Result<OpMember>> + 'r>> {
    match plan {
        Plan::Set(members) => Ok(Box::new(members.into_iter().map(Ok))),
        Plan::Ancestors { tips, hidden } => {
            let start = start_of(repo, tips)?;
            // Hiding a set is hiding its newest member: below it, everything
            // is its ancestor and hidden with it.
            let stop: HashSet<OpId> = match hidden {
                Some(plan) => materialize(repo, *plan)?
                    .into_iter()
                    .map(|m| m.id)
                    .collect(),
                None => HashSet::new(),
            };
            Ok(Box::new(chain(repo, start).take_while(move |m| match m {
                Ok(m) => !stop.contains(&m.id),
                Err(_) => true,
            })))
        }
        Plan::Descend { seeds } => {
            // The descendants of a set are everything between the tip and its
            // oldest member — so walk down and stop once every seed is behind
            // us. Bounded by that segment, never by the log.
            let mut wanted: HashSet<OpId> = materialize(repo, *seeds)?
                .into_iter()
                .map(|m| m.id)
                .collect();
            if wanted.is_empty() {
                return Ok(Box::new(std::iter::empty()));
            }
            let mut out = Vec::new();
            for m in chain(repo, OpLog::open(repo)?.tip()?) {
                let m = m?;
                wanted.remove(&m.id);
                out.push(m);
                if wanted.is_empty() {
                    break;
                }
            }
            Ok(Box::new(out.into_iter().map(Ok)))
        }
        Plan::Union(a, b) => Ok(Box::new(Merge {
            left: run(repo, *a)?.peekable(),
            right: run(repo, *b)?.peekable(),
            seen: HashSet::new(),
        })),
        Plan::Filter { keep, probe, want } => {
            let probe: HashSet<OpId> = materialize(repo, *probe)?
                .into_iter()
                .map(|m| m.id)
                .collect();
            Ok(Box::new(run(repo, *keep)?.filter(move |m| match m {
                Ok(m) => probe.contains(&m.id) == want,
                Err(_) => true,
            })))
        }
        Plan::Latest(of) => Ok(Box::new(run(repo, *of)?.take(1))),
        Plan::Extremes { of, heads } => {
            // A chain is totally ordered, so the heads of a set are its newest
            // member and the roots its oldest. Both are one pass.
            let members = materialize(repo, *of)?;
            let pick = if heads {
                members.first().copied()
            } else {
                members.last().copied()
            };
            Ok(Box::new(pick.into_iter().map(Ok)))
        }
        Plan::Scan(pred) => {
            let tip = OpLog::open(repo)?.tip()?;
            Ok(Box::new(chain(repo, tip).filter_map(move |m| match m {
                Err(err) => Some(Err(err)),
                Ok(m) => match matches(repo, m.id, &pred) {
                    Ok(true) => Some(Ok(m)),
                    Ok(false) => None,
                    Err(err) => Some(Err(err)),
                },
            })))
        }
    }
}

/// The one walk: the log from `start` backwards along `fufu-prev`, lazily, so
/// `-r '::@' -n 25` costs twenty-five decodes at any depth.
fn chain<'r>(
    repo: &'r gix::Repository,
    start: Option<OpId>,
) -> Box<dyn Iterator<Item = Result<OpMember>> + 'r> {
    let mut next = start;
    Box::new(std::iter::from_fn(move || {
        let id = next.take()?;
        let log = match OpLog::open(repo) {
            Ok(log) => log,
            Err(err) => return Some(Err(err)),
        };
        match log.get(id) {
            Err(err) => Some(Err(err)),
            Ok(op) => {
                next = op.prev();
                Some(Ok(OpMember {
                    id,
                    time: op.time(),
                }))
            }
        }
    }))
}

/// Where a walk starts. A set of tips on a chain is its newest member: walking
/// from any older one would only repeat what that walk already covers.
fn start_of(repo: &gix::Repository, tips: Tips) -> Result<Option<OpId>> {
    match tips {
        Tips::Everything => OpLog::open(repo)?.tip(),
        Tips::Of(plan) => Ok(materialize(repo, *plan)?.first().map(|m| m.id)),
    }
}

fn materialize(repo: &gix::Repository, plan: Plan) -> Result<Vec<OpMember>> {
    run(repo, plan)?.collect()
}

fn matches(repo: &gix::Repository, id: OpId, pred: &Predicate) -> Result<bool> {
    let op = OpLog::open(repo)?.get(id)?;
    Ok(match pred {
        Predicate::OnBranch(pattern) => op.branch().is_some_and(|b| pattern.matches(b)),
        Predicate::Session(pattern) => op.session().is_some_and(|s| pattern.matches(s)),
        Predicate::Kind(kind) => op.kind() == *kind,
    })
}

/// A union, merged on time. Both sides arrive newest first, so taking twenty
/// rows pulls at most twenty from each.
struct Merge<'r> {
    left: std::iter::Peekable<Box<dyn Iterator<Item = Result<OpMember>> + 'r>>,
    right: std::iter::Peekable<Box<dyn Iterator<Item = Result<OpMember>> + 'r>>,
    seen: HashSet<OpId>,
}

impl Iterator for Merge<'_> {
    type Item = Result<OpMember>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let take_left = match (self.left.peek(), self.right.peek()) {
                (None, None) => return None,
                (Some(Err(_)), _) => true,
                (_, Some(Err(_))) => false,
                (Some(Ok(l)), Some(Ok(r))) => l.time >= r.time,
                (Some(Ok(_)), None) => true,
                (None, Some(Ok(_))) => false,
            };
            let next = if take_left {
                self.left.next()
            } else {
                self.right.next()
            };
            match next {
                Some(Ok(m)) => {
                    if self.seen.insert(m.id) {
                        return Some(Ok(m));
                    }
                }
                other => return other,
            }
        }
    }
}

// ── refusals ──────────────────────────────────────────────────────────────

/// The mirror of `usage/op-in-rev-position`, and it usually turns up on a
/// branch name: one log spans every branch, so narrowing to one is a
/// predicate rather than a name you can write on its own.
fn rev_in_op_position(token: &str) -> Error {
    Error::coded(
        "usage/rev-in-op-position",
        format!(
            "`{token}` names a revision, and this position takes operations. One log spans \
             every branch, so narrowing to one is `on_branch()` rather than the branch's name"
        ),
        vec![
            format!("ff op log 'on_branch({token})'"),
            format!("ff log -r {token}"),
        ],
    )
}

fn base_here() -> Error {
    Error::coded(
        "usage/revset-wrong-space",
        "`base()` returns the commit an operation ran on, and this position takes \
         operations; it is the crossing back to history, so it belongs where revisions do",
        vec!["ff log -r 'base(@)'".into(), "ff op log '@'".into()],
    )
}

fn unknown_function(name: &str) -> Error {
    Error::coded(
        "usage/revset-unknown-function",
        format!(
            "no revset function named `{name}`; operations have base, on_branch, session, \
             kind, plus latest, heads and roots"
        ),
        vec![
            "ff op log 'kind(op)'".into(),
            "ff op log 'session(<name>)'".into(),
        ],
    )
}

fn arity(name: &str, want: usize, got: usize) -> Error {
    Error::coded(
        "usage/revset-arity",
        format!("{name}() takes {want} argument(s), not {got}"),
        vec!["ff op log 'kind(op)'".into()],
    )
}

fn arity_kind(name: &str, want: &str) -> Error {
    Error::coded(
        "usage/revset-arity",
        format!("{name}({want}) was given an argument of the wrong kind"),
        vec![format!("ff op log '{name}({want})'")],
    )
}

fn bad_kind(word: &str) -> Error {
    Error::coded(
        "usage/revset-arity",
        format!("no operation kind named `{word}`; there are four: capture, op, foreign, note"),
        vec![
            "ff op log 'kind(op)'".into(),
            "ff op log 'kind(capture)'".into(),
        ],
    )
}

#[cfg(test)]
mod tests {
    use ff_testsupport::Fixture;

    use super::*;
    use crate::revset::parse::parse;

    const NOW: i64 = 1_700_000_000;

    /// A log with both kinds on it, a session tag, and two branches — enough
    /// for every predicate to have something to say.
    fn logged() -> Fixture {
        let fx = Fixture::new();
        fx.set_config("user.name", "Op Sets");
        fx.set_config("user.email", "sets@test.test");
        fx.write("a.txt", "a\n");
        fx.commit("init");
        let repo = fx.repo();
        crate::ops::reconcile(&repo, NOW - 100).unwrap();

        let capture = |fx: &Fixture, body: &str, at: i64, session: Option<&str>| {
            fx.write("a.txt", body);
            let repo = fx.repo();
            let prov = crate::snapshot::Provenance::new("manual", Some(body.into()))
                .with_session(session.map(str::to_string));
            crate::ops::capture_with(
                &repo,
                &prov,
                &crate::snapshot::TakeOptions {
                    now: Some(at),
                    max_file_size: None,
                },
            )
            .unwrap();
        };
        capture(&fx, "one\n", NOW - 50, None);
        capture(&fx, "two\n", NOW - 40, Some("nightly"));
        let repo = fx.repo();
        crate::close::close(
            &repo,
            &crate::close::CloseOptions {
                message: Some("landed".into()),
                now: Some(NOW - 30),
                argv: Vec::new(),
                ..Default::default()
            },
            &crate::snapshot::Provenance::new("pre", Some("ff commit".into())),
        )
        .unwrap();
        capture(&fx, "three\n", NOW - 10, None);
        fx
    }

    fn ids(repo: &gix::Repository, src: &str) -> Vec<OpId> {
        let expr = parse(src).expect("parses");
        evaluate(repo, &expr)
            .expect("evaluates")
            .map(|m| m.map(|m| m.id))
            .collect::<Result<Vec<_>>>()
            .expect("members")
    }

    /// Ancestry follows the log link, not the parents. An operation's slot 2
    /// is the commit it ran on and the rest are pins, so a walk that followed
    /// parents would leave the log on its first step.
    #[test]
    fn ancestry_walks_the_log_and_nothing_else() {
        let fx = logged();
        let repo = fx.repo();
        let all = ids(&repo, "::@");
        assert!(all.len() >= 5, "the whole log: {}", all.len());
        for id in &all {
            assert!(
                crate::ops::is_op_commit(&repo, id.object_id()).unwrap(),
                "every member is an operation, never a base commit"
            );
        }
        // `@^` is the operation before the newest, exactly as `ff op show` reads it.
        assert_eq!(ids(&repo, "@^"), vec![all[1]]);
        assert_eq!(ids(&repo, "@~2"), vec![all[2]]);
    }

    /// The chain is totally ordered, which is what makes these cheap: ranges
    /// are a walk that stops, and heads/roots are the ends of the set.
    #[test]
    fn ranges_and_extremes_read_the_chain() {
        let fx = logged();
        let repo = fx.repo();
        let all = ids(&repo, "::@");

        // `x..y` excludes x and includes y.
        let range = ids(&repo, "@~3..@");
        assert_eq!(range, all[..3].to_vec(), "three newest, x excluded");

        // Descendants are the segment down from the tip — bounded, because on
        // a chain there is somewhere to walk down from.
        assert_eq!(ids(&repo, "@~2::"), all[..3].to_vec());
        assert_eq!(ids(&repo, "@~3::@~1"), all[1..4].to_vec());

        assert_eq!(ids(&repo, "heads(::@)"), vec![all[0]]);
        assert_eq!(
            ids(&repo, "roots(::@)"),
            vec![*all.last().unwrap()],
            "the floor is the oldest thing there is"
        );
        assert_eq!(ids(&repo, "latest(kind(capture))"), vec![all[0]]);
    }

    /// The three predicates operations have, and the set algebra over them.
    #[test]
    fn predicates_and_set_algebra() {
        let fx = logged();
        let repo = fx.repo();

        let ops = ids(&repo, "kind(op)");
        assert_eq!(ops.len(), 1, "one close");
        assert!(ids(&repo, "kind(note)").len() == 1, "the floor note");
        assert!(!ids(&repo, "kind(capture)").is_empty());

        let tagged = ids(&repo, "session(nightly)");
        assert_eq!(tagged.len(), 1, "one tagged capture");

        assert!(!ids(&repo, "on_branch(main)").is_empty());
        assert!(ids(&repo, "on_branch(nosuchbranch)").is_empty());

        // Complement is against the log, and union/intersection compose.
        let not_captures = ids(&repo, "~kind(capture)");
        assert!(not_captures.contains(&ops[0]));
        assert!(
            !not_captures
                .iter()
                .any(|id| ids(&repo, "kind(capture)").contains(id))
        );
        assert_eq!(ids(&repo, "kind(op) & kind(capture)"), Vec::<OpId>::new());
        assert_eq!(
            ids(&repo, "kind(op) | session(nightly)").len(),
            2,
            "a union of two disjoint one-member sets"
        );
    }

    /// Every refusal the space has, raised before anything is walked.
    #[test]
    fn refusals_name_the_space() {
        let fx = logged();
        let repo = fx.repo();
        let hex = crate::ops::OpLog::open(&repo)
            .unwrap()
            .tip()
            .unwrap()
            .unwrap()
            .hex();

        for (src, id) in [
            // Hex is how you say *commit*; it is not a second spelling here.
            (hex[..8].to_string(), "op/not-found"),
            // A branch name is the mirror of an op id typed among revisions.
            ("main".to_string(), "usage/rev-in-op-position"),
            // Revision-space functions have nothing to read here...
            (
                "description(x)".to_string(),
                "usage/revset-unknown-function",
            ),
            // ...and base() goes the other way, so it is refused by name.
            ("base(@)".to_string(), "usage/revset-wrong-space"),
            ("kind(nope)".to_string(), "usage/revset-arity"),
            ("kind(op, x)".to_string(), "usage/revset-arity"),
        ] {
            let expr = parse(&src).expect("parses");
            let err = match evaluate(&repo, &expr) {
                Err(err) => err,
                Ok(_) => panic!("{src} must be refused"),
            };
            assert_eq!(err.id(), id, "{src}: {err}");
        }
    }

    /// The universe is the live log. A rewind leaves operations addressable by
    /// id — that is what keeps `ff op restore <abandoned-id>` honest — but a
    /// forked-off branch is not somewhere the log *is*, so no set contains it.
    #[test]
    fn the_universe_is_the_live_log() {
        let fx = logged();
        let repo = fx.repo();
        let abandoned = crate::ops::OpLog::open(&repo)
            .unwrap()
            .tip()
            .unwrap()
            .unwrap();

        let repo = fx.repo();
        crate::undo::undo(
            &repo,
            &crate::undo::RewindOptions {
                now: Some(NOW),
                ..Default::default()
            },
            &crate::snapshot::Provenance::new("pre", Some("ff undo".into())),
        )
        .unwrap();

        let repo = fx.repo();
        assert!(
            !ids(&repo, "::@").contains(&abandoned),
            "what the pointer stepped off is not a member of any set"
        );
        // But it still resolves by id, which is the promise that matters.
        assert!(
            crate::ops::OpLog::open(&repo)
                .unwrap()
                .get(abandoned)
                .is_ok()
        );
    }
}
