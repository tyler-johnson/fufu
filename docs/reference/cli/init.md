# ff init

Starts a repository with the safety net already on: a `git init`, then fufu's own two — the floor of the operation log, and the gc guard, the config that stops `git gc` expiring fufu's refs. Both are written before you have typed anything else, so [`ff undo`](undo.md) has somewhere to land from your first command onward.

The default branch is `init.defaultBranch` if you set one, and `main` if you did not.

Run inside a repository that already exists, it means turn fufu on here: the same two things, and it says so rather than pretending it made anything. That is the way to adopt a repository git created, or one you cloned before fufu was on the machine.

It does not touch your shell or your agent. [`ff hook`](hook.md) installs those when you want them, and [`ff doctor`](doctor.md) says what is wired and what is not.

## Usage

```
Usage: ff init [OPTIONS] [dir]

Arguments:
  [dir]
          Where to create it; the current directory when omitted

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
ff init                        here
ff init myproject              in a new directory
ff init                        again, in a repo git made: adopt it
ff doctor                      is the net actually on?
ff git init --bare             a bare repository is still git's job
```
