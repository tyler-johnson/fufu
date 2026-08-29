One revision, with its patch: the commit's furniture — id, author, age, subject — then what it did, measured against its first parent.

Bare, it shows `@`: the open change, header and all, with exactly the body `ff diff` prints. One renderer, so the thing you are about to commit and the thing you committed last read the same way.

A merge names the ambiguity instead of picking a parent for you. git prints no diff there either; this says why, and where the per-parent view is.

Revisions only. `ff show <op>` is refused and points at `ff op show` — the operation log is its own address space, which is what lets hex mean commit everywhere and letters mean operation everywhere. Blobs and trees stay git's: `ff git show HEAD:file.txt`.

## Examples

```
ff show                        the open change — the same body as ff diff
ff show HEAD                   what the last commit did
ff show main~2 src/            that commit, narrowed to src/
ff show --json                 header and hunks as fields
ff op show <op>                the other address space
ff git show HEAD:file.txt      a blob at a revision, git's job
```
