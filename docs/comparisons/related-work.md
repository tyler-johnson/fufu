# Related work

fufu invented very little. Each of its load-bearing ideas was proved somewhere else first, and this page places it among its neighbors: what each tool claims, which square it sits on, and the square they collectively left open.

## jj

jj (Jujutsu) is the workflow proof: automatic snapshotting of all work, effortless movement between lines of work, descendants that rebase themselves, and an operation log that makes everything undoable.

fufu takes that workflow wholesale and disagrees only with what it costs. jj is a new version control system whose own store is authoritative, with the git repository as a projection of it.

The full argument — what fufu takes, what the inversion buys, and what it honestly gives up — is [fufu vs jj](vs-jj.md).

## jog

jog is fufu's proving ground: continuous capture of the working copy over an ordinary git repository, shipped and lived with daily.

fufu took the capture floor whole — snapshot automatically before every action, with no verb for asking — along with the passthrough-and-alias pattern that pulls raw git commands into the net.

It also took the operational furniture around a tool you trust with your work: a doctor that inspects the safety net, plain git config as the settings store, and keeping hand-dropped work recoverable long after git would have swept it.

Where they part is ambition. jog changes nothing about how you work — it is a safety net under an unchanged git workflow — while fufu deliberately does, building movement, history rewriting, and undo on top of the floor jog proved. Lessons carried over; compatibility was never owed.

## Sapling

Sapling is Meta's source control system, grown from a long Mercurial lineage and hardened on repositories of enormous scale. Its open-source client speaks to git repositories.

What it claims is a better full client: smartlog as the view of your work, `absorb` folding edits into the right commits, automatic restacking, and undo. Much of the vocabulary this documentation uses traces to it.

The square it sits on is its own interface with its own idea of what you work on. Commits come first and names are optional, so daily work drifts toward anonymous heads, and git serves as the storage and interchange layer beneath a different client.

fufu owes it, jointly with git-branchless, the demonstration that undo, smartlog, and restack can be built over git storage.

## git-branchless

git-branchless brings that same workflow to anyone's repository as a supplement rather than a replacement: a family of verbs — smartlog, undo, restack, move — layered over an unmodified git repository, with git's own commands remaining the daily interface.

Mechanically it is fufu's closest neighbor. Plain git storage underneath, the tool's own records layered on top as bookkeeping rather than authority.

Where it parts from fufu is the paradigm its name announces. It embraces working on commits rather than branches, with a detached HEAD as a normal state — jj's model of what you hold in your hands, arrived at over git storage.

fufu's anonymous branches sit in the opposite corner: heads that are real refs from birth, merely not yet named.

## GitButler

GitButler questions a constraint the other tools all accept: that a working directory holds one branch at a time. Its virtual branches let several lines of work coexist in one checkout, sorted into lanes and committed independently, wrapped in a graphical client with its own safety net of automatic snapshots.

The square it sits on is the working copy itself. While its workspace is active, the checked-out state is the tool's own composition of the applied lanes, and the repository is meaningful chiefly through the tool that arranged it.

It shares fufu's conviction that the working copy deserves software managing it, and answers the authority question the other way. The tool's arrangement of the tree is the state, where fufu insists the tree always reads as an ordinary checkout of one branch.

## The unclaimed square

Lay these on a grid and a pattern shows. jj and Sapling prove the workflow by becoming the authority over the repository. git-branchless keeps the repository plain and adopts the anonymous-head paradigm anyway. GitButler manages the working copy through an arrangement only it fully understands. jog keeps everything plain and changes nothing about how you work.

The square none of them claim is jj's workflow on an unmodified git repository, abandonable at any moment: continuous capture below, history editing as recorded, undoable operations above, and the boring-git-repository [invariant](../concepts/invariant.md) enforced throughout.

That is the square fufu sits on, and the rest of this documentation is the case that it is habitable. What the trade looks like from a git user's chair is [fufu vs git](vs-git.md); the full accounting against the tool that proved the workflow is [fufu vs jj](vs-jj.md).
