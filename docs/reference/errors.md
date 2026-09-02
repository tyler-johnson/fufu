# Error ids

Every failure fufu reports carries a stable id: `ref/contended`, `usage/bad-flags`, `held/rewrite-conflict`. The id is the contract and the message is not. Prose gets reworded between releases; ids do not, so a script branches on `error.id` and never matches a sentence. [`ff explain <id>`](cli/explain.md) has the long form of each, with the ways out. `ff explain --list` prints this same table from the binary you have, and `ff explain --list --json` hands it over as data, each entry carrying its `exit`.

## Exit codes

Five codes, one meaning each:

| code | meaning |
| --- | --- |
| 0 | done — or yes, for a command that answers a question |
| 1 | no — the command failed, or the check's answer is negative |
| 2 | the command line was wrong |
| 3 | held — nothing was touched, and a human decision is required |
| 4 | contended — nothing was touched, and the same command run again is the answer |

The code follows the id: `usage/*` exits 2, `held/*` exits 3, `ref/contended` exits 4, everything else exits 1. For a script, 3 and 4 are the two codes that mean nothing moved, and they ask opposite things. On 3 stop and surface it: a [held rewrite](../concepts/held-rewrites.md) is parked, and only a person can say what happens next. On 4 run the same command again, with a cap: contention is another writer holding the ref for a moment, but a lock file nobody clears makes the answer the same every time, so retry a few times and then surface that.

The near miss is `publish/unrecorded`, which stays at 1: the push landed and only the operation log lost the race to record it, so the tree did move, and re-running `ff publish` records it.

## The index

Every id in the registry behind `ff explain`, one row each, with the code it exits and its one-line meaning. Two ids are structural rather than raised. `internal` is what every uncoded failure reports, and its message is the whole of what is known. `repo/not-found` is the command running somewhere no git repository can be found. The table is generated from `crates/ff-cli/src/explain.rs` by a test — edit there, then `make docs-gen`.

<!-- errors:begin — generated from ENTRIES in crates/ff-cli/src/explain.rs by a test; edit there, then make docs-gen -->

| id | exit | meaning |
| --- | --- | --- |
| `branch/aliased-copy` | 1 | the copy that branch tracks wears another branch's name |
| `branch/ambiguous` | 1 | that branch prefix matches more than one branch |
| `branch/checked-out-elsewhere` | 1 | another worktree has that branch checked out |
| `branch/exists` | 1 | a branch of that name already exists |
| `branch/invalid-name` | 1 | git would not accept that branch name |
| `branch/is-current` | 1 | that is the branch you are on |
| `branch/not-found` | 1 | no branch here goes by that name |
| `branch/shared-lease-refused` | 1 | the shared copy moved since you last looked, so it was not deleted |
| `clone/bad-url` | 1 | that is not a URL fufu can address |
| `clone/failed` | 1 | the pack arrived and the working tree could not be written |
| `clone/refused` | 1 | the remote answered, and said no |
| `clone/target-exists` | 1 | the directory to clone into already has something in it |
| `clone/unreachable` | 1 | the remote never answered |
| `commit/empty` | 1 | there is nothing to close: the tree matches HEAD |
| `edit/not-in-history` | 1 | that commit is not in the branch you are standing on |
| `editor/failed` | 1 | the editor did not produce a description |
| `held/already-held` | 3 | a rewrite is already held on this branch |
| `held/expired` | 3 | the held rewrite no longer has a question to answer |
| `held/moved` | 3 | the repository changed while the resolution was open |
| `held/none` | 3 | nothing is held on this branch |
| `held/op-revert` | 3 | the inversion conflicts with work done since |
| `held/resolving` | 3 | a resolution is already open on this branch |
| `held/rewrite-conflict` | 3 | the rewrite stops at a commit it cannot replay |
| `held/unresolved` | 3 | conflict markers are still standing in the working tree |
| `held/unsupported` | 3 | the held rewrite selected paths, and the open change reaches beyond them |
| `hook/declined` | 1 | one of your git hooks refused the commit |
| `identity/missing` | 1 | git has no name and email to sign work with |
| `init/bare` | 1 | ff init was asked for a bare repository |
| `init/failed` | 1 | the repository could not be created there |
| `internal` | 1 | an unclassified failure |
| `op/ambiguous` | 1 | that id prefix matches more than one operation |
| `op/floor` | 1 | there is nothing recorded before that operation |
| `op/not-found` | 1 | no operation goes by that id |
| `op/nothing-to-redo` | 1 | there is no forward step to take |
| `op/trimmed` | 1 | that operation is no longer on the log |
| `op/unreadable` | 1 | an operation on the log could not be decoded |
| `publish/failed` | 1 | the push did not go through |
| `publish/lease-refused` | 1 | the remote moved since you last looked at it |
| `publish/no-git` | 1 | git is not on PATH, and pushing still needs it |
| `publish/rejected` | 1 | the remote refused the push |
| `publish/retarget` | 1 | the branch already answers to a different remote |
| `publish/unknown-remote` | 1 | --to named a remote this repository does not have |
| `publish/unreachable` | 1 | the remote never answered |
| `publish/unrecorded` | 1 | the push went through and the log could not write it down |
| `ref/contended` | 4 | another process is holding that ref |
| `repo/bare` | 1 | this is a bare repository, and the verb needs a working tree |
| `repo/detached` | 1 | HEAD is not on a branch |
| `repo/mid-operation` | 1 | git is in the middle of something |
| `repo/not-found` | 1 | no git repository here, or in any parent directory |
| `restack/no-base` | 1 | there is no base to replay this branch onto |
| `restack/own-remote` | 1 | a branch cannot be restacked onto its own shared copy |
| `restack/unrelated` | 1 | the branch and its base share no history |
| `restore/nothing-selected` | 1 | restore was given nothing to restore |
| `revset/deferred-descendants` | 1 | descendants are not available yet |
| `revset/regex-unavailable` | 1 | regex patterns are recognized but not available yet |
| `rewrite/merge-in-range` | 1 | a merge commit sits in the range being replayed |
| `rewrite/not-in-history` | 1 | that commit is not in the history under you |
| `session/moved` | 1 | the session branch has commits of its own now |
| `session/none` | 1 | there is no editing session to finish |
| `session/open` | 1 | you are already inside an editing session |
| `session/unreachable` | 1 | the edited commit has left the branch the session lands on |
| `sign/failed` | 1 | the signing program ran and refused |
| `sign/no-key` | 1 | ssh signing needs a key and user.signingkey is empty |
| `sign/no-program` | 1 | the signing program is not on PATH |
| `sign/unknown-format` | 1 | gpg.format names a signing format fufu does not know |
| `sync/ambiguous-remote` | 1 | more than one remote, and nothing says which one this branch answers to |
| `sync/fetch-failed` | 1 | git could not fetch from the remote |
| `target/unresolvable` | 1 | that target resolves, but not to something this verb can use |
| `undo/not-undoable` | 1 | that operation has nothing in it to invert |
| `undo/nothing` | 1 | the operation log has nothing left to undo |
| `undo/trimmed` | 1 | the state that undo would put back has been trimmed away |
| `usage/absorb-into-open` | 2 | absorb was named the open change as its target |
| `usage/at-op-unsupported` | 2 | that verb does not read a past state yet |
| `usage/bad-flags` | 2 | those flags do not go together |
| `usage/bad-restore-target` | 2 | --at was given something that is neither an age nor a date |
| `usage/bad-session` | 2 | that is not a usable session name |
| `usage/bad-value` | 2 | the value did not parse as this setting's type |
| `usage/collide-same-branch` | 2 | collide was given one branch twice |
| `usage/foreign-verb` | 2 | that is a git verb fufu answers rather than runs |
| `usage/git-policy` | 2 | fufu.gitPolicy is strict, and this git word has a fufu verb |
| `usage/lift-from-open` | 2 | lift was named the open change as its source |
| `usage/mcp-verb-unavailable` | 2 | that verb is not offered through the MCP tool |
| `usage/needs-message` | 2 | a description was needed and there was no terminal to ask on |
| `usage/no-such-directory` | 2 | -C names a directory that is not there |
| `usage/no-such-path` | 2 | that path names nothing here |
| `usage/op-in-rev-position` | 2 | that is an operation, and this position takes a revision |
| `usage/restack-onto-self` | 2 | a branch cannot be restacked onto itself |
| `usage/rev-in-op-position` | 2 | that names a commit, and this position takes an operation |
| `usage/revset-adjacent-operands` | 2 | two revisions stand side by side with no operator between them |
| `usage/revset-ambiguous` | 2 | that name is both a ref and an object, and fufu will not pick one |
| `usage/revset-arity` | 2 | that function was called with the wrong arguments |
| `usage/revset-empty` | 2 | the revset is empty |
| `usage/revset-empty-set` | 2 | the expression is valid and matches nothing |
| `usage/revset-expected-expression` | 2 | an operator or a call is missing the expression it needs |
| `usage/revset-no-symmetric-difference` | 2 | there is no `a...b`; the set language already says it |
| `usage/revset-not-a-commit` | 2 | that names an object, but not a commit |
| `usage/revset-not-a-point` | 2 | the expression matches more than one revision, and this takes exactly one |
| `usage/revset-open-suffix` | 2 | `@` is the open change, and it takes no suffixes |
| `usage/revset-parent-shorthand` | 2 | there is no `x-` suffix; git already spells it `x^` |
| `usage/revset-range-suffix` | 2 | `x^!` and `x^@` are rev-list ranges, not revisions |
| `usage/revset-unbalanced-parens` | 2 | the parentheses in that expression do not pair up |
| `usage/revset-unknown-function` | 2 | no revset function goes by that name |
| `usage/revset-unknown-revision` | 2 | nothing in revision space answers to that name |
| `usage/revset-unterminated-brace` | 2 | a git suffix opened a brace and never closed it |
| `usage/revset-unterminated-quote` | 2 | a pattern value opened a quote and never closed it |
| `usage/revset-wrong-space` | 2 | that function reads operations, and this position takes revisions |
| `usage/unknown-error-id` | 2 | no error goes by that id |
| `usage/unknown-key` | 2 | no fufu setting goes by that name |
| `usage/unknown-slug` | 2 | that is not a slug ff hook knows |
| `usage/unknown-subcommand` | 2 | that family does not have that subcommand |
| `worktree/busy` | 1 | something is running in that worktree |
| `worktree/exists` | 1 | that path is already taken |
| `worktree/is-current` | 1 | that is the worktree you are standing in |
| `worktree/is-main` | 1 | the main worktree is not removable |
| `worktree/not-found` | 1 | no linked worktree by that name |
| `worktree/unborn` | 1 | no commits to check out yet |

<!-- errors:end -->
