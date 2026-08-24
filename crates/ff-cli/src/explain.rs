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
        exits: &["ff branch list", "ff describe -b <name>"],
    },
    Entry {
        id: "branch/not-found",
        summary: "no branch here goes by that name",
        detail: "Names resolve against local branches, so a branch that exists on the remote but \
                 not here will not be found. Its tracking ref does have a name — the remote's, \
                 then the branch's — and ff start forks a local branch from that. ff branch list \
                 shows those under remote only, spelled exactly as ff start takes them, so the \
                 name to type is on the screen rather than reconstructed. The remote is not \
                 spelled origin here because it is only usually called that, and an exit that \
                 guessed wrong would send you to a ref that does not exist: ff remote says what \
                 yours is called.",
        exits: &["ff branch list", "ff remote", "ff start <remote>/<branch>"],
    },
    Entry {
        id: "branch/ambiguous",
        summary: "that branch prefix matches more than one branch",
        detail: "A prefix has to name one branch, and this one names several. Every candidate is \
                 listed so you can pick; typing one more character is usually enough. \
                 ff branch list says what is local.",
        exits: &["ff branch list"],
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
        exits: &["ff worktree list", "git worktree list"],
    },
    Entry {
        id: "branch/aliased-copy",
        summary: "the copy that branch tracks wears another branch's name",
        detail: "--shared removes the shared copy of the branch you are deleting, and this \
                 branch's upstream points somewhere that is not it: branch.<n>.merge names one \
                 branch and the branch itself is called another. fufu will not send a delete to \
                 a ref it cannot say is yours, because the copy it would take down is somebody \
                 else's. The plain delete still works, and leaves everything on the remote \
                 standing.",
        exits: &["ff branch list", "ff remote"],
    },
    Entry {
        id: "branch/shared-lease-refused",
        summary: "the shared copy moved since you last looked, so it was not deleted",
        detail: "Every push fufu makes is leased: it says what it last saw the remote standing \
                 at, and the remote refuses when that is no longer true. Somebody pushed to the \
                 shared copy after your last fetch, so it is still there and holding commits you \
                 have not seen — which is exactly the case where deleting it would lose work. \
                 The local half of the delete did happen and is undoable, so ff undo brings the \
                 branch back; look at what arrived before deciding the copy should go.",
        exits: &["ff undo", "ff branch list"],
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
        id: "usage/absorb-into-open",
        summary: "absorb was named the open change as its target",
        detail: "The open change is already where your changes are, so there is nothing to fold \
                 it into. Name a commit that has closed — ff log says which ones sit under you — \
                 or close the change first with ff commit and absorb into the commit that lands.",
        exits: &["ff absorb", "ff commit -m <msg>"],
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
        exits: &["ff op log", "ff restore --all --at-op <op>"],
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
        id: "commit/empty",
        summary: "there is nothing to close: the tree matches HEAD",
        detail: "The working tree is the open change, so a tree that matches HEAD is a change \
                 that does not exist — and a description does not make one, because a commit \
                 that says something while changing nothing is exactly the placeholder state \
                 fufu refuses to keep. Nothing was written, so a pending description is still \
                 there, and ff describe -m will park one for the next close. Naming paths \
                 narrows the question to those paths, so a clean slice refuses the same way a \
                 clean tree does — the same rule, a narrower scope.",
        exits: &["ff status", "ff describe -m <message>"],
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
        exits: &["ff log", "ff branch list"],
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
                 a read-only command as of one, and `ff op log` takes whole expressions over \
                 them.",
        exits: &["ff op show <op>", "ff op log '<expr>'"],
    },
    Entry {
        id: "usage/rev-in-op-position",
        summary: "that names a commit, and this position takes an operation",
        detail: "The mirror of usage/op-in-rev-position. It turns up on `@^2` — an operation's \
                 first parent is the operation before it, which is why git's own suffixes work \
                 here at all, but every parent past the first leaves the log: slot 2 is the commit \
                 the operation ran on, and the rest are the shas it pinned. It also turns up on a \
                 branch name inside an ff op log expression, where one log spans every branch, \
                 so narrowing to one is the on_branch() predicate rather than a name. Either way the crossing \
                 back to history is spelled base(), so that it is something you asked for rather \
                 than something a suffix did quietly.",
        exits: &[
            "ff op show @",
            "ff op log 'on_branch(<name>)'",
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
        exits: &["ff log -r 'latest(main)'", "ff op log 'kind(op)'"],
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
                 questions about operations, so they belong in an ff op log expression. base() \
                 goes the other way: it takes operations and returns the commits they ran on, which makes \
                 it a revision-space function with an op-space argument — and the only crossing \
                 between the two.",
        exits: &["ff op log 'kind(op)'", "ff log -r 'base(@)'"],
    },
    Entry {
        id: "usage/revset-empty-set",
        summary: "the expression is valid and matches nothing",
        detail: "Every name in it resolved, so this is not a typo in the usual sense — it is a set \
                 that came out empty. The common causes are a range whose endpoints are the wrong \
                 way round, an intersection of two sets that never overlap, and a predicate no \
                 commit satisfies.",
        exits: &["ff log", "ff branch list"],
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
        exits: &["ff config <key>"],
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
        id: "usage/no-such-path",
        summary: "that path names nothing here",
        detail: "A path has to name something the repository can see — a file or directory on \
                 disk, or an entry in the last commit. Neither, and it is a typo, so the command \
                 is refused rather than answered with an empty log: an empty log for a \
                 misspelled path is the worst kind of wrong answer, because it looks like an \
                 answer. Paths go in the positional and revisions go behind -r, so no `--` \
                 separator is needed and none is accepted — `ff log main` is a question about \
                 the path main, even where a branch called main exists. A whole sentence in the \
                 path slot is usually a forgotten -m, which is why the exits say so when the \
                 token has spaces in it.",
        exits: &["ff status", "ff log"],
    },
    Entry {
        id: "usage/unknown-subcommand",
        summary: "that family does not have that subcommand",
        detail: "A verb that groups subcommands answers an unknown one itself rather than letting \
                 the parser call it an unexpected argument, because the usual cause is not a typo. \
                 ff branch <name> used to claim the anonymous branch you were standing on; naming \
                 a branch is ff describe -b now, on the same axis as -m, and it takes proper names \
                 too. What is left in the family is the bookkeeping: list, and delete.",
        exits: &["ff branch list", "ff describe -b <name>"],
    },
    Entry {
        id: "usage/foreign-verb",
        summary: "that is a git verb fufu answers rather than runs",
        detail: "A handful of git words name something fufu does differently, so typing one is a \
                 question rather than a typo and it gets an answer instead of a parse error. \
                 checkout was two jobs and is two verbs here; diff and stash describe states fufu \
                 keeps rather than commands you run. rebase already has an answer — ff restack \
                 replays a branch onto its base — and so does pull: ff sync lines a branch up \
                 with its base and its remote in one move and publishes under a lease. merge is \
                 the position rather than the gap: fufu replays instead, and a merge commit is \
                 what a forge makes when work lands. blame and tag stay git's, and each is \
                 answered for the half git has not got — the operations behind a file since you \
                 last committed, and putting back a tag that was deleted. The passthrough still \
                 runs the real thing, capture-first, when you want git's own behavior instead.",
        exits: &["ff status", "ff git <args>"],
    },
    Entry {
        id: "usage/lift-from-open",
        summary: "lift was named the open change as its source",
        detail: "The open change is what a lift lands in, so naming it as a source has nothing \
                 committed to take back out. Name a commit that has closed — ff log says which \
                 ones sit under you — and lift takes its files back into the open change.",
        exits: &["ff lift", "ff log"],
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
        id: "edit/not-in-history",
        summary: "that commit is not in the branch you are standing on",
        detail: "An editing session ends by replaying the branch's commits onto the edited one, \
                 so the commit has to be in that branch's history — one it is not in has nowhere \
                 to land, because the session would end with the branch you left and the commit \
                 you edited unable to meet. It sits on another line of work, or below a fork you \
                 have since left. ff log says what is under you, and ff switch brings a branch \
                 that contains it into reach.",
        exits: &["ff log", "ff switch <branch>"],
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
        id: "held/rewrite-conflict",
        summary: "the rewrite stops at a commit it cannot replay",
        detail: "The replay reached a commit whose changes cannot be reapplied over the \
                 rewrite, so nothing was written. A verb asks this question before it plans \
                 and records a hold instead, so this is the engine answering someone who \
                 did not ask first — which, from the command line, should not happen.",
        exits: &["ff status", "ff resolve"],
    },
    Entry {
        id: "held/expired",
        summary: "the held rewrite no longer has a question to answer",
        detail: "A hold records what you asked for, not the plan it could not finish \
                 computing, so it is asked again against the repository as it stands rather \
                 than replayed from what it stored. This time there was no answer: the base \
                 branch is gone, or the commit it aimed at has left the branch's history, or \
                 the editing session it was going to land was ended by hand. Nothing was \
                 written and the hold is still there — dropping it is a decision, and this is \
                 only the report that it has outlived its meaning.",
        exits: &["ff resolve --abandon", "ff status", "ff log"],
    },
    Entry {
        id: "held/already-held",
        summary: "a rewrite is already held on this branch",
        detail: "One hold per branch. Several holds on one stack would have to agree an \
                 order — which queues behind which, and what the second one is even \
                 replaying over — and fufu has not answered that, so a second conflicting \
                 rewrite refuses rather than guessing. Resolve or drop the one standing and \
                 the branch is free again. A rewrite that would land cleanly is not refused: \
                 it is not competing for anything, and the hold re-derives itself the next \
                 time it is asked.",
        exits: &["ff resolve", "ff resolve --abandon", "ff status"],
    },
    Entry {
        id: "held/none",
        summary: "nothing is held on this branch",
        detail: "ff resolve deals with a held rewrite — a conflict one of the verbs recorded \
                 instead of interrupting you — and this branch has no such record. It either \
                 never happened, or was already dealt with: re-running the verb that recorded \
                 it will land it, and ff status says what is actually open on the branch.",
        exits: &["ff status", "ff log"],
    },
    Entry {
        id: "held/resolving",
        summary: "a resolution is already open on this branch",
        detail: "Re-running ff resolve would materialize the same conflicts again over the \
                 very edits the open session is collecting, so it refuses instead. The \
                 markers are in your working tree right now: fix them and the rewrite lands, \
                 or ff resolve --abandon drops the session and the hold together.",
        exits: &["ff done", "ff resolve --abandon", "ff status"],
    },
    Entry {
        id: "held/moved",
        summary: "the repository changed while the resolution was open",
        detail: "A resolution session is built on the conflicts it was given, and ff done \
                 re-derives them before it lands anything. This time they came out different: \
                 a commit landed, a branch moved, or the base changed while the markers were \
                 in your working tree, so the fixes would land in the wrong place. Nothing \
                 moved. Re-resolve to look at the conflicts as they stand now, or abandon \
                 to drop the session and the hold together.",
        exits: &["ff resolve", "ff resolve --abandon", "ff status"],
    },
    Entry {
        id: "held/unresolved",
        summary: "conflict markers are still standing in the working tree",
        detail: "ff done attributes your fixes to the commits that owned each region, and a \
                 region still carrying its markers is a fix that is not finished — or a fix \
                 that created a conflict further up the stack. Nothing moved: the branch, \
                 the hold and the session all stand exactly as they did. Fix what remains, \
                 then ff done again, or ff resolve --abandon to start from the hold.",
        exits: &["ff status", "ff resolve --abandon"],
    },
    Entry {
        id: "held/unsupported",
        summary: "the held rewrite selected paths, and the open change reaches beyond them",
        detail: "A filtered absorb or lift rewrites only the paths it selected, so the \
                 markers it lays down would overwrite open changes standing outside that \
                 filter — work lost, which fufu does not do silently. Commit the change you \
                 want kept (ff commit), or drop the hold (ff resolve --abandon) and re-run \
                 the verb with the paths it should actually select.",
        exits: &["ff commit -m <msg>", "ff resolve --abandon", "ff status"],
    },
    Entry {
        id: "rewrite/merge-in-range",
        summary: "a merge commit sits in the range being replayed",
        detail: "Re-parenting a merge is unambiguous, and a reword does it happily, but \
                 replaying a merge is not — the same change can come out of either side. A \
                 rewrite that would move a tree over it refuses rather than picking a side.",
        exits: &["ff log"],
    },
    Entry {
        id: "rewrite/not-in-history",
        summary: "that commit is not in the history under you",
        detail: "A rewrite re-parents everything between the commit you named and the branch \
                 tip, so the commit has to be an ancestor of that tip. This one is not: it sits \
                 on another line of work, or below a fork you have since left. ff log says what \
                 is under you, and ff log -r <rev> says where a revision actually sits.",
        exits: &["ff log", "ff log -r <rev>"],
    },
    Entry {
        id: "restack/no-base",
        summary: "there is no base to replay this branch onto",
        detail: "A restack replays a branch onto the base it sits on — the parent recorded when \
                 it was forked, falling back to trunk. Standing on trunk itself there is no \
                 base, since trunk sits on nothing. Name one with --onto to say where it goes \
                 instead.",
        exits: &["ff restack <branch> --onto <base>", "ff status"],
    },
    Entry {
        id: "restack/own-remote",
        summary: "a branch cannot be restacked onto its own shared copy",
        detail: "A branch's shared copy is not a base it sits on — it is the same branch \
                 somewhere else, and replaying onto it is reconciling with the remote, \
                 which is what ff sync does: it fetches, takes in what arrived, and \
                 replays. --onto is for naming a different branch to sit on.",
        exits: &["ff sync", "ff restack <branch> --onto <base>"],
    },
    Entry {
        id: "restack/unrelated",
        summary: "the branch and its base share no history",
        detail: "A replay measures its range from a common ancestor, and two histories that \
                 share none have no range to replay. ff log says what each line of work sits \
                 on, so the two can be compared against each other.",
        exits: &["ff log", "ff restack <branch> --onto <base>"],
    },
    Entry {
        id: "session/none",
        summary: "there is no editing session to finish",
        detail: "An editing session is what ff edit opens: a branch minted at a commit and \
                 switched to, so the commit's content can be edited with your whole toolchain, \
                 and the session is recorded so ff done knows where to land it. You are not \
                 standing on one right now, so there is nothing to finish or to abandon. \
                 ff status says whether a session is running, and ff edit <rev> opens one on \
                 the commit you mean.",
        exits: &["ff edit <rev>", "ff status"],
    },
    Entry {
        id: "session/open",
        summary: "you are already inside an editing session",
        detail: "Sessions do not nest: two at once would each record the other as the branch \
                 to replay onto, and there is no ordering to say which lands first. The one \
                 running has to be dealt with first — ff done lands it, ff done --abandon \
                 drops it, or ff switch moves away and defers it, where it waits until you \
                 come back.",
        exits: &["ff done", "ff done --abandon", "ff switch <branch>"],
    },
    Entry {
        id: "session/moved",
        summary: "the session branch has commits of its own now",
        detail: "Only a foreign git commit can add one, and landing the session would fold it \
                 into the amended commit. Its content is already in the working tree and would \
                 survive the amend, but its message would not, and dropping a message nobody \
                 asked to lose is the guess fufu will not make. ff done --abandon drops the \
                 session without landing it, and ff restack is the move to make once you have \
                 decided what the branch becomes.",
        exits: &["ff done --abandon", "ff restack"],
    },
    Entry {
        id: "session/unreachable",
        summary: "the edited commit has left the branch the session lands on",
        detail: "A session ends by replaying the branch's commits onto the commit it was opened \
                 on, so that commit has to still be in the branch's history. It is not — \
                 something moved the branch while the session was open, and landing would have \
                 nothing to land onto. ff done --abandon drops the session without landing it, \
                 and ff log says where the branch stands now.",
        exits: &["ff done --abandon", "ff log"],
    },
    Entry {
        id: "usage/restack-onto-self",
        summary: "a branch cannot be restacked onto itself",
        detail: "--onto names the branch to replay onto, and that has to be a different one — \
                 a branch replayed onto itself is the same history it already is. Name the base \
                 you want it to sit on, or drop the flag to use the one recorded.",
        exits: &["ff restack <branch> --onto <base>", "ff branch list"],
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
        id: "publish/no-git",
        summary: "git is not on PATH, and pushing still needs it",
        detail: "Reads, rewrites, and now the fetch behind ff sync all run in this process. The \
                 push behind ff publish does not: gix speaks the half of the git protocol that \
                 receives a pack and nothing that sends one, so there is no native push to use \
                 yet. Everything else works without git on PATH — your commits are safe here \
                 either way, and they are what a later publish sends.",
        exits: &["ff git push", "ff doctor"],
    },
    Entry {
        id: "sync/fetch-failed",
        summary: "git could not fetch from the remote",
        detail: "The fetch ran and git refused; its own message is quoted. Nothing was \
                 reconciled and nothing moved, because sync will not decide whose divergence is \
                 whose against a fetch that did not happen. The passthrough runs the same fetch \
                 by hand when you want git's full output, and --no-fetch reconciles with the \
                 tracking refs already here.",
        exits: &["ff git fetch <remote>", "ff sync --no-fetch"],
    },
    Entry {
        id: "publish/unreachable",
        summary: "the remote never answered",
        detail: "git exited 128, which is how it says it did not get as far as talking to the \
                 other side: a bad URL, a host that will not resolve, or credentials it could \
                 not supply. Nothing was pushed. This is the one failure publish cannot tell \
                 apart from a network that is simply down, so it reports what git said rather \
                 than guessing which one it was.",
        exits: &["ff git push <remote>", "ff status"],
    },
    Entry {
        id: "publish/lease-refused",
        summary: "the remote moved since you last looked at it",
        detail: "Every push carries a lease: the tracking ref as it stands, offered back to the \
                 remote as what it expects to find there. Somebody pushed in between, so the \
                 remote declined and nothing was overwritten — which is the lease working, not \
                 failing. Your commits are still here. Run ff sync first: it fetches what \
                 arrived and replays on top of it, and the publish afterwards offers a lease \
                 that is current.",
        exits: &["ff sync", "ff publish", "ff status"],
    },
    Entry {
        id: "publish/rejected",
        summary: "the remote refused the push",
        detail: "The remote answered and said no — a protected branch, a pre-receive hook, or a \
                 permission you do not have. That is a decision on the far side rather than \
                 anything wrong here, so its message is passed along whole. Nothing local \
                 changed: publish only ever writes to the other side.",
        exits: &["ff git push <remote>", "ff status"],
    },
    Entry {
        id: "publish/failed",
        summary: "the push did not go through",
        detail: "git failed in a way publish could not classify as unreachable, leased out, or \
                 refused, so its own message is passed along unedited. Nothing local changed. \
                 Running the push by hand through the passthrough usually says more, since git \
                 prints a fuller transcript when it owns the terminal.",
        exits: &["ff git push <remote>", "ff status"],
    },
    Entry {
        id: "publish/unrecorded",
        summary: "the push went through and the log could not write it down",
        detail: "The commits are on the remote. What failed is the note publish appends \
                 afterwards — the row that lets ff sync and ff status tell your own published \
                 tip apart from work somebody else pushed. Nothing is lost either way: without \
                 the row the next sync reads the shared copy as theirs and replays onto it, \
                 which never drops a commit. Publishing again once the log is writable records \
                 it, and a contended log usually means another fufu process is mid-operation.",
        exits: &["ff op log", "ff status"],
    },
    Entry {
        id: "publish/unknown-remote",
        summary: "--to named a remote this repository does not have",
        detail: "--to says which of the remotes you already have a branch answers to, and \
                 nothing more — it does not add one, because a name and a URL are two \
                 different facts and only one of them was given. A typo is the usual cause, \
                 and ff remote is the list the name was checked against — the same config \
                 fufu read to refuse. A remote that is genuinely missing gets added once, by \
                 name and URL, and then there is something for --to to name.",
        exits: &["ff remote", "ff git remote add <name> <url>"],
    },
    Entry {
        id: "publish/retarget",
        summary: "the branch already answers to a different remote",
        detail: "A branch has one shared copy, and everything fufu knows about publishing is \
                 keyed to that: the lease is the tracking ref as you last saw it, and the \
                 record of where this repository last left the copy names the branch rather \
                 than the remote. Sending the same branch to a second remote would leave two \
                 copies drifting apart with one memory between them, and the next lease would \
                 be offered against a tip the other remote never held. So --to gives a branch \
                 an upstream and will not move one it already has. ff publish sends to the \
                 remote it answers to now; re-pointing it is a deliberate act, and git's own \
                 set-upstream is where that lives until fufu has a verb for it.",
        exits: &[
            "ff publish",
            "ff git branch --set-upstream-to <remote>/<branch>",
        ],
    },
    Entry {
        id: "sync/ambiguous-remote",
        summary: "more than one remote, and nothing says which one this branch answers to",
        detail: "With several remotes configured and none of them named origin there is no \
                 honest default, and picking one would decide where your work goes on a coin \
                 flip. Setting the branch's upstream once settles it for every later sync and \
                 every publish, and ff publish --to <remote> is how to set it and send in the \
                 same breath. ff remote is the list to pick that name from. A repository with \
                 one remote, or with one called origin, never reaches this.",
        exits: &[
            "ff publish --to <remote>",
            "ff remote",
            "ff git branch --set-upstream-to <remote>/<branch>",
        ],
    },
    Entry {
        id: "init/bare",
        summary: "ff init was asked for a bare repository",
        detail: "A bare repository has no working tree, which is the thing fufu captures — so \
                 there would be no floor for ff undo to land on and nothing for a capture to \
                 hold. That is not a repository fufu has anything to add to, so it does not \
                 pretend otherwise: git makes bare repositories, and ff git init --bare runs \
                 exactly that.",
        exits: &["ff git init --bare"],
    },
    Entry {
        id: "init/failed",
        summary: "the repository could not be created there",
        detail: "gix creates the directory and everything under .git, so a failure here is the \
                 filesystem's: a path that is a file, a directory that cannot be written, a \
                 volume that is full. The message carries what was actually refused. Nothing \
                 was half-written — fufu arms a repository only once it exists.",
        exits: &[],
    },
    Entry {
        id: "clone/target-exists",
        summary: "the directory to clone into already has something in it",
        detail: "A clone that failed halfway removes the directory it built, so cloning into a \
                 directory somebody already put files in would put those files at risk of a \
                 cleanup nobody asked for. git refuses the same case for the same reason. Name \
                 a different directory, or clone beside it and move what you meant to keep.",
        exits: &[],
    },
    Entry {
        id: "clone/bad-url",
        summary: "that is not a URL fufu can address",
        detail: "fufu speaks the git protocol itself, so the URL is parsed here rather than \
                 handed to git to complain about: https://, ssh://, git://, file paths, and \
                 the scp-style user@host:path all work. This also covers a target directory \
                 that cannot be worked out from the URL, which is what a URL ending in a slash \
                 and nothing else leaves — name the directory yourself.",
        exits: &[],
    },
    Entry {
        id: "clone/unreachable",
        summary: "the remote never answered",
        detail: "Nothing came back from the far side: no DNS, no route, no listener, a proxy \
                 that dropped it. Nothing was written and there is nothing to clean up. If the \
                 URL works in git but not here, the difference is usually installation config \
                 fufu reads through git — check ff doctor, and ff git ls-remote against the \
                 same URL says whether git can reach it either.",
        exits: &["ff doctor"],
    },
    Entry {
        id: "clone/refused",
        summary: "the remote answered, and said no",
        detail: "The far side is there and declined: authentication it would not accept, a \
                 repository that is not there or not yours, or a branch name that matched \
                 nothing (or matched several). Credentials come from git's own helpers, which \
                 fufu inherits rather than reimplements, so a clone that git can do and fufu \
                 cannot is worth reporting.",
        exits: &[],
    },
    Entry {
        id: "clone/failed",
        summary: "the pack arrived and the working tree could not be written",
        detail: "The download finished and the checkout did not: a path the filesystem would \
                 not take, a name that collides case-insensitively, a disk that filled. The \
                 half-built directory is removed, so this is a clone to run again rather than \
                 one to repair.",
        exits: &[],
    },
    Entry {
        id: "worktree/exists",
        summary: "that path is already taken",
        detail: "fufu checks a worktree out into a directory that does not exist or is empty, \
                 and will not write into one holding anything, because the checkout would mix \
                 with what is already there. Name an empty directory, or remove what is in \
                 this one.",
        exits: &["ff worktree list"],
    },
    Entry {
        id: "worktree/not-found",
        summary: "no linked worktree by that name",
        detail: "fufu names linked worktrees by the id git files them under, which is the \
                 checkout directory's basename. An id can outlive its worktree: the \
                 administrative directory and its operation chain stand after the checkout is \
                 gone, and a chain whose worktree is gone is not itself a worktree, so it is \
                 not named here.",
        exits: &["ff worktree list"],
    },
    Entry {
        id: "worktree/is-main",
        summary: "the main worktree is not removable",
        detail: "Every linked worktree's administrative directory lives inside the main \
                 worktree's git directory, so removing the main one would take the others with \
                 it. git refuses the same thing for the same reason.",
        exits: &["ff worktree list"],
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
                 the one you meant to be in. If there is no repository yet, ff init makes one \
                 with the safety net already on, and ff clone brings one down the same way.",
        exits: &["ff init", "ff clone <url>"],
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

/// The `try:` block a failure prints: what the raise site said, or what the
/// id means when the site said nothing.
///
/// Most `Error::coded` calls pass `vec![]`, and that was never a claim that
/// there is no way out — the way out is a property of the id, and the id
/// already has one written down here. `ff explain branch/not-found` has
/// always said `ff branch list`; the failure itself said nothing at all, so
/// an agent that hit `no branch named x` went to git rather than to the
/// verb sitting one line away. Both surfaces now read the same registry, and
/// a raise site only carries exits of its own when it knows something the id
/// does not — which is why the narrower list wins when there is one.
///
/// The last resort is the registry itself. A coded failure with nothing to
/// suggest is still a failure with prose behind it, and naming the lookup is
/// better than a dead end.
pub fn exits_for(err: &Error) -> Vec<String> {
    if !err.exits().is_empty() {
        return err.exits().to_vec();
    }
    let id = err.id();
    match find(id) {
        // `internal` is the id every uncoded error reports, and its own prose
        // says the message is the whole of what is known. Sending someone to
        // read that is the one lookup worth refusing.
        Some(entry) if entry.exits.is_empty() && id != "internal" => {
            vec![format!("ff explain {id}")]
        }
        Some(entry) => entry.exits.iter().map(|e| (*e).to_string()).collect(),
        None => Vec::new(),
    }
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
    use clap::CommandFactory;

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

    /// Every exit string this workspace hands a user, from both places one
    /// can come from: the registry entry, and the raise site that overrode it.
    ///
    /// The two are checked together on purpose. They are the same promise
    /// written twice — "type this next" — and a verb renamed out from under
    /// either half fails a user identically, so neither half gets to rot
    /// while the other stays honest.
    #[test]
    fn every_exit_names_live_surface() {
        let crates = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crates/ is the manifest dir's parent")
            .to_path_buf();

        let mut checked = 0usize;
        for entry in ENTRIES {
            for exit in entry.exits {
                check_exit(exit, &format!("registry entry {}", entry.id));
                checked += 1;
            }
        }
        for file in rust_sources(&crates) {
            let text = std::fs::read_to_string(&file).expect("read source");
            for (id, exit) in raised_exits(&production_source(&text)) {
                check_exit(&exit, &format!("{id} raised in {}", file.display()));
                checked += 1;
            }
        }
        // Same reason the id walks prove they read the tree: a scanner that
        // quietly matched nothing would pass while checking nothing.
        assert!(
            checked > 100,
            "only {checked} exits found — the walk is broken, not the exits"
        );
    }

    /// One exit string, held to what the CLI actually declares.
    ///
    /// Hidden is disqualifying, not just unknown: retired spellings stay
    /// declared and hidden so typing one reaches an answer, and an exit is
    /// the one place that answer must never be where we send someone.
    fn check_exit(exit: &str, whose: &str) {
        let tokens = argv(exit);
        let Some(first) = tokens.first() else {
            panic!("{whose}: an empty exit");
        };
        // git is the other tool an exit may legitimately name — `git rebase
        // --abort` has no fufu spelling. Its surface is not ours to check.
        if first == "git" {
            return;
        }
        assert_eq!(first, "ff", "{whose}: `{exit}` names neither ff nor git");

        let root = crate::cli::Cli::command();
        let mut cmd = &root;
        let mut rest = &tokens[1..];
        while let Some(sub) = rest.first().and_then(|name| cmd.find_subcommand(name)) {
            assert!(
                !sub.is_hide_set(),
                "{whose}: `{exit}` sends someone to {:?}, which is hidden",
                sub.get_name()
            );
            cmd = sub;
            rest = &rest[1..];
            // Passthrough: everything after `ff git` is git's to parse.
            if cmd.get_name() == "git" {
                return;
            }
        }
        // A placeholder standing where a verb goes — `ff <verb> --at-op <op>`,
        // the shape of a flag several verbs declare. There is no one command
        // to hold it to, so the flag is checked against all of them and the
        // line is not parsed: it was never meant to be typed as written.
        let shape = rest.first().is_some_and(|tok| tok == PLACEHOLDER);
        for flag in rest.iter().filter(|tok| tok.starts_with('-')) {
            let arg = find_arg(cmd, flag)
                .or_else(|| find_arg(&root, flag))
                .or_else(|| shape.then(|| anywhere(&root, flag)).flatten())
                .unwrap_or_else(|| panic!("{whose}: `{exit}` passes {flag}, which does not exist"));
            assert!(
                !arg.is_hide_set(),
                "{whose}: `{exit}` passes {flag}, which is hidden"
            );
        }
        if shape {
            return;
        }
        if let Err(err) = <crate::cli::Cli as clap::Parser>::try_parse_from(&tokens) {
            // Not every non-Ok is a failure: clap reports `-v` and `--help` as
            // errors carrying the text they printed, which is exactly what
            // those exits are for. The grammar is what is under test.
            use clap::error::ErrorKind::{DisplayHelp, DisplayVersion};
            assert!(
                matches!(err.kind(), DisplayVersion | DisplayHelp),
                "{whose}: `{exit}` does not parse:\n{err}"
            );
        }
    }

    /// What a placeholder becomes once `argv` has filled it.
    const PLACEHOLDER: &str = "x";

    /// The first declaration of `flag` anywhere in the tree, at any depth.
    fn anywhere<'a>(cmd: &'a clap::Command, flag: &str) -> Option<&'a clap::Arg> {
        find_arg(cmd, flag).or_else(|| cmd.get_subcommands().find_map(|sub| anywhere(sub, flag)))
    }

    /// An exit string as argv. Shell quoting is resolved, since a revset is
    /// one argument however many spaces it has, and a placeholder becomes a
    /// value — what is under test is the grammar around it.
    fn argv(exit: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut token = String::new();
        let mut quote: Option<char> = None;
        let mut started = false;
        for ch in exit.chars() {
            match quote {
                Some(q) if ch == q => quote = None,
                Some(_) => token.push(ch),
                None if ch == '\'' || ch == '"' => {
                    quote = Some(ch);
                    started = true;
                }
                None if ch.is_whitespace() => {
                    if started {
                        out.push(std::mem::take(&mut token));
                        started = false;
                    }
                }
                None => {
                    token.push(ch);
                    started = true;
                }
            }
        }
        if started {
            out.push(token);
        }
        out.into_iter()
            .map(|tok| {
                if tok.starts_with('<') || tok.starts_with('{') {
                    "x".to_string()
                } else {
                    tok
                }
            })
            .collect()
    }

    fn find_arg<'a>(cmd: &'a clap::Command, flag: &str) -> Option<&'a clap::Arg> {
        cmd.get_arguments().find(|arg| {
            if let Some(long) = flag.strip_prefix("--") {
                arg.get_long() == Some(long)
            } else {
                flag.strip_prefix('-')
                    .and_then(|s| s.chars().next())
                    .is_some_and(|c| arg.get_short() == Some(c))
            }
        })
    }

    /// The exits each `Error::coded(` call passes, paired with its id.
    ///
    /// The call's last `vec![` is the exits argument; every string literal
    /// inside it is one exit, `format!` template and all — a placeholder the
    /// caller fills is still the grammar the user is shown.
    fn raised_exits(text: &str) -> Vec<(String, String)> {
        let mut found = Vec::new();
        for (idx, _) in text.match_indices("Error::coded(") {
            let body = &text[idx + "Error::coded(".len()..];
            let Some(end) = call_end(body) else { continue };
            let body = &body[..end];
            let Some(id) = literals(body).into_iter().next() else {
                continue;
            };
            let Some(vec_at) = body.rfind("vec![") else {
                continue;
            };
            for element in elements(&body[vec_at + "vec![".len()..]) {
                // The element's *first* literal: the template of a `format!`,
                // or the whole of a plain `"…".into()`. A literal further in
                // is an argument being interpolated, not an exit.
                if let Some(exit) = literals(&element).into_iter().next() {
                    found.push((id.clone(), exit));
                }
            }
        }
        found
    }

    /// A `vec![…]` body split into its elements, on commas that sit outside
    /// every bracket and every string.
    fn elements(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut current = String::new();
        let mut depth = 0i32;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '"' => {
                    current.push(ch);
                    while let Some(c) = chars.next() {
                        current.push(c);
                        if c == '\\' {
                            if let Some(escaped) = chars.next() {
                                current.push(escaped);
                            }
                        } else if c == '"' {
                            break;
                        }
                    }
                }
                '(' | '[' | '{' => {
                    depth += 1;
                    current.push(ch);
                }
                ')' | '}' => {
                    depth -= 1;
                    current.push(ch);
                }
                ']' if depth == 0 => break,
                ']' => {
                    depth -= 1;
                    current.push(ch);
                }
                ',' if depth == 0 => out.push(std::mem::take(&mut current)),
                _ => current.push(ch),
            }
        }
        if !current.trim().is_empty() {
            out.push(current);
        }
        out
    }

    /// Offset of the `)` closing a call whose `(` was just consumed, with
    /// string literals skipped so a paren inside prose does not close it.
    fn call_end(text: &str) -> Option<usize> {
        let bytes = text.as_bytes();
        let mut depth = 1usize;
        let mut i = 0usize;
        while i < bytes.len() {
            match bytes[i] {
                b'"' => {
                    i += 1;
                    while i < bytes.len() && bytes[i] != b'"' {
                        i += if bytes[i] == b'\\' { 2 } else { 1 };
                    }
                }
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// Every string literal in `text`, escapes resolved to the character.
    fn literals(text: &str) -> Vec<String> {
        let mut out = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let mut i = 0usize;
        while i < chars.len() {
            if chars[i] != '"' {
                i += 1;
                continue;
            }
            i += 1;
            let mut lit = String::new();
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' && i + 1 < chars.len() {
                    i += 1;
                }
                lit.push(chars[i]);
                i += 1;
            }
            i += 1;
            out.push(lit);
        }
        out
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
