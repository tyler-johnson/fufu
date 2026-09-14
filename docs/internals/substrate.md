# Substrate

fufu implements local repository operations in Rust using [gitoxide](https://github.com/GitoxideLabs/gitoxide) (`gix`, pinned by `Cargo.lock`). Network transport, authentication, signing, hooks, and maintenance can invoke external programs. This page maps that boundary for contributors; [installation](../install.md) gives the setup requirements.

## Reads are native

Core ref, object, index, status, log, and revision-set reads run in-process. Replay predictions use in-memory merge objects. Avoiding a Git subprocess for each read reduces repeated command overhead, but makes no fixed timing promise; see the [measured performance scope](../performance.md).

A pure core read is not the entire CLI invocation. Its preflight can capture, reconcile, fetch, or run maintenance, and its renderer can invoke a pager or signature verifier. [Architecture](architecture.md) separates those layers.

## The execution ladder, as it stands

| Work | Implementation and external requirements |
| --- | --- |
| Local snapshot, commit, index rebuild, checkout, branch update, replay, undo | Native core code; configured commit hooks and signing can still run programs. |
| [`ff clone`](../reference/cli/clone.md) and [`ff pull`](../reference/cli/pull.md) fetch | Native gix protocol handling with blocking reqwest/rustls for HTTP(S); clone checkout is native too. |
| Network configuration and authentication | May invoke `git config -l` for installation configuration, configured credential helpers, `ssh` for SSH URLs, and `git-upload-pack` for filesystem remotes. Availability depends on the transport and configuration. |
| Fetch with incomplete linked-worktree administration | `net.rs` falls back to `git fetch` for the specific unreadable `commondir`/`gitdir` condition that prevents native fetch. |
| [`ff push`](../reference/cli/push.md) | Invokes `git push` for sends and classifies its result. Named-branch sends can invoke it more than once. |
| Commit hooks | Runs configured executable hooks through gix's command wrapper; details below. |
| Signing and verification | Invokes the configured GPG, X.509, or SSH programs; see [signing](../reference/signing.md). |
| Manual [`ff op trim`](../reference/cli/op-trim.md) | Best-effort `git gc --auto`, even without dropped operations; skipped when Git is unavailable. Automatic trim omits that subprocess. |
| Editor, pager, extensions | Runs the configured editor/pager or `ff-<name>` executable. |
| [`ff git`](../reference/cli/git.md) | Runs Git itself with the supplied arguments after policy checks and a capture attempt. |
| Passive update check | Eligible official builds can start a short-lived detached copy of fufu; see [No daemon](#no-daemon). |

Native HTTP fetch does **not** honor `http.proxy`. Push and Git passthrough use Git's transport. [Network configuration](../reference/config.md#what-fufu-reads-from-gits-config) gives the proxy-dependent fetch procedure. “Native fetch” describes protocol handling, not a promise of zero child processes or support for every Git setting.

`crates/ff-cli/tests/zero_spawn.rs` checks selected local operations with a trap Git executable on PATH. It tests the configured local path, not every possible helper, integration, or maintenance path.

## Differential testing is the compatibility contract

`crates/ff-testsupport` runs Git and fufu against comparable fixtures and normalizes their results. The differential suites in `crates/ff-core/tests/` cover status, snapshots, closing commits, switching, pulling, restacking, undo, index writing, signing, and revision queries.

The index tests compare semantics rather than serialized bytes: after fufu writes an index, Git must see the intended staged tree and accept the index for its next operation. Other fixtures cover unborn and detached HEADs, staged-only edits, renames, and conflicts. These tests establish behavior for the cases they exercise; they do not establish LFS or submodule support.

## Behavioral compatibility

`crates/ff-core/src/hooks.rs` resolves hooks through `core.hooksPath` (otherwise `<common-dir>/hooks`), runs them from the worktree root, and sets `GIT_EDITOR=:`. Missing hooks are skipped; Unix also requires the executable bit. Message hooks receive a temporary `COMMIT_EDITMSG.fufu-<pid>` path, not Git's fixed message filename.

The [FAQ hook table](../faq.md#does-fufu-run-my-git-hooks) owns the per-command and per-mode rules, including absorb/lift's open versus closed sources, `-m`, and `--no-verify`. A nonzero gate result aborts the operation; `post-commit` is notification-only and does not fail the completed commit.

### What the hooks see

For operations that run a pre-commit hook, `hooks::Window` stages the proposed selected content so hook runners can inspect it with Git. Dropping an unlanded window restores the prior index bytes. File changes made by a hook are separate and can remain after a refusal.

After a successful pre-commit hook, the caller re-scans working files and includes formatter edits within the selected paths. A hook therefore need not re-stage those edits for fufu to include them. This differs from a Git workflow where the index determines the committed tree. Hook runners that depend on other Git behaviors need their own integration checks.

## No daemon

fufu requires no resident daemon. Commands compute on invocation and use disk caches between invocations. Eligible official builds can launch a detached [`ff update --check`](../reference/cli/update.md) process on the configured cadence; it refreshes the user cache and exits. `fufu.updateCheck=false` disables that mechanism. [`ff watch`](../reference/cli/watch.md) is a foreground stream started explicitly. See [update settings](../reference/config.md#updatecheck).

## The git-free destination

A fully Git-free installation was a goal in the founding design, not the current dependency contract. Install Git for push, passthrough, filesystem remotes, and the fallback fetch path. Local core operations can run without it; credentials, signing, hooks, and editors may need their own programs.

LFS-dependent repositories are unsupported, and submodule workflows are untested. Native checkout/filter compatibility should be established with specific fixtures before claiming additional ecosystem support. [Project](../project.md#what-fufu-needs-from-git) states the current release and dependency policy.
