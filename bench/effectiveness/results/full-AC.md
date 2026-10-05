# Effectiveness bench report

Runs: 162 · arms: A, C · tasks: 27 · models: sonnet
Harness errors (excluded from every metric): 0

## Verdict: NO-GO

- paired tasks: 27 · bootstrap 5000 resamples of tasks, 95% CI
- turns ratio C/A: 0.82 [CI 0.77–0.92] (need point <= 0.80 and CI upper <= 1.00)
- tool turns (num_turns = tool calls + 1) ratio C/A, reference only: 0.89
- wall-clock ratio C/A: 0.95 [CI 0.81–1.13] (need point <= 0.80 and CI upper <= 1.00)
- success rate C−A: -2.5 pts [CI -11.1–6.2] (CI upper must be >= −5: C not provably worse by >5 pts)
- silent-stale Graphite answers: 0 (must be 0)
- Turns not cut enough (or not significantly) — check attribution: was Graphite used?

## Arm A totals

- runs 81, success 68/81
- median turns 12 (tool turns 12), median wall 103.0 s
- median files read 3 (Read tool 0, via Bash 3)
- median tool calls 11, median calls before first edit 7

## Arm C totals

- runs 81, success 66/81
- median turns 10 (tool turns 11), median wall 91.3 s
- median files read 1 (Read tool 0, via Bash 0)
- median tool calls 10, median calls before first edit 5

## Per task (medians over repeats)

| task | kind | diff | arm | n | turns (min–max) | wall s (min–max) | tools | search | files read | calls before edit | graphite calls / hook answers | searches after complete | success |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| core-consolidate-pin-model | refactor | medium | A | 3 | 10 (9–11) | 91.9 (56.1–93.9) | 10 | 3 | 7 | 5 | – / – | – | 1.00 |
| core-consolidate-pin-model | refactor | medium | C | 3 | 8 (7–8) | 42.2 (40.9–86.9) | 7 | 3 | 2 | 5 | 0 / 12 | 0 | 1.00 |
| dao-idempotent-publish | bugfix | hard | A | 3 | 18 (14–20) | 160.2 (150.8–172.7) | 17 | 7 | 5 | 10 | – / – | – | 0.00 |
| dao-idempotent-publish | bugfix | hard | C | 3 | 15 (14–15) | 107.3 (100.0–152.0) | 14 | 3 | 1 | 8 | 0 / 21 | – | 0.67 |
| foreman-validating-reject | feature | hard | A | 3 | 20 (14–25) | 208.6 (171.9–209.0) | 21 | 4 | 5 | 11 | – / – | – | 1.00 |
| foreman-validating-reject | feature | hard | C | 3 | 14 (11–14) | 237.6 (154.6–307.6) | 16 | 3 | 3 | 9 | 1 / 18 | 0 | 0.67 |
| instrument-branch-prefix | feature | medium | A | 3 | 9 (8–11) | 75.5 (54.3–97.0) | 8 | 4 | 5 | 5 | – / – | – | 1.00 |
| instrument-branch-prefix | feature | medium | C | 3 | 8 (8–8) | 94.8 (70.4–131.0) | 7 | 2 | 0 | 4 | 0 / 11 | 0 | 1.00 |
| marshal-resolve-by-entry | signature | hard | A | 3 | 12 (10–19) | 133.5 (94.0–213.1) | 11 | 2 | 5 | 7 | – / – | – | 1.00 |
| marshal-resolve-by-entry | signature | hard | C | 3 | 16 (15–19) | 199.4 (142.3–234.6) | 15 | 2 | 1 | 6 | 0 / 17 | 0.0 | 1.00 |
| milestone-status-ssot | refactor | medium | A | 3 | 16 (14–16) | 109.7 (103.1–135.1) | 16 | 3 | 8 | 9 | – / – | – | 1.00 |
| milestone-status-ssot | refactor | medium | C | 3 | 13 (13–17) | 84.7 (74.9–91.3) | 13 | 2 | 1 | 5 | 0 / 12 | 0 | 1.00 |
| owner-publish-choice | feature | hard | A | 3 | 12 (12–12) | 93.2 (77.7–114.1) | 11 | 4 | 8 | 8 | – / – | – | 1.00 |
| owner-publish-choice | feature | hard | C | 3 | 13 (12–14) | 107.5 (104.9–111.3) | 14 | 3 | 0 | 8 | 1 / 27 | 0 | 1.00 |
| q-check-artifacts | question | easy | A | 3 | 3 (3–3) | 16.3 (15.6–18.1) | 2 | 2 | 0 | – | – / – | – | 1.00 |
| q-check-artifacts | question | easy | C | 3 | 2 (2–2) | 14.8 (8.3–17.1) | 1 | 0 | 0 | – | 1 / 0 | 0 | 1.00 |
| q-find-project-dir | question | hard | A | 3 | 4 (4–4) | 27.5 (19.8–44.6) | 3 | 2 | 4 | – | – / – | – | 1.00 |
| q-find-project-dir | question | hard | C | 3 | 5 (5–5) | 26.9 (26.0–34.8) | 4 | 2 | 5 | – | 2 / 6 | 0 | 1.00 |
| q-resolve-owner | question | easy | A | 3 | 3 (3–3) | 13.7 (12.8–20.3) | 2 | 2 | 0 | – | – / – | – | 1.00 |
| q-resolve-owner | question | easy | C | 3 | 3 (3–3) | 14.0 (13.2–18.9) | 2 | 0 | 0 | – | 2 / 0 | 0 | 1.00 |
| q-resolve-task-yaml-path | question | medium | A | 3 | 3 (3–4) | 20.3 (16.8–25.1) | 2 | 2 | 0 | – | – / – | – | 1.00 |
| q-resolve-task-yaml-path | question | medium | C | 3 | 3 (3–4) | 48.4 (24.4–63.0) | 2 | 0 | 0 | – | 2 / 0 | 0 | 1.00 |
| refinement-stays-finished | bugfix | hard | A | 3 | 16 (14–19) | 120.5 (103.0–173.8) | 15 | 3 | 0 | 6 | – / – | – | 1.00 |
| refinement-stays-finished | bugfix | hard | C | 3 | 14 (13–15) | 109.5 (108.9–134.1) | 15 | 2 | 3 | 3 | 1 / 18 | 0.0 | 1.00 |
| regent-is-ancestor-tristate | bugfix | medium | A | 3 | 12 (10–12) | 89.8 (74.3–128.6) | 11 | 3 | 2 | 5 | – / – | – | 1.00 |
| regent-is-ancestor-tristate | bugfix | medium | C | 3 | 7 (7–9) | 81.3 (56.2–85.9) | 7 | 2 | 2 | 5 | 1 / 5 | 0 | 1.00 |
| regent-key-files-at-ref | feature | hard | A | 3 | 20 (17–20) | 187.3 (116.7–202.1) | 20 | 5 | 8 | 11 | – / – | – | 1.00 |
| regent-key-files-at-ref | feature | hard | C | 3 | 14 (13–17) | 158.0 (138.8–183.0) | 13 | 3 | 2 | 8 | 0 / 20 | 0.5 | 0.33 |
| regent-lints-every-task | refactor | medium | A | 3 | 14 (14–14) | 131.4 (126.2–135.4) | 13 | 3 | 2 | 8 | – / – | – | 1.00 |
| regent-lints-every-task | refactor | medium | C | 3 | 12 (12–15) | 94.4 (93.7–139.9) | 12 | 3 | 2 | 6 | 0 / 8 | 0 | 1.00 |
| regent-project-auto-enqueue | feature | medium | A | 3 | 13 (12–15) | 123.6 (67.2–139.5) | 13 | 3 | 8 | 6 | – / – | – | 1.00 |
| regent-project-auto-enqueue | feature | medium | C | 3 | 10 (9–11) | 92.9 (79.8–117.3) | 9 | 2 | 2 | 5 | 1 / 12 | 0 | 1.00 |
| regent-tristate-siblings | bugfix | medium | A | 3 | 12 (12–13) | 114.6 (107.3–115.6) | 12 | 4 | 4 | 6 | – / – | – | 0.33 |
| regent-tristate-siblings | bugfix | medium | C | 3 | 11 (10–12) | 77.4 (71.9–196.7) | 10 | 3 | 0 | 5 | 0 / 8 | 0.0 | 0.00 |
| rover-auth-on-push | signature | medium | A | 3 | 9 (7–9) | 63.1 (35.8–79.0) | 8 | 2 | 7 | 5 | – / – | – | 1.00 |
| rover-auth-on-push | signature | medium | C | 3 | 7 (7–8) | 51.4 (42.8–65.2) | 7 | 1 | 0 | 4 | 0 / 10 | 0.0 | 1.00 |
| rover-rebase-base-branch | signature | hard | A | 3 | 10 (8–14) | 97.1 (37.2–101.2) | 9 | 3 | 5 | 6 | – / – | – | 1.00 |
| rover-rebase-base-branch | signature | hard | C | 3 | 8 (6–8) | 37.0 (36.7–49.5) | 8 | 2 | 1 | 6 | 1 / 10 | 0 | 1.00 |
| runner-auto-merge-queued | bugfix | medium | A | 3 | 17 (15–17) | 124.5 (115.2–145.8) | 16 | 3 | 4 | 6 | – / – | – | 0.67 |
| runner-auto-merge-queued | bugfix | medium | C | 3 | 12 (12–17) | 124.3 (101.8–126.8) | 12 | 2 | 1 | 5 | 0 / 10 | 0.0 | 1.00 |
| runner-durable-attempts | feature | hard | A | 3 | 13 (11–19) | 170.8 (162.2–209.0) | 12 | 4 | 3 | 6 | – / – | – | 1.00 |
| runner-durable-attempts | feature | hard | C | 3 | 16 (15–17) | 161.6 (159.1–168.4) | 17 | 3 | 2 | 8 | 0 / 14 | 0 | 0.67 |
| runner-gh-timeouts | bugfix | medium | A | 3 | 13 (11–14) | 88.8 (86.0–123.0) | 13 | 3 | 2 | 5 | – / – | – | 0.00 |
| runner-gh-timeouts | bugfix | medium | C | 3 | 10 (10–10) | 92.8 (77.4–147.9) | 12 | 2 | 1 | 6 | 0 / 7 | 0 | 0.00 |
| runner-pr-base-guard | bugfix | medium | A | 3 | 12 (12–14) | 120.4 (118.4–139.2) | 13 | 4 | 3 | 8 | – / – | – | 1.00 |
| runner-pr-base-guard | bugfix | medium | C | 3 | 8 (8–9) | 78.2 (69.5–262.5) | 8 | 2 | 0 | 5 | 0 / 10 | – | 1.00 |
| runner-wall-clock | feature | medium | A | 3 | 12 (9–18) | 99.7 (83.0–763.6) | 12 | 4 | 3 | 8 | – / – | – | 0.67 |
| runner-wall-clock | feature | medium | C | 3 | 9 (8–13) | 159.8 (88.6–186.8) | 9 | 2 | 1 | 7 | 1 / 10 | – | 0.67 |
| sourcerer-monorepo-root | signature | easy | A | 3 | 11 (11–11) | 47.7 (46.4–70.9) | 10 | 3 | 3 | 6 | – / – | – | 0.00 |
| sourcerer-monorepo-root | signature | easy | C | 3 | 9 (7–10) | 60.2 (43.5–68.6) | 10 | 2 | 2 | 6 | 0 / 6 | 1 | 0.00 |
| tools-pin-model-default | feature | medium | A | 3 | 8 (7–10) | 54.4 (45.9–60.1) | 7 | 1 | 5 | 5 | – / – | – | 1.00 |
| tools-pin-model-default | feature | medium | C | 3 | 8 (7–9) | 61.4 (35.9–65.2) | 8 | 1 | 0 | 5 | 0 / 10 | 0 | 1.00 |
| zen-dedup-relevance | feature | hard | A | 3 | 16 (13–18) | 271.4 (139.9–394.0) | 17 | 1 | 0 | 7 | – / – | – | 1.00 |
| zen-dedup-relevance | feature | hard | C | 3 | 16 (14–25) | 458.7 (436.0–470.1) | 16 | 0 | 0 | 7 | 0 / 15 | – | 1.00 |

## Arm C Graphite use and attribution

- runs that got a complete Graphite answer: 49; of those still searched (grep/find/Grep/Glob) afterwards: 3 (median searches after: 0)
- graphite_not_used: 0
- graphite_used_success: 66
- failure_despite_graphite: 8
- graphite_caused_failure: 7
- hooks (81 runs): median graph answers 10, enrich 0, fallbacks 0, re-asks after complete 3
- hook answer bytes: median 11578 vs raw grep 9177; hook latency median 251 ms
- overlap inside one compound command (segments of `a; b`, `&&`, `||`): median duplicated path:line keys 0, bytes 0 (total 4836 B; 8 commands)
- overlap across calls (same session, ≤10 s): median duplicated keys 0 of 65, bytes 0 (total 19895 B over 81 runs)

Graphite correctness misses (candidate — review each):

| run | task | omitted file(s) |
|---|---|---|
| full-C-g0-runner-gh-timeouts-C-0-9af30b | runner-gh-timeouts | tools/orch/runner/runner/core/state.py |
| full-C-g0-runner-gh-timeouts-C-1-c112cb | runner-gh-timeouts | tools/orch/runner/runner/core/state.py |
| full-C-g0-runner-gh-timeouts-C-2-99407b | runner-gh-timeouts | tools/orch/runner/runner/core/state.py |
| full-C-g0-foreman-validating-reject-C-2-d3aded | foreman-validating-reject | tools/orch/foreman/foreman/core/hooks.py |
| full-C-g1-sourcerer-monorepo-root-C-0-469750 | sourcerer-monorepo-root | tools/flow/sourcerer/sourcerer/wt/services/completer.py, tools/flow/sourcerer/sourcerer/wt/services/navigator.py |
| full-C-g1-sourcerer-monorepo-root-C-1-763163 | sourcerer-monorepo-root | tools/flow/sourcerer/sourcerer/wt/services/completer.py, tools/flow/sourcerer/sourcerer/wt/services/navigator.py |
| full-C-g1-sourcerer-monorepo-root-C-2-2ee1db | sourcerer-monorepo-root | tools/flow/sourcerer/sourcerer/wt/services/completer.py, tools/flow/sourcerer/sourcerer/wt/services/navigator.py |


## Per-task deltas (C vs A)

| task | turns Δ% | wall Δ% | success A→C |
|---|---|---|---|
| core-consolidate-pin-model | -20 | -54 | 1.00→1.00 |
| dao-idempotent-publish | -17 | -33 | 0.00→0.67 |
| foreman-validating-reject | -30 | 14 | 1.00→0.67 |
| instrument-branch-prefix | -11 | 26 | 1.00→1.00 |
| marshal-resolve-by-entry | 33 | 49 | 1.00→1.00 |
| milestone-status-ssot | -19 | -23 | 1.00→1.00 |
| owner-publish-choice | 8 | 15 | 1.00→1.00 |
| q-check-artifacts | -33 | -9 | 1.00→1.00 |
| q-find-project-dir | 25 | -2 | 1.00→1.00 |
| q-resolve-owner | 0 | 2 | 1.00→1.00 |
| q-resolve-task-yaml-path | 0 | 138 | 1.00→1.00 |
| refinement-stays-finished | -12 | -9 | 1.00→1.00 |
| regent-is-ancestor-tristate | -42 | -9 | 1.00→1.00 |
| regent-key-files-at-ref | -30 | -16 | 1.00→0.33 |
| regent-lints-every-task | -14 | -28 | 1.00→1.00 |
| regent-project-auto-enqueue | -23 | -25 | 1.00→1.00 |
| regent-tristate-siblings | -8 | -32 | 0.33→0.00 |
| rover-auth-on-push | -22 | -19 | 1.00→1.00 |
| rover-rebase-base-branch | -20 | -62 | 1.00→1.00 |
| runner-auto-merge-queued | -29 | -0 | 0.67→1.00 |
| runner-durable-attempts | 23 | -5 | 1.00→0.67 |
| runner-gh-timeouts | -23 | 5 | 0.00→0.00 |
| runner-pr-base-guard | -33 | -35 | 1.00→1.00 |
| runner-wall-clock | -25 | 60 | 0.67→0.67 |
| sourcerer-monorepo-root | -18 | 26 | 0.00→0.00 |
| tools-pin-model-default | 0 | 13 | 1.00→1.00 |
| zen-dedup-relevance | 0 | 69 | 1.00→1.00 |

## Run details

| run | outcome | judge | regression | hidden | coverage | files read (tool/bash) | recall/precision |
|---|---|---|---|---|---|---|---|
| full-A-g0-rover-rebase-base-branch-A-0-182e2d | success | solved | True | 1.00 | 1.00 | 7 (0/7) | – |
| full-A-g0-regent-is-ancestor-tristate-A-0-5e4452 | success | solved | True | 0.66 | 1.00 | 1 (0/1) | – |
| full-A-g0-rover-auth-on-push-A-0-8366d2 | success | solved | True | 0.00 | 0.67 | 7 (0/7) | – |
| full-A-g0-runner-gh-timeouts-A-0-4faf40 | fail | partial | True | 0.88 | 0.50 | 0 (0/0) | – |
| full-A-g0-runner-wall-clock-A-0-ffa373 | success | solved | True | 0.50 | 1.00 | 1 (0/1) | – |
| full-A-g0-regent-lints-every-task-A-0-52927c | success | solved | True | 0.00 | 1.00 | 2 (1/2) | – |
| full-A-g0-regent-project-auto-enqueue-A-0-d7bafc | success | solved | True | 0.71 | 1.00 | 8 (0/8) | – |
| full-A-g0-foreman-validating-reject-A-0-210f5f | success | solved | True | 0.25 | 0.75 | 8 (0/8) | – |
| full-A-g0-q-resolve-owner-A-0-093127 | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-A-g0-rover-rebase-base-branch-A-1-71359e | success | solved | True | 1.00 | 1.00 | 0 (0/0) | – |
| full-A-g0-regent-is-ancestor-tristate-A-1-de4713 | success | solved | True | 0.87 | 1.00 | 3 (0/3) | – |
| full-A-g0-rover-auth-on-push-A-1-a09bc8 | success | solved | True | 0.00 | 1.00 | 9 (0/9) | – |
| full-A-g0-runner-gh-timeouts-A-1-d68851 | fail | partial | True | 0.88 | 0.50 | 2 (0/2) | – |
| full-A-g0-runner-wall-clock-A-1-da670e | fail | partial | True | 0.00 | 1.00 | 3 (1/3) | – |
| full-A-g0-regent-lints-every-task-A-1-0608db | success | solved | True | 0.00 | 1.00 | 2 (0/2) | – |
| full-A-g0-regent-project-auto-enqueue-A-1-b0c668 | success | solved | True | 0.71 | 1.00 | 8 (0/8) | – |
| full-A-g0-foreman-validating-reject-A-1-c80801 | success | solved | True | 0.25 | 0.50 | 5 (0/5) | – |
| full-A-g0-q-resolve-owner-A-1-5fe5aa | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-A-g0-rover-rebase-base-branch-A-2-df1529 | success | solved | True | 1.00 | 1.00 | 5 (0/5) | – |
| full-A-g0-regent-is-ancestor-tristate-A-2-2385f8 | success | solved | True | 0.87 | 1.00 | 2 (2/1) | – |
| full-A-g0-rover-auth-on-push-A-2-09ed4d | success | solved | True | 0.00 | 0.67 | 0 (0/0) | – |
| full-A-g0-runner-gh-timeouts-A-2-75d4f3 | fail | partial | True | 0.88 | 0.50 | 2 (0/2) | – |
| full-A-g0-runner-wall-clock-A-2-b3b9fb | success | solved | True | 0.50 | 1.00 | 4 (0/4) | – |
| full-A-g0-regent-lints-every-task-A-2-829f94 | success | solved | True | 0.00 | 1.00 | 4 (0/4) | – |
| full-A-g0-regent-project-auto-enqueue-A-2-b42c80 | success | solved | True | 0.71 | 1.00 | 0 (0/0) | – |
| full-A-g0-foreman-validating-reject-A-2-5481d3 | success | solved | True | 0.25 | 0.50 | 0 (0/0) | – |
| full-A-g0-q-resolve-owner-A-2-26448d | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-A-g1-marshal-resolve-by-entry-A-0-bd4278 | success | solved | True | 0.73 | 1.00 | 0 (0/0) | – |
| full-A-g1-regent-tristate-siblings-A-0-359b41 | fail | partial | True | 0.83 | 1.00 | 1 (0/1) | – |
| full-A-g1-runner-durable-attempts-A-0-71d9e4 | success | solved | True | 0.92 | 1.00 | 3 (0/3) | – |
| full-A-g1-runner-auto-merge-queued-A-0-479aaa | success | solved | True | 0.80 | 1.00 | 4 (0/4) | – |
| full-A-g1-zen-dedup-relevance-A-0-e5b3e1 | success | solved | True | 0.00 | 1.00 | 0 (0/0) | – |
| full-A-g1-sourcerer-monorepo-root-A-0-74c43c | fail | partial | True | – | 0.33 | 3 (0/3) | – |
| full-A-g1-owner-publish-choice-A-0-76e9f6 | success | solved | True | 0.85 | 0.75 | 8 (0/8) | – |
| full-A-g1-milestone-status-ssot-A-0-8acae3 | success | solved | True | 1.00 | 1.00 | 9 (0/9) | – |
| full-A-g1-q-find-project-dir-A-0-97ae7a | success | – | – | – | – | 4 (0/4) | 1.0/1.0 |
| full-A-g1-marshal-resolve-by-entry-A-1-57736e | success | solved | True | 0.73 | 1.00 | 5 (0/5) | – |
| full-A-g1-regent-tristate-siblings-A-1-9849ff | fail | partial | True | 0.89 | 1.00 | 6 (0/6) | – |
| full-A-g1-runner-durable-attempts-A-1-2ce340 | success | solved | True | 0.92 | 1.00 | 0 (0/0) | – |
| full-A-g1-runner-auto-merge-queued-A-1-04807a | success | solved | True | 0.87 | 1.00 | 4 (0/4) | – |
| full-A-g1-zen-dedup-relevance-A-1-45782b | success | solved | True | 0.00 | 0.75 | 0 (0/0) | – |
| full-A-g1-sourcerer-monorepo-root-A-1-dec768 | fail | partial | True | – | 0.33 | 2 (0/2) | – |
| full-A-g1-owner-publish-choice-A-1-7563f9 | success | solved | True | 0.97 | 0.75 | 4 (0/4) | – |
| full-A-g1-milestone-status-ssot-A-1-29a1b1 | success | solved | True | 1.00 | 1.00 | 8 (0/8) | – |
| full-A-g1-q-find-project-dir-A-1-6e6813 | success | – | – | – | – | 4 (0/4) | 1.0/1.0 |
| full-A-g1-marshal-resolve-by-entry-A-2-8e4f2e | success | solved | True | 0.73 | 1.00 | 6 (0/6) | – |
| full-A-g1-regent-tristate-siblings-A-2-8610cf | success | solved | True | 0.85 | 1.00 | 4 (0/4) | – |
| full-A-g1-runner-durable-attempts-A-2-3e72aa | success | solved | True | 0.92 | 1.00 | 3 (0/3) | – |
| full-A-g1-runner-auto-merge-queued-A-2-920177 | fail | partial | True | 0.87 | 1.00 | 4 (0/4) | – |
| full-A-g1-zen-dedup-relevance-A-2-0143a2 | success | solved | True | 0.00 | 0.75 | 0 (0/0) | – |
| full-A-g1-sourcerer-monorepo-root-A-2-0f7c4d | fail | partial | True | – | 0.33 | 3 (0/3) | – |
| full-A-g1-owner-publish-choice-A-2-21264e | success | solved | True | 0.97 | 0.75 | 10 (0/10) | – |
| full-A-g1-milestone-status-ssot-A-2-c22bcc | success | solved | True | 1.00 | 1.00 | 6 (0/6) | – |
| full-A-g1-q-find-project-dir-A-2-70fbd8 | success | – | – | – | – | 3 (0/3) | 1.0/1.0 |
| full-A-g2-regent-key-files-at-ref-A-0-38059d | success | solved | True | 0.00 | 1.00 | 2 (0/2) | – |
| full-A-g2-core-consolidate-pin-model-A-0-dc43b8 | success | solved | True | 0.91 | 1.00 | 7 (0/7) | – |
| full-A-g2-runner-pr-base-guard-A-0-3a44f4 | success | solved | True | 0.22 | 1.00 | 3 (0/3) | – |
| full-A-g2-dao-idempotent-publish-A-0-89a7b0 | fail | partial | True | 0.00 | 1.00 | 7 (0/7) | – |
| full-A-g2-instrument-branch-prefix-A-0-17d0b1 | success | solved | True | 0.91 | 1.00 | 4 (0/4) | – |
| full-A-g2-tools-pin-model-default-A-0-64f9e1 | success | solved | True | – | 1.00 | 9 (0/9) | – |
| full-A-g2-refinement-stays-finished-A-0-9a41f3 | success | solved | True | 0.92 | 0.75 | 0 (0/0) | – |
| full-A-g2-q-resolve-task-yaml-path-A-0-e341da | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-A-g2-q-check-artifacts-A-0-3ca279 | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-A-g2-regent-key-files-at-ref-A-1-605153 | success | solved | True | 0.00 | 1.00 | 9 (0/9) | – |
| full-A-g2-core-consolidate-pin-model-A-1-fd1c94 | success | solved | True | 0.91 | 1.00 | 1 (0/1) | – |
| full-A-g2-runner-pr-base-guard-A-1-0b5c67 | success | solved | True | 0.26 | 1.00 | 4 (0/4) | – |
| full-A-g2-dao-idempotent-publish-A-1-610e91 | fail | partial | True | 0.08 | 1.00 | 5 (0/5) | – |
| full-A-g2-instrument-branch-prefix-A-1-85d2e4 | success | solved | True | 0.91 | 1.00 | 5 (0/5) | – |
| full-A-g2-tools-pin-model-default-A-1-8e557c | success | solved | True | – | 1.00 | 5 (0/5) | – |
| full-A-g2-refinement-stays-finished-A-1-08576f | success | solved | True | 0.97 | 0.75 | 0 (0/0) | – |
| full-A-g2-q-resolve-task-yaml-path-A-1-b9c8ed | success | – | – | – | – | 1 (0/1) | 1.0/1.0 |
| full-A-g2-q-check-artifacts-A-1-f5c10e | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-A-g2-regent-key-files-at-ref-A-2-6043e7 | success | solved | True | 0.00 | 1.00 | 8 (0/8) | – |
| full-A-g2-core-consolidate-pin-model-A-2-374633 | success | solved | True | 0.91 | 1.00 | 8 (0/8) | – |
| full-A-g2-runner-pr-base-guard-A-2-90ab8f | success | solved | True | 0.04 | 1.00 | 3 (2/3) | – |
| full-A-g2-dao-idempotent-publish-A-2-c1c191 | fail | partial | True | 0.00 | 1.00 | 2 (0/2) | – |
| full-A-g2-instrument-branch-prefix-A-2-1b5f48 | success | solved | True | 0.91 | 1.00 | 5 (0/5) | – |
| full-A-g2-tools-pin-model-default-A-2-a98023 | success | solved | True | – | 1.00 | 1 (0/1) | – |
| full-A-g2-refinement-stays-finished-A-2-14fe1f | success | solved | True | 0.97 | 0.75 | 7 (0/7) | – |
| full-A-g2-q-resolve-task-yaml-path-A-2-5973f1 | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-A-g2-q-check-artifacts-A-2-7b6eb8 | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-C-g0-rover-rebase-base-branch-C-0-404f29 | success | solved | True | 1.00 | 1.00 | 1 (0/1) | – |
| full-C-g0-regent-is-ancestor-tristate-C-0-7cd795 | success | solved | True | 0.89 | 1.00 | 2 (2/0) | – |
| full-C-g0-rover-auth-on-push-C-0-1b599e | success | solved | True | 0.00 | 1.00 | 0 (0/0) | – |
| full-C-g0-runner-gh-timeouts-C-0-9af30b | fail | partial | True | 0.88 | 0.50 | 1 (1/0) | – |
| full-C-g0-runner-wall-clock-C-0-5f96fe | success | solved | True | 0.50 | 1.00 | 1 (0/1) | – |
| full-C-g0-regent-lints-every-task-C-0-a5f487 | success | solved | True | 0.00 | 1.00 | 2 (2/0) | – |
| full-C-g0-regent-project-auto-enqueue-C-0-ec3576 | success | solved | True | 0.57 | 1.00 | 0 (0/0) | – |
| full-C-g0-foreman-validating-reject-C-0-f726ee | success | solved | True | 0.25 | 0.75 | 5 (3/2) | – |
| full-C-g0-q-resolve-owner-C-0-6d7dc0 | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-C-g0-rover-rebase-base-branch-C-1-44166d | success | solved | True | 1.00 | 1.00 | 3 (0/3) | – |
| full-C-g0-regent-is-ancestor-tristate-C-1-c8ffe7 | success | solved | True | 0.89 | 1.00 | 2 (0/2) | – |
| full-C-g0-rover-auth-on-push-C-1-f3d14f | success | solved | True | 0.00 | 1.00 | 0 (0/0) | – |
| full-C-g0-runner-gh-timeouts-C-1-c112cb | fail | partial | True | 0.82 | 0.50 | 0 (0/0) | – |
| full-C-g0-runner-wall-clock-C-1-ff398b | success | solved | True | 0.50 | 1.00 | 0 (0/0) | – |
| full-C-g0-regent-lints-every-task-C-1-b9e969 | success | solved | True | 0.00 | 1.00 | 1 (0/1) | – |
| full-C-g0-regent-project-auto-enqueue-C-1-071b5d | success | solved | True | 0.71 | 1.00 | 2 (0/2) | – |
| full-C-g0-foreman-validating-reject-C-1-2274bd | success | solved | True | 0.25 | 0.75 | 2 (2/0) | – |
| full-C-g0-q-resolve-owner-C-1-f3256c | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-C-g0-rover-rebase-base-branch-C-2-020129 | success | solved | True | 1.00 | 1.00 | 0 (0/0) | – |
| full-C-g0-regent-is-ancestor-tristate-C-2-1086e0 | success | solved | True | 0.87 | 1.00 | 2 (2/0) | – |
| full-C-g0-rover-auth-on-push-C-2-f75304 | success | solved | True | 0.00 | 1.00 | 0 (0/0) | – |
| full-C-g0-runner-gh-timeouts-C-2-99407b | fail | partial | True | 0.88 | 0.50 | 1 (1/0) | – |
| full-C-g0-runner-wall-clock-C-2-1c868b | fail | partial | True | 0.50 | 1.00 | 1 (0/1) | – |
| full-C-g0-regent-lints-every-task-C-2-509f45 | success | solved | True | 0.00 | 1.00 | 2 (2/0) | – |
| full-C-g0-regent-project-auto-enqueue-C-2-d31689 | success | solved | True | 0.57 | 1.00 | 3 (0/3) | – |
| full-C-g0-foreman-validating-reject-C-2-d3aded | fail | partial | True | 0.25 | 0.75 | 3 (3/0) | – |
| full-C-g0-q-resolve-owner-C-2-2d60a7 | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-C-g1-marshal-resolve-by-entry-C-0-3628bc | success | solved | True | 0.73 | 1.00 | 1 (1/0) | – |
| full-C-g1-regent-tristate-siblings-C-0-b11ead | fail | partial | True | 0.83 | 1.00 | 0 (0/0) | – |
| full-C-g1-runner-durable-attempts-C-0-d54a10 | fail | partial | True | 0.92 | 0.67 | 2 (2/0) | – |
| full-C-g1-runner-auto-merge-queued-C-0-d30501 | success | solved | True | 0.87 | 1.00 | 0 (0/0) | – |
| full-C-g1-zen-dedup-relevance-C-0-f2f5cd | success | solved | True | 0.00 | 0.75 | 0 (0/0) | – |
| full-C-g1-sourcerer-monorepo-root-C-0-469750 | fail | failed | True | – | 0.33 | 3 (3/0) | – |
| full-C-g1-owner-publish-choice-C-0-908f7c | success | solved | True | 0.85 | 0.75 | 3 (0/3) | – |
| full-C-g1-milestone-status-ssot-C-0-e8a566 | success | solved | True | 1.00 | 1.00 | 1 (0/1) | – |
| full-C-g1-q-find-project-dir-C-0-9554b4 | success | – | – | – | – | 6 (0/6) | 1.0/1.0 |
| full-C-g1-marshal-resolve-by-entry-C-1-c6e600 | success | solved | True | 0.63 | 1.00 | 0 (0/0) | – |
| full-C-g1-regent-tristate-siblings-C-1-fe64b2 | fail | partial | True | 0.83 | 1.00 | 0 (0/0) | – |
| full-C-g1-runner-durable-attempts-C-1-4af558 | success | solved | True | 0.92 | 1.00 | 1 (0/1) | – |
| full-C-g1-runner-auto-merge-queued-C-1-afbabc | success | solved | True | 0.87 | 1.00 | 1 (1/0) | – |
| full-C-g1-zen-dedup-relevance-C-1-0f7b24 | success | solved | True | 0.00 | 0.75 | 0 (0/0) | – |
| full-C-g1-sourcerer-monorepo-root-C-1-763163 | fail | partial | True | – | 0.33 | 2 (2/0) | – |
| full-C-g1-owner-publish-choice-C-1-8f1d69 | success | solved | True | 0.91 | 0.75 | 0 (0/0) | – |
| full-C-g1-milestone-status-ssot-C-1-659aaa | success | solved | True | 1.00 | 1.00 | 2 (0/2) | – |
| full-C-g1-q-find-project-dir-C-1-70f7c0 | success | – | – | – | – | 5 (0/5) | 0.957/1.0 |
| full-C-g1-marshal-resolve-by-entry-C-2-b67068 | success | solved | True | 0.73 | 1.00 | 2 (0/2) | – |
| full-C-g1-regent-tristate-siblings-C-2-8d6196 | fail | partial | True | 0.85 | 1.00 | 2 (0/2) | – |
| full-C-g1-runner-durable-attempts-C-2-1a67a2 | success | solved | True | 0.92 | 1.00 | 2 (0/2) | – |
| full-C-g1-runner-auto-merge-queued-C-2-31003e | success | solved | True | 0.87 | 1.00 | 1 (1/0) | – |
| full-C-g1-zen-dedup-relevance-C-2-1a3eba | success | solved | True | 0.00 | 0.75 | 0 (0/0) | – |
| full-C-g1-sourcerer-monorepo-root-C-2-2ee1db | fail | partial | True | – | 0.33 | 1 (1/0) | – |
| full-C-g1-owner-publish-choice-C-2-7bafd3 | success | solved | True | 0.97 | 0.75 | 0 (0/0) | – |
| full-C-g1-milestone-status-ssot-C-2-76315d | success | solved | True | 1.00 | 1.00 | 1 (0/1) | – |
| full-C-g1-q-find-project-dir-C-2-4d31c3 | success | – | – | – | – | 5 (0/5) | 1.0/1.0 |
| full-C-g2-regent-key-files-at-ref-C-0-8013c3 | fail | partial | True | 0.00 | 1.00 | 3 (0/3) | – |
| full-C-g2-core-consolidate-pin-model-C-0-f2f424 | success | solved | True | 0.91 | 1.00 | 2 (0/2) | – |
| full-C-g2-runner-pr-base-guard-C-0-90c253 | success | solved | True | 0.26 | 1.00 | 0 (0/0) | – |
| full-C-g2-dao-idempotent-publish-C-0-b1125b | success | solved | True | 0.00 | 1.00 | 0 (0/0) | – |
| full-C-g2-instrument-branch-prefix-C-0-8166bd | success | solved | True | 0.95 | 1.00 | 3 (0/3) | – |
| full-C-g2-tools-pin-model-default-C-0-6357bf | success | solved | True | – | 1.00 | 0 (0/0) | – |
| full-C-g2-refinement-stays-finished-C-0-e082a4 | success | solved | True | 0.92 | 0.75 | 3 (0/3) | – |
| full-C-g2-q-resolve-task-yaml-path-C-0-246708 | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-C-g2-q-check-artifacts-C-0-004db1 | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-C-g2-regent-key-files-at-ref-C-1-25abbb | success | solved | True | 0.00 | 1.00 | 2 (0/2) | – |
| full-C-g2-core-consolidate-pin-model-C-1-e891c3 | success | solved | True | 0.91 | 1.00 | 5 (0/5) | – |
| full-C-g2-runner-pr-base-guard-C-1-0c84cb | success | solved | True | 0.26 | 1.00 | 0 (0/0) | – |
| full-C-g2-dao-idempotent-publish-C-1-c10eb4 | success | solved | True | 0.08 | 1.00 | 1 (1/1) | – |
| full-C-g2-instrument-branch-prefix-C-1-e740c7 | success | solved | True | 0.91 | 1.00 | 0 (0/0) | – |
| full-C-g2-tools-pin-model-default-C-1-c3a60e | success | solved | True | – | 1.00 | 1 (0/1) | – |
| full-C-g2-refinement-stays-finished-C-1-6f64e7 | success | solved | True | 0.92 | 0.75 | 2 (0/2) | – |
| full-C-g2-q-resolve-task-yaml-path-C-1-66958c | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-C-g2-q-check-artifacts-C-1-18580f | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-C-g2-regent-key-files-at-ref-C-2-3f4f7b | fail | partial | True | 0.00 | 1.00 | 2 (0/2) | – |
| full-C-g2-core-consolidate-pin-model-C-2-2aae6b | success | solved | True | 0.91 | 1.00 | 0 (0/0) | – |
| full-C-g2-runner-pr-base-guard-C-2-b1a051 | success | solved | True | 0.26 | 1.00 | 1 (1/0) | – |
| full-C-g2-dao-idempotent-publish-C-2-be0b75 | fail | partial | True | 0.08 | 1.00 | 3 (1/3) | – |
| full-C-g2-instrument-branch-prefix-C-2-dc8623 | success | solved | True | 0.95 | 1.00 | 0 (0/0) | – |
| full-C-g2-tools-pin-model-default-C-2-898d1d | success | solved | True | – | 1.00 | 0 (0/0) | – |
| full-C-g2-refinement-stays-finished-C-2-c37d15 | success | solved | True | 0.97 | 0.75 | 4 (4/0) | – |
| full-C-g2-q-resolve-task-yaml-path-C-2-1026c0 | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
| full-C-g2-q-check-artifacts-C-2-43e4ae | success | – | – | – | – | 0 (0/0) | 1.0/1.0 |
