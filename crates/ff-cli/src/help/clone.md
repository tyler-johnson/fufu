Copy a repository and check out its default branch with fufu snapshots enabled. The destination defaults to the URL's last path segment with `.git` removed. The remote is named origin unless `-o` supplies another name.

## Examples

Replace URL with the repository's SSH, HTTPS, or local URL.

```sh
ff clone URL                    # Use the default directory and branch
ff clone URL project            # Choose the directory
ff clone URL -b release          # Check out release
ff clone URL --depth 1           # Download shallow history
ff init                         # Enable fufu in an existing checkout
```

### Options

### Destination and history

A nonempty destination directory is refused. `--depth <n>` limits downloaded history; fufu operations work within that available history. On a fetch or checkout failure, the backend attempts to remove a destination verified empty or absent before cloning. Cleanup can fail; inspect remaining files before retrying. Later fufu initialization failures can leave the completed checkout in place.

The clone establishes the operation log's earliest recovery point and the garbage-collection guard. Shell and agent hooks are separate machine setup through `ff hook`.

### Transport and credentials

Clone uses fufu's native Git transport, not `git clone`. It reads Git's `url.<base>.insteadOf` and `credential.helper` configuration, calls credential helpers when needed, and uses SSH for SSH URLs. Its native HTTP backend does not honor `http.proxy`. Use `ff git clone` for Git's transport when that proxy setting is required, then `ff init` in the checkout.
