# Project

The facts a first visit checks: the license, the security contact, what a release promises, and how the tool is tested.

## License

fufu is [MIT-licensed](https://github.com/tyler-johnson/fufu/blob/main/LICENSE). The license file ships in the repository and inside every release archive.

## Security

Report vulnerabilities through [GitHub private vulnerability reporting](https://github.com/tyler-johnson/fufu/security/advisories/new): a private channel to the maintainer, with no public issue until a fix ships. Please do not file security reports as public issues. The same policy lives in [SECURITY.md](https://github.com/tyler-johnson/fufu/blob/main/SECURITY.md) in the repository.

## Stability and releases

fufu is pre-1.0: the command surface is settling, and a minor version may change flags, output, or configuration — every breaking change is named in the [changelog](changelog.md), never slipped in silently. The latest release is the supported release; fixes land at the tip rather than being backported. Releases are cut from tags and built in CI for six targets — Linux, macOS, and Windows, each on amd64 and arm64 — and published with checksums; [install](install.md#pin-and-verify) covers pinning and verifying one.

## What fufu needs from git

No declared minimum version. The daily surface — status, commit, switch, undo, log, restore — runs in-process and spawns no git at all; git on PATH is reached for the push, credential helpers, the [`ff git`](reference/cli/git.md) passthrough, and trim's best-effort `gc --auto`, and those calls lean only on long-stable git behavior. The [substrate](internals/substrate.md#the-git-free-destination) page tracks that line as it moves.

## How it is tested

fufu's one non-negotiable promise — the repository stays a boring git repository — is tested differentially: a permanent harness (`crates/ff-testsupport`) runs fufu and the real git binary side by side across 22 differential suites and asserts they agree on what is left on disk, covering the close, switch, sync, stash, signing, the index, the revset grammar, and the rest. The sharpest of those is the index contract: after fufu writes `.git/index`, real git must see exactly the intended content staged and accept the file for its own next operation. CI runs the full suite on Linux, macOS, and Windows for every code change — [platforms](install.md#platforms) says what that covers per OS.
