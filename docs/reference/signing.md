# Commit signing

fufu writes commit objects itself, so signing them is fufu's job too — gitoxide implements none. What that job is, though, is entirely git's: fufu reads git's configuration keys, runs git's signing programs, produces git's `gpgsig` header, and its signatures verify under `git verify-commit` and `git log --show-signature`. There is no `fufu.*` setting here. A repository that already signs under git signs under fufu without being told twice.

## Turning it on

Exactly as git documents it:

```sh
git config commit.gpgsign true
git config gpg.format ssh                       # or openpgp (the default), or x509
git config user.signingkey ~/.ssh/id_ed25519.pub
```

`ff doctor` has a `signing` row that says whether that will work — the format, the program it names, the key, and for ssh the allowed-signers file verification needs. It is read-only and runs nothing, so asking costs no pinentry prompt.

## The three formats

| `gpg.format` | program | program overrides |
|---|---|---|
| `openpgp` (default) | `gpg` | `gpg.openpgp.program`, then `gpg.program` |
| `x509` | `gpgsm` | `gpg.x509.program` |
| `ssh` | `ssh-keygen` | `gpg.ssh.program` |

`user.signingkey` names the key. For openpgp and x509 it is optional — gpg falls back to a default key of its own. For ssh it is required: a path (the public half is enough, `ssh-keygen` finds the private one beside it) or the key itself, in which case it is signed through the agent. `gpg.ssh.defaultKeyCommand` satisfies the requirement too, when it produces one.

Verification reads three more of git's keys: `gpg.ssh.allowedSignersFile`, `gpg.ssh.revocationFile`, and `gpg.minTrustLevel`.

## What gets signed

Every commit that is *your* work:

- `ff commit` — the close.
- Every commit a rewrite replays: `ff describe`, `ff absorb`, `ff lift`, `ff restack`, `ff sync`, `ff done`, `ff resolve`.

This is a deliberate departure from git, where `git rebase` needs `rebase.gpgSign` set separately and a rebase silently unsigns a branch without it. `commit.gpgsign` governs every user commit fufu writes, replays included. fufu's whole model is that history moves under you — a restack that quietly unsigned three commits is exactly the failure signing exists to prevent.

The cost is one signer run per replayed commit, the same as `git rebase -S`. On a passphrase-protected gpg key with no agent cached, a restack of ten commits is ten prompts.

What is *not* signed, and will not be:

- **Operation-journal commits.** They carry the `fufu <fufu@local>` identity rather than yours; a signature on one would assert something untrue about who wrote it.
- **Park and stash commits.** They carry your identity, but they are internal scratch that never leaves the repository. `git stash` does not sign either.

## Per-invocation overrides

`ff commit` takes `-S`/`--sign` and `--no-sign`. Both are plain switches, deliberately not git's `-S<keyid>`: `ff commit` takes positional paths, so an optional-value short flag would make `ff commit -S file.txt` ambiguous. The key always comes from `user.signingkey`.

```sh
ff commit -S -m "signed"        # sign, whatever commit.gpgsign says
ff commit --no-sign -m "quick"  # do not, whatever it says
```

The rewrite verbs get no flag — config governs them, and the one-off override is git's own environment escape hatch, which gix honors:

```sh
GIT_CONFIG_COUNT=1 GIT_CONFIG_KEY_0=commit.gpgsign GIT_CONFIG_VALUE_0=false ff restack
```

## Reading signatures back

`ff show <rev>` verifies the commit it shows and prints a `signature:` line when there is one — the verdict, who signed, and on a second line which key said so:

```
7e2de1ff  Tyler Johnson  13m ago
  core: honor git's commit signing configuration
  signature: verified — signed by Tyler Johnson <tyler@tylerjohnson.me> (gpg 9B295D68)
```

`--json` carries the same as a `signature` object. An unsigned commit gets no line and costs no signer run.

`ff log` marks signed commits `signed`, by default and for free: whether a commit carries a signature is a header on an object the walk already decoded, so it costs no signer run. `ff status` marks its parent commit row the same way. The word is deliberately `signed` and not `verified` — nothing was checked to say it.

Checking is what `--signatures` buys, and it replaces the mark with the verdict, the tool, and the eight characters that identify the key:

```
●  owrowrvq 7e2de1ff  13m ago  verified gpg 9B295D68
│  core: honor git's commit signing configuration
```

| mark | `%G?` | meaning |
|---|---|---|
| `verified` | `G` | the signature checks out |
| `bad signature` | `B` | it does not |
| `untrusted key` | `U` | it checks out, but below `gpg.minTrustLevel` |
| `expired signature` | `X` | it checks out, expired |
| `expired key` | `Y` | it checks out, made by an expired key |
| `revoked key` | `R` | it checks out, made by a revoked key |
| `unverifiable` | `E` | could not be checked — no key, no allowed-signers file, no verifier |

That costs one signer run per signed row, which is why it is a flag rather than the default; unsigned rows are skipped, so the cost is proportional to how much there is to check. The runs go in parallel — up to one per core, capped at eight — because they are independent and almost entirely process startup. Verifying twenty ssh-signed commits (two `ssh-keygen` runs apiece) costs about 94ms on four cores against 245ms in a row.

Only verification is parallel. Signing is not, and will not be: it can stop for a passphrase, and several pinentry prompts racing for one terminal is not a speed-up. A rewrite signs its commits one after another for the same reason `git rebase -S` does. An unsigned commit is marked nothing at all, with or without the flag: most rows in most repositories are unsigned, and a column of `unsigned` would be a column of noise.

git's `%G?` letters are still what the machine surface carries — `ff show --json` and `ff log --signatures --json` both report `code` — but a row prints words. A bare `G` in a column reads as "gpg" about as readily as "good".

`ff log --json` carries `signed` on every commit whether or not the flag was given, and adds a `signature` object only under `--signatures`: the key's absence is what says nothing was verified, which is not the same claim as null.

## The predicted sha

`ff log` and `ff status` normally show a sha in the `@` row — the id the close *would* mint, computed by building the commit object and hashing it without writing it. With signing on, that prediction is impossible: the signature is not knowable without running the signer, and running one on every status render is out of the question.

So with `commit.gpgsign` on, the `@` row's sha column is blank — the same empty column an unborn branch shows. This is a real, accepted regression for signing users, not an oversight.

## Failure modes

Signing is resolved once per verb, before the first object is written, so a misconfiguration costs nothing. If the signer itself refuses, the commit object was written but no ref moved and no operation was recorded — the same shape as any other pre-transaction refusal, and `ff status` still shows the change open.

- `sign/unknown-format` — `gpg.format` names something that is not `openpgp`, `x509` or `ssh`.
- `sign/no-key` — the ssh format with no `user.signingkey` and no `gpg.ssh.defaultKeyCommand`.
- `sign/no-program` — the program is not on `PATH`.
- `sign/failed` — the signer ran and refused. Its own words are in the message.

`ff explain <id>` has the long form of each, and [the error id index](errors.md) lists every id with its exit code.

fufu captures the signer's stderr rather than letting it through, which is what makes those messages worth reading. A passphrase prompt still reaches your terminal: gpg-agent's pinentry opens the tty itself through `GPG_TTY` rather than inheriting fufu's.
