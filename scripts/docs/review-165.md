# Integrated documentation review — #165

Verified on September 14, 2026 (UTC), directly on main, from series base `68614022` through `97ac515b` plus this review's fixes, committed as `338c2184`. The review read every #154 handoff and independently inspected current sources, rendered pages, and scratch results. Parent acceptance and Tyler's subsequent landing-page/tutorial correction are recorded below; the original check results describe the tree reviewed at that time.

## Parent acceptance — #154

Accepted on September 14, 2026 (UTC), after all eleven children (#155–#165) finished. The final sole serial Astra reviewer read both briefs and every parent handoff, compared the integrated changes with `68614022`, and checked installation → tutorial → everyday work, recovery → conflicts, stacks → push, and agent activation → JSON scripting against their help and reference owners. The inventory covers the original scope, including the two added help pages. No outstanding implementation omission was identified within the documentation/help task: defaults and next steps precede details, revision syntax has one discoverable owner, examples have executable sources, and recovery limits and per-command effects agree across reader and agent instructions.

Tyler's README correction is satisfied byte for byte against `3f72274d`: light factual edits, original voice and structure, and link-free feature bullets. Historical release notes, changelog history, and the founding design body remain preserved. The consolidated product bugs, policy decisions, unverified suspicion, and measurement boundaries below remain unresolved follow-up work; this acceptance makes no repair, compatibility, or performance promise.

Verification rests on #165's evidence below and the orchestrator's independent rerun at `338c2184`: formatting, workspace/all-target Clippy, the full workspace suite, generation plus four non-rewrite drift checks, strict documentation build, all 39 guide transcripts, and demo replay passed. The final reviewer independently checked the full-suite log totals (**1,908 passed, 0 failed, 0 ignored**), source/page inventory, protected content, acceptance Markdown links, and worktree/base-relative whitespace. Full-suite rerun output: `/home/pi/.local/share/opencode/tool-output/tool_09dcae5db001aj6FJLFkqjbRhl`. Only this acceptance record changed between that rerun and `81db8862`. The untracked `site/` matched #165's archived build byte for byte and was preserved at `/tmp/opencode/fufu-docs-154-orchestrator-site`.

### User-directed landing page and tutorial correction

After parent acceptance at `81db8862`, Tyler extended the README constraint to `docs/index.md`: preserve the original voice, introduction, structure, and feature bullets, with only the short approved recovery/hooks qualifications. The feature bullets are intentionally link-free; navigation belongs in Where to go. README itself remains unchanged.

Tyler also superseded #158's exact-fixture tutorial requirement. The tutorial again starts by cloning fufu's real repository, motivates each action, and lets readers make their own edits and adapt sample IDs and output. Pull/push is a contextual demonstration for a repository where the reader has push access. Install and adoption introductions follow that narrative. Hermetic fixtures remain contributor tooling; the collaboration recording hides teammate setup and shows pulling and pushing. Final acceptance follows this user-directed illustrative walkthrough, rather than requiring every page command and output to mirror a fixture. The exact tutorial comparisons below remain historical verification of the former page.

Correction checks: dogfood build, strict documentation/asset build, first-command links, shell syntax, formatting, and demo replay passed. The existing rendered-link audit passed on all 91 pages with 7,480 local link/fragment occurrences and 323 pre-series anchors preserved. The tutorial fixture replay passed, and only the collaboration cast/GIF was regenerated and reviewed; the other recordings still illustrate the same actions. README remains byte-identical to `3f72274d`. No new page-mirroring tests were added, and all test pushes remained in disposable local repositories.

## Coverage

All 91 Markdown pages under `docs/` were reviewed: 45 hand-maintained pages and 46 generated CLI pages. The following inventory uses basenames within each directory; every name ends in `.md`.

| Directory | Pages |
| --- | --- |
| `docs/` | adopting, changelog, faq, index, install, performance, project, tutorial |
| `docs/concepts/` | branches, changes, glossary, held-rewrites, invariant, push-boundary, snapshots-and-undo, two-regimes |
| `docs/guides/` | plain-git-teammates, recovery, rewriting-history, stacked-changes, worktrees |
| `docs/agents/` | machine-surface, setup, why |
| `docs/reference/` | config, doctor, errors, revisions, signing |
| `docs/reference/hooks/` | bash, claude, codex, cursor, fish, gemini, index, powershell, zsh |
| `docs/comparisons/` | command-table, related-work, vs-git, vs-jj |
| `docs/internals/` | architecture, design, substrate |

All 46 help sources under `crates/ff-cli/src/help/` received a prose and terminal-rendering pass: absorb, branch, clone, collide, commit, config, describe, diff, doctor, done, edit, evolog, explain, fold, git, history, hook, init, lift, log, map, op, op-diff, op-log, op-restore, op-revert, op-show, op-trim, pull, push, redo, remote, resolve, restack, restore, root, show, status, switch, trigger, undo, unhook, update, version, watch, worktree. The generated directory contains those command pages, with index replacing root.

Other reviewed surfaces: README; command/flag descriptions and lane declarations in `cli.rs`; `help.rs`, `docsgen.rs`, and their main entry point; all 127 error entries and caller advice; all 13 setting descriptions in `cmd/config.rs`; shipped `integ/skill.md` and `integ/briefing.rs`; navigation; Makefile and CI; SECURITY; current changelog and historical wrappers. Historical changelog entries, the founding design body, and all 16 tagged release-note files match the pre-series tree. README matches the approved three short qualifications in `3f72274d`, including link-free feature bullets.

The end-to-end reading paths cover installation/activation, everyday commits and parking, file and whole-state recovery, edit/rewrite sessions and arrivals, stacks and remote updates, and JSON scripting. The contract audit covers capture versus manual trigger, exclusions/retention/worktree scope, internal objects versus branch history, three ID spaces and `@`, leases versus ownership, remote rollback limits, partial cascades and per-verb exits, ref-only revert, strict policy/client differences, dry-run/fetch/maintenance effects, doctor repairs, JSON exceptions, and Git/proxy dependencies. No stale letters-only operation example, leaked scene separator, or current MCP installation instruction remains in the reviewed pages.

## Integrated fixes

- Corrected two stale error-advice assertions missed by the preceding selected suites: CLI absorb explanations and core switch refusal advice.
- Fixed root help after global options: [`ff --json help`](../../docs/reference/cli/index.md) and `ff -C . help` now retain examples and the complete command list. Layout selection uses Clap's argument definitions to skip option values; the regression also checks short help with a literal `help` value and bundled flags.
- Corrected edit-session opening to two undo steps in the shipped skill, and removed the suggestion that switching away clears open work blocking arrival resolution from [resolve help](../../docs/reference/cli/resolve.md) and concepts.
- Reconciled help with the current references for shared-delete races, off-branch push/open-state effects, failed push bookkeeping, clone cleanup limits, Claude-only policy replies, managed JSON ownership, SSH verifier process counts, restore capture grouping, describe's hook flag, and explain's passive update lane. Corrected stale doctor and automatic-install source comments.
- Added fold and switch outcomes to the scripting exit table and made pull's dry-run distinction explicit. Fixed FAQ's same-file splitting destination. Restored four pre-series anchors: `what-is-promised`, `published-history-is-append-only`, `the-append-only-boundary`, and `the-one-write-fix`. Generator maintenance instructions now live in a comment on the CLI index.

## Check results

All builds used `CARGO_BUILD_JOBS=1`; Rust tests used `--test-threads=1`. The docs venv was prepended to PATH, and `FF_DOCS_GEN` was unset except inside the generation target. The local host was Linux aarch64 on the Pi. The exact commands, environment, UTC completion times, exits, and complete outputs are in `/tmp/opencode/flight165-evidence/commands.jsonl` and its named logs.

| Command or audit | Actual result |
| --- | --- |
| `cargo test --workspace --no-fail-fast -- --test-threads=1` | Final: **1,908 passed, 0 failed, 0 ignored**, including the new renderer regression; 120 result blocks including doctests. |
| `make fmt-check` | Passed after the final renderer change. |
| `make lint` | Workspace/all-target Clippy with `-D warnings` passed after the final renderer change. |
| `make build` | Final dogfood build passed. |
| `make docs-gen`, then `cargo test -p ff-cli --bins docsgen -- --test-threads=1` without rewrite mode | Four generation checks and four subsequent drift checks passed; digest comparison found no regeneration change. |
| `cargo test -p ff-cli --bins -- --test-threads=1` | All 205 guards passed: formatting, grouping, aliases, skill syntax/budgets, error registry, config/error/reference drift, and small-stack command construction. Also included in the final workspace run. |
| `cargo test -p ff-cli --test help --test cli --test aliases -- --test-threads=1` | 60 passed after the renderer fix; also included in the final workspace run. |
| `make docs` with `/tmp/opencode/fufu-docs-155-venv/bin` on PATH | Asset validation and MkDocs strict build passed. Material printed its upstream MkDocs-2 notice; no strict-build link or asset finding. |
| `python3 scripts/docs/guide-transcripts.py` | 39 blocks and state assertions passed: recovery 12, rewriting 10, stacks 7, plain-Git 5, worktrees 5. |
| Independent guide-scene replay | All 34 fixtures passed separately; 238 displayed commands parsed. Wrong-ID, duplicated-output, setup-failure, and verb-failure probes behaved correctly. |
| Tutorial source and page replay (before the user correction above) | 41 command/edit matches, 19 normalized output comparisons, 12 final file/history/remote assertions, and nine failure-propagation probes passed against the former fixture-style page. |
| Hook and machine transcript replay | All eight installer/file/removal transcripts and eight valid-JSON projections matched; repeated-ID/prefix relationships checked. |
| `make demo-check` | Golden demo output and all eight tutorial steps passed on the final binary. Seven existing cast streams and GIF companions validated; recorded terminal output inspected. Recording commands/output did not change, so no re-recording or blessing was needed. |
| `bash scripts/bench/test_report.sh`; `make bench-docs` | 22 report checks passed. Regeneration preserved historical table cells and raw data; no timing run. |
| Shell syntax | All 14 `scripts/docs/*.sh` files and `install.sh` passed syntax checks. |
| Integrated inventory/render audit | 91 rendered pages; 7,488 local rendered link/fragment occurrences valid, including navigation; 323 pre-series non-generated anchors preserved; 704 help-source lines retained; 47 help shell blocks/220 command lines checked; 127 human/JSON explanations and 250 suggestions checked. |

The initial full suite completed with 1,905 passed and two stale-assertion failures; both were fixed. During editing, existing guards caught an 81-column help link and a 16,001-byte skill; both were shortened without weakening guards (final skill: 15,949 bytes). The new prefixed-help regression failed before its fix. The base-relative anchor audit found the four missing anchors above. Every affected check subsequently passed, followed by the final full workspace run.

Fresh scratch probes also verified ignored/oversized capture behavior; capture before reader refusal; actual interactive Bash prompt capture; all four client installer/capture/delete/restore/policy paths; doctor repairs and capture without `--fix`; config precedence and invalid retention; SSH signing and verification; 213 revision/operation checks including 19 extracted examples; unfinished Git merge/bisect repairs; ref-only revert; stale resolution abandonment/recreation; blocked arrival repair; worktree removal recovery; strict tag refusal; transport fallback; extension environment inheritance; and the product findings below. Destructive operations and remote writes used disposable repositories only.

## Consolidated remaining issues

These findings are documented current behavior or separate product decisions. Related closed flights identify implementation history, not an open repair assignment.

| Classification | Finding and evidence | Related flights |
| --- | --- | --- |
| Verified product bug | Off-branch push sends correct tips but writes the current checkout's tree into named targets' open state. The stack source asserts the wrong tree and resumed deletions. See `guides-final.log`, `guide-scenes.log`, `scripts/docs/stacked-changes-transcript.sh`, CLI `cmd/push.rs:136–148`, core `push.rs:419–435`. | Found in #160; related #122, #143, #145, #150 (closed). No dedicated open repair flight found. |
| Verified product bug | A successful remote send followed by failed note append returns `push/unrecorded`; an up-to-date retry leaves the note/published/seen records missing. Pull restores seen only. See `error-repairs.log`, `probes-164-trace.json`, CLI `cmd/push.rs:108–151`, core `push.rs:183,387–445`. | Found in #164; related #122 and #150 (closed). No dedicated open repair flight found. |
| Verified revision bugs | Open-change range/extrema identities have documented exceptions; equal-time forward traversal omits descendants with intermediate refs. Independently reproduced at timestamp `1789351734`: `HEAD~2::` returned one of three expected commits. See `revisions.log`, `equal-time.log`, core `revset/eval.rs:326–364,486–535,573–616`. | Found in #157; related #141, #143, #144 (closed). No dedicated open repair flight found. |
| Verified policy mismatch | Strict passthrough refuses tag pushes although fufu has no tag-send verb. Scratch tag send succeeds after explicit policy selection. | #101 remains backlog; related #135 (closed). |
| Verified diagnostic issue | A completed commit followed by raw reset can report “previous op may not have completed.” The reset fixture verifies the correct recovered ref and file; the warning's cause is not established here. | Found in #160; related reconciliation/reporting #108 (closed). |
| Verified behavior; exit-policy decision | Reword can hold a stale child while returning 0. Docs and skill require inspection of the cascade. See `coverage-probes.log`. Whether to standardize this with pull/restack's exit 3 is product work. | #83 and #155 (closed); unresolved product decision recorded here. |
| Verified behavior; API-policy decision | Extensions preserve inherited `FF_REPO`/`FF_SESSION` when no replacement resolves. See `agents.log`. Clearing stale inherited values would change the extension contract. | #161; related extension simplification #148 (closed). |
| Policy decisions | Forward payload/error-ID compatibility under envelope 1 remains unspecified. Making doctor read-only would change its verified capture/fetch/maintenance behavior; misleading source comments were corrected here. Manual trim's `gc --auto` behavior remains documented. | #53 and #151 (closed); #104 remains backlog for GC policy. |
| Unverified suspicion | #155's unbounded-lift/root-selection observation has no independently established cause and is not proven to be the equal-time forward-walk bug. Current recipes use verified bounded ranges. | #155; related #144 (closed). |
| Measurement boundary | Current-release latency and the proposed large-scale budget remain unmeasured. Published data retains its fufu 0.12.0, Git 2.50.1, jj development-build, and Cortex-A76 provenance. | #103 remains backlog. |

Client UI activation/trust was not exercised; installer checks and simulated payloads establish only those tested paths. Bash prompt execution was exercised. Zsh, Fish, and PowerShell are absent on this host; the optional PowerShell Rust test returns early rather than counting as ignored. Cross-platform CI and external comparison-tool runtime behavior are outside this local run's evidence.

## Evidence locations

- Durable scope, fixes, results, and issue consolidation: this file.
- Full command ledger and logs: `/tmp/opencode/flight165-evidence/commands.jsonl`, especially `workspace-final.log`, `coverage-final.log`, `final-drift.log`, `docs-anchors.log`, `demo-final.log`, and `guides-final.log`.
- Per-file fingerprints, catalog, source excerpts, and actual repair commands/results: `inventory.json`, `catalog.json`, `catalog-sources.log`, and `probes-164-trace.json` in that evidence directory.
- Check scripts as executed, with hashes: `/tmp/opencode/flight165-evidence/tools/` and `summary.json`.
- Strict-built site: `/tmp/opencode/fufu-docs-165-site`.
