Names the one command that updates this copy of fufu, and offers to run it. fufu never writes an `ff` binary itself: whatever placed one owns replacing it.

So it works out which of four channels this copy came through, and answers accordingly:

- a source build gets `cargo install`
- a Homebrew binary gets `brew upgrade fufu`
- a binary mise or nix or a hand copy placed gets the releases page
- a binary sitting where the install script puts it gets the `curl … | sh` line

That last one is the only channel ff acts on. It checks the latest release, prints the command, and runs it after `-y` or a typed yes. Without a terminal to ask, printing the command is the whole answer. `-y` on any other channel is an error rather than a silent no-op.

Official builds also look for new releases without being asked. A check runs at most once per fufu.updateCheck (daily by default) and lands a one-line notice on stderr, naming the same command this verb would. Nothing installs itself, and a release is announced at most once, ever.

--check is that background lane: it refreshes the cache and prints nothing.

## Examples

```
ff update                      what updates this fufu, and offer to run it
ff update -y                   run it without asking
ff config updateCheck false    turn the background check off
```
