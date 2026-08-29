<div align="center">

# fufu

**git that flies itself**

*Version control for humans and agents: automatic snapshots,<br>
effortless branching, whole-repo undo. And it's built on ordinary git,<br>
so your tools and your remotes all still work.*

</div>

---

fufu (`ff`) is a version control interface built on ordinary git. It snapshots the working tree before every action, parks work automatically when you switch branches, folds fixes into the commits they belong to, and makes every operation undoable in one keystroke — including operations made behind its back with raw git.

Its one non-negotiable promise: at every instant, the repository is a boring git repository. fufu never creates a state plain git cannot represent; it only automates the transitions between such states. Teammates, CI, IDEs, and forges see nothing unusual, and deleting fufu loses convenience, never data or history. That promise — [the invariant](https://tyler-johnson.github.io/fufu/concepts/invariant/) — settles every design question in the tool.

fufu takes jj's working model — the tree as the change, no staging area, first-class undo — and rebuilds it as a layer over an unmodified git repository, abandonable and returnable at any moment. [fufu vs jj](https://tyler-johnson.github.io/fufu/comparisons/vs-jj/) is the thesis, and [related work](https://tyler-johnson.github.io/fufu/comparisons/related-work/) places the neighbors.

It is built for repositories agents work in as much as humans. An agent with shell access and git is one confident `reset --hard` from destroying an afternoon; under fufu the tree is snapshotted before every tool action, and the human reviews and reverses the lot with `ff history` and `ff undo`. [Why agents want fufu](https://tyler-johnson.github.io/fufu/agents/why/) is the argument, [setup](https://tyler-johnson.github.io/fufu/agents/setup/) is the wiring, and [the machine surface](https://tyler-johnson.github.io/fufu/agents/machine-surface/) is the JSON contract.

## Documentation

The documentation lives at **[tyler-johnson.github.io/fufu](https://tyler-johnson.github.io/fufu/)**.

- [Tutorial](https://tyler-johnson.github.io/fufu/tutorial/) — the whole loop once, every transcript from a real run.
- [Adopting fufu](https://tyler-johnson.github.io/fufu/adopting/) — `ff init` in a repository git made, what changes, and how to leave.
- [Concepts](https://tyler-johnson.github.io/fufu/concepts/invariant/) — the invariant, the two regimes, changes, snapshots and undo, branches, the push boundary, held rewrites.
- [Guides](https://tyler-johnson.github.io/fufu/guides/recovery/) — recovery first, then rewriting history, stacked changes, plain-git teammates, worktrees.
- [Command table](https://tyler-johnson.github.io/fufu/comparisons/command-table/) — what you'd type in git, and what it is in fufu.
- [CLI reference](https://tyler-johnson.github.io/fufu/reference/cli/) — generated from the help pages, and `ff <verb> --help` says the same thing offline.
- [FAQ](https://tyler-johnson.github.io/fufu/faq/)

## Install

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

Then `ff clone <url>` gets a repository, and `ff init` inside one you already have turns fufu on there. The [install page](https://tyler-johnson.github.io/fufu/install/) has the details, and `ff doctor` verifies a finished setup.

## The repository

- `crates/ff-core` — the engine: capture, the operation log, rewrite and replay, native git on [gitoxide](https://github.com/GitoxideLabs/gitoxide).
- `crates/ff-cli` — the `ff` binary: the verbs, the help pages, the JSON surface. The CLI reference and the config registry in the docs generate from this crate, enforced by tests.
- `crates/ff-testsupport` — the differential harness: fufu's behavior is tested against the git binary as a permanent compatibility contract.
- `docs/` — the MkDocs site. Console transcripts in the tutorial and guides come from the scripts in `scripts/docs/`, run against the real binary.
- `DESIGN.md` — the founding design document, included verbatim in the docs; the concepts section is where its material is rewritten as description.

## License

[MIT](LICENSE)
