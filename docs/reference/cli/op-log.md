# ff op log

Every operation, newest first, wearing the ids the `ff op` verbs take. Every one: captures outnumber verb operations by more than an order of magnitude, and the log reports what happened rather than deciding what is worth reading. Narrowing is the expression's job — `ff op log 'kind(op)'` — and where you can go *back* to is `ff history`, which is a different question and has its own verb.

The argument is the set language over operations: the same operators as `ff log`, reading the other address space, and positional here for the same reason an operation id is positional in `ff op show` — the position differs only in how many members it accepts. Ancestry follows the log, so `@^` is the operation before the newest and `::@` is the whole log. Operations bring three functions of their own — on_branch(), session() and kind() — and share latest(), heads() and roots(). Filtering to one session is `session(<name>)`, and that is the only session filter there is.

--at-op and --at bound the walk at a past operation rather than the tip, so `ff op log --at 2h` is the log as it read two hours ago, and an expression alongside them is evaluated against that bounded log.

This verb captures first, like every verb but `ff init` and `ff clone`, so on a dirty tree the newest row is this command's own capture — intended, and the same note `ff evolog` carries.

The bold prefix on each id is the shortest one these verbs resolve unambiguously, so an id copied from here never lands on an ambiguity.

## Usage

```
Usage: ff op log [OPTIONS] [revset]

Arguments:
  [revset]
          Operations to show, as a revset over the operation log

Options:
  -n, --max-count <COUNT>
          Number of rows to show; 0 means unlimited
          
          [default: 25]

      --at-op <op>
          Read as of this operation (a letters-spelled id, `@`, `@^`, `@~3`)

      --json
          Emit machine-readable JSON

      --at <time>
          Read as of the operation current at this time (30m/2h/3d, or a date)

      --session <name>
          Session name for this invocation

  -C, --cwd <dir>
          Run as if fufu had been started in <dir>

  -h, --help
          Print help (see a summary with '-h')
```

## Examples

```
ff op log                      the last 25 operations, every kind
ff op log 'kind(op)'           verb operations only
ff op log 'kind(capture)'      the machine-rate rows alone
ff op log 'session(nightly)'   one session's operations
ff op log '~on_branch(main)'   everything that happened elsewhere
ff log -r 'base(@)'            the commit the newest operation ran on
ff op log --at 2h              the log as it read two hours ago
```
