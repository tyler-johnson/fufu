# fufu

## Releases

1. Bump `version` in the root `Cargo.toml`, then `cargo update -w --offline`.
2. Close `CHANGELOG.md`'s `## Unreleased` as `## vX.Y.Z — <date>`, using the UTC date the cut commit will carry.
3. Write `.github/release-notes/vX.Y.Z.md`.
4. Commit as `cut vX.Y.Z`, tag `vX.Y.Z` annotated, push `main`, then push the tag.

The tutorial's transcripts follow the release the way install.md's `ff version` block does: when a release changes verb output or moves history, rerun `scripts/docs/tutorial-transcript.sh` and reconcile `docs/tutorial.md`.

The tag push runs `release.yml`: six native builds, a GitHub release whose body is the notes file, and a formula bump into the tap. It reads the notes from the tag's tree, so a later edit needs `gh release edit vX.Y.Z --notes-file`. Under `fufu.gitPolicy=strict` the tag push is wrongly refused; prefix it with `GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=fufu.gitPolicy GIT_CONFIG_VALUE_0=observe`.

No code changes ride a release commit, so trust CI on the base rather than running the suite locally.

### CHANGELOG.md

Terse bullets under Keep a Changelog headings — Added, Changed, Removed, Fixed, Known issues. Each bullet names the surface it affects. A removed setting and a shipped regression each get their own entry.

### Release notes

Two or three sentences saying what the release adds, then plain headings and short prose. No comparison to the previous release, no argument for each change, no diffstats. Minor items go in a `## Miscellaneous` bullet list. Shorter is better.
