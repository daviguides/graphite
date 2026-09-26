# Effectiveness bench report

Runs: 20 · arms: A, C · tasks: 5 · models: sonnet
Harness errors (excluded from every metric): 0

## Verdict: NO-GO

- paired tasks: 5 · bootstrap 5000 resamples of tasks, 95% CI
- turns ratio C/A: 1.00 [CI 0.71–1.09] (need point <= 0.80 and CI upper <= 1.00)
- wall-clock ratio C/A: 0.97 [CI 0.69–1.28] (need point <= 0.80 and CI upper <= 1.00)
- success rate C−A: 0.0 pts [CI 0.0–0.0] (CI upper must be >= −5: C not provably worse by >5 pts)
- silent-stale Graphite answers: 0 (must be 0)
- Turns not cut enough (or not significantly) — check attribution: was Graphite used?

## Arm A totals

- runs 10, success 10/10
- median turns 10.5, median wall 64.5 s
- median files read 2.0 (Read tool 0.5, via Bash 2.0)
- median tool calls 9.5, median calls before first edit 5.0

## Arm C totals

- runs 10, success 10/10
- median turns 9.5, median wall 51.0 s
- median files read 2.5 (Read tool 1.0, via Bash 0.5)
- median tool calls 8.5, median calls before first edit 6.0

## Per task (medians over repeats)

| task | kind | diff | arm | n | turns (min–max) | wall s (min–max) | tools | search | files read | calls before edit | graphite calls / hook answers | searches after complete | success |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| core-consolidate-pin-model | refactor | medium | A | 2 | 10.0 (10–10) | 76.7 (73.6–79.8) | 9.0 | 3.0 | 5.5 | 4.5 | – / – | – | 1.00 |
| core-consolidate-pin-model | refactor | medium | C | 2 | 8.5 (8–9) | 53.2 (28.8–77.7) | 7.5 | 1.0 | 6.0 | 3.5 | 1.0 / 4.0 | 2 | 1.00 |
| q-resolve-owner | question | easy | A | 2 | 3.0 (3–3) | 11.9 (11.8–12.1) | 2.0 | 2.0 | 0.0 | – | – / – | – | 1.00 |
| q-resolve-owner | question | easy | C | 2 | 3.0 (3–3) | 11.6 (11.5–11.7) | 2.0 | 0.0 | 0.0 | – | 2.0 / 0.0 | 0.0 | 1.00 |
| regent-is-ancestor-tristate | bugfix | medium | A | 2 | 11.0 (11–11) | 65.8 (55.4–76.3) | 10.0 | 2.0 | 2.0 | 5.0 | – / – | – | 1.00 |
| regent-is-ancestor-tristate | bugfix | medium | C | 2 | 12.0 (9–15) | 79.3 (59.9–98.8) | 11.0 | 2.0 | 1.0 | 5.0 | 0.0 / 7.0 | – | 1.00 |
| runner-pr-base-guard | bugfix | medium | A | 2 | 17.0 (15–19) | 95.0 (74.1–115.9) | 16.0 | 4.5 | 4.5 | 9.0 | – / – | – | 1.00 |
| runner-pr-base-guard | bugfix | medium | C | 2 | 12.0 (10–14) | 69.0 (56.5–81.4) | 11.0 | 0.5 | 2.5 | 6.0 | 1.0 / 6.0 | 0.5 | 1.00 |
| sourcerer-monorepo-root | signature | easy | A | 2 | 10.5 (9–12) | 32.5 (28.2–36.8) | 9.5 | 2.0 | 3.0 | 6.0 | – / – | – | 1.00 |
| sourcerer-monorepo-root | signature | easy | C | 2 | 10.5 (10–11) | 41.5 (37.4–45.5) | 9.5 | 0.0 | 4.0 | 6.0 | 1.0 / 8.0 | 0.0 | 1.00 |

## Arm C Graphite use and attribution

- runs that got a complete Graphite answer: 7; of those still searched (grep/find/Grep/Glob) afterwards: 2 (median searches after: 0)
- graphite_not_used: 0
- graphite_used_success: 10
- failure_despite_graphite: 0
- graphite_caused_failure: 0
- hooks (10 runs): median graph answers 2.5, enrich 2.0, fallbacks 0.0, re-asks after complete 0
- hook answer bytes: median 5986.0 vs raw grep 2807.5; hook latency median 27.5 ms
- overlap inside one compound command (segments of `a; b`, `&&`, `||`): median duplicated path:line keys 0.0, bytes 0.0 (total 0 B; 0 commands)
- overlap across calls (same session, ≤10 s): median duplicated keys 0.0 of 3.5, bytes 0.0 (total 600 B over 10 runs)


## Per-task deltas (C vs A)

| task | turns Δ% | wall Δ% | success A→C |
|---|---|---|---|
| core-consolidate-pin-model | -15 | -31 | 1.00→1.00 |
| q-resolve-owner | 0 | -3 | 1.00→1.00 |
| regent-is-ancestor-tristate | 9 | 21 | 1.00→1.00 |
| runner-pr-base-guard | -29 | -27 | 1.00→1.00 |
| sourcerer-monorepo-root | 0 | 28 | 1.00→1.00 |

## Run details

| run | outcome | judge | regression | hidden | coverage | files read (tool/bash) | recall/precision |
|---|---|---|---|---|---|---|---|
| pilot-A-clean-regent-is-ancestor-tristate-A-0-7bcff9 | success | solved | True | 0.89 | 1.00 | 2 (2/0) | – |
| pilot-A-clean-sourcerer-monorepo-root-A-0-e695ce | success | solved | True | – | 1.00 | 4 (4/0) | – |
| pilot-A-clean-runner-pr-base-guard-A-0-8a6dab | success | solved | True | 0.26 | 1.00 | 4 (2/3) | – |
| pilot-A-clean-core-consolidate-pin-model-A-0-6a35dd | success | solved | True | 1.00 | 1.00 | 2 (0/2) | – |
| pilot-A-clean-q-resolve-owner-A-0-a9271f | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| pilot-A-clean-regent-is-ancestor-tristate-A-1-67a3e8 | success | solved | True | 0.87 | 1.00 | 2 (1/2) | – |
| pilot-A-clean-sourcerer-monorepo-root-A-1-1791bb | success | solved | True | – | 1.00 | 2 (0/2) | – |
| pilot-A-clean-runner-pr-base-guard-A-1-885e68 | success | solved | True | 0.26 | 1.00 | 5 (3/3) | – |
| pilot-A-clean-core-consolidate-pin-model-A-1-6ab5b4 | success | solved | True | 1.00 | 1.00 | 9 (0/9) | – |
| pilot-A-clean-q-resolve-owner-A-1-861749 | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| pilot-C-regent-is-ancestor-tristate-C-0-807d7e | success | solved | True | 0.66 | 1.00 | 2 (2/0) | – |
| pilot-C-sourcerer-monorepo-root-C-0-60a2e7 | success | solved | True | – | 1.00 | 4 (2/2) | – |
| pilot-C-runner-pr-base-guard-C-0-a2037b | success | solved | True | 0.26 | 1.00 | 3 (2/1) | – |
| pilot-C-core-consolidate-pin-model-C-0-7fa61e | success | solved | True | 1.00 | 1.00 | 6 (0/6) | – |
| pilot-C-q-resolve-owner-C-0-9baf12 | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| pilot-C-regent-is-ancestor-tristate-C-1-951522 | success | solved | True | 0.87 | 1.00 | 0 (0/0) | – |
| pilot-C-sourcerer-monorepo-root-C-1-d455f2 | success | solved | True | – | 1.00 | 4 (2/2) | – |
| pilot-C-runner-pr-base-guard-C-1-4684ab | success | solved | True | 0.22 | 1.00 | 2 (2/0) | – |
| pilot-C-core-consolidate-pin-model-C-1-62d95f | success | solved | True | 1.00 | 1.00 | 6 (0/6) | – |
| pilot-C-q-resolve-owner-C-1-ff2b62 | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
