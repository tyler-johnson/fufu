//! Curated error ids with prose. The single source of truth for what each
//! id means and how to leave it.

use ff_core::{Error, Result};
use std::io::Write;

pub struct Entry {
    pub id: &'static str,
    /// One line: what this error means.
    pub summary: &'static str,
    /// A short paragraph: why it happens and what the exits do.
    pub detail: &'static str,
    pub exits: &'static [&'static str],
}

pub static ENTRIES: &[Entry] = &[
    Entry {
        id: "branch/exists",
        summary: "a branch of that name already exists",
        detail: "fufu never reuses a branch name implicitly. A name that is already taken could be \
                 someone's work, and quietly landing on top of it is the one guess worth refusing. \
                 Pick another name, or switch to that branch if it was the one you meant.",
        exits: &["ff branch <name>"],
    },
    Entry {
        id: "branch/not-found",
        summary: "no branch here goes by that name",
        detail: "Names resolve against local branches, so a branch that exists on the remote but \
                 not here will not be found. Adding @<remote> fetches it and lands you on a copy. \
                 Bare ff branch lists what is local.",
        exits: &["ff branch", "ff switch <branch>@origin"],
    },
    Entry {
        id: "branch/ambiguous",
        summary: "that branch prefix matches more than one branch",
        detail: "A prefix has to name one branch, and this one names several. Every candidate is \
                 listed so you can pick; typing one more character is usually enough. Bare ff \
                 branch lists what is local.",
        exits: &["ff branch"],
    },
    Entry {
        id: "branch/already-named",
        summary: "that branch already has a proper name",
        detail: "Claiming is for anonymous branches — the ones fufu minted a petname for — and \
                 renaming a branch someone chose a name for is a different, louder act. \
                 ff describe -b does it, and is the one rename that may touch proper names.",
        exits: &["ff describe -b <name>"],
    },
    Entry {
        id: "branch/invalid-name",
        summary: "git would not accept that branch name",
        detail: "Branch names are validated by round-tripping them through git's own ref name \
                 rules rather than by a list fufu keeps, so anything git refuses is refused here \
                 in the same words. Spaces, a leading dash, `..`, and a trailing `.lock` are the \
                 usual causes.",
        exits: &[],
    },
    Entry {
        id: "branch/is-current",
        summary: "that is the branch you are on",
        detail: "Deleting the branch you are standing on would leave HEAD pointing at nothing, so \
                 fufu asks you to move first. Switching away takes the open change with it — the \
                 tree belongs to its branch, and it will be here when you come back.",
        exits: &["ff switch <branch>"],
    },
    Entry {
        id: "branch/checked-out-elsewhere",
        summary: "another worktree has that branch checked out",
        detail: "git refuses to rename or delete a branch that a linked worktree is sitting on, \
                 because that worktree's HEAD would stop resolving. gix has no such check, so \
                 fufu carries its own and reports which worktree holds it. Switch that worktree \
                 away, or remove it, and try again.",
        exits: &["git worktree list"],
    },
    Entry {
        id: "repo/bare",
        summary: "this is a bare repository, and the verb needs a working tree",
        detail: "A bare repository has no working tree, so there is nothing to snapshot, commit, \
                 restore into, or switch. Read-only verbs still work here; anything that touches \
                 files does not. Run the command from a clone that has a working tree.",
        exits: &[],
    },
    Entry {
        id: "repo/detached",
        summary: "HEAD is not on a branch",
        detail: "fufu keeps HEAD attached — every head is a real branch ref from the moment it \
                 exists — and the verbs that describe or record work need to know which branch \
                 the work belongs to. A detached HEAD usually means a raw git checkout of a \
                 commit; switching to a branch reattaches it.",
        exits: &["ff switch <branch>"],
    },
    Entry {
        id: "identity/missing",
        summary: "git has no name and email to sign work with",
        detail: "Every commit fufu writes carries an author, including the snapshots the capture \
                 floor takes on your behalf, so there is nothing sensible to do without an \
                 identity. Set it once globally, or per repository when this work belongs to a \
                 different one.",
        exits: &[
            "git config user.name <name>",
            "git config user.email <email>",
        ],
    },
    Entry {
        id: "repo/mid-operation",
        summary: "git is in the middle of something",
        detail: "A rebase, a merge, a cherry-pick or a bisect leaves the repository in a state \
                 only that operation knows how to finish, and a verb that moved refs or the \
                 working tree underneath it would strand it. Finish or abort it with git — fufu \
                 owns merges in a later phase — and the verb will run.",
        exits: &["git rebase --abort", "git merge --abort"],
    },
    Entry {
        id: "usage/bad-restore-target",
        summary: "--at was given something that is neither an age nor a date",
        detail: "--at takes a time and only a time: a compact age like 90s/15m/2h/3d/1w, or any \
                 date git itself can parse. It resolves to the operation that was current at \
                 that moment. Nothing here has to out-guess an id, which is the point of \
                 splitting the flag — an operation is named by --at-op, in letters, and a \
                 revision by --from.",
        exits: &[
            "ff restore --all --at 2h",
            "ff restore --all --at-op <op>",
            "ff op log",
        ],
    },
    Entry {
        id: "restore/nothing-selected",
        summary: "restore was given nothing to restore",
        detail: "Restore is deliberately explicit: it takes the paths you name, or --all for the \
                 whole tree, and never guesses a selection on your behalf. Either form pairs with \
                 one source flag — --from for a revision, --at-op for an operation, --at for a \
                 time — and without one the source is the commit under the open change.",
        exits: &[
            "ff restore --all",
            "ff restore <path> --from <rev>",
            "ff restore <path> --at-op <op>",
        ],
    },
    Entry {
        id: "undo/nothing",
        summary: "the operation log has nothing left to undo",
        detail: "Undo walks fufu's operation log. Either nothing has been recorded yet, or \
                 everything recorded is a note — a marker for something that happened rather than \
                 something that was done, which has no state behind it to put back. ff op log \
                 shows what the log still holds.",
        exits: &["ff op log"],
    },
    Entry {
        id: "undo/not-undoable",
        summary: "that operation has nothing in it to invert",
        detail: "ff op revert inverts the ref transitions an operation made, so it has nothing to \
                 do with the two kinds that made none. A capture only records the working tree — \
                 that invariant is what keeps the log small — and a note marks something that \
                 happened rather than something that was done. To go back to what a capture \
                 holds, restore to it: that is a worktree question, not a rollback. Note that \
                 ff undo does not land here, because it steps over runs of captures on purpose.",
        exits: &["ff op log", "ff restore --at-op <op>"],
    },
    Entry {
        id: "undo/trimmed",
        summary: "the state that undo would put back has been trimmed away",
        detail: "Undo restores the complete state an operation's predecessor recorded, and trim \
                 has dropped part of it past the keep window (fufu.keep, 90 days by default). \
                 ff op restore --force lands on whatever remains and names each missing piece \
                 instead of refusing; a longer keep window prevents the next one. Bare ff undo \
                 has no --force by design: a run whose state was trimmed is a run undo should \
                 decline rather than half-apply. Nothing was changed.",
        exits: &["ff op restore <op> --force", "ff config keep <duration>"],
    },
    Entry {
        id: "op/not-found",
        summary: "no operation goes by that id",
        detail: "Operation ids are spelled in letters (k through z) and never in hex, which is what \
                 keeps hex meaning \"commit\" everywhere in fufu. A hex-shaped id in an \
                 operation-typed position is refused for that reason rather than resolved. \
                 ff op log prints ids in exactly the form these verbs accept.",
        exits: &["ff op log"],
    },
    Entry {
        id: "op/ambiguous",
        summary: "that id prefix matches more than one operation",
        detail: "A prefix has to name one operation, and this one names several. Every candidate is \
                 listed so you can pick; typing one more letter is usually enough. ff op log bolds \
                 the shortest prefix that is unique, so an id copied from there never lands here.",
        exits: &["ff op log"],
    },
    Entry {
        id: "op/trimmed",
        summary: "that operation is no longer on the log",
        detail: "Trim drops operations past the keep window (fufu.keep, 90 days by default), so an \
                 id from an old transcript can name something real that is simply gone. The \
                 operation may still be readable as an object even when moving to it is not — \
                 restoring to something trim dropped is a different answer from restoring to \
                 something merely old. A longer keep window prevents the next one.",
        exits: &["ff op log", "ff config keep <duration>"],
    },
    Entry {
        id: "op/floor",
        summary: "there is nothing recorded before that operation",
        detail: "Undo steps back to the state before a run, and the oldest operation on the log \
                 has nothing before it — it is the floor. That happens at a fresh repository, or \
                 after trim removed everything earlier. Nothing was changed.",
        exits: &["ff op log"],
    },
    Entry {
        id: "op/unreadable",
        summary: "an operation on the log could not be decoded",
        detail: "Operations are ordinary git commits carrying a small record, and this one does not \
                 parse — a truncated write, a partial transfer, or a hand-edited ref. fufu refuses \
                 to guess at damaged state rather than acting on half of it. ff doctor checks the \
                 log's structure; the objects themselves are readable with plain git.",
        exits: &["ff doctor", "ff op log"],
    },
    Entry {
        id: "ref/contended",
        summary: "another process is holding that ref",
        detail: "Two things tried to move the same ref at once — often a second fufu command, an \
                 editor's git integration, or a hook. Nothing was changed. Contention is a fact \
                 rather than a fault: run it again once the other write finishes.",
        exits: &[],
    },
    Entry {
        id: "hook/declined",
        summary: "one of your git hooks refused the commit",
        detail: "fufu runs your pre-commit and commit-msg hooks itself, so a hook that exits \
                 non-zero stops the close exactly as it would under git. The hook's own output \
                 says why. --no-verify skips them, with the usual caveat that they were there \
                 for a reason.",
        exits: &["ff commit --no-verify"],
    },
    Entry {
        id: "editor/failed",
        summary: "the editor did not produce a description",
        detail: "The bare form of describe seeds a temporary file and opens $EDITOR on it. When \
                 the editor cannot be launched, or exits non-zero, the description is left exactly \
                 as it was rather than half-written. Passing -m skips the editor entirely.",
        exits: &["ff describe -m <msg>"],
    },
    Entry {
        id: "target/unresolvable",
        summary: "that target resolves, but not to something this verb can use",
        detail: "The spelling was understood — a target that denotes nothing raises a \
                 usage/revset- refusal naming the spelling instead. What it resolved to is the \
                 problem. On ff start that is the open change: start always opens a clean \
                 branch, so @ is the one revision it cannot fork at, however it is spelled. To \
                 move the open change onto a branch of its own, close it there with \
                 ff commit -b <name>.",
        exits: &["ff commit -b <name>", "ff log"],
    },
    Entry {
        id: "usage/revset-adjacent-operands",
        summary: "two revisions stand side by side with no operator between them",
        detail: "Most often this is jj's infix difference: fufu does not have one, because `a & ~b` \
                 already says it and a second spelling would be a second thing to learn. Git \
                 requires ~ to be followed by digits or nothing, so `a ~ b` lexes as two operands \
                 rather than an operator. The same error covers plain juxtaposition, where an \
                 operator was simply left out.",
        exits: &["ff log -r '<a> & ~<b>'"],
    },
    Entry {
        id: "usage/revset-parent-shorthand",
        summary: "there is no `x-` suffix; git already spells it `x^`",
        detail: "fufu takes gitrevisions whole, so the parent of a revision is `x^` and jj's `x-` \
                 would be a second way to say it. `@-` goes the same way, and the rule catches it \
                 twice over: naming a commit it is `HEAD`, since the open change sits on HEAD's \
                 commit, and naming an operation it is `@^`, since an operation's first parent is \
                 the operation before it — which makes `@-3` exactly `@~3`. None of this touches \
                 ordinary names: a branch called my-branch is just a branch.",
        exits: &["ff log -r '<x>^'", "ff log -r HEAD", "ff op show @^"],
    },
    Entry {
        id: "revset/deferred-descendants",
        summary: "descendants are not available yet",
        detail: "Walking forward needs a child index, which git does not keep — every walk it \
                 offers runs from a tip backwards. Rather than scan all of history and call it a \
                 query, `x+` and descendants() are left out until they can be answered within the \
                 cost fufu promises. `x::` is the unbounded descendant form that does exist today.",
        exits: &["ff log -r '<x>::'"],
    },
    Entry {
        id: "usage/revset-no-symmetric-difference",
        summary: "there is no `a...b`; the set language already says it",
        detail: "What fufu inherits whole is gitrevisions' revision grammar — its symbols and \
                 suffixes. Ranges are its own set algebra, and in a set language `a...b` is exactly \
                 `(a..b) | (b..a)`, so it stays out for the same reason `x-` does. Inheriting a \
                 spelling is not the same as inheriting a meaning.",
        exits: &["ff log -r '(<a>..<b>) | (<b>..<a>)'"],
    },
    Entry {
        id: "usage/revset-expected-expression",
        summary: "an operator or a call is missing the expression it needs",
        detail: "Something that takes an operand did not get one — a trailing & or |, a ~ with \
                 nothing after it, or a dangling comma in a call. Note that complement is `~x`; \
                 `^x` is not a prefix operator in this grammar, since ^ belongs to git's suffixes.",
        exits: &["ff log -r '~<x>'"],
    },
    Entry {
        id: "usage/revset-unbalanced-parens",
        summary: "the parentheses in that expression do not pair up",
        detail: "An opening paren with no closer, a closer with nothing open, or a comma outside \
                 any call. Quoting the whole expression keeps a shell from eating parens before \
                 fufu sees them, which is the usual cause when the text looked balanced as typed.",
        exits: &["ff log -r '(<a> | <b>) & <c>'"],
    },
    Entry {
        id: "usage/revset-unterminated-brace",
        summary: "a git suffix opened a brace and never closed it",
        detail: "Forms like `@{2 days ago}`, `^{tree}`, and `^{/regex}` run to their closing brace, \
                 and braces nest, so an unclosed one swallows the rest of the expression. The error \
                 quotes the run that never ended.",
        exits: &["ff log -r '<branch>@{1}'"],
    },
    Entry {
        id: "usage/revset-unterminated-quote",
        summary: "a pattern value opened a quote and never closed it",
        detail: "Pattern values may be quoted so they can carry characters the grammar otherwise \
                 reads as operators — `regex:\"^fix\"`, `substring:\"fix bug\"`. Inside the quotes \
                 only \\\" and \\\\ are escapes; everything else is literal, which is what keeps a \
                 regex's own backslashes intact.",
        exits: &["ff log -r 'description(substring:\"<text>\")'"],
    },
    Entry {
        id: "usage/revset-empty",
        summary: "the revset is empty",
        detail: "-r was given nothing, or only whitespace. An empty expression has no obvious \
                 reading — the whole history and no commits are both defensible — so fufu asks \
                 rather than picking one. `::@` is every ancestor of the open change.",
        exits: &["ff log -r '::@'"],
    },
    Entry {
        id: "usage/revset-open-suffix",
        summary: "`@` is the open change, and it takes no suffixes",
        detail: "git's `@` means HEAD; fufu's means the open change sitting on top of it. Since \
                 the two differ by exactly one commit, a suffix on `@` would be off by one against \
                 every reader's expectation, so fufu refuses it instead of quietly translating. \
                 The commit under the open change is `HEAD`, and git's suffixes attach there: `@^` \
                 is `HEAD`, and `@~2` is `HEAD~`.",
        exits: &["ff log -r HEAD", "ff log -r 'HEAD~'"],
    },
    Entry {
        id: "usage/revset-range-suffix",
        summary: "`x^!` and `x^@` are rev-list ranges, not revisions",
        detail: "Both are git shorthands that expand to more than one revision — `x^!` is x \
                 without its parents, `x^@` is x's parents without x — so neither names a single \
                 revision the way every other suffix does. fufu has its own set algebra for \
                 ranges, which says the same things without borrowing a spelling that only reads \
                 as a suffix.",
        exits: &["ff log -r '<x>^..<x>'", "ff log -r '<x>^ | <x>^2'"],
    },
    Entry {
        id: "usage/revset-ambiguous",
        summary: "that name is both a ref and an object, and fufu will not pick one",
        detail: "Both lookups run for every name, and neither wins by precedence. Tools that rank \
                 them resolve a name to a branch even when a commit of the same spelling exists, \
                 and say nothing about the one they dropped — which is the failure this refusal \
                 exists to prevent. Spell the full ref path or the full sha; the error names both \
                 candidates so you can pick.",
        exits: &["ff log -r refs/heads/<name>", "ff log -r <full-sha>"],
    },
    Entry {
        id: "usage/revset-unknown-revision",
        summary: "nothing in revision space answers to that name",
        detail: "The name is not a ref, not an object prefix long enough for git to accept, and \
                 not one of fufu's own symbols. Object prefixes have to be at least four \
                 characters — git's own minimum, borrowed rather than restated — so a shorter one \
                 reads as an ordinary name and finds nothing.",
        exits: &["ff log", "ff branch"],
    },
    Entry {
        id: "usage/revset-not-a-commit",
        summary: "that names an object, but not a commit",
        detail: "A revset denotes a set of commits, so a spelling that peels to a tree or a blob \
                 has nothing to contribute to one. This is usually `^{tree}` or a `<rev>:<path>` \
                 spelling, both of which are legal gitrevisions naming a legal object — just not \
                 the kind a set of commits can hold.",
        exits: &["ff log"],
    },
    Entry {
        id: "usage/op-in-rev-position",
        summary: "that is an operation, and this position takes a revision",
        detail: "History has revisions; the log of what fufu did has operations. They never mix in \
                 one argument, which is what lets hex mean commit everywhere and letters mean \
                 operation everywhere with no rule to remember. An operation id typed here is \
                 usually the right id and the wrong verb: `ff op show` reads one, `--at-op` runs \
                 a read-only command as of one, and `ff op log -r` takes whole expressions over \
                 them.",
        exits: &["ff op show <op>", "ff op log -r '<expr>'"],
    },
    Entry {
        id: "usage/rev-in-op-position",
        summary: "that names a commit, and this position takes an operation",
        detail: "The mirror of usage/op-in-rev-position. It turns up on `@^2` — an operation's \
                 first parent is the operation before it, which is why git's own suffixes work \
                 here at all, but every parent past the first leaves the log: slot 2 is the commit \
                 the operation ran on, and the rest are the shas it pinned. It also turns up on a \
                 branch name inside ff op log -r, where one log spans every branch, so narrowing \
                 to one is the on_branch() predicate rather than a name. Either way the crossing \
                 back to history is spelled base(), so that it is something you asked for rather \
                 than something a suffix did quietly.",
        exits: &[
            "ff op show @",
            "ff op log -r 'on_branch(<name>)'",
            "ff log -r 'base(@)'",
        ],
    },
    Entry {
        id: "usage/revset-unknown-function",
        summary: "no revset function goes by that name",
        detail: "The registry holds every function the language has, and the error lists the ones \
                 that exist. Revision space has latest, heads, roots, description and author; \
                 operation space has on_branch, session and kind, plus the same three set \
                 functions. base() belongs to revision space and takes operations, because it is \
                 the crossing between them.",
        exits: &["ff log -r 'latest(main)'", "ff op log -r 'kind(op)'"],
    },
    Entry {
        id: "usage/revset-arity",
        summary: "that function was called with the wrong arguments",
        detail: "Every function declares how many arguments it takes and what kind each one is: a \
                 set, or a pattern. A set where a pattern belongs is refused rather than coerced, \
                 because guessing would make description(main) mean the branch on one day and the \
                 word on another. The error names the signature.",
        exits: &["ff log -r 'description(substring:\"<text>\")'"],
    },
    Entry {
        id: "usage/revset-wrong-space",
        summary: "that function reads operations, and this position takes revisions",
        detail: "One grammar spans both address spaces — the same operators and the same functions \
                 over operations instead of over history — but the vocabularies differ, because \
                 each space can only name what it has. on_branch(), session() and kind() are \
                 questions about operations, so they belong after ff op log -r. base() goes the \
                 other way: it takes operations and returns the commits they ran on, which makes \
                 it a revision-space function with an op-space argument — and the only crossing \
                 between the two.",
        exits: &["ff op log -r 'kind(op)'", "ff log -r 'base(@)'"],
    },
    Entry {
        id: "usage/revset-empty-set",
        summary: "the expression is valid and matches nothing",
        detail: "Every name in it resolved, so this is not a typo in the usual sense — it is a set \
                 that came out empty. The common causes are a range whose endpoints are the wrong \
                 way round, an intersection of two sets that never overlap, and a predicate no \
                 commit satisfies.",
        exits: &["ff log", "ff branch"],
    },
    Entry {
        id: "usage/revset-not-a-point",
        summary: "the expression matches more than one revision, and this takes exactly one",
        detail: "Verbs that act on a revision need exactly one, and fufu will not pick for you — \
                 which of several commits you meant is not something a tool can know. Narrowing is \
                 a spelling you choose: latest() takes the newest member, and heads() takes the \
                 members nothing else in the set descends from.",
        exits: &["ff log -r 'latest(<expr>)'", "ff log -r 'heads(<expr>)'"],
    },
    Entry {
        id: "revset/regex-unavailable",
        summary: "regex patterns are recognized but not available yet",
        detail: "A regex engine is a megabyte and a half of dependency, and fufu will not carry it \
                 before a caller needs it. The prefix is recognized anyway so that typing it is \
                 answered rather than mistaken for a ref name — glob: and substring: cover most of \
                 what regex: gets reached for.",
        exits: &[
            "ff log -r 'description(glob:\"fix*\")'",
            "ff log -r 'description(substring:\"fix\")'",
        ],
    },
    Entry {
        id: "usage/unknown-key",
        summary: "no fufu setting goes by that name",
        detail: "Settings live in a typed registry, so a name that is not in it would silently do \
                 nothing if it were written. Bare ff config lists every setting with its value, \
                 its meaning, and whether it is still at its default.",
        exits: &["ff config"],
    },
    Entry {
        id: "usage/bad-value",
        summary: "the value did not parse as this setting's type",
        detail: "Every setting is validated through the same parser its readers use, before \
                 anything touches disk — a value that would be ignored at read time is refused at \
                 write time instead. ff config <key> shows the current value and the shape expected.",
        exits: &[],
    },
    Entry {
        id: "usage/bad-flags",
        summary: "those flags do not go together",
        detail: "The message names the combination. Flags that would contradict each other are \
                 refused rather than quietly resolved by precedence, so the command you get is \
                 always the command you wrote.",
        exits: &[],
    },
    Entry {
        id: "usage/needs-message",
        summary: "a description was needed and there was no terminal to ask on",
        detail: "The bare form of describe opens an editor, which needs a terminal; in a script, \
                 a hook, or anything running non-interactively there is nobody to answer it. Pass \
                 the text with -m instead. FF_NONINTERACTIVE forces this behavior even on a \
                 terminal.",
        exits: &["ff describe -m <msg>"],
    },
    Entry {
        id: "op/nothing-to-redo",
        summary: "there is no forward step to take",
        detail: "Redo walks forward along the branch of the log an undo stepped off, and reads \
                 where the pointer has been out of the ref's own reflog to find it. Either no \
                 undo has been recorded, or work has landed since one — and work after an undo \
                 forks the log rather than truncating it, so redo stops offering a path it can no \
                 longer take instead of stepping over something nobody asked it to abandon. \
                 Nothing is lost either way: the operations are still there, and ff op restore \
                 still lands on them by id.",
        exits: &["ff op log", "ff undo"],
    },
    Entry {
        id: "usage/at-op-unsupported",
        summary: "that verb does not read a past state yet",
        detail: "--at-op and --at ride every verb that reads, and for most of them that means \
                 resolving a target the verb already resolves. For ff status, ff log, ff evolog \
                 and ff branch it means something larger — rendering a whole view against an \
                 operation's recorded ref table instead of against the refs on disk — and that \
                 is not built. Saying so beats an unknown-argument error, which would teach that \
                 the flags do not exist.",
        exits: &[
            "ff op show <op>",
            "ff op log",
            "ff restore <path> --at-op <op>",
        ],
    },
    Entry {
        id: "held/op-revert",
        summary: "the inversion conflicts with work done since",
        detail: "ff op revert takes one change back out while later work stands, which it can \
                 only do where the refs it would move still hold what that operation left them \
                 at. One of them has moved since, so inverting would quietly take the later work \
                 with it. Nothing was changed. ff op show says what the operation did, and \
                 ff op restore rewinds to it wholesale if that is what you meant.",
        exits: &["ff op show <op>", "ff op restore <op>", "ff op log"],
    },
    Entry {
        id: "usage/bad-session",
        summary: "that is not a usable session name",
        detail: "A session name can be any text — spaces, punctuation, and unicode are all fine. \
                 The only limits are the ones storing it as a commit-message trailer imposes: no \
                 control characters or line breaks, and 128 bytes at most. Names are compared \
                 exactly as given, so nothing is silently rewritten to fit.",
        exits: &[],
    },
    Entry {
        id: "usage/unknown-error-id",
        summary: "no error goes by that id",
        detail: "Error ids are stable and namespaced — usage/ for a command line that was wrong, \
                 held/ for work that stopped waiting on a decision, and a bare name for \
                 everything else. ff explain --list prints every id fufu can raise.",
        exits: &["ff explain --list"],
    },
    Entry {
        id: "repo/not-found",
        summary: "no git repository here, or in any parent directory",
        detail: "fufu works inside a git repository, and searches upward from the current \
                 directory to find one. Either this is not a working tree, or you are outside \
                 the one you meant to be in.",
        exits: &[],
    },
    Entry {
        id: "internal",
        summary: "an unclassified failure",
        detail: "This error has no curated id yet: it is a failure passed through from git, the \
                 filesystem, or fufu's own internals rather than a decision waiting on you. The \
                 message is the whole of what is known. If it reproduces, it is worth reporting.",
        exits: &[],
    },
];

/// Find an entry by id, or None.
pub fn find(id: &str) -> Option<&'static Entry> {
    ENTRIES.iter().find(|e| e.id == id)
}

/// Render one entry to stdout. When exits are present, the try: block follows.
pub fn render(entry: &Entry) -> std::io::Result<()> {
    let mut out = std::io::stdout();
    writeln!(out, "{}", entry.id)?;
    writeln!(out, "{}", entry.summary)?;
    writeln!(out)?;
    wrap(&mut out, entry.detail, 80)?;
    if !entry.exits.is_empty() {
        writeln!(out)?;
        writeln!(out, "  try:")?;
        for hint in entry.exits {
            writeln!(out, "    {hint}")?;
        }
    }
    Ok(())
}

/// Render every entry as `id  summary` (list mode).
pub fn render_list() -> std::io::Result<()> {
    let mut out = std::io::stdout();
    // Compute the widest id column so summaries align.
    let max_id = ENTRIES.iter().map(|e| e.id.len()).max().unwrap_or(0);
    for entry in ENTRIES {
        writeln!(
            out,
            "{:<width$}  {}",
            entry.id,
            entry.summary,
            width = max_id
        )?;
    }
    Ok(())
}

/// Emit JSON for one entry.
pub fn emit_json(entry: &Entry) -> Result<()> {
    let data = serde_json::json!({
        "id": entry.id,
        "summary": entry.summary,
        "detail": entry.detail,
        "exits": entry.exits,
    });
    crate::machine::emit("explain", &data)
}

/// Emit JSON for the list: array of entry objects.
pub fn emit_json_list() -> Result<()> {
    let entries: Vec<serde_json::Value> = ENTRIES
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.id,
                "summary": e.summary,
                "detail": e.detail,
                "exits": e.exits,
            })
        })
        .collect();
    let data = serde_json::json!({ "entries": entries });
    crate::machine::emit("explain", &data)
}

/// Error when an id is not found in the registry.
pub fn unknown_id(id: &str) -> Error {
    Error::coded(
        "usage/unknown-error-id",
        format!("no such error id: {id}"),
        vec!["ff explain --list".into()],
    )
}

/// Wrap `text` to `width` columns, writing to `out`. Simple word-wrap: break
/// at spaces, never mid-word.
fn wrap(out: &mut impl Write, text: &str, width: usize) -> std::io::Result<()> {
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut col = 0;
    for word in words {
        if col > 0 && col + 1 + word.len() > width {
            writeln!(out)?;
            col = 0;
        }
        if col > 0 {
            write!(out, " ")?;
            col += 1;
        }
        write!(out, "{word}")?;
        col += word.len();
    }
    if col > 0 {
        writeln!(out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registry_entry_has_prose() {
        let mut seen = Vec::new();
        for entry in ENTRIES {
            assert!(!seen.contains(&entry.id), "duplicate id: {}", entry.id);
            seen.push(entry.id);
            assert!(!entry.summary.is_empty(), "{}: summary is empty", entry.id);
            assert!(!entry.detail.is_empty(), "{}: detail is empty", entry.id);
        }
    }

    /// The registry is a promise, and a promise nothing checks is a promise
    /// that rots. Every id raised anywhere in the workspace must be
    /// explainable, so adding a coded error without an entry fails here
    /// rather than at a user's terminal.
    #[test]
    fn every_raised_id_is_in_the_registry() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/ is the manifest dir's parent")
            .to_path_buf();

        let mut missing: Vec<(String, String)> = Vec::new();
        let mut found = 0usize;
        for file in rust_sources(&crates) {
            let text = std::fs::read_to_string(&file).expect("read source");
            for id in raised_ids(&production_source(&text)) {
                found += 1;
                if !ENTRIES.iter().any(|e| e.id == id) {
                    missing.push((id, file.display().to_string()));
                }
            }
        }
        // A walker that silently found nothing would pass this test while
        // checking nothing at all, so it has to prove it read the tree.
        assert!(
            found > 20,
            "only {found} coded ids found — the source walk is broken, not the registry"
        );
        assert!(
            missing.is_empty(),
            "Error::coded ids with no registry entry: {missing:#?}"
        );
    }

    /// Ids the registry carries that no `Error::coded` call raises.
    ///
    /// Each one is an id fufu cannot produce and must therefore explain for a
    /// different reason, stated here so a genuinely dead entry cannot hide
    /// behind a habit of adding names to this list.
    const UNRAISED: &[(&str, &str)] = &[
        (
            "repo/not-found",
            "raised structurally by Error::id() for the Discover variant, never by a coded call",
        ),
        (
            "internal",
            "the fallback id every uncoded error reports; there is nothing to raise",
        ),
    ];

    /// The mirror of the guard above, and the reason both ship.
    ///
    /// `every_raised_id_is_in_the_registry` catches an id added without prose.
    /// It cannot catch the opposite — prose left behind by an id that was
    /// removed — because removing a raise site only makes that test's job
    /// easier. `usage/needs-session` outlived `ff session` by exactly that
    /// gap: an entry a user could still reach through `ff explain --list`,
    /// describing two verbs that no longer existed.
    #[test]
    fn every_registry_entry_is_reachable() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/ is the manifest dir's parent")
            .to_path_buf();

        let mut raised: Vec<String> = Vec::new();
        for file in rust_sources(&crates) {
            let text = std::fs::read_to_string(&file).expect("read source");
            raised.extend(raised_ids(&production_source(&text)));
        }
        assert!(
            raised.len() > 20,
            "only {} coded ids found — the source walk is broken, not the registry",
            raised.len()
        );

        let orphans: Vec<&str> = ENTRIES
            .iter()
            .map(|e| e.id)
            .filter(|id| !raised.iter().any(|r| r == id))
            .filter(|id| !UNRAISED.iter().any(|(allowed, _)| allowed == id))
            .collect();
        assert!(
            orphans.is_empty(),
            "registry entries nothing raises — delete the prose or keep the raise site: \
             {orphans:#?}"
        );
    }

    /// A file with its inline test module cut off.
    ///
    /// Test modules are allowed placeholder ids: they exercise the namespace
    /// rule, not the registry. What marks one is `#[cfg(test)] mod tests`
    /// specifically, and not the bare attribute — `revset/mod.rs` declares
    /// `#[cfg(test)] mod prop;` a third of the way down, so cutting at the
    /// first attribute silently hid the rest of that file from both guards.
    /// The forward guard could not notice, since missing a raise site only
    /// makes its job easier; the reverse one found it on the first run.
    fn production_source(text: &str) -> String {
        let mut out = text;
        for (idx, _) in text.match_indices("#[cfg(test)]") {
            let rest = text[idx + "#[cfg(test)]".len()..].trim_start();
            if rest.starts_with("mod tests") {
                out = &text[..idx];
                break;
            }
        }
        out.to_string()
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

    /// The first string literal after each `Error::coded(` — which is the id,
    /// whether the call sits on one line or is wrapped across several.
    fn raised_ids(text: &str) -> Vec<String> {
        let mut ids = Vec::new();
        for (idx, _) in text.match_indices("Error::coded(") {
            let rest = &text[idx..];
            let Some(open) = rest.find('"') else { continue };
            let after = &rest[open + 1..];
            let Some(close) = after.find('"') else {
                continue;
            };
            ids.push(after[..close].to_string());
        }
        ids
    }
}
