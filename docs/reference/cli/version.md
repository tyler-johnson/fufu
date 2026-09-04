# ff version

Which fufu this is: the full name, the release, and the commit and date it was built from, with the project's home under it.

A build made without git available (a source tarball, a crates.io vendor, a docker context with no .git) names the release alone — there is no commit to name.

Then whether it is the current one, read from the cache the passive update lane keeps rather than from the network: nothing here reaches out, and nothing here waits. A line appears only when a newer release is cached; up to date says nothing.

--json splits the line into fields — version, commit, date, and the update status. `ff -v` is the same answer spelled as a flag: it reads the update cache, says the "available" line, and takes --json exactly as the verb does.

## Usage

```
Usage: ff version [OPTIONS]

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
ff version                     the release, the build, the update lane
ff -v                          the same, spelled as a flag
ff version --json              the same, as fields
```
