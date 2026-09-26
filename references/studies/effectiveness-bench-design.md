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
- wall-clock, API duration, cost and tokens;
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

## Task set (22 tasks)

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
python3 analyze.py results/full-A.jsonl results/full-B.jsonl -o results/report.md
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

| task | kind | turns (min–max) | wall s (min–max) | agent $ | files read | calls before 1st edit | success |
|---|---|---|---|---|---|---|---|
| core-consolidate-pin-model | refactor | 10 (10–10) | 76.7 (73.6–79.8) | 0.29 | 5.5 | 4.5 | 2/2 |
| q-resolve-owner | question | 3 (3–3) | 11.9 (11.8–12.1) | 0.17 | 0 | – | 2/2 |
| regent-is-ancestor-tristate | bugfix | 11 (11–11) | 65.8 (55.4–76.3) | 0.38 | 2 | 5 | 2/2 |
| runner-pr-base-guard | bugfix | 17 (15–19) | 95.0 (74.1–115.9) | 0.47 | 4.5 | 9 | 2/2 |
| sourcerer-monorepo-root | signature | 10.5 (9–12) | 32.5 (28.2–36.8) | 0.30 | 3 | 6 | 2/2 |

Totals: median 10.5 turns, 64.5 s, $0.31 per run; agent $3.21 + judge $0.76.
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

| task | arm | turns | wall s | agent $ | files read | searches | graphite calls | searches after complete answer | success | attribution |
|---|---|---|---|---|---|---|---|---|---|---|
| core-consolidate-pin-model | A | 10 | 77 | 0.29 | 5.5 | 3 | – | – | 2/2 | – |
| | B | 8 | 51 | 0.27 | 6.5 | 2 | 1 | 2 | 2/2 | used_success |
| q-resolve-owner | A | 3 | 12 | 0.17 | 0 | 2 | – | – | 2/2 | – |
| | B | 3.5 | 10 | 0.19 | 0 | 1.5 | 1 | 1.5 | 2/2 | used_success |
| regent-is-ancestor-tristate | A | 11 | 66 | 0.38 | 2 | 2 | – | – | 2/2 | – |
| | B | 13 | 63 | 0.39 | 2 | 3 | 0 | – | 2/2 | **not_used** |
| runner-pr-base-guard | A | 17 | 95 | 0.47 | 4.5 | 4.5 | – | – | 2/2 | – |
| | B | 11 | 64 | 0.38 | 1 | 2.5 | 1.5 | 2.5 | 2/2 | used_success |
| sourcerer-monorepo-root | A | 10.5 | 32 | 0.30 | 3 | 2 | – | – | 2/2 | – |
| | B | 11 | 41 | 0.35 | 2 | 1 | 1 | 1 | 2/2 | used_success |

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

## Full-run estimate

From the clean pilot. The pilot's 5 tasks are easier than the full set (6 of
18 code tasks are "hard"), so code runs are scaled ×1.5 (agent cost) and
×1.4 (time) from the pilot's code-run means ($0.36, 96 s incl. checks);
questions at pilot values ($0.17, 14 s). Judge ≈ $0.10 per code run.

One pass = 22 tasks × 1 repeat × 1 arm ≈ **$11 (range $8–16), ≈ 40 min
(range 30–55)** sequential.

| plan | passes | cost | wall-clock sequential |
|---|---|---|---|
| A + B, 3 repeats | 6 | **≈ $65 ($50–95)** | **≈ 4 h (3–5.5)** |
| A + B, 5 repeats | 10 | **≈ $110 ($80–160)** | **≈ 6.7 h (5–9)** |
| + placebo arm, 3 repeats | +3 | +≈ $33 | +≈ 2 h |
| + placebo arm, 5 repeats | +5 | +≈ $55 | +≈ 3.3 h |

Runs are independent (own worktree, own daemon), so 3–4 in parallel would
cut wall-clock roughly proportionally, subject to API rate limits; the
harness runs them sequentially today. A placebo arm (arm B's setup + prompt
line with a no-op tool) is not built — waiting on the user's call.

Recommendation: 3 repeats. Within-task variance is small; more tasks, not
more repeats, is what narrows the CI.