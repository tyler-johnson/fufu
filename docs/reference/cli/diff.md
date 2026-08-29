# ff diff

The open change as a patch: what `ff commit` would land, and what it says. Every other view here reports `path +N -M`; this is the same tree diff read down to the line.

It is the one patch tool that sees the whole change. `git diff` is blind to untracked files, and an untracked file is exactly where a wrong commit comes from — so the file you just created shows up here with its content, without an `ff status` first to make it visible.

The body is git's unified diff, verbatim, because a patch format is not fufu's to invent: what comes out of here is what `git apply` takes. The diffstat is `ff status`, and this verb deliberately does not reprint it.

Paths narrow it, by the rule `ff restore` speaks: a file, or a directory prefix. No globs.

## Usage

```
Usage: ff diff [OPTIONS] [path]...

Arguments:
  [path]...
          Files or directories to limit the patch to; all of them when omitted

Options:
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
ff diff                        the whole open change, with content
ff diff src/                   just what changed under src/
ff diff --json                 hunks and lines as fields
ff diff > fix.patch            output git apply reads back
ff status                      the same change, as counts
ff op diff <a> <b>             the same question between two operations
```
