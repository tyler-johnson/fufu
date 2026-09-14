# ff clone

Copy a repository and check out its default branch with fufu snapshots enabled. The destination defaults to the URL's last path segment with `.git` removed. The remote is named origin unless `-o` supplies another name.

## Usage

```
Usage: ff clone [OPTIONS] <url> [dir]
```

## Examples

Replace URL with the repository's SSH, HTTPS, or local URL.

```sh
ff clone URL                    # Use the default directory and branch
ff clone URL project            # Choose the directory
ff clone URL -b release          # Check out release
ff clone URL --depth 1           # Download shallow history
ff init                         # Enable fufu in an existing checkout
```

## Options

```
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
          Fetch now on commands that support fetching, regardless of cadence

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Destination and history

A nonempty destination directory is refused. `--depth <n>` limits downloaded history; fufu operations work within that available history. On a fetch or checkout failure, the backend attempts to remove a destination verified empty or absent before cloning. Cleanup can fail; inspect remaining files before retrying. Later fufu initialization failures can leave the completed checkout in place.

The clone establishes the operation log's earliest recovery point and the garbage-collection guard. Shell and agent hooks are separate machine setup through [`ff hook`](hook.md).

## Transport and credentials

Clone uses fufu's native Git transport, not `git clone`. It reads Git's `url.<base>.insteadOf` and `credential.helper` configuration, calls credential helpers when needed, and uses SSH for SSH URLs. Its native HTTP backend does not honor `http.proxy`. Use [`ff git clone`](git.md) for Git's transport when that proxy setting is required, then [`ff init`](init.md) in the checkout.
