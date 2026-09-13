# ff init

Create a Git repository with fufu snapshots enabled, or enable fufu in an existing repository. With no directory, use the current directory. Existing work and history stay in place.

## Usage

```
Usage: ff init [OPTIONS] [dir]
```

## Examples

```sh
ff init                         # Create or adopt here
ff init myproject               # Create in a new directory
ff hook                         # Install machine integrations
ff doctor                       # Check the setup
ff git init --bare              # Use Git for a bare repository
```

## Options

```
Arguments:
  [dir]
          Where to create it; the current directory when omitted

Options:
      --json
          Emit machine-readable JSON

      --fetch
          Fetch now on commands that support fetching, regardless of cadence

      --no-fetch
          Skip the fetch: read the tracking refs as they stand

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Repository setup

A new repository uses `init.defaultBranch`, or main when unset. Initialization establishes the operation log's earliest recovery point and the garbage-collection configuration that protects fufu's refs. Recovery starts from recorded state; initialization cannot recover earlier uncaptured work.

Bare repositories have no working copy to snapshot and are refused. Use Git to create them.

## Machine integrations

Repository initialization does not install shell or agent hooks. [`ff hook`](hook.md) installs them per machine; follow its activation, restart, and trust instructions. [`ff doctor`](doctor.md) checks the log and integration configuration.
