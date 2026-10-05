# Effectiveness bench report

Runs: 20 · arms: A, C · tasks: 5 · models: sonnet
Harness errors (excluded from every metric): 1 — pilot-issue-A2-owner-publish-choice-A-0-505bb9: agent process killed by signal (exit 143)

## Verdict: NO-GO

- paired tasks: 5 · bootstrap 5000 resamples of tasks, 95% CI
- turns ratio C/A: 0.83 [CI 0.56–1.19] (need point <= 0.80 and CI upper <= 1.00)
- wall-clock ratio C/A: 0.86 [CI 0.61–0.94] (need point <= 0.80 and CI upper <= 1.00)
- success rate C−A: -10.0 pts [CI -30.0–0.0] (CI upper must be >= −5: C not provably worse by >5 pts)
- silent-stale Graphite answers: 0 (must be 0)
- Turns not cut enough (or not significantly) — check attribution: was Graphite used?

## Arm A totals

- runs 10, success 10/10
- median turns 14.5, median wall 89.8 s
- median files read 6.5 (Read tool 0.0, via Bash 6.5)
- median tool calls 13.5, median calls before first edit 7.0

## Arm C totals

- runs 10, success 9/10
- median turns 11.5, median wall 68.7 s
- median files read 3.0 (Read tool 0.0, via Bash 3.0)
- median tool calls 10.5, median calls before first edit 5.0

## Per task (medians over repeats)

| task | kind | diff | arm | n | turns (min–max) | wall s (min–max) | tools | search | files read | calls before edit | graphite calls / hook answers | searches after complete | success |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| core-consolidate-pin-model | refactor | medium | A | 2 | 9.5 (9–10) | 100.3 (88.1–112.5) | 8.5 | 3.5 | 8.0 | 4.5 | – / – | – | 1.00 |
| core-consolidate-pin-model | refactor | medium | C | 2 | 10.5 (10–11) | 88.8 (72.0–105.6) | 9.5 | 3.5 | 5.0 | 4.0 | 0.0 / 13.0 | 0.0 | 1.00 |
| owner-publish-choice | feature | hard | A | 2 | 13.5 (12–15) | 66.2 (42.4–89.9) | 12.5 | 5.5 | 10.0 | 8.5 | – / – | – | 1.00 |
| owner-publish-choice | feature | hard | C | 2 | 16.0 (14–18) | 56.7 (47.3–66.1) | 15.0 | 2.0 | 5.0 | 7.5 | 1.0 / 15.5 | 0.0 | 1.00 |
| refinement-stays-finished | bugfix | hard | A | 2 | 18.0 (16–20) | 94.2 (89.7–98.8) | 17.0 | 4.5 | 6.5 | 9.5 | – / – | – | 1.00 |
| refinement-stays-finished | bugfix | hard | C | 2 | 10.0 (7–13) | 57.6 (22.7–92.6) | 9.0 | 3.0 | 2.0 | 6.0 | 0.5 / 5.5 | – | 0.50 |
| regent-is-ancestor-tristate | bugfix | medium | A | 2 | 12.5 (10–15) | 74.0 (52.1–95.8) | 11.5 | 1.5 | 0.5 | 4.5 | – / – | – | 1.00 |
| regent-is-ancestor-tristate | bugfix | medium | C | 2 | 10.0 (8–12) | 69.2 (48.3–90.0) | 9.0 | 1.0 | 1.0 | 3.5 | 1.0 / 5.0 | 1 | 1.00 |
| runner-pr-base-guard | bugfix | medium | A | 2 | 14.5 (14–15) | 92.5 (85.2–99.8) | 13.5 | 3.5 | 4.0 | 7.5 | – / – | – | 1.00 |
| runner-pr-base-guard | bugfix | medium | C | 2 | 12.0 (11–13) | 61.1 (50.9–71.3) | 11.0 | 2.0 | 2.0 | 6.0 | 1.0 / 8.0 | – | 1.00 |

## Arm C Graphite use and attribution

- runs that got a complete Graphite answer: 5; of those still searched (grep/find/Grep/Glob) afterwards: 1 (median searches after: 0)
- graphite_not_used: 1
- graphite_used_success: 9
- failure_despite_graphite: 0
- graphite_caused_failure: 0
- hooks (10 runs): median graph answers 9.5, enrich 0.0, fallbacks 0.0, re-asks after complete 3
- hook answer bytes: median 9636.0 vs raw grep 7176.0; hook latency median 226.0 ms
- overlap inside one compound command (segments of `a; b`, `&&`, `||`): median duplicated path:line keys 0.0, bytes 0.0 (total 1141 B; 3 commands)
- overlap across calls (same session, ≤10 s): median duplicated keys 0.0 of 50.5, bytes 0.0 (total 4137 B over 10 runs)


## Per-task deltas (C vs A)

| task | turns Δ% | wall Δ% | success A→C |
|---|---|---|---|
| core-consolidate-pin-model | 11 | -11 | 1.00→1.00 |
| owner-publish-choice | 19 | -14 | 1.00→1.00 |
| refinement-stays-finished | -44 | -39 | 1.00→0.50 |
| regent-is-ancestor-tristate | -20 | -6 | 1.00→1.00 |
| runner-pr-base-guard | -17 | -34 | 1.00→1.00 |

## Run details

| run | outcome | judge | regression | hidden | coverage | files read (tool/bash) | recall/precision |
|---|---|---|---|---|---|---|---|
| pilot-issue-A2-runner-pr-base-guard-A-0-53ddb5 | success | solved | True | 0.26 | 1.00 | 4 (0/4) | – |
| pilot-issue-A2-core-consolidate-pin-model-A-0-3c8005 | success | solved | True | 0.91 | 1.00 | 8 (0/8) | – |
| pilot-issue-A2-regent-is-ancestor-tristate-A-0-b2fdaf | success | solved | True | 0.66 | 1.00 | 0 (0/0) | – |
| pilot-issue-A2-owner-publish-choice-A-0-505bb9 | harness_error | – | – | – | – | 0 (0/0) | – |
| pilot-issue-A2-refinement-stays-finished-A-0-b77645 | success | solved | True | 0.97 | 0.75 | 6 (0/6) | – |
| pilot-issue-A2-runner-pr-base-guard-A-1-19747c | success | solved | True | 0.26 | 1.00 | 4 (0/4) | – |
| pilot-issue-A2-core-consolidate-pin-model-A-1-ec6d6b | success | solved | True | 0.91 | 1.00 | 8 (0/8) | – |
| pilot-issue-A2-regent-is-ancestor-tristate-A-1-7f27fd | success | solved | True | 0.87 | 1.00 | 1 (0/1) | – |
| pilot-issue-A2-owner-publish-choice-A-1-1d3065 | success | solved | True | 0.97 | 0.75 | 8 (0/8) | – |
| pilot-issue-A2-refinement-stays-finished-A-1-9ceb2f | success | solved | True | 0.92 | 0.75 | 7 (0/7) | – |
| pilot-issue-A2-rerun-owner-publish-choice-A-0-c0b597 | success | solved | True | 0.85 | 0.75 | 12 (0/12) | – |
| pilot-issue-C2-runner-pr-base-guard-C-0-95e759 | success | solved | True | 0.26 | 1.00 | 2 (2/1) | – |
| pilot-issue-C2-core-consolidate-pin-model-C-0-a1fa41 | success | solved | True | 0.91 | 1.00 | 6 (0/6) | – |
| pilot-issue-C2-regent-is-ancestor-tristate-C-0-cda108 | success | solved | True | 0.66 | 1.00 | 1 (0/1) | – |
| pilot-issue-C2-owner-publish-choice-C-0-025466 | success | solved | True | 0.85 | 0.75 | 5 (0/5) | – |
| pilot-issue-C2-refinement-stays-finished-C-0-638573 | fail | solved | False | 0.92 | 0.75 | 0 (0/0) | – |
| pilot-issue-C2-runner-pr-base-guard-C-1-ee1927 | success | solved | True | 0.26 | 1.00 | 2 (0/2) | – |
| pilot-issue-C2-core-consolidate-pin-model-C-1-0027d5 | success | solved | True | 0.91 | 1.00 | 4 (0/4) | – |
| pilot-issue-C2-regent-is-ancestor-tristate-C-1-fe4310 | success | solved | True | 0.66 | 1.00 | 1 (0/1) | – |
| pilot-issue-C2-owner-publish-choice-C-1-9437a8 | success | solved | True | 0.85 | 0.75 | 5 (0/5) | – |
| pilot-issue-C2-refinement-stays-finished-C-1-a1687c | success | solved | True | 0.97 | 0.75 | 4 (0/4) | – |
