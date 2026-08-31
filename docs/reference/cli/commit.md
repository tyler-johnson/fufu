# ff commit

There is no staging step: the working tree is the change, and closing it is the commit. -m describes what is closing and wins over any pending description left by `ff describe`. -b lands the close on a branch — it claims the anonymous branch you are standing on, or forks a fresh one from here, leaving the branch you were on where it was.

Paths close a slice: a file or a directory — the same rule `ff restore` and `ff diff` speak, no globs — and what lies under it lands while the rest stays open, still the change you are in the middle of. That is selection at the moment of the close, not a staging area that persists. There is still nothing between commits: the choice is an argument at the moment of the close, made once rather than maintained. The remainder is left without a description, and `ff describe -m` gives it one.

Signing follows git's configuration: `commit.gpgsign` and `gpg.format`, with the key from `user.signingkey`, in all three formats git signs in — openpgp, x509 and ssh. -S signs a repository that does not, --no-sign declines to sign one that does. Both are plain switches: `ff commit` takes positional paths, so git's `-S<keyid>` would make `ff commit -S file.txt` ambiguous, and the key always comes from `user.signingkey`. With signing on, the `@` row shows no predicted sha — a signature is not something a render can know without running the signer.

A clean tree has nothing to close either way: a description does not make one — it waits for the next close instead. Every close is recorded, so `ff undo` takes it back — tree and refs together.

## Usage

```
Usage: ff commit [OPTIONS] [path]...

Arguments:
  [path]...
          Files or directories to close, leaving the rest open; all of it when omitted

Options:
  -m <msg>
          Describe what is closing; wins over the pending description

      --no-verify
          Skip pre-commit and commit-msg hooks

  -b <branch>
          Branch to land the close on: claim an anonymous one, or fork here

      --json
          Emit machine-readable JSON

  -S, --sign
          Sign the commit, whatever commit.gpgsign says; the key is user.signingkey

      --no-sign
          Do not sign the commit, whatever commit.gpgsign says

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

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
