Open a session to edit an existing commit's files. A revision is required. Fufu creates a session branch at that commit and switches there; `ff done` records your edits, replays later commits, and returns you to the original branch.

## Examples

```sh
ff edit HEAD~2                  # Edit an earlier commit
ff edit HEAD                    # Edit the latest commit
# Edit the files in this checkout, then finish:
ff done
```

### Options

### Branch targets and parked work

A branch name is treated as a switch, not an editing session: `ff edit main` continues main. Use a commit revision when you want a session.

The original branch stays in place while you edit. Its uncommitted work is parked there and resumes when the session ends. Switching away from the session also parks its work, so you can resume later. `ff resolve` uses a similar session for held rewrite conflicts.

### Finishing and recovery

`ff done` amends the selected commit and replays what followed. `ff done --abandon` returns without applying the session, keeping its uncommitted work in retained operation history. `ff undo` can reverse session operations; `ff history` shows the individual steps.
