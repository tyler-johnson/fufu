No subcommands — arity decides. Bare `ff config` lists every setting with its value, its meaning, and a (default) marker. A key alone gets it; a key plus a value sets it; --unset returns it to the default; --global widens the set or unset to every repo.

Storage is plain git config under `fufu.<key>`, so `git config fufu.keep` and fufu can never disagree, and precedence is git's own. What git config cannot do is tell you what settings exist, what they default to, or whether a value will parse — and every fufu reader falls back to its default on a value it cannot read, so a typo'd setting looks set and does nothing. Values here are validated through the readers' own parsers before anything touches disk.

## Examples

```
ff config                      every setting, defaults marked
ff config keep                 what the retention window is
ff config keep 30d             set it, this repo
ff config --global pager bat   set it, every repo
ff config gitPolicy strict     refuse raw git that has a fufu verb
ff config toolPolicy coach     nudge rather than refuse ff in the shell
ff config --unset autoTrim     back to the default
```
