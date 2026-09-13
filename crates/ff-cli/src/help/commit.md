Record eligible working-copy edits in branch history, without staging. By default, all changed files are included. `-m` supplies the message and overrides a pending description set by `ff describe`. `ff ci` is the short spelling.

## Examples

```sh
ff commit -m "parser: handle unicode escapes"
ff commit                       # Use the pending description
ff commit src/parser.rs -m "parser: fix escapes"
ff commit src/ -m "parser: cleanup"  # Leave other paths uncommitted
ff commit -b unicode-cleanup -m "parser: fix escapes"
ff commit -S -m "signed change"
ff commit --no-sign -m "unsigned change"
ff commit --no-verify            # Skip pre-commit and commit-msg
```

### Options

### Messages, paths, and branches

A clean tree has nothing to commit; a pending description waits for later edits. The open change is your current uncommitted work. Committing closes it and records the result on the branch. One `ff undo` takes the commit back, including local refs and files.

Paths select files or directory prefixes, without globs. Unselected edits remain open, without a pending description; use `ff describe -m` to give the remainder a message. Selection is by path, not hunk. For Git's interactive staging, `ff git commit -p` uses Git's interface and is refused under strict Git policy.

`-b` renames an automatically named branch, or creates a new branch here when the current branch already has a chosen name. The commit is recorded on that branch.

Ignored untracked files and content above `fufu.maxFileSize` are excluded by snapshot rules. Review `ff diff` and any capture warnings before committing.

### Hooks and signing

Commit runs the configured commit hooks. `--no-verify` skips `pre-commit` and `commit-msg`. Signing follows `commit.gpgsign`, `gpg.format`, and `user.signingkey`, supporting OpenPGP, X.509, and SSH. `-S` enables signing for this commit; `--no-sign` disables it. Both use the configured key.

### Internal objects and timestamps

The open change already has an internal Git commit object under `refs/fufu/open/<branch>`. Its SHA is not proof that it has entered branch history. A full commit can reuse that object when it advances the branch. Signing, partial selection, or a hook changing the tree or message requires a new object and a `re-minted:` report. A different `-m` message also creates a new object, without that report.

The author time is when the change began; the committer time is when it closed. With signing enabled, the open `@` row hides its SHA because the unsigned open object cannot be the final signed commit.
