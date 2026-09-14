# fufu

## Releases

1. Bump `version` in the root `Cargo.toml`, then `cargo update -w --offline`.
2. Close `CHANGELOG.md`'s `## Unreleased` as `## vX.Y.Z — <date>`, using the UTC date the cut commit will carry.
3. Write `.github/release-notes/vX.Y.Z.md`.
4. Commit as `cut vX.Y.Z`, tag `vX.Y.Z` annotated, push `main`, then push the tag.
5. Once `release.yml` is green, update `docs/install.md`: the `ff version` block shows `fufu X.Y.Z (<cut commit short hash> <date>)` and the checksum block names `ff_X.Y.Z_linux_amd64.tar.gz`. Commit as `docs: update install examples for vX.Y.Z`.

The tutorial's transcripts follow the release the way install.md does: when a release changes verb output or moves history, rerun `scripts/docs/tutorial-transcript.sh` and reconcile `docs/tutorial.md`.

The recordings follow it too, and CI says when: `make demo-check` replays the demo's commands against `scripts/docs/demo.golden.txt` and every tutorial step for its exit status. When it fails, `make demo` re-records the casts and gifs, `docs/assets/demo.*` and the six under `docs/assets/tutorial/` — then `scripts/docs/demo-check.sh --bless`. Rendering needs agg and JetBrains Mono. The demo's commands live in `scripts/docs/demo-steps.sh`; the tutorial's clips and its transcripts come from one file, `scripts/docs/tutorial-steps.sh`, so a change to the tutorial's commands is a change there.

The tag push runs `release.yml`: six native builds, a GitHub release whose body is the notes file, and a formula bump into the tap. It reads the notes from the tag's tree, so a later edit needs `gh release edit vX.Y.Z --notes-file`. Under `fufu.gitPolicy=strict` the tag push is wrongly refused; prefix it with `GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=fufu.gitPolicy GIT_CONFIG_VALUE_0=observe`.

No code changes ride a release commit, so trust CI on the base rather than running the suite locally.

### CHANGELOG.md

Terse bullets under Keep a Changelog headings — Added, Changed, Removed, Fixed, Known issues. Each bullet names the surface it affects. A removed setting and a shipped regression each get their own entry.

Edit current/unreleased prose against observed behavior, including defaults, capture and network effects, and recovery limits. Keep historical release entries and tagged release notes in their release's terms; do not update old commands or claims to sound current. Label proposed behavior explicitly and keep it separate from shipped changes.

### Release notes

Two or three sentences saying what the release adds, then plain headings and short prose. No comparison to the previous release, no argument for each change, no diffstats. Minor items go in a `## Miscellaneous` bullet list. Shorter is better.

State capabilities and limits for the version being released. Link current usage to its owning concept or reference page. Performance claims require an actual recorded run with fixture, version, machine, and comparison direction; a benchmark plan is not evidence. Keep generator-maintenance instructions in comments or contributor guidance, not reader introductions.

## Docs

A verb's first mention on a page links to its page under `docs/reference/cli/`, and later mentions stay bare. Headings, fenced blocks, and the generated regions of config.md and errors.md are left alone. The pages under `docs/reference/cli/` get the same links from the generator in `crates/ff-cli/src/docsgen.rs`, so edit the help page under `crates/ff-cli/src/help/` and run `make docs-gen`.
