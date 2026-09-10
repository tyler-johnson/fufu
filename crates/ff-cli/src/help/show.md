One revision, with its patch: the commit's furniture — id, author, age, subject — then what it did, measured against its first parent. Bare, it shows `@`, the open change, header and all, with exactly the body `ff diff` prints.

A merge names the ambiguity instead of picking a parent for you. git prints no diff there either; this says why, and where the per-parent view is.

A commit that carries a signature gets a signature line under its subject, with the verdict in git's own vocabulary — good, bad, untrusted, expired, revoked, unverifiable — and who signed it. An unsigned commit gets no line and costs no signer run. `--json` carries the same as a `signature` object.

Revisions only, and a change id is one: `ff show <change id>`, or any prefix of it unique in the repository, shows that commit; a prefix of the open change's id shows `@`. `ff show <op>` is refused and points at `ff op show`: the operation log is its own address space, hex like commits, and the slot decides. Blobs and trees stay git's, as `ff git show HEAD:file.txt`.

## Examples

```
ff show                        the open change — the same body as ff diff
ff show HEAD                   what the last commit did
ff show nyrszqtk               that change, by the id ff log prints
ff show main~2 src/            that commit, narrowed to src/
ff show --json                 header and hunks as fields, signature included
ff op show <op>                the other address space
ff git show HEAD:file.txt      a blob at a revision, git's job
```
