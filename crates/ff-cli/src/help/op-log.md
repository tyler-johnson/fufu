Every operation, newest first, wearing the ids the `ff op` verbs take. Captures are in there too, and they outnumber verb operations by more than an order of magnitude, so narrowing is the expression's job — `ff op log 'kind(op)'`. Where you can go *back* to is a different question, and `ff history` is the verb for it.

The bold prefix on each id is the shortest one these verbs resolve unambiguously, so an id copied from here never lands on an ambiguity.

This verb captures first, like every verb but `ff init` and `ff clone`, so on a dirty tree the newest row is this command's own capture — intended, and the same note `ff evolog` carries.

### The expression

The argument is the set language over operations: the same operators as `ff log`, reading the other address space, and positional the way an operation id is positional in `ff op show`.

Ancestry follows the log, so `@^` is the operation before the newest and `::@` is the whole log. Operations bring three functions of their own — on_branch(), session() and kind() — and share latest(), heads() and roots(). Filtering to one session is `session(<name>)`, and that is the only session filter there is.

--at-op and --at bound the walk at a past operation rather than the tip, so `ff op log --at 2h` is the log as it read two hours ago, and an expression alongside them is evaluated against that bounded log.

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
