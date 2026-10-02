# Effectiveness bench report

Runs: 20 · arms: A, C · tasks: 5 · models: sonnet
Harness errors (excluded from every metric): 1 — pilot-issue-A-regent-is-ancestor-tristate-A-0-10b4e5: agent process killed by signal (exit 143)

## Verdict: NO-GO

- paired tasks: 5 · bootstrap 5000 resamples of tasks, 95% CI
- turns ratio C/A: 0.93 [CI 0.72–1.23] (need point <= 0.80 and CI upper <= 1.00)
- wall-clock ratio C/A: 0.81 [CI 0.73–1.73] (need point <= 0.80 and CI upper <= 1.00)
- success rate C−A: 0.0 pts [CI 0.0–0.0] (CI upper must be >= −5: C not provably worse by >5 pts)
- silent-stale Graphite answers: 0 (must be 0)
- Turns not cut enough (or not significantly) — check attribution: was Graphite used?

## Arm A totals

- runs 10, success 10/10
- median turns 14.0, median wall 92.3 s
- median files read 3.5 (Read tool 0.0, via Bash 3.5)
- median tool calls 13.0, median calls before first edit 7.0

## Arm C totals

- runs 10, success 10/10
- median turns 13.0, median wall 77.0 s
- median files read 2.5 (Read tool 0.0, via Bash 2.5)
- median tool calls 12.0, median calls before first edit 5.5

## Per task (medians over repeats)

| task | kind | diff | arm | n | turns (min–max) | wall s (min–max) | tools | search | files read | calls before edit | graphite calls / hook answers | searches after complete | success |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| core-consolidate-pin-model | refactor | medium | A | 2 | 10.5 (10–11) | 49.9 (37.7–62.0) | 9.5 | 2.5 | 4.0 | 4.0 | – / – | – | 1.00 |
| core-consolidate-pin-model | refactor | medium | C | 2 | 9.5 (7–12) | 86.4 (78.1–94.8) | 8.5 | 2.5 | 6.5 | 4.5 | 0.5 / 5.5 | 0.0 | 1.00 |
| owner-publish-choice | feature | hard | A | 2 | 18.0 (14–22) | 94.2 (87.1–101.2) | 17.0 | 4.0 | 13.5 | 10.0 | – / – | 0 | 1.00 |
| owner-publish-choice | feature | hard | C | 2 | 13.0 (12–14) | 73.4 (72.5–74.3) | 12.0 | 3.0 | 7.5 | 7.5 | 1.0 / 11.5 | 0.5 | 1.00 |
| refinement-stays-finished | bugfix | hard | A | 2 | 15.0 (12–18) | 103.2 (97.5–109.0) | 14.0 | 5.0 | 3.0 | 7.5 | – / – | – | 1.00 |
| refinement-stays-finished | bugfix | hard | C | 2 | 18.5 (16–21) | 95.7 (90.7–100.7) | 17.5 | 3.0 | 0.5 | 5.5 | 1.0 / 12.5 | 0.0 | 1.00 |
| regent-is-ancestor-tristate | bugfix | medium | A | 2 | 15.0 (14–16) | 96.2 (83.4–108.9) | 14.0 | 3.5 | 1.5 | 6.5 | – / – | – | 1.00 |
| regent-is-ancestor-tristate | bugfix | medium | C | 2 | 14.0 (11–17) | 70.0 (63.9–76.0) | 13.0 | 3.0 | 2.0 | 6.5 | 0.5 / 11.5 | – | 1.00 |
| runner-pr-base-guard | bugfix | medium | A | 2 | 12.0 (10–14) | 89.7 (72.8–106.5) | 11.0 | 3.0 | 3.5 | 6.5 | – / – | – | 1.00 |
| runner-pr-base-guard | bugfix | medium | C | 2 | 12.0 (10–14) | 72.6 (57.7–87.5) | 11.0 | 1.5 | 2.5 | 6.5 | 0.0 / 12.0 | 0.0 | 1.00 |

## Arm C Graphite use and attribution

- runs that got a complete Graphite answer: 8; of those still searched (grep/find/Grep/Glob) afterwards: 1 (median searches after: 0.0)
- graphite_not_used: 0
- graphite_used_success: 10
- failure_despite_graphite: 0
- graphite_caused_failure: 0
- hooks (10 runs): median graph answers 10.0, enrich 0.0, fallbacks 0.0, re-asks after complete 2.5
- hook answer bytes: median 13286.5 vs raw grep 12176.5; hook latency median 187.0 ms
- overlap inside one compound command (segments of `a; b`, `&&`, `||`): median duplicated path:line keys 0.0, bytes 0.0 (total 0 B; 0 commands)
- overlap across calls (same session, ≤10 s): median duplicated keys 0.0 of 52.5, bytes 0.0 (total 3000 B over 10 runs)


## Per-task deltas (C vs A)

| task | turns Δ% | wall Δ% | success A→C |
|---|---|---|---|
| core-consolidate-pin-model | -10 | 73 | 1.00→1.00 |
| owner-publish-choice | -28 | -22 | 1.00→1.00 |
| refinement-stays-finished | 23 | -7 | 1.00→1.00 |
| regent-is-ancestor-tristate | -7 | -27 | 1.00→1.00 |
| runner-pr-base-guard | 0 | -19 | 1.00→1.00 |

## Run details

| run | outcome | judge | regression | hidden | coverage | files read (tool/bash) | recall/precision |
|---|---|---|---|---|---|---|---|
| pilot-issue-A-runner-pr-base-guard-A-0-5da96f | success | solved | True | 0.26 | 1.00 | 3 (0/3) | – |
| pilot-issue-A-core-consolidate-pin-model-A-0-4f1f3c | success | solved | True | 0.68 | 1.00 | 8 (0/8) | – |
| pilot-issue-A-regent-is-ancestor-tristate-A-0-10b4e5 | harness_error | – | – | – | – | 0 (0/0) | – |
| pilot-issue-A-owner-publish-choice-A-0-1eb3f0 | success | solved | True | 0.97 | 0.75 | 14 (0/14) | – |
| pilot-issue-A-refinement-stays-finished-A-0-2f90ca | success | solved | True | 0.97 | 0.75 | 6 (0/6) | – |
| pilot-issue-A-runner-pr-base-guard-A-1-58fe75 | success | solved | True | 0.26 | 1.00 | 4 (0/4) | – |
| pilot-issue-A-core-consolidate-pin-model-A-1-8bda91 | success | solved | True | 0.91 | 1.00 | 0 (0/0) | – |
| pilot-issue-A-regent-is-ancestor-tristate-A-1-3c44ac | success | solved | True | 0.87 | 1.00 | 2 (1/2) | – |
| pilot-issue-A-owner-publish-choice-A-1-95450d | success | solved | True | 0.85 | 0.75 | 13 (0/13) | – |
| pilot-issue-A-refinement-stays-finished-A-1-156e70 | success | solved | True | 0.92 | 0.75 | 0 (0/0) | – |
| pilot-issue-A-rerun-regent-is-ancestor-tristate-A-0-0bd8ee | success | solved | True | 0.68 | 1.00 | 1 (1/1) | – |
| pilot-issue-C-runner-pr-base-guard-C-0-c4e958 | success | solved | True | 0.26 | 1.00 | 2 (0/2) | – |
| pilot-issue-C-core-consolidate-pin-model-C-0-767a95 | success | solved | True | 0.91 | 1.00 | 7 (0/7) | – |
| pilot-issue-C-regent-is-ancestor-tristate-C-0-931cc1 | success | solved | True | 0.89 | 1.00 | 2 (0/2) | – |
| pilot-issue-C-owner-publish-choice-C-0-a4e61d | success | solved | True | 0.97 | 0.75 | 8 (0/8) | – |
| pilot-issue-C-refinement-stays-finished-C-0-a0822a | success | solved | True | 0.92 | 0.75 | 0 (0/0) | – |
| pilot-issue-C-runner-pr-base-guard-C-1-903493 | success | solved | True | 0.26 | 1.00 | 3 (0/3) | – |
| pilot-issue-C-core-consolidate-pin-model-C-1-c2ce8d | success | solved | True | 0.91 | 1.00 | 6 (0/6) | – |
| pilot-issue-C-regent-is-ancestor-tristate-C-1-bb1203 | success | solved | True | 0.87 | 1.00 | 2 (2/1) | – |
| pilot-issue-C-owner-publish-choice-C-1-a526fd | success | solved | True | 0.85 | 0.75 | 7 (0/7) | – |
| pilot-issue-C-refinement-stays-finished-C-1-f227da | success | solved | True | 0.97 | 0.75 | 1 (0/1) | – |
