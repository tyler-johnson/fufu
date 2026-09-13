# Revisions and IDs

| Object being named | Where to get its ID | Commands that accept it |
| --- | --- | --- |
| One Git commit object: a hexadecimal commit SHA | [`ff log`](cli/log.md), [`ff show`](cli/show.md) | Revision arguments: `ff log -r`, `ff show`, [`ff describe`](cli/describe.md), [`ff restore --from`](cli/restore.md), [`ff absorb`](cli/absorb.md) / [`ff lift`](cli/lift.md) source and target flags. |
| A change across rewrites: a change ID using k–z | The letters column in `ff log`; `ff show --json` for a closed change's full ID | The same revision arguments; [`ff evolog`](cli/evolog.md) inspects the change's recorded evolution. |
| One retained operation: a hexadecimal operation ID | [`ff op log`](cli/op-log.md), [`ff history`](cli/history.md), `ff evolog` | [`ff op show`](cli/op-show.md), [`ff op diff`](cli/op-diff.md), [`ff op restore`](cli/op-restore.md), [`ff op revert`](cli/op-revert.md), and supported `--at-op` flags. |

The argument or flag chooses the **address space**: revisions name commits and the open change; operations name recorded repository states and actions. A hexadecimal operation ID is not a revision even though it looks like a commit SHA. Use the IDs printed in your repository; IDs in other pages' transcripts belong to those examples.

## Common examples

These reads assume a branch named `main` and at least three commits on its first-parent history. Quote expressions so the shell passes operators, spaces, and parentheses to fufu unchanged.

```sh
ff log -r HEAD                 # the current branch tip, one commit
ff show @                     # the open change
ff show '@^'                  # HEAD, the commit beneath the open change
ff show '@~3'                 # HEAD~2
ff log -r '@~3..@'            # on linear history: two commits and the open change
ff log -r 'HEAD~2..HEAD'      # on linear history: the last two recorded commits
ff log -r '::main'            # main and all its ancestors
ff log -r 'latest(description(substring:fix))'
```

The last command returns the newest matching commit, or no rows if no commit message contains `fix`. A range can include merged history; it is not a count of first-parent steps.

The following commands read **operations**. They assume at least four retained entries on this worktree's current log:

```sh
ff op show @                  # the current operation
ff op show '@^'               # its predecessor
ff op show '@~3'              # three operations back
ff op log '@~3..@'            # the three newest operations; excludes @~3
ff op log 'kind(op)'          # recorded command operations
ff op log 'on_branch(exact:main) & ~kind(capture)'
ff log -r 'base(@)'           # the commit the current operation ran on
```

Operations include individual snapshots. Three operation steps need not equal three undo steps: `ff history` groups adjacent snapshots from a session. Repository readers attempt a snapshot before resolving their arguments, so a dirty working copy can make `@` a new operation. See [snapshot timing and coverage](../concepts/snapshots-and-undo.md#when-snapshots-run).

## Commit SHAs and change IDs

A commit SHA identifies one exact Git object. Changing its message, content, parents, or signature changes its SHA. A full SHA is 40 hexadecimal characters in the repositories fufu supports today; displayed commit hashes are normally eight characters.

A full change ID is 32 letters from k–z. It identifies work across fufu rewrites: rewording, restacking, or moving content can change the SHA while surviving changes keep their change IDs. The open change's ID follows it into branch history. Its displayed internal SHA need not be the final committed SHA; see [internal storage and branch history](../concepts/changes.md#internal-storage-and-branch-history).

fufu and jj store change IDs in a `change-id` commit header. A commit without a valid header gets an ID derived from its SHA, identical wherever that object is read. A foreign rewrite that removes the header, or rewrites a headerless commit, changes that derived identity. Observing an outside rewrite does not reconstruct an identity the tool discarded. See [using fufu alongside Git](../concepts/two-regimes.md#returning-after-outside-changes).

### Prefixes and divergent copies

Commit-SHA and change-ID prefixes require **at least four characters**. Letter IDs and hexadecimal IDs are case-insensitive. The bold change-ID prefix in a colored log is the shortest distinguishing prefix **on that page**, sometimes only one letter. It does not promise that the resolver accepts that prefix or that it is unique elsewhere in the repository. Copy at least four characters and add more if resolution is ambiguous.

Change-ID lookup searches HEAD, local branches, and tags first; only if that finds no match does it search remote-tracking history. It scans at most 10,000 distinct commits across those searches. Use a commit SHA for older or unreachable history. The current open change's ID is also considered.

Two different change IDs with a shared prefix need a longer prefix. Two visible local commits with the **same full change ID** are divergent copies: a local branch or tag may still retain an earlier version of a rewrite. More letters cannot distinguish them; use the desired commit's SHA. A local rewrite is not considered divergent merely because its old version remains in remote-tracking history.

A name matching both a ref and an object/change ID is refused rather than guessed. Use the full ref path or full ID indicated by the error. Among refs alone, Git's lookup order applies: the literal name, `refs/<name>`, tags, local branches, remote-tracking refs, then a remote's `HEAD`. Full paths such as `refs/heads/main` and `refs/tags/v1` avoid that name overlap.

A full operation ID is 40 hexadecimal characters; operation views normally display twelve. Operation-prefix highlighting uses a different domain: retained live and abandoned operation history, including other worktrees' chains. A short hexadecimal operation prefix can be accepted even below four characters if unique. New operations can make a formerly unique prefix ambiguous, and trimming can remove or rewrite operation IDs. Keep a full ID when scripting and respect [retention limits](../concepts/snapshots-and-undo.md#coverage-and-limits).

## Revision names and suffixes

| Name | Meaning |
| --- | --- |
| `@` | The current open change, including when it is empty. It is distinct from HEAD. |
| `HEAD` | The checked-out commit; normally the current branch tip, or the detached commit. It does not exist before the first commit. |
| `trunk` | The configured or inferred main development branch. Resolution tries `fufu.trunk`, `origin/HEAD`, the sole local `main` or `master`, then a sole local branch. Ambiguity requires configuration. A literal ref named `trunk` pointing elsewhere is also ambiguous. |
| `main`, `refs/heads/main` | A local branch tip. Naming a branch does not include its parked change. |
| `v1`, `refs/tags/v1` | A lightweight or annotated tag, peeled to a commit. |
| `origin/main`, `refs/remotes/origin/main` | The locally recorded remote-tracking tip, not a query to the server. |
| A SHA or change ID | One resolved commit, or the open change when its change ID is named. |

Parent suffixes attach directly to a name or ID and can be chained:

| Suffix | Meaning and boundary |
| --- | --- |
| `x^`, `x^1` | First parent. A root commit has no parent. |
| `x^2` | Second parent of a merge; fails if that parent does not exist. Larger numbers select that parent number. |
| `x~`, `x~1`, `x~3` | Follow the first parent once or three times. These do not select a merge's third parent. |
| `x^0`, `x~0` | The commit itself, with no parent step. |
| `x^^`, `x~2^` | Chained steps, applied from left to right. |
| `x^{}`, `x^{commit}`, `x^{tag}`, `x^{object}` | Peel a tag, or require the named object type. The final result must still peel to a commit; trees and blobs are not revision-set members. |
| `main@{0}`, `main@{1}` | Current or previous entry in that ref's retained Git reflog. This is ref history, not snapshot history. |
| `main@{2026-09-13 12:00:00 +0000}` | The ref's Git reflog value at a date. Quote the whole expression in the shell. A date before the retained reflog can resolve to its oldest entry. |
| `main@{upstream}`, `main@{u}` | The branch's configured upstream; fails without a resolvable upstream. |
| `main@{push}` | The branch's configured push-tracking destination; requires a resolvable configuration and ref. |
| `@{-1}` | The previous checkout recorded in HEAD's Git reflog. Requires a retained Git-style checkout entry; fufu switches do not necessarily provide that entry. |
| `main^{/fix}` | The newest reachable commit whose message contains the literal search text. This Git-style suffix uses text matching in the current build, not regular expressions. |

Message search also accepts `main^{/!-fix}` to find a message that does not contain `fix`; `!!` at the start of the search text escapes a literal leading `!`.

For the open change, `@^`, `@^1`, `@~`, and `@~1` all step onto `HEAD`. Thus `@~3` is `HEAD~2`. `@^0` and `@~0` remain `@`. The open change has only one parent: `@^2` fails, but `@^^2` can select HEAD's second parent after the first step. Before the first commit, `@` and `::@` still work; `HEAD` and parent steps do not.

The open change has no reflog: `@@{1}` is refused. Bare `@{1}` is the Git reflog shorthand starting from HEAD, not a suffix on the open change. Tag peeling and message search cannot be applied directly to `@`; first step onto a commit if needed.

## Revision sets and grammar

A **revision set**, or **revset**, is an expression selecting zero, one, or several revisions. `ff log -r main` selects one tip; `ff log -r '::main'` selects its history. Without `-r`, log walks from HEAD and shows the open row in its changes view. Both log families default to 25 rows; `-n 0` removes the row limit.

`ff show`, `ff describe <rev>`, `ff restore --from`, and absorb/lift `--into` require exactly one revision. Empty and multiple-member results are refused; fufu does not silently choose one. `ff log` can display an empty set. Absorb/lift `--from` requires a nonempty contiguous run on the branch's line, with the open change allowed; being a valid set alone does not make it a valid rewrite source. Preview a source with log before moving it.

### Set operators and range endpoints

In this table, `a` and `b` are expressions. Ancestors and descendants include their starting members. The **visible commit universe** is history reachable from HEAD, local branches, tags, and remote-tracking refs. It excludes fufu's internal refs and does not automatically include the open change. A directly named retained commit SHA can still resolve outside that universe.

| Form | Selection |
| --- | --- |
| `a \| b` | Union, with duplicate members removed. |
| `a & b` | Intersection. |
| `~a` | Visible commits except the exact members of `a`; ancestors are not implicitly removed. |
| `a & ~b` | Set difference within the visible universe. |
| `::b`, `..b` | `b` and its ancestors; a missing left endpoint excludes nothing. |
| `a..b` | Ancestors of `b` excluding ancestors of `a`. On a linear chain, excludes `a` and includes `b`. |
| `a..` | All visible commits excluding ancestors of `a`; the missing right endpoint does **not** mean HEAD or `@`. |
| `a::b` | Descendants of `a` that are also ancestors of `b`, including endpoints when connected. |
| `a::` | Descendants of `a` within visible history. This can scan the whole visible history. |

At least one endpoint is required; bare `..` and `::` are invalid. Endpoints can be sets, for example `(main | origin/main)..HEAD`. These ancestry forms follow all commit parents, while `~3` follows only first parents. Prefer explicit right endpoints for a branch-local rewrite: `HEAD~2..HEAD` selects that branch's last two commits on linear history; `HEAD~2..` can also include other branches.

Include the open change explicitly, for example `@ | ::HEAD` or `trunk..@`. Predicates and complements scan committed history; they do not implicitly add `@`. Current open-change handling has these boundary exceptions:

- `@..@` retains the open row instead of returning an empty set.
- `HEAD::@` returns HEAD without the open row. Forward ranges only include `@` when it is in the starting set.
- `heads(@ | HEAD)` can retain both members, and `roots(@ | HEAD~)` can retain both, because the extrema functions do not fully account for the open change's ancestry.

Use `@` directly when you want just the open change, and committed endpoints when relying on ordinary set identities.

Forward revision walks currently have an ordering limitation: with equal commit timestamps and refs on intermediate commits, `a::` can omit descendants. Check the returned members before using such a selection for a rewrite; bounded `..` ranges are the usual choice for the source runs shown in this documentation.

### Precedence and parentheses

From tightest to loosest:

1. Names with attached suffixes, function calls, and parenthesized expressions.
2. Ancestry and ranges: `::`, `..`.
3. Prefix complement: `~`.
4. Intersection: `&`.
5. Union: `|`.

Operators at the same level associate left to right. `main | HEAD & trunk` means `main | (HEAD & trunk)`; `~main::HEAD` means `~(main::HEAD)`. Use parentheses when combining ranges or when the intended grouping is not obvious. Suffixes belong on revision leaves, not on parenthesized sets or function results. Whitespace can separate operators, but a suffix stays attached: `HEAD~2` is ancestry; `HEAD ~ 2` is not.

The complete set-expression structure is:

```text
expression    := revision-with-suffixes | "(" expression ")" | function-call
                 | "~" expression
                 | expression ("|" | "&" | "::" | "..") expression
                 | ("::" | "..") expression
                 | expression ("::" | "..")
function-call := name "(" argument ("," argument)* ")"
argument      := expression | pattern
```

The parser accepts nested expressions as endpoints, including prefix complements; parenthesize these for clarity. Function signatures below constrain argument kinds and counts. An empty expression is an error; it is not an empty set.

### Functions

Every implemented function takes exactly **one argument**. There are no zero-argument `heads()` or `roots()` defaults.

| Revision function | Result |
| --- | --- |
| `latest(set)` | The first member in newest-first evaluation order, with `@` first when included. This uses commit time, not change-ID order; do not depend on a particular winner at equal timestamps. Empty input stays empty. |
| `heads(set)` | Members without descendants elsewhere in the set. May return several unrelated tips. See the open-change exception above. |
| `roots(set)` | Members without ancestors elsewhere in the set. May return several roots; empty input stays empty. See the open-change exception above. |
| `description(pattern)` | Visible commits whose full message matches; the message can include a trailing newline. Does not search the pending open description. |
| `author(pattern)` | Visible commits whose author name **or** author email matches, tested separately. |
| `base(operations)` | Distinct commits the selected operations ran on. The argument is evaluated in operation space; entries without a base, such as the initial recovery point, contribute nothing. |

For example, `ff log -r 'base(@~3..@)'` maps the three latest operations to their base commits and removes duplicates. `@` inside `base()` means the current **operation**. This is a query about recorded bases, not a way to render the whole repository as it looked then. `base()` belongs in revision expressions and is refused in `ff op log`.

### Pattern types

`description`, `author`, and the operation predicates `on_branch` and `session` accept these case-sensitive patterns:

| Pattern | Meaning |
| --- | --- |
| `fix`, `substring:fix` | Contains the literal text `fix`. Bare arguments default to substring matching. |
| `exact:main` | Equals the entire field. |
| `glob:fix*` | Matches the whole field using Git-style wildcards: `*`, `?`, and character classes such as `[ab]`. Slashes are ordinary characters; `*` can cross them. A backslash escapes a wildcard. |
| `substring:"fix bug"` | Quoted value containing spaces or expression punctuation. |

For example, `ff log -r 'description(substring:"fix bug")'` passes both shell quoting and pattern quoting correctly. Inside a double-quoted pattern value, `\"` means a literal quote and `\\` a literal backslash; other backslashes are preserved. Use an explicit pattern prefix when quoting. An empty substring matches every field; an empty exact pattern matches only an empty field. `regex:` is recognized but refused.

## Operation expressions

Operation leaves are `@` or hexadecimal operation IDs/prefixes. Branches, tags, `HEAD`, `trunk`, and change IDs do not name operations. Use `on_branch(exact:main)` to select operations recorded on a branch, rather than passing `main` as a leaf.

Operation suffixes follow the recorded predecessor link: `^` or `^1` for one step, `~n` for n steps, with chaining such as `@^^` or `@~2^`. `~0` stays at the same operation. Unlike revisions, `^0` is refused, as are `^2` and larger parent numbers. No suffix crosses to the base commit. A step before the earliest retained operation fails with `op/floor`.

Union, intersection, complement, `..`, `::`, and parentheses use the same precedence as revisions. The operation universe is the current worktree's live log: `::@` includes its earliest recovery point, `..@` is the same set, `@~3..@` excludes the older endpoint, and `@~3::@` includes both. A missing right endpoint means the live tip. Operations abandoned by undo are outside that universe but can still be named directly by a retained ID. For those entries, prefer explicit IDs and predecessor walks; forward ranges are walks of the live chain.

| Operation function | Result |
| --- | --- |
| `latest(set)`, `heads(set)` | The newest member; empty input stays empty. |
| `roots(set)` | The oldest member; empty input stays empty. |
| `on_branch(pattern)` | Operations whose recorded branch matches. |
| `session(pattern)` | Operations whose recorded session label matches. An unlabeled operation does not match. |
| `kind(capture)` | Snapshots. The other exact kind names are `op` (recorded command), `foreign` (observed outside ref changes), and `note` (log bookkeeping). This argument is a kind name, not a wildcard search. |

For example, `ff op log 'latest(kind(capture))'` selects the latest snapshot, while `ff op log 'session(exact:nightly)'` selects entries labeled `nightly`. These functions take one argument, as in revision space. `description()` and `author()` are not operation predicates.

## Paths, sources, and past-state reads

Argument positions are command-specific:

```sh
ff log main                   # history of a path called main
ff log -r main                # the revision named main
ff log -r '::main' src/       # that history filtered to paths under src/
ff show main src/             # one revision's patch filtered to src/
```

Paths are literal file names or directory prefixes, not globs. `ff log` follows a single file's renames by default; with `-r` it filters the set without following renames. `ff show` takes its revision first and paths afterward. For restore, paths are positional and the source uses a flag:

| Restore source | Accepted kind and default |
| --- | --- |
| No source flag | HEAD, the commit beneath the open change. Requires paths or `--all`. |
| `--from <rev>` | A revision expression resolving to exactly one commit. Branches, tags, SHA/change IDs, and functions such as `base()` work. The open change `@` is refused. |
| `--at-op <op>` | One operation address: hexadecimal ID/prefix, `@`, and first-parent suffixes. Does not accept a set or a function such as `latest()`. |
| `--at <time>` | The first operation on the live log at or before that time. Compact ages use `s`, `m`, `h`, `d`, or `w`; dates use Git-style date parsing. No operation at or before the time is an error. This flag does not accept an ID. |

Choose one source flag. Restore resolves its source **before** its mandatory pre-restore snapshot, then writes only selected worktree files. It does not move HEAD, branches, or the index. Run recovery exercises in a scratch repository; see the [recovery guide](../guides/recovery.md) for the capture mechanism and complete examples.

Past-state flags are not a global historical-view mode:

| Command | Current `--at-op` / `--at` behavior |
| --- | --- |
| `ff restore` | Uses the selected operation's file tree as the source. |
| `ff op log` | Without an expression, walks backward from the selected operation. With an expression, evaluates it against the **live** log, then keeps only results at or before the selected operation. `@` in that expression still means the live tip. |
| `ff op show` | Uses the selected operation when the positional operation is omitted. An explicit positional address takes precedence. |
| `ff op diff` | Uses the selected operation for the omitted second endpoint. An explicit second endpoint takes precedence; the first is always explicit. |
| [`ff status`](cli/status.md), `ff log`, `ff evolog`, [`ff branch`](cli/branch.md), [`ff worktree`](cli/worktree.md) | Declare these flags but refuse past-state reads with `usage/at-op-unsupported`. |

`ff show`, [`ff remote`](cli/remote.md), [`ff collide`](cli/collide.md), and the map do not declare these flags. Show reads a revision's patch; use `ff op show` for an operation's recorded files and ref transitions. For example, `ff op log --at-op '@~3'` starts three operations back, while `ff op log '@' --at-op '@~3'` returns no rows because the live tip is outside that bound.

## Differences from Git and jj

The implemented language is a subset of Git's revision spellings plus fufu's set operators. It does not implement every form accepted by Git or jj.

- `a...b` is refused. Use `(a..b) | (b..a)` for symmetric difference.
- jj's `x-` is refused; use `x^`. Infix `a ~ b` and adjacent `a b` are refused; write `a & ~b` or an explicit union/intersection.
- `x+` and `descendants()` are unavailable. `x::` is the supported unbounded descendant form.
- Git's `x^!`, `x^@`, and `x^-n` range shorthands are not supported. Write sets explicitly, such as `main^..main` or `main^ | main^2` when main is a merge.
- `HEAD:path`, index-stage paths, and a bare `:/message` are not revision leaves. Use [`ff git`](cli/git.md) for Git object/path syntax, or `description(pattern)` for commit-message selection.
- `regex:` predicates are unavailable. Use `substring:`, `exact:`, or `glob:`. Recognizing a spelling in an error message does not mean it is implemented.
