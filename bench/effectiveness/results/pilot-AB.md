# Effectiveness bench report

Runs: 20 · arms: A, B · tasks: 5 · models: sonnet
Harness errors (excluded from every metric): 0

## Verdict: NO-GO

- paired tasks: 5 · bootstrap 5000 resamples of tasks, 95% CI
- turns ratio B/A: 1.05 [CI 0.65–1.18] (need point <= 0.80 and CI upper <= 1.00)
- wall-clock ratio B/A: 0.88 [CI 0.66–1.27] (need point <= 0.80 and CI upper <= 1.00)
- success rate B−A: 0.0 pts [CI 0.0–0.0] (CI upper must be >= −5: B not provably worse by >5 pts)
- silent-stale Graphite answers: 0 (must be 0)
- Turns not cut enough (or not significantly) — check attribution: was Graphite used?

## Arm A totals

- runs 10, success 10/10
- median turns 10.5, median wall 64.5 s, median cost $0.31, total cost $3.21 (+ judge $0.76)
- median files read 2.0 (Read tool 0.5, via Bash 2.0)
- median tool calls 9.5, median calls before first edit 5.0

## Arm B totals

- runs 10, success 10/10
- median turns 10.5, median wall 50.6 s, median cost $0.35, total cost $3.17 (+ judge $0.72)
- median files read 2.0 (Read tool 1.0, via Bash 1.0)
- median tool calls 9.5, median calls before first edit 5.5

## Per task (medians over repeats)

| task | kind | diff | arm | n | turns (min–max) | wall s (min–max) | cost $ | tools | search | files read | calls before edit | success |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| core-consolidate-pin-model | refactor | medium | A | 2 | 10.0 (10–10) | 76.7 (73.6–79.8) | 0.29 | 9.0 | 3.0 | 5.5 | 4.5 | 1.00 |
| core-consolidate-pin-model | refactor | medium | B | 2 | 8.0 (7–9) | 50.8 (27.6–74.1) | 0.27 | 7.0 | 2.0 | 6.5 | 3.5 | 1.00 |
| q-resolve-owner | question | easy | A | 2 | 3.0 (3–3) | 11.9 (11.8–12.1) | 0.17 | 2.0 | 2.0 | 0.0 | – | 1.00 |
| q-resolve-owner | question | easy | B | 2 | 3.5 (3–4) | 10.5 (10.2–10.8) | 0.19 | 2.5 | 1.5 | 0.0 | – | 1.00 |
| regent-is-ancestor-tristate | bugfix | medium | A | 2 | 11.0 (11–11) | 65.8 (55.4–76.3) | 0.38 | 10.0 | 2.0 | 2.0 | 5.0 | 1.00 |
| regent-is-ancestor-tristate | bugfix | medium | B | 2 | 13.0 (11–15) | 62.8 (60.5–65.1) | 0.39 | 12.0 | 3.0 | 2.0 | 5.0 | 1.00 |
| runner-pr-base-guard | bugfix | medium | A | 2 | 17.0 (15–19) | 95.0 (74.1–115.9) | 0.47 | 16.0 | 4.5 | 4.5 | 9.0 | 1.00 |
| runner-pr-base-guard | bugfix | medium | B | 2 | 11.0 (11–11) | 63.7 (56.4–71.0) | 0.38 | 10.0 | 2.5 | 1.0 | 6.5 | 1.00 |
| sourcerer-monorepo-root | signature | easy | A | 2 | 10.5 (9–12) | 32.5 (28.2–36.8) | 0.30 | 9.5 | 2.0 | 3.0 | 6.0 | 1.00 |
| sourcerer-monorepo-root | signature | easy | B | 2 | 11.0 (10–12) | 41.2 (37.6–44.8) | 0.35 | 10.0 | 1.0 | 2.0 | 5.5 | 1.00 |

## Arm B failure attribution

- runs that got a complete Graphite answer: 8; of those still searched (grep/find/Grep/Glob) afterwards: 8 (median searches after: 2.0)
- graphite_not_used: 2
- graphite_used_success: 8
- failure_despite_graphite: 0
- graphite_caused_failure: 0


## Per-task deltas (B vs A)

| task | turns Δ% | wall Δ% | success A→B |
|---|---|---|---|
| core-consolidate-pin-model | -20 | -34 | 1.00→1.00 |
| q-resolve-owner | 17 | -12 | 1.00→1.00 |
| regent-is-ancestor-tristate | 18 | -5 | 1.00→1.00 |
| runner-pr-base-guard | -35 | -33 | 1.00→1.00 |
| sourcerer-monorepo-root | 5 | 27 | 1.00→1.00 |

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
| pilot-B-regent-is-ancestor-tristate-B-0-6dd3cf | success | solved | True | 0.66 | 1.00 | 2 (1/1) | – |
| pilot-B-sourcerer-monorepo-root-B-0-9de53f | success | solved | True | – | 1.00 | 2 (2/0) | – |
| pilot-B-runner-pr-base-guard-B-0-2fb3cb | success | solved | True | 0.22 | 1.00 | 1 (1/1) | – |
| pilot-B-core-consolidate-pin-model-B-0-a80928 | success | solved | True | 1.00 | 1.00 | 7 (0/7) | – |
| pilot-B-q-resolve-owner-B-0-7edeed | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| pilot-B-regent-is-ancestor-tristate-B-1-df8307 | success | solved | True | 0.66 | 1.00 | 2 (2/1) | – |
| pilot-B-sourcerer-monorepo-root-B-1-d3e854 | success | solved | True | – | 1.00 | 2 (2/1) | – |
| pilot-B-runner-pr-base-guard-B-1-2ee69f | success | solved | True | 0.26 | 1.00 | 1 (1/1) | – |
| pilot-B-core-consolidate-pin-model-B-1-3d9bb3 | success | solved | True | 1.00 | 1.00 | 6 (0/6) | – |
| pilot-B-q-resolve-owner-B-1-734b38 | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
