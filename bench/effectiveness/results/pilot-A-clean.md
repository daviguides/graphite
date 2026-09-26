# Effectiveness bench report

Runs: 10 · arms: A · tasks: 5 · models: sonnet
Harness errors (excluded from every metric): 0

## Verdict: NO VERDICT — arm B has not run yet (baseline only).


## Arm A totals

- runs 10, success 10/10
- median turns 10.5, median wall 64.5 s, median cost $0.31, total cost $3.21 (+ judge $0.76)
- median files read 2.0 (Read tool 0.5, via Bash 2.0)
- median tool calls 9.5, median calls before first edit 5.0

## Per task (medians over repeats)

| task | kind | diff | arm | n | turns (min–max) | wall s (min–max) | cost $ | tools | search | files read | calls before edit | success |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| core-consolidate-pin-model | refactor | medium | A | 2 | 10.0 (10–10) | 76.7 (73.6–79.8) | 0.29 | 9.0 | 3.0 | 5.5 | 4.5 | 1.00 |
| q-resolve-owner | question | easy | A | 2 | 3.0 (3–3) | 11.9 (11.8–12.1) | 0.17 | 2.0 | 2.0 | 0.0 | – | 1.00 |
| regent-is-ancestor-tristate | bugfix | medium | A | 2 | 11.0 (11–11) | 65.8 (55.4–76.3) | 0.38 | 10.0 | 2.0 | 2.0 | 5.0 | 1.00 |
| runner-pr-base-guard | bugfix | medium | A | 2 | 17.0 (15–19) | 95.0 (74.1–115.9) | 0.47 | 16.0 | 4.5 | 4.5 | 9.0 | 1.00 |
| sourcerer-monorepo-root | signature | easy | A | 2 | 10.5 (9–12) | 32.5 (28.2–36.8) | 0.30 | 9.5 | 2.0 | 3.0 | 6.0 | 1.00 |


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
