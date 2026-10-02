# Effectiveness bench — design and pilot

> The v1.0 go/no-go instrument (features.md, v1.0 exit criteria): does a
> coding agent finish real Continuum tasks faster with Graphite than without,
> without losing correctness? Harness: `bench/effectiveness/`.

## What it measures

Two arms, same everything except Graphite:

| | Arm A — baseline | Arm B — Graphite |
|---|---|---|
| Agent | Claude Code headless (`claude -p`, stream-json) | same |
| Model | fixed per run (`--model`, default `sonnet`) | same |
| Sandbox | fresh git worktree at the task's start commit | same |
| Prompt | task statement + bench rules | + one line pointing at the `graphite` CLI |
| Setup | uv env pre-warmed (not timed) | + `graphite init --json` (starts the daemon, blocks until indexed; not timed) |
| Teardown | — | `graphite daemon stop` |

Arm B's prompt line points at the real CLI: `graphite lookup <symbol>`,
`graphite blast <symbol> [--depth N]`, `graphite diff-impact`, all `--json`.
The binary comes from `GRAPHITE_BIN` or `PATH` and is prepended to the agent's
`PATH`. Rebuild before any arm-B run: `cargo build --release -p graphite-cli`.

Per run it records:
- turns, tool calls by type (Read, Grep, Edit…, Bash split into search / list /
  read / edit / run / git / graphite), calls before the first edit;
- wall-clock, API duration and tokens (money is recorded but not reported);
- **files read** — via the Read tool and via Bash (cat / head / sed -n / grep
  FILE / rg FILE…, cd-aware, only paths that exist in the worktree), reported
  separately and as a union;
- files edited, outcome (`success` / `fail` / `harness_error`);
- arm B: every `graphite` command with its JSON output (`graphite.jsonl`),
  paths Graphite returned, whether a **complete** answer (`completeness.status
  = complete`) was received, and **searches after a complete Graphite answer**
  (grep/find/Grep/Glob the agent still ran — tells whether it trusts the graph).

**Outcomes.** Only the agent's own finish, turn limit or budget limit (or its
own timeout) is an agent result. Anything else — a suite that did not run
(no junit, nothing collected), a judge that never answered after 3 attempts,
the agent killed by a signal, no result event, an API failure, arm setup
failing, any harness exception — is `harness_error` and is excluded from
every metric; the report lists them. Agent processes run in their own
session so no parent/waiter/monitor signal can reach them.

**Go** (analyze.py), on tasks with both arms, per-task medians over repeats,
95% CI from 5000 paired bootstrap resamples of tasks:
- turns ratio B/A: point ≤ 0.80 **and** CI upper ≤ 1.00;
- wall-clock ratio B/A: point ≤ 0.80 **and** CI upper ≤ 1.00;
- success rate B−A: CI upper ≥ −5 points (B not shown worse by more than 5);
- zero silent-stale Graphite answers.

**Arm B attribution** per run: `graphite_not_used`, `graphite_used_success`,
`failure_despite_graphite` (every ground-truth file the agent missed was in
some Graphite answer), `graphite_caused_failure` (a ground-truth file the
agent did not touch/list appeared in no Graphite answer — a candidate
Graphite correctness miss, listed with the file; review each, the query may
have been about a different symbol).

**Rethink rules** (from features.md): Graphite used but no faster or less
correct → revisit the `diff_impact` answer shape; Graphite not used → surface
problem, pull v1.1 hooks forward and rerun; correctness dropped → stop.

## Task set (22 tasks at first, 27 since 2026-10-01)

Mined from Continuum history (`mine.py`, `show_task.py`). Code tasks replay a
real commit: the agent starts at `ref~1` and gets an issue-style statement of
what `ref` delivered — symptom and required interface, not the
implementation. Question tasks ask "what breaks if I change X" at a pinned
commit.

| id | kind | difficulty | reference | tools touched |
|---|---|---|---|---|
| rover-rebase-base-branch | signature | hard | e8b211a1c1 | rover + runner |
| marshal-resolve-by-entry | signature | hard | efca286a1b | marshal |
| regent-key-files-at-ref | feature | hard | 60de6dec9c | regent |
| regent-is-ancestor-tristate | bugfix | medium | b3fc411912 | regent |
| regent-tristate-siblings | bugfix | medium | c3300fddf6 | regent |
| core-consolidate-pin-model | refactor | medium | d6555252bf | continuum-core + refiner + intake |
| rover-auth-on-push | signature | medium | 0cfc753b1c | rover |
| runner-durable-attempts | feature | hard | 7553e7eb6e | runner |
| runner-pr-base-guard | bugfix | medium | 4b07891d28 | runner |
| runner-gh-timeouts | bugfix | medium | 900e76143c | runner |
| runner-auto-merge-queued | bugfix | medium | 24c1aec482 | runner |
| dao-idempotent-publish | bugfix | hard | ce363fce75 | dao-cli + instrument |
| runner-wall-clock | feature | medium | ea401cabed | runner |
| zen-dedup-relevance | feature | hard | e73dae002b | zen-review |
| instrument-branch-prefix | feature | medium | 72f3c8febe | dao-cli + instrument |
| regent-lints-every-task | refactor | medium | 082b68faaf | regent |
| sourcerer-monorepo-root | signature | easy | 41c02e3e6d | sourcerer |
| tools-pin-model-default | feature | medium | 502651cc84 | 7 tools (no suite) |
| q-resolve-task-yaml-path | question | medium | ce363fce75 | 12 caller files |
| q-resolve-owner | question | easy | ce363fce75 | 7 caller files |
| q-find-project-dir | question | hard | ce363fce75 | 26 caller files |
| q-check-artifacts | question | easy | ce363fce75 | 5 caller files |

Mix: 5 signature changes with callers, 6 bug fixes, 5 features, 2
refactors, 4 impact questions; 1 easy code task, 11 medium, 6 hard.

## Success checks

Code tasks — **success = no suite regression AND judge verdict "solved"**:

- **Regression** — the tool suites touched by the reference run on the agent's
  tree; failures must be a subset of the start tree's failures (frozen in
  `truth/<id>.json`). Some Continuum tests fail in any sandbox (git remote /
  gh auth); the subset rule tolerates those.
- **Judge** — an LLM (default `opus`, no tools) gets the task statement, the
  reference diff, the agent diff and the test signals, and returns
  solved / partial / failed with what's missing. Internal structure may
  differ; behaviour the task asked for may not.
- **Hidden tests (reported, not gating)** — the reference commit's own test
  files overlaid on the agent tree. They discriminate (e.g. regent
  is-ancestor: 26/47 pass at start, 47/47 at the reference) but are coupled
  to the reference's private helpers and mocked git call sequences, so a
  correct solution with different structure can fail them.
- **Coverage (reported)** — recall of the reference's non-test source files
  among files the agent edited.

Question tasks — success = recall ≥ 0.9 and precision ≥ 0.8 of the paths after
`AFFECTED:` against the frozen caller list (`git grep` of `symbol(` at the
pinned commit, test files and the definition file accepted as non-spurious).

## Isolation

- Private mirror clone of Continuum with its remote removed: no run can push
  into the real repo. Continuum itself is only read.
- `--setting-sources project --strict-mcp-config`: no user hooks, plugins or
  MCP servers. Verified by probe: user plugins (e.g. caveman) are absent.
- **Caveat:** `~/.claude/CLAUDE.md` (and RTK.md) still load — only OAuth
  keeps us off `--bare`. A bench-rules system prompt tells the agent to ignore
  its commit/push/RTK/style instructions and never commit. Same in both arms.
- Web, cron, notification, workflow and worktree tools are disallowed;
  `Task` (subagents) stays, as in real use.

## Usage

```bash
cd bench/effectiveness
python3 build_truth.py                  # freeze ground truth (suites: ~40 s/task)
python3 run.py --arm A --tasks all --repeats 3 --label full-A
cargo build --release -p graphite-cli     # from repo root, before any arm-B run
GRAPHITE_BIN=$PWD/../../target/release/graphite python3 run.py --arm B --tasks all --repeats 3 --label full-B
python3 analyze.py results/full-A.jsonl results/full-B.jsonl -o results/report.md   # --treatment C when several arms
```

Arm B is a command template in `arms.toml` (`setup`, `teardown`,
`prompt_suffix`, `requires`); it refuses to run until `graphite` is on PATH.
Run long batches detached in their own session: `python3 detach.py work/x.log python3 run.py …` (prints the PID; watch that PID, not a shell). Harness tests: `uv run --with pytest python -m pytest tests -q`.

## Harness bugs found by the first pilot (fixed)

The first pilot (`results/pilot-A-buggy.jsonl`) scored 6/10 and three of the
four failures were the harness, not the agent:

1. **Suite did not run** (sourcerer rep 1, `no-junit`): `uv run pytest` could not
   spawn pytest — sourcerer keeps it in a `dev` extra. Fix: install
   `--all-extras --all-groups`, `--continue-on-collection-errors`; no junit or
   zero tests → `harness_error`; the truth build refuses a baseline from a suite
   that did not run (and was rebuilt).
2. **Judge error** (regent rep 1, empty reply): 3 attempts with backoff
   (5/20/60 s); still failing → `harness_error`.
3. **Run killed from outside** (q-resolve-owner rep 1, exit 143 after 8 s): a
   dying background waiter's signal reached the agent. Fix: own session per
   agent process; killed by a signal / no result event → `harness_error`.
4. **Files read via Bash were invisible**: the agent reads mostly with
   `cat`/`sed -n`/`grep FILE`; the old metric said 0 files read on runs that
   read 4. Now counted from Bash commands (cd-aware, existing paths only).

Arm-B smoke found a fifth: **Graphite's `.graphite/` index** (binary) ended up
in the agent diff and crashed decoding. Fixed product-side (daemon writes
`.graphite/.gitignore`, 064cdd7) and bench-side (pathspec exclusion, tolerant
decoding) as defence in depth. `tests/test_harness.py` (19 tests) pins each
bug to the recorded run that exposed it.

## Clean pilot — arm A (baseline)

5 tasks × 2 repeats, `sonnet`, judge `opus`. **10/10 success, 0 harness
errors.** 12.9 min end to end, sequential. `results/pilot-A-clean.md`.

| task | kind | turns (min–max) | wall s (min–max) | files read | calls before 1st edit | success |
|---|---|---|---|---|---|---|
| core-consolidate-pin-model | refactor | 10 (10–10) | 76.7 (73.6–79.8) | 5.5 | 4.5 | 2/2 |
| q-resolve-owner | question | 3 (3–3) | 11.9 (11.8–12.1) | 0 | – | 2/2 |
| regent-is-ancestor-tristate | bugfix | 11 (11–11) | 65.8 (55.4–76.3) | 2 | 5 | 2/2 |
| runner-pr-base-guard | bugfix | 17 (15–19) | 95.0 (74.1–115.9) | 4.5 | 9 | 2/2 |
| sourcerer-monorepo-root | signature | 10.5 (9–12) | 32.5 (28.2–36.8) | 3 | 6 | 2/2 |

Totals: median 10.5 turns, 64.5 s per run.
Hidden reference tests pass 0.26–1.00 on solved runs (runner-pr-base-guard
0.26: the reference tests mock the reference's own helper names) — confirms
they are a signal, not a gate. Judge verdicts all first-attempt.

Variance within a task is small (turns ±1–2, wall ±20–25%), so 3 repeats
already give stable per-task medians; the task count (not repeats) drives
CI width.

## Arm-B smoke

2 tasks × 1, real CLI (`target/release/graphite`), same settings.

| run | outcome | turns | wall s | graphite calls | complete answer | searches after it | attribution |
|---|---|---|---|---|---|---|---|
| q-resolve-owner | success | 4 | 12.9 | 1 (`lookup` + `blast --depth 1`) | yes | **2** | graphite_used_success |
| regent-is-ancestor-tristate | success | 9 | 48.3 | 0 | – | – | graphite_not_used |

Setup (`graphite init`: 884 files, 9.4K symbols, ~2.9 s) and teardown worked;
`graphite.jsonl` holds both commands and their JSON; attribution classified
both runs. Two early signals, not conclusions (n=2):
- **Conversion:** on the bug-fix task the agent never called Graphite — the
  prompt line alone didn't route it. That is the "not used → surface problem"
  branch; v1.1 hooks are the planned answer.
- **Trust:** on the question task the agent got a complete Graphite answer
  and still ran 2 greps to confirm it. The full run reports this per run
  (`search_after_complete_graphite`) to see whether it is systematic.

## Pilot — arm A vs arm B (Graphite, `--json` CLI)

Same 5 tasks × 2 repeats, same model, build includes 064cdd7.
`results/pilot-B.jsonl`, report `results/pilot-AB.md`. 0 harness errors.

| task | arm | turns | wall s | files read | searches | graphite calls | searches after complete answer | success | attribution |
|---|---|---|---|---|---|---|---|---|---|
| core-consolidate-pin-model | A | 10 | 77 | 5.5 | 3 | – | – | 2/2 | – |
| | B | 8 | 51 | 6.5 | 2 | 1 | 2 | 2/2 | used_success |
| q-resolve-owner | A | 3 | 12 | 0 | 2 | – | – | 2/2 | – |
| | B | 3.5 | 10 | 0 | 1.5 | 1 | 1.5 | 2/2 | used_success |
| regent-is-ancestor-tristate | A | 11 | 66 | 2 | 2 | – | – | 2/2 | – |
| | B | 13 | 63 | 2 | 3 | 0 | – | 2/2 | **not_used** |
| runner-pr-base-guard | A | 17 | 95 | 4.5 | 4.5 | – | – | 2/2 | – |
| | B | 11 | 64 | 1 | 2.5 | 1.5 | 2.5 | 2/2 | used_success |
| sourcerer-monorepo-root | A | 10.5 | 32 | 3 | 2 | – | – | 2/2 | – |
| | B | 11 | 41 | 2 | 1 | 1 | 1 | 2/2 | used_success |

(medians over 2 repeats; searches = grep/rg/find/ls via Bash + Grep/Glob)

Paired, 5 tasks, 5000 bootstrap resamples:
- turns ratio B/A **1.05** [95% CI 0.65–1.18]
- wall-clock ratio B/A **0.88** [0.66–1.27]
- success 10/10 both arms (difference 0 pts)
- verdict by the rule: **NO-GO** — but with 5 tasks the CIs are far too wide
  to support any conclusion either way; this is a pipeline check, not a
  result.

Reading:
- **Used 8/10 runs**, never on regent-is-ancestor-tristate (both repeats):
  the prompt line alone doesn't route a bug-fix task through the graph.
- **Every run that got a complete Graphite answer still searched afterwards**
  (1–2.5 greps): the agent verifies the graph instead of trusting it — that
  pattern cancels most of the gain.
- Where it was used on multi-file work the numbers moved the right way
  (runner-pr-base-guard −35% turns, −33% wall; core-consolidate −20% turns,
  −34% wall); on small tasks (sourcerer, q-resolve-owner) a Graphite call is
  an extra step, not a replacement.
- Correctness unchanged.

Next arms (below): B2 tests whether the compact text format is trusted
without re-grepping; C adds interception hooks to fix routing and trust.

## Next arms (prepared, not run)

- **B2 — Graphite CLI, text format.** Same as B, prompt points at `graphite
  lookup X`, `graphite blast X`, `graphite diff-impact` with no `--json`
  (compact text with call-site lines, landed 3d4fdc1..478fbda). Text output
  captured in `graphite.jsonl` with its byte size.
- **C — B2 + interception hooks.** Setup runs `graphite hooks install --repo
  {tree}` (project `.claude/settings.json`, PreToolUse on Bash / Grep / Read);
  teardown uninstalls; `.graphite/hooks.jsonl` copied per run. Hook-served
  answers count as Graphite answers in attribution and in
  `search_after_complete_graphite`. Waits on the `hooks-search` work.

## Pilot — arm C (Graphite CLI text format + interception hooks)

Same 5 tasks × 2 repeats, same model. Binaries from `feat/interception` at
fe2f5af (`build_interception.sh`), dry smoke passed first. 0 harness errors.
`results/pilot-C.jsonl`, report `results/pilot-AC.md`.

| task | arm | turns | wall s | tools | files read | searches after complete | graphite calls | hook answers (answer+enrich) | rewrites | fallbacks | overlap intra / cross keys | success | attribution |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| core-consolidate-pin-model | A | 10 | 76.7 | 9 | 5.5 | – | – | – | – | – | – | 2/2 | – |
| | B | 8 | 50.8 | 7 | 6.5 | 2 | 1 | – | – | – | – | 2/2 | used_success |
| | C | 8.5 | 53.2 | 7.5 | 6 | 2 | 1 | 4 | 1.5 | 0 | 0 / 0 | 2/2 | used_success |
| q-resolve-owner | A | 3 | 11.9 | 2 | 0 | – | – | – | – | – | – | 2/2 | – |
| | B | 3.5 | 10.5 | 2.5 | 0 | 1.5 | 1 | – | – | – | – | 2/2 | used_success |
| | C | 3 | 11.6 | 2 | 0 | 0 | 2 | 0 | 0 | 0 | 0 / 0 | 2/2 | used_success |
| regent-is-ancestor-tristate | A | 11 | 65.8 | 10 | 2 | – | – | – | – | – | – | 2/2 | – |
| | B | 13 | 62.8 | 12 | 2 | – | 0 | – | – | – | – | 2/2 | not_used |
| | C | 12 | 79.3 | 11 | 1 | – | 0 | 7 | 3 | 0 | 0 / 3 | 2/2 | used_success |
| runner-pr-base-guard | A | 17 | 95 | 16 | 4.5 | – | – | – | – | – | – | 2/2 | – |
| | B | 11 | 63.7 | 10 | 1 | 2.5 | 1.5 | – | – | – | – | 2/2 | used_success |
| | C | 12 | 69 | 11 | 2.5 | 0.5 | 1 | 6 | 0.5 | 0 | 0 / 0 | 2/2 | used_success |
| sourcerer-monorepo-root | A | 10.5 | 32.5 | 9.5 | 3 | – | – | – | – | – | – | 2/2 | – |
| | B | 11 | 41.2 | 10 | 2 | 1 | 1 | – | – | – | – | 2/2 | used_success |
| | C | 10.5 | 41.5 | 9.5 | 4 | 0 | 1 | 8 | 1 | 0 | 0 / 0 | 2/2 | used_success |

(medians over 2 repeats. Searches after complete, arm C: when a hook served the
first complete answer, only searches the graph did not answer count; when the
complete answer came from an explicit `graphite` call (7 of 10 runs), the
transcript count is used.)

Paired, 5 tasks, 5000 bootstrap resamples — CIs wide, pipeline check only:
- turns C/A **1.00** [0.71–1.09]; wall-clock C/A **0.97** [0.69–1.28]
- turns C/B **0.95** [0.86–1.09]; wall-clock C/B **1.08** [1.01–1.26]
- success 10/10 in every arm

Hooks over the 10 runs: 59 Bash PreToolUse decisions → 12 rewritten, 47 passed
through (24 "unsupported shell construct", 16 "contains a command graphite
does not handle", 7 "nothing graphite can answer"); 28 graph answers + 22
enrichments; 0 fallbacks; hook latency median 27.5 ms. Graph answer bytes
57.2 KB vs 24.6 KB a plain grep would have printed. Overlap: no intra-command
repeats; cross-call repeats only on regent (3 keys, 300 B per run median).

Reading:
- **Routing fixed, speed not:** Graphite reached every run (regent went from
  never used in B to 7 hook answers in C), but turns and wall-clock are flat
  vs A and slightly worse than B on wall-clock.
- **Most agent commands bypass the hooks:** 80% of Bash decisions passed
  through, mostly for shell constructs and commands the hook doesn't parse —
  the interception surface is the bottleneck, not the answers.
- **Trust improved:** post-answer searches dropped (median 0 in 4 of 5 tasks,
  vs 1–2.5 in B).
- **Answers are larger than the greps they replace** (2.3× bytes), with no
  measurable turn saving — worth checking whether that size buys anything.
- Correctness unchanged; no Graphite-caused failure.

## Issue-form tasks (2026-10-01)

**Why.** Pilot C was flat because the statements named the file or function
to change: localization was short before Graphite could help. Every code
statement is now written as a real issue — symptom and expected behaviour,
user-facing names only (CLI commands and flags, error messages, config keys,
task.yaml values). The old text stays as `statement_hinted`
(`run.py --statement hinted`; default `issue`), and the judge grades against
the statement the agent saw.

**Leak check** (`leaks.py`, `tests/test_tasks.py`): no path the reference
diff touches (full or basename) and no def/class it changes or edits inside
may appear in the issue statement; plain lowercase names count only when
written as code. All 23 code tasks pass; the check flags 14 of the 18 old
hinted statements. `leak_allow` exempts a config file users edit
(`model_pins.json`, `owners.yaml`). Question tasks name their symbol by
design: not localization tasks, not checked, out of issue-form pilots.

**Five new cross-module tasks** (`mine_cross.py`: non-test source in ≥2
tools, tests in the commit, 30–600 changed lines), issue-form from the
start, truth frozen with suites:

| id | kind | ref | tools | hidden tests start → ref |
|---|---|---|---|---|
| regent-project-auto-enqueue | feature | fd7e623d75 | dao-cli + regent | 12 → 15 pass |
| owner-publish-choice | feature | e3a567043f | dao-cli + regent + instrument + foreman | 28 → 40 pass |
| refinement-stays-finished | bugfix | 76530285c6 | refiner + regent | 33 → 37 pass |
| foreman-validating-reject | feature | f6af34e43c | dao-cli + foreman | 0 → 4 pass |
| milestone-status-ssot | refactor | 93be3c744b | dao-cli + foreman | 38 → 39 pass |

## Pilot — issue-form tasks, arm A vs arm C

5 tasks × 2 repeats, `sonnet`, judge `opus`, statements `issue`: the three
shared with earlier pilots (runner-pr-base-guard, core-consolidate-pin-model,
regent-is-ancestor-tristate) + owner-publish-choice and
refinement-stays-finished. Arm C binaries from `feat/interception` at
fe2f5af (`build_interception.sh fe2f5af` → `work/target-fe2f5af/`). A and C
ran concurrently as separate processes. `results/pilot-issue-{A,A-rerun,C}.jsonl`,
report `results/pilot-issue-AC.md`.

One harness error: an arm-A run (regent-is-ancestor-tristate rep 0) got
SIGTERM 5 s in (exit 143) after its first grep, with nothing of the harness
signalling it (own session; no timeout). Excluded and rerun
(`pilot-issue-A-rerun`); the sender was not identified.

| arm | runs | success | turns mean / median | wall s mean / median |
|---|---|---|---|---|
| A (no Graphite) | 10 | 10/10 | 14.1 / 14.0 | 86.6 / 92.3 |
| C (lines + hooks) | 10 | 10/10 | 13.4 / 13.0 | 79.6 / 77.0 |

| task | A turns | C turns | A wall s | C wall s |
|---|---|---|---|---|
| core-consolidate-pin-model | 10.5 | 9.5 | 49.9 | 86.4 |
| owner-publish-choice | 18.0 | 13.0 | 94.2 | 73.4 |
| refinement-stays-finished | 15.0 | 18.5 | 103.2 | 95.7 |
| regent-is-ancestor-tristate | 15.0 | 14.0 | 96.2 | 70.0 |
| runner-pr-base-guard | 12.0 | 12.0 | 89.7 | 72.6 |

(medians over 2 repeats)

Paired, 5 tasks, 5000 bootstrap resamples — CIs wide, pilot only:
- turns C/A **0.93** [0.72–1.23]
- wall-clock C/A **0.81** [0.73–1.73]
- success 10/10 both arms; 0 silent-stale answers; attribution
  `graphite_used_success` 10/10, no Graphite-caused failure.

core-consolidate's C wall-clock (+73%) is API time, not tools: run C-0 made
6 calls in 7 turns with 77 s of API time vs 37 s for A-1 with similar output
tokens; hook latency in that run summed 0.2 s.

**Hooks (10 C runs).** 107 Bash PreToolUse decisions: 40 rewritten, 67
passed through — "unsupported shell construct" 35, "contains a command
graphite does not handle" 32. 101 graph answers + 5 enrichments, 0
fallbacks; hook latency median 187 ms per run. Answer bytes 144.2 KB vs
116.9 KB the plain greps would have printed (1.23×; pilot C was 2.3×).
Searches after a complete answer: median 0 (1 of 8 runs searched again).

**Turns by phase** (`phases.py`: each top-level turn split evenly over its
tool calls; localize = grep/rg/find/ls/Grep/Glob/graphite/Task, read =
Read/cat/sed -n/git show, edit, test = pytest/uv/python/make/ruff):

| set | arm | runs | turns | before 1st edit (median) | localize | read | edit | test | other | answer |
|---|---|---|---|---|---|---|---|---|---|---|
| issue, 5 tasks | A | 10 | 13.3 | 6.5 | 3.9 (29%) | 3.6 (27%) | 3.2 (24%) | 0.9 (7%) | 0.7 (5%) | 1.0 (8%) |
| issue, 5 tasks | C | 10 | 12.2 | 5.0 | 3.4 (28%) | 2.9 (24%) | 2.9 (23%) | 1.0 (8%) | 1.1 (9%) | 1.0 (8%) |
| hinted, 3 shared | A | 6 | 11.0 | 4.5 | 3.0 (27%) | 2.5 (23%) | 3.2 (29%) | 0.8 (8%) | 0.5 (5%) | 1.0 (9%) |
| issue, 3 shared | A | 6 | 12.2 | 5.5 | 3.0 (25%) | 3.0 (25%) | 3.3 (27%) | 1.2 (10%) | 0.7 (6%) | 1.0 (8%) |
| hinted, 3 shared | C | 6 | 9.2 | 3.5 | 1.8 (19%) | 2.1 (23%) | 2.4 (26%) | 1.1 (12%) | 0.8 (9%) | 1.0 (11%) |
| issue, 3 shared | C | 6 | 11.0 | 5.0 | 2.8 (25%) | 3.3 (30%) | 2.4 (22%) | 0.3 (3%) | 1.2 (11%) | 1.0 (9%) |

(turns here = top-level assistant messages, slightly below `num_turns`.)

Reading:
- **Rewriting the statements alone did not lengthen localization** on the
  three shared tasks: arm A spends 3.0 localize turns hinted and 3.0 issue.
  The symptom text still carries searchable words — config names
  (`model_pins.json`), output strings ("delivered nothing"), git and result
  names (`git show`, `is-ancestor`, MERGE_FAILED) — and the agent's first
  grep lands on them. The issue form added ~1 turn of reading, not of searching.
- **The new cross-module tasks are where localization grows:** arm A spends
  5.0–5.5 localize turns on owner-publish-choice and
  refinement-stays-finished, vs 2.5–3.5 on the older tasks.
- **C cut turns on the task with the most localizing** (owner-publish-choice:
  −28% turns, −22% wall, A read 13.5 files vs C 7.5) but not on
  refinement-stays-finished (+23% turns, more edit and test turns in C).
- Even here, localize is under a third of turns (29% in A): the ceiling for a
  pure locating tool on these tasks stays near 30%; reading and editing are
  as large. The wall-clock point estimate (0.81) is at the −20% target, the
  turns point (0.93) is not, and neither CI excludes 1.
- Correctness unchanged: 10/10 both arms, judge "solved" on every run.

## Full-run estimate (time)

From the clean pilot: code runs ≈ 96 s each including checks, questions ≈ 14 s.
The full set is harder than the pilot's 5 tasks (6 of 18 code tasks are
"hard"), so code runs are scaled ×1.4. One pass = 22 tasks × 1 repeat × 1 arm
≈ **40 min (30–55)** sequential.

| plan | passes | wall-clock sequential |
|---|---|---|
| A + treatment, 3 repeats | 6 | **≈ 4 h (3–5.5)** |
| A + treatment, 5 repeats | 10 | **≈ 6.7 h (5–9)** |
| + placebo arm, 3 repeats | +3 | +≈ 2 h |
| + placebo arm, 5 repeats | +5 | +≈ 3.3 h |

Runs are independent (own worktree, own daemon), so 3–4 in parallel would cut
wall-clock roughly proportionally, subject to API rate limits; the harness runs
them sequentially today. A placebo arm is not built.

Recommendation: 3 repeats. Within-task variance is small; more tasks, not
more repeats, is what narrows the CI.
## Pilot — arm C2 (interception gaps closed) vs a fresh arm A

Same 5 issue-form tasks × 2 repeats, `sonnet`, judge `opus`. Arm C2 binaries
from `feat/interception` at cb52198 (default budget without a pipe, `$(find …)`
and chained reads covered, per-identifier verdict for alternation greps):
`build_interception.sh cb52198` → `work/target-cb52198/`. A fresh arm A ran
concurrently with C2 and nothing else building, so A2 is the fair baseline
(the first issue-form A ran while the interception fork compiled).
`results/pilot-issue-{A2,A2-rerun,C2}.jsonl`, reports
`results/pilot-issue-A2C2.md` (C2 vs A2) and `results/pilot-issue-Apooled-C2.md`.

**Harness.** One arm-A run (owner-publish-choice rep 0) again got SIGTERM
1.7 s in (exit 143), right after the CLI init event; second time, both in
arm A while arm C ran concurrently; sender still unidentified. Excluded and
rerun alone (`pilot-issue-A2-rerun`).
**Harness bug fixed** (b176b22): runner-pr-base-guard C2 rep 0 was scored a
regression on `test_sync::test_diverged_umbrella_is_blocked_not_overwritten`,
which passes 4/4 full-suite runs on the same agent diff (exactly the 10
baseline failures) — a test flaky under load. A new suite failure now counts
only if it fails again on a rerun (`checks.confirm_new_failures`);
`rescore.py` re-checked the two failed C2 runs: runner → success (judge
solved), refinement-stays-finished rep 0 → still fail (real: it redefined
`refiner_egest_done` in place and left two tests asserting the old meaning
failing, never ran the suite, 7 turns). No other issue-form run had failed,
so the fix changes no other verdict.

| arm | runs | success | turns mean / median | wall s mean / median |
|---|---|---|---|---|
| A (issue pilot 1) | 10 | 10/10 | 14.1 / 14.0 | 86.6 / 92.3 |
| A2 (fresh, concurrent with C2) | 10 | 10/10 | 13.6 / 14.5 | 85.4 / 89.8 |
| C (fe2f5af) | 10 | 10/10 | 13.4 / 13.0 | 79.6 / 77.0 |
| C2 (cb52198) | 10 | 9/10 | 11.7 / 11.5 | 66.7 / 68.7 |

Paired, 5 tasks, 5000 bootstrap resamples (median of per-task ratios):
- C2/A2: turns **0.83** [0.56–1.19], wall **0.86** [0.61–0.94]; success −10 pts [−30–0]
- C2/pooled A (20 runs): turns **0.86** [0.59–1.10], wall **0.66** [0.59–1.18]
- C2/C: turns 1.00 [0.54–1.23], wall 0.84 [0.60–1.03]
- C/pooled A, for reference: turns 0.95 [0.86–1.09], wall 0.83 [0.78–1.15]

Mean turns C2 vs A2 −14%, mean wall −22%. Wall CI vs A2 is the first upper
bound below 1.0; turns CI still crosses 1. One task worse in C2:
owner-publish-choice turns 16 vs 13.5 (wall still −14%).

**Turns by phase** (`phases.py`):

| arm | runs | turns | before 1st edit | localize | read | edit | test | other | answer |
|---|---|---|---|---|---|---|---|---|---|
| A2 | 9 (+1 rerun) | 13.4 | 6 | 3.8 (28%) | 3.2 (24%) | 3.5 (26%) | 0.6 (4%) | 1.3 (10%) | 1.0 (7%) |
| C2 | 10 | 10.6 | 5 | 3.0 (28%) | 3.1 (29%) | 2.5 (23%) | 0.4 (4%) | 0.7 (7%) | 1.0 (9%) |

The cut comes from localize (−0.9), edit (−1.0) and other (−0.6); read is
flat: reading the code to change is now the largest phase in C2.

**Hooks (10 C2 runs).** 93 Bash PreToolUse decisions: 39 rewritten, 54
passed through ("contains a command graphite does not handle" 28,
"unsupported shell construct" 26). Of the 54: 35 test/python runs, 1 write
— legitimate — but **18 read-only compound commands still passed through**
(e.g. `cd x; grep -rn -i "a\|b" dir | head; cat file`, `grep -n "def f" -B3
-A30 file; sed -n a,bp f1; sed -n c,dp f2`, `grep -rniE … --include=*.py`,
segments mixing `graphite lookup`, `git ls-files`, `pwd`, `head -20 FILE`).
The pilot-C replay covered pilot C's commands; new runs use flags/segments
it does not. 91 answers + 3 enrichments, 0 fallbacks, hook latency median
226 ms per run. Answer bytes 91.3 KB vs 65.6 KB plain grep (1.39×; C:
140.8 vs 114.2 KB, 1.23×): absolute bytes fell 35%, the ratio rose because
small greps sit at the 384 B floor.

Attribution: 9 used_success; the refinement failure is flagged as a
candidate Graphite miss (`regent/core/pipeline.py` not in a graph answer),
but the agent read that file via sed/grep in turn 4 — not Graphite-caused.
