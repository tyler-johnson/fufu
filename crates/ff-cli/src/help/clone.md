Clones a repository and arms it on arrival: the gc guard written, the operation log's floor taken, and one line saying what landed.

fufu speaks the git protocol itself here rather than running `git clone` — it negotiates the pack, writes it, and checks out the worktree. What it still reaches outside the process for is git's configuration and authentication surface: the installation config (so `url.<base>.insteadOf` and `http.proxy` keep working), your credential helper when a remote asks for one, and `ssh` for an ssh URL. Those are inherited whole rather than reimplemented.

Ctrl-C leaves nothing behind: a clone that does not finish takes its half-built directory with it.

--depth takes only the last N commits. A shallow clone is a smaller download and a shorter history; fufu's own operations work the same way on one.

The directory is the URL's last path segment with .git stripped, unless you name one. An existing directory with anything in it is refused rather than merged into.

## Examples

```
ff clone git@github.com:you/thing.git
ff clone https://github.com/you/thing.git thing
ff clone <url> -b release        check out a branch, not the remote HEAD
ff clone <url> --depth 1         just the tip
ff init                          already have the repository? adopt it
```
