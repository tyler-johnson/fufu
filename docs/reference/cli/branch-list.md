# ff branch list

Named branches first, then the anonymous ones fufu minted, kept apart so a petname never reads as something you chose. Each row carries its tip, the subject there, and what is hanging off it: a parked change, a pending description, and how it stands against its upstream.

Then what a remote is holding that is not here: the branches a clone or a fetch left a tracking ref for and no local branch of yours tracks. Those rows wear the sigil without the brackets, because the brackets mean a name you can type at ff switch and switch resolves local names only — `ff start origin/<branch>` is the verb that forks one of these into a branch here. The section is bounded the way the map is, with a dim count standing for the rest; --all is that bound spelled off.

## Usage

```
Usage: ff branch list [OPTIONS]

Options:
      --all
          Every remote-only branch, not just the newest few

      --at-op <op>
          Read as of this operation (a letters-spelled id, `@`, `@^`, `@~3`)

      --at <time>
          Read as of the operation current at this time (30m/2h/3d, or a date)

      --json
          Emit machine-readable JSON

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff branch list                 what exists, and what is still anonymous
ff branch list --all           every remote branch too, unbounded
ff branch list --json          the same, for a machine
ff start origin/spike          fork a branch here from one of theirs
ff remote                      what the remotes are called
```
