Create a Git repository with fufu snapshots enabled, or enable fufu in an existing repository. With no directory, use the current directory. Existing work and history stay in place.

## Examples

```sh
ff init                         # Create or adopt here
ff init myproject               # Create in a new directory
ff hook                         # Install machine integrations
ff doctor                       # Check the setup
ff git init --bare              # Use Git for a bare repository
```

### Options

### Repository setup

A new repository uses `init.defaultBranch`, or main when unset. Initialization establishes the operation log's earliest recovery point and the garbage-collection configuration that protects fufu's refs. Recovery starts from recorded state; initialization cannot recover earlier uncaptured work.

Bare repositories have no working copy to snapshot and are refused. Use Git to create them.

### Machine integrations

Repository initialization does not install shell or agent hooks. `ff hook` installs them per machine; follow its activation, restart, and trust instructions. `ff doctor` checks the log and integration configuration.
