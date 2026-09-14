# Project

fufu is an MIT-licensed, pre-1.0 Git interface. This page covers support, dependencies, and testing.

## License

fufu is [MIT-licensed](https://github.com/tyler-johnson/fufu/blob/main/LICENSE). The license file ships in the repository and inside every release archive.

## Security

Report vulnerabilities privately to the maintainer through [GitHub private vulnerability reporting](https://github.com/tyler-johnson/fufu/security/advisories/new). Please do not file public security issues. [SECURITY.md](https://github.com/tyler-johnson/fufu/blob/main/SECURITY.md) states the support policy.

## Stability and releases

Minor releases may change commands, flags, output, configuration, and error IDs. Read the [changelog](changelog.md) when upgrading. Scripts should pin and test supported binary versions and check the [JSON envelope version](agents/machine-surface.md); the current contract number is not a blanket cross-release payload guarantee.

Only the latest release is supported. Fixes are included in new releases and are not backported. Tagged releases are built in CI for Linux, macOS, and Windows, each on amd64 and arm64, and published with checksums. [Installation](install.md#pin-and-verify) covers pinning and verification.

## What fufu needs from git

Install Git for a complete setup; there is no declared minimum Git version. Local core operations run in-process. Push and the [`ff git`](reference/cli/git.md) passthrough use Git; filesystem remotes require upload-pack, and a narrow native-fetch failure uses a Git fallback. Network configuration, authentication, signing, hooks, and maintenance may also invoke helpers. See the [execution table](internals/substrate.md#the-execution-ladder-as-it-stands) and [installation requirements](install.md).

## How it is tested

The differential harness in `crates/ff-testsupport` compares fufu with Git on the state left by local operations. Suites cover committing, switching, pulling, restacking, recovery, signing, index writing, and revision queries. CLI integration tests cover command behavior, output, hooks, and installers.

For example, after fufu writes an index, Git must see the intended staged content and accept it for subsequent operations. CI runs the Rust test workflow on Linux, macOS, and Windows for code changes, with Windows split across four shards.

Git passthrough, extensions, zero-spawn, and CLI signing suites are Unix-only. Some runtime checks are conditional: the PowerShell profile test runs only where `pwsh` is available. Installer tests do not prove a user's running client has loaded or trusted its hooks. [Platforms](install.md#platforms) lists the released binaries; [performance](performance.md) identifies the benchmark versions and fixture limits.
