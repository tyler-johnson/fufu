The operation log as objects. Every capture and every fufu mutation lands on one log at refs/fufu/ops, and this is the family that reads and moves it: `log` lists, `show` and `diff` read, `restore` rewinds the whole repository to one, and `revert` inverts a single one leaving later work standing. Deleting operations is `ff trim`'s job and nobody else's.

Operation ids are hex, the same as commits, and the slot decides which space a prefix is read in: these verbs and `--at-op` read operations, `-r` and `--from` read revisions. Letters are always a change id. `@` is the newest operation, and git's own first-parent suffixes work on it — `@^` is the one before, `@~3` three back — because an operation's first parent *is* the operation before it.

`ff undo` is the everyday shortcut for `ff op restore`, argument-free and repeatable; most work never needs the long form.

## Examples

```
ff op log                      what has happened, newest first
ff op show @                   what the newest operation did
ff op diff @^ @                what changed across it
ff op restore 9dfd5e5d         rewind the whole repository there
ff undo                        the same move, one run at a time
```
