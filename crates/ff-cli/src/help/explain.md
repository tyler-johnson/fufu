Look up a fufu error ID and read its meaning and recovery suggestions. Supply an ID from a refusal, or use `--list` to list known IDs. This works offline and outside a repository.

## Examples

```sh
ff explain --list               # Find known error IDs
ff explain held/op-revert       # Read a refusal's explanation
ff explain held/op-revert --json  # Read it as structured fields
```

### Options

### Lookup behavior

Error IDs such as `held/op-revert` name refusals, not commits, changes, or operations. Copy the ID from the error output. An unknown ID is refused with lookup advice; no ID without `--list` is a usage error. If both are supplied, `--list` takes precedence.

The explanation describes the error and next commands. It does not retry the failed operation or change repository state.
