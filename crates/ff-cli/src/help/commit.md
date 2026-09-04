There is no staging step: the working tree is the change, and closing it is the commit. -m describes what is closing and wins over any pending description left by `ff describe`. `ff ci` is the short spelling.

-b lands the close on a branch — it claims the anonymous branch you are standing on, or forks a fresh one from here, leaving the branch you were on where it was.

A clean tree has nothing to close, and a description does not make one — it waits for the next close instead. Every close is recorded, so `ff undo` takes it back, tree and refs together.

### Closing part of the tree

Paths close a slice: a file or a directory — the same rule `ff restore` and `ff diff` speak, no globs — and what lies under it lands while the rest stays open, still the change you are in the middle of. The remainder is left without a description, and `ff describe -m` gives it one.

Selection is by path and made once at the close; there is no hunk-level pick. When one file holds two changes, `ff git commit -p` builds that commit with git's own `-p`, capture-first, and refused under `fufu.gitPolicy strict` like every git commit.

### Signing

Signing follows git's configuration: `commit.gpgsign` and `gpg.format`, with the key from `user.signingkey`, in all three formats git signs in — openpgp, x509 and ssh. -S signs a repository that does not, --no-sign declines to sign one that does.

Both are plain switches, and the key always comes from `user.signingkey`. With signing on, the `@` row shows no predicted sha.

## Examples

```
ff commit -m "parser: handle unicode escapes"
ff commit                      close with the pending description
ff commit -b unicode-cleanup   claim the name as the work lands
ff commit --no-verify          skip pre-commit and commit-msg hooks
ff commit -S -m "signed"       sign it, whatever commit.gpgsign says
ff commit --no-sign -m "quick" do not sign it, whatever commit.gpgsign says
ff commit src/parser.rs -m "one fix"  land one file, leave the rest open
ff commit src/ -m "one fix"           a directory prefix works the same way
```
