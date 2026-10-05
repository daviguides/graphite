---
title: Benchmarks
order: 4
---

Real measurements on real code. No synthetic demos, no cherry-picked examples.

## Effectiveness bench

Measured on a production multi-language repo (Python, TypeScript, Rust; 50K+ lines). Same model (Sonnet), same tasks, multiple repeats per arm. Tasks are real bug fixes and features from project history, written as issue-form prompts without file paths or function names.

### Arms

- **A** (baseline): no Graphite, standard Claude Code
- **B** (CLI + prompt line): Graphite CLI available, mentioned in system prompt
- **C** (interception hooks): enriched grep + transparent hook rewriting

### Results (Pilot C2 vs A2, issue-form tasks)

| Metric | Without Graphite | With Graphite | Change |
|--------|-----------------|---------------|--------|
| Median turns | 14.5 | 11.5 | -21% |
| Median wall-clock | 89.8s | 68.7s | -23% |
| Files read | 6.5 | 3.0 | -54% |
| Calls before first edit | 7.0 | 5.0 | -29% |

Hook latency: 27ms median. Zero silent-stale answers observed.

### Per-task breakdown

| Task | Difficulty | Turns change | Wall-clock change |
|------|-----------|-------------|-------------------|
| refinement-stays-finished | hard | -44% | -39% |
| runner-pr-base-guard | medium | -17% | -34% |
| regent-is-ancestor-tristate | medium | -20% | -6% |
| owner-publish-choice | hard | +19% | -14% |
| core-consolidate-pin-model | medium | +11% | -11% |

Wall-clock improved on every task. Turns improved on 3 of 5.

Correctness: 10/10 without Graphite, 9/10 with. The one failure changed a function in place and never ran the test suite; its only Graphite call looked up two names that are not symbols and got nothing back. A blast query on the function it changed lists the caller it missed and the covering tests, two of which the change broke. Zero failures were caused by a Graphite answer.

### Adoption

Arm B (CLI available but not intercepted) showed the agent often didn't use Graphite, or grepped after receiving a complete answer. Arm C (interception hooks) closes this gap: the agent gets graph context transparently through its existing `grep`/`find` commands.

## Storage benchmark

Compared CozoDB, LadybugDB (Kuzu fork), and DIY (redb + Ascent) on synthetic code-shaped graphs (5K to 200K symbols).

### Key results

| Engine | Traversal (hub, depth 10) | Per-file write | Crashes |
|--------|--------------------------|---------------|---------|
| CozoDB (RocksDB) | 3–13 ms | ~0.2 ms | 0 |
| LadybugDB | 1–5 ms | 5–23 ms | 2 (SIGSEGV/SIGBUS) |
| DIY (redb + BFS) | 19–80 µs | ~0.1 ms | 0 |

Graphite uses the hybrid: CozoDB for facts/rules/search + in-memory adjacency (matching DIY speeds) for traversals.

Full benchmark source and runner in [`bench/`](https://github.com/daviguides/graphite/tree/main/bench).
