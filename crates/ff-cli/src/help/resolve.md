A held rewrite is a conflict fufu chose not to interrupt you with — and this is where you choose to deal with it, all at once. Every surviving conflict region lands in the working copy together, as ordinary labeled markers, in one session: fix them, then `ff done` lands the rewrite.

The session is a branch, the way `ff edit` opens one: an anonymous branch minted at a commit carrying the markers, and you switch to it. The hold stays on the branch you left, because it is what the session is resolving, and your open change parks there as it does on any switch; a `ff switch` away parks the fixes in progress on the session, and switching back resumes them. `ff done` lands the fixes and returns you in one operation. If the world has moved and the rewrite applies cleanly now, the hold is released instead, and re-running the verb that recorded it lands it.

A held arrival — a parked change `ff switch` could not lay back over a tip that moved — is the one hold with no session: there is no closed commit to protect, so the change is laid into the working copy in place, markers and all, and the open change is the resolution. It refuses while the branch has an open change of its own; `ff commit` it or `ff switch` away first.

--abandon drops the hold — and an open session with it, returning you to the branch — so it is also the way out of one, from either side. Opening a session is two operations, the mint and the switch, so two `ff undo` take it back.

## Examples

```
ff resolve                   materialize the hold's conflicts and fix them
ff done                      land the fixes, and the rewrite behind them
ff resolve --abandon         drop the hold instead
ff switch <branch>           step away; the fixes stay on the session
ff undo                      twice takes a fresh session back, markers and all
```
