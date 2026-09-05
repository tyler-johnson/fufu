<div align="center">

# fufu

**git that flies itself**

*Version control for humans and agents: automatic snapshots,<br>
effortless branching, whole-repo undo. And it's built on ordinary git,<br>
so your tools and your remotes all still work.*

[![ci](https://github.com/tyler-johnson/fufu/actions/workflows/ci.yml/badge.svg)](https://github.com/tyler-johnson/fufu/actions/workflows/ci.yml)

<img src="docs/assets/demo.gif" alt="A terminal running ff: a glance at the branches, changes on main parked by a single switch, a commit, a fix folded into it, then sync and publish." width="900">

</div>

---

fufu is version control done the right way:

- **Commits, all the way down.** Your working copy is an open commit. There is nothing to stage, nothing to stash, and nothing to track. When you are done making changes, close the current commit and start on the next.
- **Move HEAD, without friction.** The working copy stays with the branch. Switch, and the open commit goes with it. When you return, everything is right where it should be. Step back onto any commit and edit it; the commits above it reflow on their own.
- **Undo anything.** Every operation is recorded, which makes everything undoable. Git has the reflog, and this is a whole new level — mid-commit file edits, a bad merge on top of changes, a hard git reset. Building with version control becomes _forgiving and carefree_, as it should be.
- **First-class agent support.** Native MCP, leveraged skills, and built-in nudging. With minimal configuration, agents instinctively reach for fufu over git. Plus, a snapshot lands before every agent tool call, letting a sloppy agent reverse its bad decisions.
- **It's still git.** Real commits, real branches, an ordinary repository that every tool and teammate reads as one. Worktrees, remotes, hooks, and the rest of git are all still there. And it stays quick no matter how much history piles up.

## Documentation

The documentation lives at **[tyler-johnson.github.io/fufu](https://tyler-johnson.github.io/fufu/)**.

- [Tutorial](https://tyler-johnson.github.io/fufu/tutorial/) — the whole loop once, every transcript from a real run.
- [Adopting fufu](https://tyler-johnson.github.io/fufu/adopting/) — `ff init` in a repository git made, what changes, and how to leave.
- [Concepts](https://tyler-johnson.github.io/fufu/concepts/invariant/) — the invariant, the two regimes, changes, snapshots and undo, branches, the push boundary, held rewrites.
- [Guides](https://tyler-johnson.github.io/fufu/guides/recovery/) — recovery first, then rewriting history, stacked changes, plain-git teammates, worktrees.
- [Command table](https://tyler-johnson.github.io/fufu/comparisons/command-table/) — what you'd type in git, and what it is in fufu.
- [CLI reference](https://tyler-johnson.github.io/fufu/reference/cli/) — generated from the help pages, and `ff <verb> --help` says the same thing offline.
- [Performance](https://tyler-johnson.github.io/fufu/performance/) — the snapshot chain costs the same at ten thousand deep as at a hundred, and the gate that keeps it that way.
- [FAQ](https://tyler-johnson.github.io/fufu/faq/)

## Install

**1. Get the binary.**

Linux/macOS:

```sh
curl -fsSL https://raw.githubusercontent.com/tyler-johnson/fufu/main/install.sh | sh
```

Windows (PowerShell):

```powershell
irm https://raw.githubusercontent.com/tyler-johnson/fufu/main/install.ps1 | iex
```

Homebrew:

```sh
brew install tyler-johnson/tap/fufu
```

**2. Wire it into the shells and agent clients on this machine.** Optional, and recommended.

```sh
ff hook
```

fufu captures only when something invokes it, so without hooks only an `ff` command captures. With them a snapshot lands before every agent tool call, every git command you type, and every shell prompt.

Then `ff clone <url>` gets a repository, and `ff init` inside one you already have turns fufu on there. The [install page](https://tyler-johnson.github.io/fufu/install/) has the details, and `ff doctor` verifies a finished setup.

## The repository

- `crates/ff-core` — the engine: capture, the operation log, rewrite and replay, native git on [gitoxide](https://github.com/GitoxideLabs/gitoxide).
- `crates/ff-cli` — the `ff` binary: the verbs, the help pages, the JSON surface. The CLI reference and the config registry in the docs generate from this crate, enforced by tests.
- `crates/ff-testsupport` — the differential harness: fufu's behavior is tested against the git binary as a permanent compatibility contract.
- `docs/` — the MkDocs site. Console transcripts in the tutorial and guides come from the scripts in `scripts/docs/`, run against the real binary.
- `DESIGN.md` — the founding design document, included verbatim in the docs; the concepts section is where its material is rewritten as description.

## License

[MIT](LICENSE)
