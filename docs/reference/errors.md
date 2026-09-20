# Error IDs

Use [`ff explain <id>`](cli/explain.md) to read what a structured failure means and how to proceed. Copy the ID from the error, such as `branch/exists` or `held/moved`. `ff explain` works outside a repository; `ff explain --list` lists the installed binary's catalog.

Read the specific message before using a suggested command. Replace placeholders such as `<branch>`, `<path>`, and `<op>` with your own values, and retain the original command's targets and flags when retrying. Suggestions are diagnostic steps or choices with prerequisites, not a script to run from top to bottom. The [revisions and IDs reference](revisions.md) explains which arguments take commit SHAs, change IDs, or operation IDs.

A refusal does not imply the invocation made no disk writes or network requests. Capture, reconciliation, hooks, or fetching may already have run. A multi-branch command can update some branches before another refuses. Inspect the report and the named checkout's state before deciding whether to retry, resolve, or recover.

For scripts, `ff explain --list --json` returns entries with `id`, `summary`, `detail`, `exits`, and `exit`. Structured command failures use `error.id`; prefer that over parsing message prose. IDs and payloads have changed across releases while the envelope stayed at `ff: 1`, so [pin and test supported versions](../agents/machine-surface.md#compatibility-in-current-releases).

## Exit codes

Interpret the process exit together with its error or data report:

| code | meaning |
| --- | --- |
| 0 | Success; inspect any per-branch holds or skips in the report. |
| 1 | Failure or a negative check result; a multi-branch push may have partly succeeded. |
| 2 | Invalid, unsupported, or incompatible command-line input. |
| 3 | Held outcome or a `held/*` refusal requiring attention; not proof that a resolution session exists. |
| 4 | A ref update was contended; inspect any partial progress before a bounded retry. |

For a structured error, the code follows its ID: `usage/*` exits 2, `held/*` exits 3, `ref/contended` exits 4, and other IDs exit 1. Successful data reports have command-specific rules. [`ff pull`](cli/pull.md) and [`ff restack`](cli/restack.md) can report holds at exit 3; [`ff describe`](cli/describe.md), [`ff absorb`](cli/absorb.md), [`ff lift`](cli/lift.md), and [`ff done`](cli/done.md) can land their primary update and report downstream holds at exit 0. [`ff push`](cli/push.md) exits 1 for any refused send, otherwise 3 when a selected branch is blocked by a hold. See [conflict reports](../concepts/held-rewrites.md#reading-conflict-reports) and [command-specific machine output](../agents/machine-surface.md).

- On 3, inspect the ID and branch. A recorded [held rewrite](../concepts/held-rewrites.md) can be opened with [`ff resolve`](cli/resolve.md), but `held/op-revert` is an applicability refusal with no hold to resolve. Abandoning a rewrite resolution drops both its session and hold; retrying the original rewrite is how to start again. A parked-change arrival resolves in place and has no session for `ff done`.
- On 4, wait for the competing writer and cap retries. Contention can be reported after capture, after other writes, or after a whole-state restoration has applied but its operation pointer has not moved. Read the diagnostic and inspect [`ff status`](cli/status.md) and [`ff op log`](cli/op-log.md) when it reports partial progress. Keep explicit target IDs for retries; relative addresses can change meaning after a write.

`push/unrecorded` exits 1 after a successful remote update and failed local bookkeeping. The note, published pointer, and seen pointer are separate writes, so some may already exist. Inspect the remote and local log, then fix the reported write problem. Another push can report “nothing to push” without repairing a missing record; a reporting pull updates seen state but does not recreate a missing push note or published pointer. Local undo cannot reverse the remote update. See [leases and push records](../concepts/push-boundary.md#push-carries-a-lease).

## Choosing a repair

- **Wrong name or selection:** inspect the available branches, paths, or IDs and retry with a supported expression. An incomplete expression, an empty result, an ambiguous prefix, and divergent copies of one change ID are different failures.
- **Unfinished Git operation:** use `git status` in the checkout named in the message, then complete or abort that operation with Git. A rebase, merge, cherry-pick, revert, mailbox application, or bisect has its own completion commands.
- **Missing Git or signing program:** install or repair the named executable in the invoking environment. [`ff doctor`](cli/doctor.md) checks configuration and program availability but does not prove that credentials or signing keys work. It can capture, fetch, and run maintenance even without `--fix`.
- **Strict-policy refusal:** use a fufu command when it matches the intended task, or explicitly change the policy before using the Git passthrough. Strict currently refuses tag pushes even though fufu's push command sends branches only. See [Git policy](../concepts/two-regimes.md#which-program-ran).
- **Missing recovery data:** select an available retained state. Force options cannot recreate missing objects, and increasing retention cannot recover already removed history. Ignored untracked files, oversized content, uncaptured edits, and unsaved buffers need the [coverage limits](../concepts/snapshots-and-undo.md#coverage-and-limits).

## The index

The table lists all 128 catalog entries with their structured-error exit codes. `internal` is the fallback for unclassified failures. `repo/not-found` covers repository-discovery failures. The installed binary's `ff explain --list` is the catalog to use when it differs from this page.

<!-- errors:begin — generated from crates/ff-cli/src/explain/errors.toml by a test; edit there, then make docs-gen -->

| id | exit | meaning |
| --- | --- | --- |
| `absorb/into-trunk` | 1 | absorb's implicit target would rewrite a trunk commit |
| `branch/ambiguous` | 1 | that name matches more than one branch |
| `branch/checked-out-elsewhere` | 1 | another worktree has that branch checked out |
| `branch/exists` | 1 | a branch with that name already exists |
| `branch/invalid-name` | 1 | that branch name is not a valid Git ref name |
| `branch/is-current` | 1 | the branch to delete is the current branch |
| `branch/not-found` | 1 | the requested branch or ref was not found |
| `branch/shared-lease-refused` | 1 | the remote refused the deletion lease after the local branch was deleted |
| `branch/shared-moved` | 1 | the remote-tracking tip differs from the seen record; deletion was refused |
| `branch/shared-unseen` | 1 | there is no seen record for the remote copy; deletion was refused |
| `clone/bad-url` | 1 | the clone URL, name, or destination setup is invalid |
| `clone/failed` | 1 | objects arrived but the working-copy checkout failed |
| `clone/refused` | 1 | clone could not fetch the requested repository or branch |
| `clone/target-exists` | 1 | the clone destination is not empty |
| `clone/unreachable` | 1 | the clone transport could not establish a connection |
| `commit/empty` | 1 | there are no selected changes to commit |
| `edit/not-in-history` | 1 | the commit to edit is not in the current branch's history |
| `editor/failed` | 1 | the description editor could not start or exited unsuccessfully |
| `fetch/not-here` | 1 | this command does not support the --fetch preflight |
| `fold/conflict` | 1 | the source cannot replay cleanly onto the fold target |
| `fold/other-tree-conflict` | 1 | the target worktree's open change conflicts with the fold result |
| `fold/remote-target` | 1 | fold requires a local target branch |
| `fold/trunk-source` | 1 | fold cannot remove trunk as its source branch |
| `held/already-held` | 3 | the named branch already has a held rewrite blocking this request |
| `held/expired` | 3 | the held request or its resolution state can no longer be used |
| `held/moved` | 3 | the current rewrite no longer produces the conflicts this session was opened for |
| `held/none` | 3 | the current branch has no held request to resolve, and no merge of its base to continue |
| `held/op-revert` | 3 | refs no longer have the values required to invert this operation |
| `held/resolving` | 3 | a resolution session for this hold is already open |
| `held/rewrite-conflict` | 3 | a commit could not be replayed over the requested rewrite |
| `held/unresolved` | 3 | the resolution still has markers or produces another replay conflict |
| `held/unsupported` | 3 | the current open change prevents this hold from being resolved |
| `hook/declined` | 1 | a commit-time Git hook exited unsuccessfully |
| `hook/failed` | 1 | a client's hook file or plugin directory could not be written, or did not read back |
| `hook/malformed` | 1 | a client's config file is not the JSON object its client reads |
| `identity/missing` | 1 | the commit author name or email is not configured |
| `init/bare` | 1 | ff init does not create bare repositories |
| `init/failed` | 1 | the repository could not be created at that path |
| `internal` | 1 | an unclassified failure |
| `merge/base` | 1 | the target is this branch's base, which fufu never merges in |
| `merge/nothing` | 1 | the target is already in this branch, or is this branch |
| `merge/unrelated` | 1 | the branch and the target to merge have no common ancestor |
| `op/ambiguous` | 1 | that hexadecimal prefix matches more than one retained operation |
| `op/floor` | 1 | the request steps before the earliest retained operation |
| `op/not-found` | 1 | the requested operation or recovery source was not found |
| `op/nothing-to-redo` | 1 | there is no available redo step from the current operation tip |
| `op/trimmed` | 1 | that operation is outside retained live history |
| `op/unreadable` | 1 | required operation or ref data is unavailable or unusable |
| `pull/ambiguous-remote` | 1 | multiple remotes are configured and this branch has no selected remote |
| `pull/fetch-failed` | 1 | the remote fetch or its repository setup failed |
| `push/failed` | 1 | Git failed to complete the requested remote update |
| `push/lease-refused` | 1 | the expected remote tip no longer permits this push |
| `push/no-git` | 1 | fufu could not start Git for the network operation |
| `push/rejected` | 1 | the remote rejected the requested ref update |
| `push/retarget` | 1 | --to conflicts with the branch's configured remote |
| `push/unknown-remote` | 1 | --to named a remote that is not configured |
| `push/unreachable` | 1 | Git reported a connection, authentication, or repository failure |
| `push/unrecorded` | 1 | the remote update succeeded but local push bookkeeping failed |
| `push/unseen` | 1 | replacing remote commits requires a seen record that is missing |
| `ref/contended` | 4 | a ref update lost a race or could not acquire its lock |
| `repo/bare` | 1 | this is a bare repository, and the command needs a working copy |
| `repo/detached` | 1 | the command needs a branch with a usable current state |
| `repo/mid-operation` | 1 | an unfinished Git operation blocks this command |
| `repo/not-found` | 1 | a Git repository could not be discovered from this directory |
| `restack/no-base` | 1 | no base branch was found for this restack |
| `restack/own-remote` | 1 | restack cannot target the branch's own remote copy |
| `restack/unrelated` | 1 | the branch and requested base have no common ancestor |
| `restore/nothing-selected` | 1 | restore needs paths or --all |
| `revset/deferred-descendants` | 1 | x+ and descendants() are not implemented |
| `revset/regex-unavailable` | 1 | regex patterns are recognized but not implemented |
| `rewrite/not-in-history` | 1 | a selected commit is outside the history this rewrite can use |
| `session/moved` | 1 | the editing session branch no longer has its expected commit structure |
| `session/none` | 1 | the current branch is not an editing session to finish or abandon |
| `session/open` | 1 | an editing session blocks the requested operation |
| `session/unreachable` | 1 | the edited commit is no longer in the destination branch's history |
| `sign/failed` | 1 | the signer failed or did not produce a signature |
| `sign/no-key` | 1 | SSH signing has no configured or default key |
| `sign/no-program` | 1 | the configured signing program could not be started |
| `sign/unknown-format` | 1 | gpg.format is not openpgp, x509, or ssh |
| `switch/nothing-opened` | 1 | -m cannot describe a new change when switching to an existing branch |
| `target/unresolvable` | 1 | the command has no usable committed target |
| `undo/not-undoable` | 1 | op revert cannot invert a capture or note |
| `undo/nothing` | 1 | the current worktree's operation log has no undo step |
| `undo/trimmed` | 1 | some objects needed for the requested state are missing |
| `usage/absorb-into-open` | 2 | absorb's default source and the requested target are both the open change |
| `usage/at-op-unsupported` | 2 | this command accepts past-state flags but does not implement them |
| `usage/bad-flags` | 2 | an option is missing, retired, misplaced, or incompatible with another |
| `usage/bad-restore-target` | 2 | --at could not parse the requested age or date |
| `usage/bad-session` | 2 | the session label is empty, too long, or contains control characters |
| `usage/bad-value` | 2 | the value is not valid for this setting or option |
| `usage/collide-same-branch` | 2 | collide resolved both sides to the same branch |
| `usage/fold-into-self` | 2 | a branch cannot be folded into itself |
| `usage/foreign-verb` | 2 | this Git or jj spelling needs a different fufu command |
| `usage/git-policy` | 2 | strict Git policy refused this passthrough command |
| `usage/lift-from-open` | 2 | lift's requested source and default target are both the open change |
| `usage/move-gap` | 2 | the sources are not one contiguous run of commits |
| `usage/move-into-self` | 2 | the move's only source is its target |
| `usage/needs-message` | 2 | a required commit description is missing or empty |
| `usage/no-such-directory` | 2 | fufu could not enter the directory given to -C |
| `usage/no-such-field` | 2 | a --fields path names a key the JSON payload does not carry |
| `usage/no-such-path` | 2 | the path is absent from the working copy and HEAD |
| `usage/op-in-rev-position` | 2 | an operation ID was used where a revision is required |
| `usage/restack-onto-self` | 2 | a branch cannot be restacked onto itself |
| `usage/rev-in-op-position` | 2 | a revision or non-operation parent was used in operation space |
| `usage/revset-adjacent-operands` | 2 | two revision expressions need an operator between them |
| `usage/revset-ambiguous` | 2 | that name or prefix has multiple revision matches |
| `usage/revset-arity` | 2 | a function received the wrong argument count, kind, or value |
| `usage/revset-divergent` | 2 | one change ID identifies multiple visible commits |
| `usage/revset-empty` | 2 | the revision expression contains no text |
| `usage/revset-empty-set` | 2 | the expression is valid but selects no target |
| `usage/revset-expected-expression` | 2 | an operator or function call is missing an expression |
| `usage/revset-no-symmetric-difference` | 2 | the a...b range syntax is not supported |
| `usage/revset-not-a-commit` | 2 | the revision names a tree or blob rather than a commit |
| `usage/revset-not-a-point` | 2 | the expression selects multiple revisions where one is required |
| `usage/revset-not-a-range` | 2 | the set is not one range with a single head and root |
| `usage/revset-open-suffix` | 2 | that suffix is not supported on the open change @ |
| `usage/revset-parent-shorthand` | 2 | use ^ or ~n for parents, not the x- shorthand |
| `usage/revset-range-suffix` | 2 | x^! and x^@ are not supported revision suffixes |
| `usage/revset-unbalanced-parens` | 2 | the expression has unmatched parentheses or a misplaced comma |
| `usage/revset-unknown-function` | 2 | that function is not available in this address space |
| `usage/revset-unknown-revision` | 2 | no revision matches that name or prefix |
| `usage/revset-unterminated-brace` | 2 | a revision suffix has an opening brace without its closing brace |
| `usage/revset-unterminated-quote` | 2 | a quoted pattern value is missing its closing quote |
| `usage/revset-wrong-space` | 2 | that function belongs in the other address space |
| `usage/unknown-error-id` | 2 | this binary has no catalog entry for that error ID |
| `usage/unknown-key` | 2 | that name is not a supported fufu setting |
| `usage/unknown-slug` | 2 | the hook integration name is unknown or could not be inferred |
| `worktree/busy` | 1 | the target worktree's snapshot could not acquire its operation-log lock |
| `worktree/exists` | 1 | the worktree destination is occupied or cannot be named |
| `worktree/is-current` | 1 | the checkout to remove is the command's current worktree |
| `worktree/is-main` | 1 | the main worktree cannot be removed by this command |
| `worktree/not-found` | 1 | no linked worktree matches that ID or path |
| `worktree/unborn` | 1 | the worktree needs a committed checkout target |

<!-- errors:end -->
