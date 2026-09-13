# ff clone

Clones a repository and arms it on arrival: the gc guard written, the operation log's floor taken, and one line saying what landed.

fufu speaks the git protocol itself here rather than running `git clone` — it negotiates the pack, writes it, and checks out the worktree. It reads git's installation config for `url.<base>.insteadOf` rewrites and `credential.helper`, invokes credential helpers when needed, and uses `ssh` for an ssh URL. The native HTTP backend does not honor `http.proxy`. [`ff git clone`](git.md) uses Git's transport when proxy support is required.

## What lands on disk

- The directory is the URL's last path segment with .git stripped, unless you name one. An existing directory with anything in it is refused rather than merged into.
- --depth takes only the last N commits. A shallow clone is a smaller download and a shorter history; fufu's own operations work the same way on one.
- Ctrl-C leaves nothing behind: a clone that does not finish takes its half-built directory with it.

## Usage

```
Usage: ff clone [OPTIONS] <url> [dir]

Arguments:
  <url>
          The repository to clone from

  [dir]
          Where to put it; the URL's last path segment when omitted

Options:
  -b, --branch <name>
          Check out this branch instead of the remote's HEAD

      --depth <n>
          Shallow: only the last <n> commits

  -o, --origin <name>
          Name for the remote
          
          [default: origin]

      --json
          Emit machine-readable JSON

      --fetch
          Fetch from the remote first, whatever the cadence says

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff clone git@github.com:you/thing.git
ff clone https://github.com/you/thing.git thing
ff clone <url> -b release        check out a branch, not the remote HEAD
ff clone <url> --depth 1         just the tip
ff init                          already have the repository? adopt it
```
