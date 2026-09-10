# ff update

Names the one command that updates this copy of fufu, and offers to run it; then the same for every declared extension. fufu never writes an `ff` binary itself: whatever placed one owns replacing it.

So it works out which of four channels this copy came through, and answers accordingly:

- a source build gets `cargo install`
- a Homebrew binary gets `brew upgrade fufu`
- a binary mise or nix or a hand copy placed gets the releases page
- a binary sitting where the install script puts it gets the `curl … | sh` line

That last one is the only channel ff acts on. It checks the latest release, prints the command, and runs it after `-y` or a typed yes. Without a terminal to ask, printing the command is the whole answer. `-y` on any other channel is an error rather than a silent no-op.

Official builds also look for new releases without being asked. A check runs at most once per fufu.updateCheck (daily by default) and lands a one-line notice on stderr, naming the same command this verb would. Nothing installs itself, and a release is announced at most once, ever.

--check is that background lane: it refreshes the cache and prints nothing, for fufu and for every extension alike.

## Declared extensions

After fufu, the same walk over every extension declared with [`ff extension <name>`](extension.md), in the order they were declared, by the rules above. Each manifest may carry an `update` block of recipes keyed by channel — `brew` (the formula), `install` (the script's URL, with `bin` saying where the script places the binary, `~/.local/bin` when it does not say), `releases` (the page) — and a `build`, `official` or `source`. Absent `build` is `official`.

- a `source` build is told to rebuild it the way it was built, and nothing else: no recipe is read and no release is checked
- a binary under a Homebrew prefix gets `brew upgrade <formula>`
- a binary in the directory its install script places it gets the `curl … | sh` line, and that is the one recipe ff runs, after `-y` or a typed yes
- a binary anywhere else gets the releases page

A channel the block has no recipe for, or a manifest with no block at all, is named as one ff cannot move, with the path the binary sits at. No release is checked for an extension in this walk; the recipe is printed as it stands.

The background check covers extensions too. An `official` build whose `releases` recipe is a github.com page names its repository, and the same check that fetches fufu's latest release fetches that one's, at most once per fufu.updateCheck, under the same gates. A binary behind its latest release gets one line on stderr beside fufu's own, naming the recipe for its channel: `ff update` for the install script, the `brew upgrade` line, else the page. Each release is announced once. A `source` build, a manifest with no `releases` recipe, and a page on any other host get no check and no line.

`-y` is one answer for the whole walk: every install recipe runs without asking, and whatever the walk could not move — this fufu on a channel it does not drive, an extension with no recipe for its channel — is named at the end and the exit is 1, so a script that asked for everything is not told it moved. An install that fails is that extension's alone; the walk goes on and the exit says so.

After a move that ran, the walk ends with [`ff hook -u`](hook.md): every declared manifest is re-asked and re-recorded, then every install already wired is re-run, so the skills on disk are the ones the new binary names.

## Usage

```
Usage: ff update [OPTIONS]

Options:
      --check
          Refresh the update cache only (used by the background check)

  -y, --yes
          Run the update command without asking

      --json
          Emit machine-readable JSON

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff update                      what updates this fufu, and offer to run it
ff update -y                   run every install recipe without asking
ff config updateCheck false    turn the background check off
```
