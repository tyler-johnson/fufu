Clones a repository and arms it on arrival: the gc guard written, the operation log's floor taken, and one line saying what landed.

fufu speaks the git protocol itself here rather than running `git clone` — it negotiates the pack, writes it, and checks out the worktree. It reads git's installation config for `url.<base>.insteadOf` rewrites and `credential.helper`, invokes credential helpers when needed, and uses `ssh` for an ssh URL. The native HTTP backend does not honor `http.proxy`. `ff git clone` uses Git's transport when proxy support is required.

### What lands on disk

- The directory is the URL's last path segment with .git stripped, unless you name one. An existing directory with anything in it is refused rather than merged into.
- --depth takes only the last N commits. A shallow clone is a smaller download and a shorter history; fufu's own operations work the same way on one.
- Ctrl-C leaves nothing behind: a clone that does not finish takes its half-built directory with it.

## Examples

```
ff clone git@github.com:you/thing.git
ff clone https://github.com/you/thing.git thing
ff clone <url> -b release        check out a branch, not the remote HEAD
ff clone <url> --depth 1         just the tip
ff init                          already have the repository? adopt it
```
