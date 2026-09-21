Read or change fufu settings. With no arguments, list every setting, its value, meaning, and default marker. A key reads one setting; a key and value set it for this repository. `ff cfg` is the short spelling.

## Examples

```sh
ff config                       # List settings
ff config keep                  # Read snapshot retention
ff config keep 30d              # Set retention in this repository
ff config --global pager bat    # Set the user-level pager
ff config gitPolicy strict      # Refuse covered Git passthrough commands
ff config onConflict resolve    # Open the session on a conflict
ff config pull merge            # Leave branches behind their base standing
ff config --unset autoTrim      # Remove the repository override
```

### Options

### Scope and validation

`--global` writes user-level Git configuration for all repositories. `--unset` removes the selected scope's value, exposing a lower-precedence value or the default. Setting names are case-insensitive and the `fufu.` prefix is optional.

Values are validated before writing. Storage and precedence use ordinary Git configuration under `fufu.<key>`. A malformed value written outside this command can be ignored by its reader in favor of a default; `ff doctor` reports invalid settings.
