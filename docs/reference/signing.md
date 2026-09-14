# Commit signing

Fufu uses your existing Git signing configuration: `commit.gpgsign`, `gpg.format`, `user.signingkey`, and the format's signing program. If Git already signs commits in this repository, [`ff commit`](cli/commit.md) uses the same setup. There is no separate `fufu.*` signing setting.

## Turning it on

For an existing SSH key, configure signing in this repository:

```sh
git config commit.gpgsign true
git config gpg.format ssh
git config user.signingkey "$HOME/.ssh/id_ed25519.pub"
```

The private key must be accessible to `ssh-keygen`, either beside the public key or through your SSH agent. OpenPGP and X.509 configuration is [below](#the-three-formats).

Creating a signature and trusting it are separate steps. For SSH verification, set `gpg.ssh.allowedSignersFile` to a file containing the principals and public keys you trust. For example, add a line with your email principal followed by the contents of your `.pub` file, then configure that file's path:

```text
ada@example.com ssh-ed25519 AAAA...actual-public-key...
```

```sh
git config gpg.ssh.allowedSignersFile "$HOME/.ssh/allowed_signers"
```

Use the complete real public key, not the placeholder. Preserve any existing allowed-signers entries. With an eligible file change ready to record, test both creation and verification:

```sh
ff commit -m "Record signed work"
ff show HEAD
git verify-commit HEAD
```

[`ff show`](cli/show.md) should report `signature: verified` when the verifier accepts the signature under your trust configuration. `git verify-commit` independently checks the same Git commit object.

[`ff doctor --no-fetch`](cli/doctor.md) checks signing configuration without running the signer. Its `ok` does not establish access to the private key or validate the contents or existence of the allowed-signers file. The command's [capture, fetch, and maintenance effects](doctor.md#capture-fetch-and-maintenance) still apply.

## Per-invocation overrides

```sh
ff commit -S -m "Signed work"       # Sign even when config disables it
ff commit --no-sign -m "Local work" # Skip signing for this commit
```

`-S`/`--sign` and `--no-sign` are switches. `-S` takes no key argument; select the key through Git configuration. Rewrite verbs use configuration without a signing flag. For a one-command override in a POSIX shell:

```sh
GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=commit.gpgsign GIT_CONFIG_VALUE_0=false ff restack
```

This disables signatures on commits newly written by that [`ff restack`](cli/restack.md) invocation; it does not remove signatures from untouched objects.

## Reading signatures back

- `ff show <rev>` verifies the shown commit and prints a signature verdict when a signature is present.
- [`ff log`](cli/log.md) and [`ff status`](cli/status.md) mark signed commits `signed` by inspecting their headers. That means a signature is present, not that it has been verified.
- `ff log --signatures` invokes verification and replaces the `signed` mark with a verdict. Status has no verification flag; use `ff show HEAD` to verify its parent commit.

```sh
ff log --signatures -n 5
ff show HEAD
ff show HEAD --json
```

### What the marks print

| Mark | JSON `code` / Git `%G?` | Meaning |
| --- | --- | --- |
| `verified` | `G` | Signature verifies under the configured trust policy |
| `bad signature` | `B` | Signature verification failed |
| `untrusted key` | `U` | Signature is valid but below `gpg.minTrustLevel` |
| `expired signature` | `X` | Signature is valid but expired |
| `expired key` | `Y` | Signature is valid but its key is expired |
| `revoked key` | `R` | Signature is valid but its key is revoked |
| `unverifiable` | `E` | Could not check it, for example because a key, allowed-signers entry, or verifier is unavailable |

Unsigned commits have no human signature mark. `ff log --json` includes `signed` on each commit, and adds a `signature` object only with `--signatures`. `ff show --json` includes its verification result in `signature`. See [JSON output and scripting](../agents/machine-surface.md) for interpreting reports.

## The three formats

| `gpg.format` | Default program | Program override |
| --- | --- | --- |
| `openpgp` (default) | `gpg` | `gpg.openpgp.program`, then `gpg.program` |
| `x509` | `gpgsm` | `gpg.x509.program` |
| `ssh` | `ssh-keygen` | `gpg.ssh.program` |

`user.signingkey` selects the key. OpenPGP and X.509 can use the signing program's default key when it is unset. SSH requires a key path, a literal public key backed by an agent, or a key returned by `gpg.ssh.defaultKeyCommand`. That command is run during signer resolution, not by doctor's signing check.

Verification also reads `gpg.ssh.allowedSignersFile`, `gpg.ssh.revocationFile`, and `gpg.minTrustLevel`. SSH signing can succeed without an allowed-signers file; verification cannot. An SSH principal comes from the allowed-signers file and need not match the commit author's email.

## What gets signed

When signing is enabled, fufu signs commits it writes into user history:

- `ff commit` records the open change as a signed commit.
- Rewrites sign newly written commits: [`ff describe`](cli/describe.md), [`ff absorb`](cli/absorb.md), [`ff lift`](cli/lift.md), `ff restack`, [`ff pull`](cli/pull.md), [`ff done`](cli/done.md), and [`ff resolve`](cli/resolve.md) use the same configuration when replay writes a commit.

A rewrite can preserve another person's author identity while signing the new object with your configured key. New changes use the configured Git committer identity for both author and committer; [identity configuration](config.md#what-fufu-reads-from-gits-config) explains the environment overrides. The author field, committer field, and signer identity answer different questions; a signature does not establish that the signer originally authored the change. Existing signatures are removed from replayed objects, and signing configuration determines whether a new signature is added.

Internal open-change objects, including [parked changes](../concepts/changes.md#parking-and-resuming), are unsigned. Operation-journal objects are also unsigned and use `fufu <fufu@local>` as their Git author and committer. These internal objects are distinct from commits recorded in branch history.

<a id="the-open-commits-sha"></a>
## The open change's commit hash

Fufu stores the open change as an unsigned internal commit under `refs/fufu/open/<branch>`. Without signing, recording the change can reuse that object when the other [commit conditions](cli/commit.md) allow it. A signature changes the object's bytes and therefore its hash.

With `commit.gpgsign` enabled, `ff log` and `ff status` leave the open `@` row's SHA column blank even though the internal object exists. The commit output says `re-minted: signing is on` when signing creates the final object. Use the recorded commit's SHA afterward; see [commit SHAs and change IDs](revisions.md#commit-shas-and-change-ids).

## Signing and verification processes

Fufu writes Git's `gpgsig` header after invoking the configured signing program. Signing is serial: each newly signed commit requires a signer invocation, and a passphrase-protected key can prompt repeatedly unless its agent caches access.

Verification can run concurrently for independent commits, with up to one worker per available core, capped at eight. OpenPGP/X.509 verification uses the configured program; SSH verification normally uses two `ssh-keygen` calls per signed commit to find a principal and check the signature. Unsigned commits skip verification. Runtime depends on the program, key setup, and number of signatures checked.

## Failure modes

| Error | What to check |
| --- | --- |
| `sign/unknown-format` | Set `gpg.format` to `openpgp`, `x509`, or `ssh` |
| `sign/no-key` | Configure an SSH signing key or a working `gpg.ssh.defaultKeyCommand` |
| `sign/no-program` | Install or correct the configured signer executable |
| `sign/failed` | Read the signer's diagnostic; check private-key access, agent, and passphrase setup |

A failed signer during `ff commit` leaves the change open and does not advance the branch. Pre-command capture or reconciliation may already have written objects and operation records; a signing refusal is not a promise that the entire invocation wrote nothing. Rewrites may also have made earlier progress before a later signing failure. Correct the setup, inspect status, and retry the intended operation.

[`ff explain <id>`](cli/explain.md) gives detailed repair advice; the [error reference](errors.md) lists IDs and exit codes. Signer stderr is included in the error. GPG pinentry can still open your terminal through `GPG_TTY`, so ensure the signing environment can obtain the passphrase.
