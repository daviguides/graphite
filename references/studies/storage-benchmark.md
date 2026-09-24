# Storage benchmark: CozoDB (mnestic) vs LadybugDB vs Ascent+redb

> Run 2026-09-23. Code: `bench/` (rerun: `bench/README.md`). Raw results: `bench/results/*.jsonl`.
> Real graphs come from `bench/real-graph` (see `real-graph-shape.md`). Synthetic graphs are kept only as stress context.

## Verdict

**DIY wins under the decision rule**: persist per-file facts in redb, keep the graph as an in-memory adjacency owned by the daemon, and run blast radius as a BFS in Rust.

- **Speed:** depth-10 blast radius on the worst real hub takes **19–80 µs**, against a 10 ms target. The p99 symbol takes ≤ 5 µs and the median ≈ 1 µs.
- **Correctness:** it is correct against the reference, and incremental equals a full rebuild on every dataset.
- **Reads during writes:** reads never slow down while the watcher writes (p99 ≤ 1.4 µs).
- **Stress:** on the synthetic stress graph (50K symbols / 240K edges, hub reaches 99.8% of the graph) it still answers in 1.3 ms.
- **Maintenance:** every dependency is maintained pure Rust (redb, plus Ascent if a rule engine is wanted).
- **What we own:** the graph algorithms. SCC and PageRank are trivial and were measured here. Louvain must be written, about 200 lines.

The runners-up:

**CozoDB (mnestic)** is the best off-the-shelf engine if Datalog or the built-in algorithms are wanted, with conditions:
- Use it with the RocksDB backend (`newrocksdb`) and Rust-driven BFS.
- It meets the target on the extractor's default ("strict") real graphs: worst hub 3–4 ms.
- It misses on the extractor's permissive mode (unbounded name guesses): 13.3 ms on the sensemesh-permissive hub.
- It misses badly on synthetic stress: 131 ms.
- Louvain costs 1–4.7 s on the real graphs and pushes peak RSS to 1.4–5 GB.
- It should not sit in the hot query path.

**LadybugDB is not recommended for v1:**
- It meets the target on real graphs (worst hub 7.7–8 ms with `* SHORTEST`), but every query pays a ~0.7–1 ms planning floor.
- Per-file writes cost 7–23 ms at p50 and up to 50 ms at p95 under concurrent reads.
- Its graph algorithms (ALGO extension) cannot be loaded on macOS arm64.
- **A Cypher variable-length query that hits the query timeout segfaults the whole process** (reproduced twice).

## Decision rule applied

Rule: choose the fastest candidate that meets **depth-10 blast radius < 10 ms** and passes **incremental == full rebuild**. Maintenance risk breaks ties. Judged on the real graphs; synthetic data is stress context only.

| candidate (best strategy) | worst real hub d10 | < 10 ms on all real graphs | incr == full | reads blocked by writes | algos available | maintenance |
|---|---|---|---|---|---|---|
| **DIY redb + in-memory adjacency (`rust_bfs`)** | **80 µs** | ✅ | ✅ | no | SCC, PageRank (own code); Louvain to write | redb/Ascent active, pure Rust; we own the graph code |
| CozoDB mnestic `mem` (`rust_bfs`) | 8.6 ms | ✅ (not persistent) | ✅ | mild (p99 0.45 ms) | all built in | single-maintainer fork, 36K-LOC engine |
| LadybugDB `mem` (`cypher_shortest`) | 8.0 ms | ✅ (not persistent) | ✅ | no | none on macOS | active; C++ engine; segfault on timeout |
| LadybugDB `disk` (`cypher_shortest`) | 7.7 ms | ✅ | ✅ | no | none on macOS | same |
| CozoDB mnestic `newrocksdb` (`rust_bfs`) | 13.3 ms | ❌ (permissive hub only) | ✅ | no | all built in | single-maintainer fork |
| CozoDB mnestic `sqlite` (`rust_bfs`) | 27.1 ms | ❌ | ✅ | **yes** (p99 2.8 ms vs 0.12 ms idle) | all built in | same |

## Primary results: real code graphs

Four graphs exported by `bench/real-graph`:
- Continuum: 12.4K symbols / 28–29K edges.
- sensemesh: 23.7K symbols / 76–81K edges.

Each comes in a *strict* version (the extractor's default resolver, the realistic one) and a *permissive* version (unbounded name guesses, an upper bound on edges).

The targets are picked by depth-10 reach:

| graph | worst hub | p99 symbol | median symbol |
|---|---|---|---|
| continuum-strict | 1,792 | 192 | 3 |
| sensemesh-strict | 1,796 | 322 | 12 |
| continuum-permissive | 3,695 | 213 | 3 |
| sensemesh-permissive | 6,433 | 369 | 12 |

### Summary: best strategy per engine, worst case across the four real graphs

| engine | d10 median | d10 p99 | d10 worst hub | per-file update p50 / p95 | read p99 while writer runs | peak RSS |
|---|---|---|---|---|---|---|
| diy-ascent-redb (rust_bfs) | ≤ 1.8 µs | ≤ 5.0 µs | **80 µs** | 3.0–4.0 ms / 4.1 ms ¹ | ≤ 1.2 µs | 121–153 MB |
| cozo-mnestic-mem (rust_bfs) | 47–83 µs | ≤ 716 µs | 8.6 ms | 88–108 µs / ≤ 343 µs | ≤ 894 µs | 1.5–5.2 GB ² |
| cozo-mnestic-newrocksdb (rust_bfs) | 66–103 µs | ≤ 1.2 ms | 13.3 ms | 162–196 µs / ≤ 962 µs | ≤ 782 µs | 1.6–5.3 GB ² |
| cozo-mnestic-sqlite (rust_bfs) | 99–154 µs | ≤ 1.9 ms | 27.1 ms | 521–607 µs / ≤ 2.1 ms | ≤ 3.0 ms ³ | 1.5–5.1 GB ² |
| lbug-disk (cypher_shortest) | 0.8–1.1 ms | ≤ 1.3 ms | 7.7 ms | 8.9–10.1 ms / ≤ 32 ms | ≤ 1.5 ms | 0.25–2.1 GB |
| lbug-mem (cypher_shortest) | 0.8–1.1 ms | ≤ 1.4 ms | 8.0 ms | 5.4–6.8 ms / ≤ 28 ms | ≤ 1.5 ms | 0.2–2.1 GB |

1. redb commits with `Durability::Immediate` (fsync per commit). CozoDB and LadybugDB were run with their default sync settings. The write columns are therefore not apples-to-apples: they measure durable writes for redb and possibly non-durable writes for the others.
2. The peak is set by the Louvain run: 1.0–4.7 s and several GB on 12–24K-node graphs. Without Louvain, CozoDB's RSS on the same data is in the low hundreds of MB (synthetic medium: 0.6–0.9 GB with Louvain at 18 s).
3. The SQLite backend makes reads wait for writes. On sensemesh-strict, the median read goes from 0.12 ms idle to 0.67 ms p50 / 2.8 ms p99 with a writer running. RocksDB reads from snapshots: 0.09 ms idle, 0.12 ms p50 / 0.24 ms p99 with a writer.

### Traversal strategy comparison (all engines)

- **CozoDB:**
  - Rust-driven BFS (one indexed query per level) beats the recursive rule by 1.3–1.6× on hubs and ties it on small reaches.
  - The recursive rule with a `min(d)` aggregate is correct: keeping the minimum depth per node avoids an explosion of (node, depth) pairs.
  - Unrolled `layer_1..layer_N` rules (infigraph's pattern) are the worst on every backend: 3–4× slower than recursive on hubs. Don't use them.
- **LadybugDB:**
  - `* SHORTEST 1..N` is the only good strategy.
  - Per-level BFS from Rust costs 16–64 ms because every level pays the Cypher planning floor.
  - Naive `*1..N` returns every path and then aggregates. It needs 85–195 ms on real hubs, times out at 5 s on synthetic hubs, and returns wrong results when it times out. On the synthetic medium graph, the timed-out query **segfaulted the process** twice, once per backend: `EXC_BAD_ACCESS` in `JoinHashTable::matchUnFlatKey` ← `HashJoinProbe` ← `HashAggregate`. Crash logs: `bench/.data/crash-lbug-*-medium.log`.
- **DIY:**
  - BFS over an in-memory adjacency is 100–1000× faster than any engine query, because it has no query language, no planner and no serialization.
  - Rebuilding an Ascent program per query costs 0.6–2 ms on real graphs (6–14 ms synthetic). That is fine for rule-derived queries, but a long-lived incremental Ascent/DD instance would be the way to use rules in the hot path.

### Confidence rule at query time

Every engine applies `trusted := prov == EXTRACTED || resolved` inside the traversal.
- On strict graphs it costs +3–10%.
- On permissive graphs, trusted-only traversal is much cheaper because name guesses dominate reach (sensemesh-permissive hub: 12.6 ms all edges vs 0.15 ms trusted, CozoDB mem recursive).
- This is also a correctness signal: most of the permissive blast radius comes from inferred edges.

### Robustness (process exit codes)

- 38 benchmark processes ran, each engine/backend/dataset isolated.
- **CozoDB:** 0 crashes on every backend, including 8 s of concurrent read+write per run. The concurrent-access aborts reported by LeanKG (issues #286, #321) did not reproduce with mnestic 0.18 / rayon 1.10 in this workload.
- **LadybugDB:** 2 crashes (`SIGSEGV`, and exit 138 / `SIGBUS`), both from the timed-out variable-length query. The engine was rerun with `LBUG_SKIP_VARLEN=1` to collect its remaining metrics.
- **DIY:** 0 crashes.

### Correctness

- Every strategy of every engine matches the plain-Rust reference on the initial graph: 12 queries per engine (hub, p99 and median targets, depth 3 and 10, all edges and trusted-only).
- The exception is LadybugDB's naive `cypher_varlen` whenever it times out.
- After 200 random per-file updates, every correct strategy returns exactly the full-rebuild result and the reference result (46 checks per strategy). That holds for:
  - CozoDB: one chained atomic script — `:rm` the file's edges, `:put` symbols, `:put` edges.
  - LadybugDB: one explicit transaction.
  - DIY: one redb key overwrite plus an in-memory patch.

## Synthetic stress context

The generator has seeded, layered modules, sparse back-edges and utility hubs; 15% of edges are INFERRED. Its hub is unrealistic: it reaches 99.8% of the graph at depth 10, against 8–15% in real repos. These numbers bound worst-case behaviour; they do not represent typical code.

- **medium** (50K symbols / 240K edges), worst hub d10:
  - DIY 1.3 ms
  - LadybugDB `SHORTEST` 61 ms
  - CozoDB: mem 79 ms, RocksDB 131 ms, SQLite 251 ms
  - Only DIY meets < 10 ms.
- CozoDB Louvain on medium: 18–19 s.
- LadybugDB per-file update on medium: 20–23 ms p50.

## Caveats

- **Hardware:** Apple M5 Pro (18 cores), 48 GB RAM, macOS 26.7, Rust 1.94.0, release builds. One machine, one run per configuration.
- **Versions:**
  - mnestic 0.18.0 (crate `cozo`): features `storage-sqlite`, `storage-new-rocksdb` (rocksdb 0.22 / RocksDB 8.10), `graph-algo` (rayon 1.10.0, graph_builder 0.4.1).
  - lbug 0.20.4 with the prebuilt static liblbug (`LBUG_VERSION=0.20.4`), 1 GiB buffer pool.
  - redb 4.3.0, ascent 0.8.1.
- **Real-graph edits are simulated:** a file's extracted edges with ~15% dropped plus 1–4 calls to real call targets. Symbols are unchanged, so symbol churn (renames, adds) is not exercised.
- **Durability settings differ across engines** (footnote 1). A fair write comparison needs redb `Durability::None` / batched commits, or CozoDB/RocksDB with sync on.
- **Watcher scenario:** one writer, reads of the median target, 4 s windows, continuous and 20 writes/s. It doesn't cover multiple writer processes; the design assumes one daemon per repo.
- **LadybugDB graph algorithms were not measured.** The macOS arm64 ALGO extension (`~/.lbdb/extension/0.20.0`) needs vendored `libnetworkit` / `libarrow` / `libomp` that aren't shipped, and Homebrew's networkit 11.2.2 is ABI-incompatible (`Symbol not found: __ZTVN9NetworKit5GraphE`). Shortest path works through Cypher.
- **CozoDB algorithms are cold.** They read an unweighted rule (`e[a,b] := *edge{src:a, dst:b}`) on every call. mnestic's cached projection (`::graph create`) was dropped because with a 3-column edge relation it takes the third column as a weight, which made Dijkstra return weighted paths. A projection over a dedicated 2-column relation would cut the cost of repeated algorithm calls.
- **Schema:** CozoDB used one `edge {src, dst, kind => prov, resolved}` relation plus a reverse index `edge:rev {dst, src, …}`. Per-kind relations were not tried, since blast radius here traverses every kind. All inputs were passed as `$params`, with `:put`, never `:insert`.
- **Not run:** synthetic `large` (200K / ~1M edges). Given the medium results it would only widen the gaps.

## What this means for Graphite

1. **Hot path:** the graph lives in memory in the per-repo daemon as adjacency (forward and reverse, with a per-edge trusted bit). Every agent-facing traversal (blast radius, `diff_impact`, paths, tests covering a change) runs in Rust. The < 10 ms budget is met with 100× headroom on real repos.
2. **Persistence:** store only per-file facts (symbols plus outgoing edges), keyed by file, in redb. A file update is one key overwrite, so incremental equals full rebuild by construction. Startup rebuilds adjacency in 20–40 ms on these repos.
3. **Algorithms:** SCC and PageRank are ~50 lines each (measured: 0.1–4.6 ms). Louvain/Leiden needs an implementation or a crate. It runs off the hot path, so time matters less than for traversals.
4. **Datalog stays optional.** If derived rules are wanted (confidence, framework edges, provenance), evaluate them in-process: Ascent per query at 0.6–2 ms, or a long-lived incremental engine. Don't put an embedded database's query language in the agent's critical path. CozoDB-on-RocksDB remains a viable secondary store for ad-hoc analytics if the Datalog ergonomics are wanted.

## Full generated tables

### Real graphs

#### continuum-strict: 12,373 symbols / 27,931 edges

Targets (depth-10 reach): hub = 1,792, p99 = 192, median = 3 nodes.

##### Blast radius by traversal strategy (p50 / p95)

| engine | strategy | d3 median | d10 median | d10 p99 | d10 hub | d10 hub trusted | correct |
|---|---|---|---|---|---|---|---|
| cozo-mnestic-mem | recursive | 59 µs / 67 µs | 58 µs / 69 µs | 254 µs / 278 µs | 2.1 ms / 2.2 ms | 2.3 ms / 2.4 ms | ✅✅ |
| cozo-mnestic-mem | rust_bfs | 57 µs / 59 µs | 75 µs / 79 µs | 235 µs / 248 µs | 1.4 ms / 1.5 ms | 1.6 ms / 1.7 ms | ✅✅ |
| cozo-mnestic-mem | unrolled | 225 µs / 327 µs | 743 µs / 893 µs | 1.6 ms / 1.9 ms | 5.1 ms / 5.5 ms | 5.4 ms / 5.6 ms | ✅✅ |
| cozo-mnestic-sqlite | recursive | 92 µs / 100 µs | 94 µs / 105 µs | 613 µs / 647 µs | 5.7 ms / 5.8 ms | 5.8 ms / 6.0 ms | ✅✅ |
| cozo-mnestic-sqlite | rust_bfs | 107 µs / 116 µs | 141 µs / 153 µs | 692 µs / 762 µs | 5.0 ms / 5.2 ms | 5.2 ms / 5.4 ms | ✅✅ |
| cozo-mnestic-sqlite | unrolled | 296 µs / 398 µs | 846 µs / 1.0 ms | 2.4 ms / 2.6 ms | 14.2 ms / 14.8 ms | 14.4 ms / 14.8 ms | ✅✅ |
| cozo-mnestic-newrocksdb | recursive | 70 µs / 75 µs | 71 µs / 75 µs | 422 µs / 445 µs | 3.7 ms / 3.8 ms | 3.9 ms / 4.0 ms | ✅✅ |
| cozo-mnestic-newrocksdb | rust_bfs | 74 µs / 82 µs | 98 µs / 105 µs | 428 µs / 453 µs | 3.0 ms / 3.1 ms | 3.2 ms / 3.3 ms | ✅✅ |
| cozo-mnestic-newrocksdb | unrolled | 245 µs / 344 µs | 766 µs / 942 µs | 2.0 ms / 2.1 ms | 9.0 ms / 9.4 ms | 9.3 ms / 9.6 ms | ✅✅ |
| lbug-disk | cypher_shortest | 656 µs / 742 µs | 817 µs / 886 µs | 929 µs / 998 µs | 2.5 ms / 2.6 ms | 2.8 ms / 2.9 ms | ✅✅ |
| lbug-disk | cypher_varlen | 982 µs / 1.1 ms | 1.1 ms / 1.2 ms | 1.5 ms / 1.6 ms | 8.3 ms / 8.5 ms | 8.9 ms / 9.1 ms | ✅✅ |
| lbug-disk | rust_bfs | 16.0 ms / 16.5 ms | 21.4 ms / 21.8 ms | 32.2 ms / 32.7 ms | 49.7 ms / 50.7 ms | 44.1 ms / 45.2 ms | ✅✅ |
| lbug-mem | cypher_shortest | 689 µs / 765 µs | 842 µs / 914 µs | 951 µs / 1.0 ms | 2.6 ms / 2.7 ms | 2.9 ms / 3.1 ms | ✅✅ |
| lbug-mem | cypher_varlen | 1000 µs / 1.1 ms | 1.2 ms / 1.3 ms | 1.5 ms / 1.5 ms | 7.6 ms / 7.8 ms | 8.3 ms / 8.5 ms | ✅✅ |
| lbug-mem | rust_bfs | 16.8 ms / 17.1 ms | 22.4 ms / 22.7 ms | 33.3 ms / 33.9 ms | 50.6 ms / 51.0 ms | 45.5 ms / 46.6 ms | ✅✅ |
| diy-ascent-redb | ascent_per_query | 597 µs / 620 µs | 596 µs / 611 µs | 609 µs / 629 µs | 693 µs / 719 µs | 691 µs / 714 µs | ✅✅ |
| diy-ascent-redb | rust_bfs | 0.9 µs / 0.9 µs | 0.9 µs / 1.0 µs | 2.5 µs / 2.7 µs | 19 µs / 20 µs | 20 µs / 20 µs | ✅✅ |

##### Everything else (best correct strategy for blast)

| engine | best strategy | build | disk | reopen | cold 1st query | shortest path | SCC | Louvain | PageRank | update p50 / p95 | read while writing p50 / p99 / max | write while reading p50 / p99 | peak RSS |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cozo-mnestic-mem | rust_bfs | 53 ms | in-mem | — | 496 µs | 8.9 ms | 17.3 ms | 965.8 ms | 18.3 ms | 88 µs / 237 µs | 130 µs / 426 µs / 1.0 ms | 128 µs / 440 µs | 1478 MB |
| cozo-mnestic-sqlite | rust_bfs | 67 ms | 7.9 MB | 0 ms | 362 µs | 14.4 ms | 23.5 ms | 974.4 ms | 24.2 ms | 526 µs / 1.2 ms | 659 µs / 2.3 ms / 6.3 ms | 654 µs / 2.3 ms | 1453 MB |
| cozo-mnestic-newrocksdb | rust_bfs | 122 ms | 3.9 MB | 38 ms | 351 µs | 11.4 ms | 20.2 ms | 982.1 ms | 20.7 ms | 172 µs / 625 µs | 83 µs / 185 µs / 619 µs | 237 µs / 1.6 ms | 1638 MB |
| lbug-disk | cypher_shortest | 62 ms | 2.7 MB | 7 ms | 7.0 ms | 879 µs | unsupported | unsupported | unsupported | 8.9 ms / 22.0 ms | 894 µs / 1.1 ms / 1.3 ms | 8.9 ms / 37.2 ms | 330 MB |
| lbug-mem | cypher_shortest | 15 ms | in-mem | — | 7.0 ms | 899 µs | unsupported | unsupported | unsupported | 5.4 ms / 18.9 ms | 916 µs / 1.1 ms / 1.3 ms | 5.4 ms / 31.6 ms | 329 MB |
| diy-ascent-redb | rust_bfs | 8 ms | 1.0 MB | 19 ms | 6.5 µs | 1.2 µs | 423 µs | unsupported | 395 µs | 3.1 ms / 4.1 ms | 0.5 µs / 0.7 µs / 33 µs | 3.0 ms / 4.1 ms | 153 MB |

Notes:
- cozo-mnestic-mem shortest_path: expected Some(3), got Some(3)
- cozo-mnestic-mem scc: nontrivial SCCs expected 10, got Some(10)
- cozo-mnestic-sqlite shortest_path: expected Some(3), got Some(3)
- cozo-mnestic-sqlite scc: nontrivial SCCs expected 10, got Some(10)
- cozo-mnestic-newrocksdb shortest_path: expected Some(3), got Some(3)
- cozo-mnestic-newrocksdb scc: nontrivial SCCs expected 10, got Some(10)
- lbug-disk shortest_path: expected Some(3), got Some(3)
- lbug-disk scc: nontrivial SCCs expected 10, got None
- lbug-mem shortest_path: expected Some(3), got Some(3)
- lbug-mem scc: nontrivial SCCs expected 10, got None
- diy-ascent-redb shortest_path: expected Some(3), got Some(3)
- diy-ascent-redb scc: nontrivial SCCs expected 10, got Some(10)

#### sensemesh-strict: 23,741 symbols / 76,139 edges

Targets (depth-10 reach): hub = 1,796, p99 = 322, median = 12 nodes.

##### Blast radius by traversal strategy (p50 / p95)

| engine | strategy | d3 median | d10 median | d10 p99 | d10 hub | d10 hub trusted | correct |
|---|---|---|---|---|---|---|---|
| cozo-mnestic-mem | recursive | 66 µs / 68 µs | 66 µs / 67 µs | 621 µs / 644 µs | 3.5 ms / 3.6 ms | 3.4 ms / 3.5 ms | ✅✅ |
| cozo-mnestic-mem | rust_bfs | 46 µs / 50 µs | 47 µs / 48 µs | 569 µs / 589 µs | 2.4 ms / 2.5 ms | 2.5 ms / 2.5 ms | ✅✅ |
| cozo-mnestic-mem | unrolled | 213 µs / 325 µs | 836 µs / 1.0 ms | 2.7 ms / 2.9 ms | 9.3 ms / 9.7 ms | 8.8 ms / 9.1 ms | ✅✅ |
| cozo-mnestic-sqlite | recursive | 117 µs / 123 µs | 116 µs / 125 µs | 1.4 ms / 1.5 ms | 9.2 ms / 9.5 ms | 8.7 ms / 8.9 ms | ✅✅ |
| cozo-mnestic-sqlite | rust_bfs | 98 µs / 106 µs | 99 µs / 106 µs | 1.5 ms / 1.5 ms | 8.2 ms / 8.4 ms | 7.8 ms / 8.0 ms | ✅✅ |
| cozo-mnestic-sqlite | unrolled | 328 µs / 391 µs | 977 µs / 1.2 ms | 5.2 ms / 5.5 ms | 27.3 ms / 28.2 ms | 23.8 ms / 24.5 ms | ✅✅ |
| cozo-mnestic-newrocksdb | recursive | 85 µs / 94 µs | 86 µs / 95 µs | 940 µs / 977 µs | 5.2 ms / 5.5 ms | 5.1 ms / 5.6 ms | ✅✅ |
| cozo-mnestic-newrocksdb | rust_bfs | 68 µs / 74 µs | 66 µs / 72 µs | 913 µs / 939 µs | 4.2 ms / 4.4 ms | 4.1 ms / 4.3 ms | ✅✅ |
| cozo-mnestic-newrocksdb | unrolled | 268 µs / 330 µs | 899 µs / 1.1 ms | 3.7 ms / 3.9 ms | 15.3 ms / 16.0 ms | 13.7 ms / 14.2 ms | ✅✅ |
| lbug-disk | cypher_shortest | 879 µs / 978 µs | 1.1 ms / 1.2 ms | 1.3 ms / 1.5 ms | 3.0 ms / 3.2 ms | 3.1 ms / 3.4 ms | ✅✅ |
| lbug-disk | cypher_varlen | 1.3 ms / 1.4 ms | 1.6 ms / 1.7 ms | 5.7 ms / 6.1 ms | 95.1 ms / 98.4 ms | 60.6 ms / 64.1 ms | ✅✅ |
| lbug-disk | rust_bfs | 20.4 ms / 22.0 ms | 20.2 ms / 21.8 ms | 103.7 ms / 106.7 ms | 103.9 ms / 107.5 ms | 112.6 ms / 128.1 ms | ✅✅ |
| lbug-mem | cypher_shortest | 968 µs / 1.1 ms | 1.1 ms / 1.2 ms | 1.4 ms / 1.5 ms | 3.3 ms / 3.4 ms | 3.3 ms / 3.5 ms | ✅✅ |
| lbug-mem | cypher_varlen | 1.3 ms / 1.5 ms | 1.6 ms / 1.7 ms | 5.7 ms / 5.9 ms | 85.6 ms / 88.8 ms | 61.0 ms / 64.8 ms | ✅✅ |
| lbug-mem | rust_bfs | 20.7 ms / 21.4 ms | 20.6 ms / 21.7 ms | 102.6 ms / 105.4 ms | 104.0 ms / 106.1 ms | 117.7 ms / 120.4 ms | ✅✅ |
| diy-ascent-redb | ascent_per_query | 1.9 ms / 2.0 ms | 1.9 ms / 2.0 ms | 1.9 ms / 2.0 ms | 2.0 ms / 2.2 ms | 2.0 ms / 2.1 ms | ✅✅ |
| diy-ascent-redb | rust_bfs | 1.6 µs / 1.8 µs | 1.8 µs / 1.8 µs | 4.4 µs / 4.5 µs | 22 µs / 22 µs | 22 µs / 23 µs | ✅✅ |

##### Everything else (best correct strategy for blast)

| engine | best strategy | build | disk | reopen | cold 1st query | shortest path | SCC | Louvain | PageRank | update p50 / p95 | read while writing p50 / p99 / max | write while reading p50 / p99 | peak RSS |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cozo-mnestic-mem | rust_bfs | 116 ms | in-mem | — | 339 µs | 25.8 ms | 45.3 ms | 3675.2 ms | 44.5 ms | 88 µs / 257 µs | 139 µs / 449 µs / 1.1 ms | 136 µs / 462 µs | 4631 MB |
| cozo-mnestic-sqlite | rust_bfs | 185 ms | 20.6 MB | 0 ms | 468 µs | 41.4 ms | 60.4 ms | 3647.5 ms | 59.6 ms | 521 µs / 1.4 ms | 669 µs / 2.8 ms / 6.5 ms | 666 µs / 2.8 ms | 4578 MB |
| cozo-mnestic-newrocksdb | rust_bfs | 342 ms | 10.0 MB | 78 ms | 381 µs | 32.1 ms | 50.7 ms | 3733.8 ms | 51.0 ms | 172 µs / 727 µs | 117 µs / 243 µs / 1.2 ms | 209 µs / 1.5 ms | 4752 MB |
| lbug-disk | cypher_shortest | 67 ms | 3.5 MB | 7 ms | 6.8 ms | 2.1 ms | unsupported | unsupported | unsupported | 10.1 ms / 28.2 ms | 1.2 ms / 1.5 ms / 1.9 ms | 10.9 ms / 53.0 ms | 996 MB |
| lbug-mem | cypher_shortest | 27 ms | in-mem | — | 7.2 ms | 2.1 ms | unsupported | unsupported | unsupported | 6.8 ms / 21.8 ms | 1.2 ms / 1.5 ms / 1.8 ms | 7.2 ms / 48.0 ms | 984 MB |
| diy-ascent-redb | rust_bfs | 11 ms | 1.0 MB | 39 ms | 4.8 µs | 1.5 µs | 902 µs | unsupported | 1.3 ms | 3.0 ms / 4.1 ms | 1.0 µs / 1.2 µs / 47 µs | 3.1 ms / 5.6 ms | 128 MB |

Notes:
- cozo-mnestic-mem shortest_path: expected Some(8), got Some(8)
- cozo-mnestic-mem scc: nontrivial SCCs expected 42, got Some(42)
- cozo-mnestic-sqlite shortest_path: expected Some(8), got Some(8)
- cozo-mnestic-sqlite scc: nontrivial SCCs expected 42, got Some(42)
- cozo-mnestic-newrocksdb shortest_path: expected Some(8), got Some(8)
- cozo-mnestic-newrocksdb scc: nontrivial SCCs expected 42, got Some(42)
- lbug-disk shortest_path: expected Some(8), got Some(8)
- lbug-disk scc: nontrivial SCCs expected 42, got None
- lbug-mem shortest_path: expected Some(8), got Some(8)
- lbug-mem scc: nontrivial SCCs expected 42, got None
- diy-ascent-redb shortest_path: expected Some(8), got Some(8)
- diy-ascent-redb scc: nontrivial SCCs expected 42, got Some(42)

#### continuum-permissive: 12,377 symbols / 28,899 edges

Targets (depth-10 reach): hub = 3,695, p99 = 213, median = 3 nodes.

##### Blast radius by traversal strategy (p50 / p95)

| engine | strategy | d3 median | d10 median | d10 p99 | d10 hub | d10 hub trusted | correct |
|---|---|---|---|---|---|---|---|
| cozo-mnestic-mem | recursive | 62 µs / 64 µs | 63 µs / 67 µs | 310 µs / 328 µs | 5.1 ms / 5.4 ms | 222 µs / 235 µs | ✅✅ |
| cozo-mnestic-mem | rust_bfs | 62 µs / 63 µs | 83 µs / 85 µs | 318 µs / 331 µs | 3.2 ms / 3.4 ms | 293 µs / 301 µs | ✅✅ |
| cozo-mnestic-mem | unrolled | 261 µs / 379 µs | 763 µs / 944 µs | 2.3 ms / 2.6 ms | 10.1 ms / 10.9 ms | 1.9 ms / 2.2 ms | ✅✅ |
| cozo-mnestic-sqlite | recursive | 102 µs / 138 µs | 102 µs / 107 µs | 763 µs / 832 µs | 14.1 ms / 15.1 ms | 533 µs / 580 µs | ✅✅ |
| cozo-mnestic-sqlite | rust_bfs | 116 µs / 118 µs | 154 µs / 159 µs | 873 µs / 929 µs | 12.1 ms / 12.7 ms | 695 µs / 745 µs | ✅✅ |
| cozo-mnestic-sqlite | unrolled | 289 µs / 397 µs | 866 µs / 1.0 ms | 3.7 ms / 3.9 ms | 28.6 ms / 30.7 ms | 2.4 ms / 2.6 ms | ✅✅ |
| cozo-mnestic-newrocksdb | recursive | 74 µs / 81 µs | 76 µs / 83 µs | 520 µs / 545 µs | 8.4 ms / 8.7 ms | 338 µs / 357 µs | ✅✅ |
| cozo-mnestic-newrocksdb | rust_bfs | 78 µs / 80 µs | 103 µs / 106 µs | 579 µs / 604 µs | 6.4 ms / 6.6 ms | 439 µs / 469 µs | ✅✅ |
| cozo-mnestic-newrocksdb | unrolled | 309 µs / 387 µs | 831 µs / 1.0 ms | 2.9 ms / 3.2 ms | 17.3 ms / 18.1 ms | 2.1 ms / 2.4 ms | ✅✅ |
| lbug-disk | cypher_shortest | 771 µs / 898 µs | 903 µs / 998 µs | 1.0 ms / 1.2 ms | 4.8 ms / 6.4 ms | 1.1 ms / 1.6 ms | ✅✅ |
| lbug-disk | cypher_varlen | 1.2 ms / 1.4 ms | 1.3 ms / 1.4 ms | 2.9 ms / 3.4 ms | 11.9 ms / 12.8 ms | 1.7 ms / 2.0 ms | ✅✅ |
| lbug-disk | rust_bfs | 16.8 ms / 17.7 ms | 22.9 ms / 24.5 ms | 45.3 ms / 46.7 ms | 59.4 ms / 60.9 ms | 38.2 ms / 40.5 ms | ✅✅ |
| lbug-mem | cypher_shortest | 736 µs / 828 µs | 925 µs / 1.0 ms | 1.0 ms / 1.1 ms | 4.4 ms / 4.7 ms | 1.0 ms / 1.2 ms | ✅✅ |
| lbug-mem | cypher_varlen | 1.5 ms / 1.9 ms | 1.5 ms / 1.9 ms | 3.3 ms / 3.7 ms | 12.9 ms / 13.5 ms | 1.9 ms / 2.2 ms | ✅✅ |
| lbug-mem | rust_bfs | 17.7 ms / 18.8 ms | 24.3 ms / 26.2 ms | 49.2 ms / 52.2 ms | 61.4 ms / 63.9 ms | 39.5 ms / 42.3 ms | ✅✅ |
| diy-ascent-redb | ascent_per_query | 803 µs / 948 µs | 835 µs / 1.0 ms | 850 µs / 1.1 ms | 1.1 ms / 1.3 ms | 825 µs / 951 µs | ✅✅ |
| diy-ascent-redb | rust_bfs | 1.0 µs / 1.1 µs | 1.0 µs / 1.0 µs | 2.9 µs / 3.1 µs | 42 µs / 51 µs | 1.8 µs / 2.0 µs | ✅✅ |

##### Everything else (best correct strategy for blast)

| engine | best strategy | build | disk | reopen | cold 1st query | shortest path | SCC | Louvain | PageRank | update p50 / p95 | read while writing p50 / p99 / max | write while reading p50 / p99 | peak RSS |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cozo-mnestic-mem | rust_bfs | 50 ms | in-mem | — | 302 µs | 10.0 ms | 19.2 ms | 1146.3 ms | 21.7 ms | 108 µs / 270 µs | 147 µs / 525 µs / 1.8 ms | 145 µs / 535 µs | 1498 MB |
| cozo-mnestic-sqlite | rust_bfs | 76 ms | 8.2 MB | 0 ms | 414 µs | 16.3 ms | 26.6 ms | 1172.7 ms | 27.0 ms | 607 µs / 1.2 ms | 730 µs / 2.0 ms / 6.1 ms | 727 µs / 2.0 ms | 1465 MB |
| cozo-mnestic-newrocksdb | rust_bfs | 138 ms | 4.0 MB | 39 ms | 322 µs | 12.6 ms | 22.9 ms | 1153.4 ms | 21.8 ms | 196 µs / 635 µs | 92 µs / 295 µs / 13.2 ms | 257 µs / 1.8 ms | 1630 MB |
| lbug-disk | cypher_shortest | 56 ms | 2.7 MB | 7 ms | 7.0 ms | 1.4 ms | unsupported | unsupported | unsupported | 10.0 ms / 21.9 ms | 950 µs / 1.3 ms / 1.6 ms | 9.9 ms / 48.1 ms | 248 MB |
| lbug-mem | cypher_shortest | 16 ms | in-mem | — | 7.2 ms | 1.6 ms | unsupported | unsupported | unsupported | 6.7 ms / 19.5 ms | 1.2 ms / 2.1 ms / 2.5 ms | 7.8 ms / 60.1 ms | 210 MB |
| diy-ascent-redb | rust_bfs | 7 ms | 1.0 MB | 20 ms | 1.8 µs | 1.6 µs | 346 µs | unsupported | 422 µs | 3.1 ms / 4.2 ms | 0.5 µs / 0.7 µs / 251 µs | 3.9 ms / 5.8 ms | 153 MB |

Notes:
- cozo-mnestic-mem shortest_path: expected Some(7), got Some(7)
- cozo-mnestic-mem scc: nontrivial SCCs expected 10, got Some(10)
- cozo-mnestic-sqlite shortest_path: expected Some(7), got Some(7)
- cozo-mnestic-sqlite scc: nontrivial SCCs expected 10, got Some(10)
- cozo-mnestic-newrocksdb shortest_path: expected Some(7), got Some(7)
- cozo-mnestic-newrocksdb scc: nontrivial SCCs expected 10, got Some(10)
- lbug-disk shortest_path: expected Some(7), got Some(7)
- lbug-disk scc: nontrivial SCCs expected 10, got None
- lbug-mem shortest_path: expected Some(7), got Some(7)
- lbug-mem scc: nontrivial SCCs expected 10, got None
- diy-ascent-redb shortest_path: expected Some(7), got Some(7)
- diy-ascent-redb scc: nontrivial SCCs expected 10, got Some(10)

#### sensemesh-permissive: 23,741 symbols / 81,068 edges

Targets (depth-10 reach): hub = 6,433, p99 = 369, median = 12 nodes.

##### Blast radius by traversal strategy (p50 / p95)

| engine | strategy | d3 median | d10 median | d10 p99 | d10 hub | d10 hub trusted | correct |
|---|---|---|---|---|---|---|---|
| cozo-mnestic-mem | recursive | 67 µs / 69 µs | 67 µs / 69 µs | 833 µs / 894 µs | 12.6 ms / 13.7 ms | 154 µs / 162 µs | ✅✅ |
| cozo-mnestic-mem | rust_bfs | 49 µs / 51 µs | 49 µs / 52 µs | 716 µs / 789 µs | 8.6 ms / 9.9 ms | 229 µs / 248 µs | ✅✅ |
| cozo-mnestic-mem | unrolled | 231 µs / 320 µs | 602 µs / 753 µs | 3.0 ms / 3.3 ms | 36.8 ms / 40.8 ms | 1.1 ms / 1.4 ms | ✅✅ |
| cozo-mnestic-sqlite | recursive | 119 µs / 128 µs | 119 µs / 126 µs | 1.9 ms / 1.9 ms | 31.9 ms / 32.6 ms | 362 µs / 391 µs | ✅✅ |
| cozo-mnestic-sqlite | rust_bfs | 102 µs / 108 µs | 102 µs / 108 µs | 1.9 ms / 1.9 ms | 27.1 ms / 27.7 ms | 502 µs / 530 µs | ✅✅ |
| cozo-mnestic-sqlite | unrolled | 278 µs / 354 µs | 763 µs / 850 µs | 5.9 ms / 6.1 ms | 101.8 ms / 102.6 ms | 1.5 ms / 1.7 ms | ✅✅ |
| cozo-mnestic-newrocksdb | recursive | 87 µs / 92 µs | 87 µs / 96 µs | 1.2 ms / 1.2 ms | 17.7 ms / 18.4 ms | 223 µs / 241 µs | ✅✅ |
| cozo-mnestic-newrocksdb | rust_bfs | 69 µs / 72 µs | 68 µs / 72 µs | 1.1 ms / 1.2 ms | 13.3 ms / 13.7 ms | 319 µs / 332 µs | ✅✅ |
| cozo-mnestic-newrocksdb | unrolled | 230 µs / 299 µs | 672 µs / 818 µs | 4.1 ms / 4.3 ms | 55.9 ms / 57.1 ms | 1.4 ms / 1.6 ms | ✅✅ |
| lbug-disk | cypher_shortest | 676 µs / 769 µs | 1.0 ms / 1.1 ms | 1.3 ms / 1.4 ms | 7.7 ms / 7.9 ms | 1.1 ms / 1.2 ms | ✅✅ |
| lbug-disk | cypher_varlen | 1.0 ms / 1.2 ms | 1.4 ms / 1.6 ms | 7.7 ms / 7.9 ms | 195.4 ms / 197.3 ms | 1.5 ms / 1.6 ms | ❌❌ |
| lbug-disk | rust_bfs | 19.5 ms / 19.8 ms | 19.5 ms / 19.8 ms | 97.7 ms / 98.5 ms | 103.8 ms / 104.7 ms | 62.1 ms / 63.7 ms | ✅✅ |
| lbug-mem | cypher_shortest | 694 µs / 785 µs | 1.1 ms / 1.2 ms | 1.3 ms / 1.5 ms | 8.0 ms / 8.1 ms | 1.1 ms / 1.2 ms | ✅✅ |
| lbug-mem | cypher_varlen | 1.0 ms / 1.1 ms | 1.4 ms / 1.5 ms | 7.6 ms / 7.8 ms | 174.9 ms / 177.0 ms | 1.5 ms / 1.6 ms | ❌❌ |
| lbug-mem | rust_bfs | 19.9 ms / 20.3 ms | 19.9 ms / 20.3 ms | 99.9 ms / 101.0 ms | 108.7 ms / 110.9 ms | 65.2 ms / 66.8 ms | ✅✅ |
| diy-ascent-redb | ascent_per_query | 1.8 ms / 1.9 ms | 1.8 ms / 1.9 ms | 1.9 ms / 1.9 ms | 2.3 ms / 2.4 ms | 1.8 ms / 1.9 ms | ✅✅ |
| diy-ascent-redb | rust_bfs | 1.2 µs / 1.8 µs | 1.3 µs / 1.8 µs | 5.0 µs / 5.3 µs | 80 µs / 87 µs | 1.4 µs / 1.5 µs | ✅✅ |

##### Everything else (best correct strategy for blast)

| engine | best strategy | build | disk | reopen | cold 1st query | shortest path | SCC | Louvain | PageRank | update p50 / p95 | read while writing p50 / p99 / max | write while reading p50 / p99 | peak RSS |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cozo-mnestic-mem | rust_bfs | 132 ms | in-mem | — | 334 µs | 29.1 ms | 49.5 ms | 4657.6 ms | 49.5 ms | 88 µs / 343 µs | 168 µs / 894 µs / 1.3 ms | 163 µs / 895 µs | 5168 MB |
| cozo-mnestic-sqlite | rust_bfs | 219 ms | 22.0 MB | 0 ms | 493 µs | 45.6 ms | 65.7 ms | 3977.4 ms | 64.9 ms | 547 µs / 2.1 ms | 726 µs / 3.0 ms / 7.9 ms | 724 µs / 3.0 ms | 5092 MB |
| cozo-mnestic-newrocksdb | rust_bfs | 379 ms | 10.5 MB | 92 ms | 442 µs | 34.2 ms | 53.8 ms | 4013.2 ms | 53.5 ms | 162 µs / 962 µs | 119 µs / 782 µs / 4.5 ms | 209 µs / 1.6 ms | 5279 MB |
| lbug-disk | cypher_shortest | 72 ms | 3.6 MB | 7 ms | 6.9 ms | 1.9 ms | unsupported | unsupported | unsupported | 10.0 ms / 32.0 ms | 1.1 ms / 1.4 ms / 1.8 ms | 11.0 ms / 51.1 ms | 2074 MB |
| lbug-mem | cypher_shortest | 27 ms | in-mem | — | 6.9 ms | 1.9 ms | unsupported | unsupported | unsupported | 6.5 ms / 28.4 ms | 1.1 ms / 1.4 ms / 1.7 ms | 7.5 ms / 49.7 ms | 2057 MB |
| diy-ascent-redb | rust_bfs | 14 ms | 1.0 MB | 39 ms | 7.8 µs | 1.6 µs | 920 µs | unsupported | 1.5 ms | 4.0 ms / 4.1 ms | 0.9 µs / 1.2 µs / 60 µs | 3.0 ms / 5.0 ms | 121 MB |

Notes:
- cozo-mnestic-mem shortest_path: expected Some(8), got Some(8)
- cozo-mnestic-mem scc: nontrivial SCCs expected 51, got Some(51)
- cozo-mnestic-sqlite shortest_path: expected Some(8), got Some(8)
- cozo-mnestic-sqlite scc: nontrivial SCCs expected 51, got Some(51)
- cozo-mnestic-newrocksdb shortest_path: expected Some(8), got Some(8)
- cozo-mnestic-newrocksdb scc: nontrivial SCCs expected 51, got Some(51)
- lbug-disk shortest_path: expected Some(8), got Some(8)
- lbug-disk scc: nontrivial SCCs expected 51, got None
- lbug-disk correct_incremental_eq_full[cypher_varlen]: after 200 updates: 0/46 differ from full rebuild, 1/46 differ from reference
- lbug-disk correct_vs_reference_initial[cypher_varlen]: 1 mismatching queries of 12
- lbug-mem shortest_path: expected Some(8), got Some(8)
- lbug-mem scc: nontrivial SCCs expected 51, got None
- lbug-mem correct_incremental_eq_full[cypher_varlen]: after 200 updates: 0/46 differ from full rebuild, 1/46 differ from reference
- lbug-mem correct_vs_reference_initial[cypher_varlen]: 1 mismatching queries of 12
- diy-ascent-redb shortest_path: expected Some(8), got Some(8)
- diy-ascent-redb scc: nontrivial SCCs expected 51, got Some(51)


### Synthetic (stress)

#### small: 5,000 symbols / 23,904 edges

Targets (depth-10 reach): hub = 4,987, p99 = 4,987, median = 252 nodes.

##### Blast radius by traversal strategy (p50 / p95)

| engine | strategy | d3 median | d10 median | d10 p99 | d10 hub | d10 hub trusted | correct |
|---|---|---|---|---|---|---|---|
| cozo-mnestic-mem | recursive | 74 µs / 77 µs | 320 µs / 339 µs | 10.2 ms / 10.5 ms | 10.4 ms / 10.6 ms | 10.7 ms / 10.9 ms | ✅✅ |
| cozo-mnestic-mem | rust_bfs | 68 µs / 72 µs | 353 µs / 371 µs | 6.5 ms / 6.7 ms | 6.5 ms / 6.7 ms | 7.1 ms / 7.3 ms | ✅✅ |
| cozo-mnestic-mem | unrolled | 248 µs / 353 µs | 2.0 ms / 2.2 ms | 36.0 ms / 37.0 ms | 28.7 ms / 29.5 ms | 29.9 ms / 30.8 ms | ✅✅ |
| cozo-mnestic-sqlite | recursive | 119 µs / 126 µs | 706 µs / 736 µs | 25.5 ms / 25.9 ms | 25.8 ms / 26.5 ms | 26.1 ms / 26.5 ms | ✅✅ |
| cozo-mnestic-sqlite | rust_bfs | 126 µs / 135 µs | 856 µs / 891 µs | 21.6 ms / 22.1 ms | 22.0 ms / 22.4 ms | 22.5 ms / 22.9 ms | ✅✅ |
| cozo-mnestic-sqlite | unrolled | 309 µs / 415 µs | 3.0 ms / 3.3 ms | 106.7 ms / 107.3 ms | 85.2 ms / 86.1 ms | 83.8 ms / 84.6 ms | ✅✅ |
| cozo-mnestic-newrocksdb | recursive | 88 µs / 94 µs | 469 µs / 492 µs | 15.0 ms / 15.2 ms | 15.1 ms / 15.4 ms | 15.4 ms / 15.6 ms | ✅✅ |
| cozo-mnestic-newrocksdb | rust_bfs | 87 µs / 94 µs | 531 µs / 564 µs | 11.1 ms / 11.3 ms | 11.3 ms / 11.9 ms | 11.7 ms / 12.1 ms | ✅✅ |
| cozo-mnestic-newrocksdb | unrolled | 265 µs / 362 µs | 2.3 ms / 2.5 ms | 60.9 ms / 62.1 ms | 49.4 ms / 51.6 ms | 48.9 ms / 49.5 ms | ✅✅ |
| lbug-disk | cypher_shortest | 710 µs / 782 µs | 842 µs / 918 µs | 6.2 ms / 6.4 ms | 6.2 ms / 6.4 ms | 6.5 ms / 6.7 ms | ✅✅ |
| lbug-disk | cypher_varlen | 1.1 ms / 1.2 ms | 2.7 ms / 3.1 ms | 943.9 ms / 980.2 ms | 207.5 ms / 216.0 ms | 135.7 ms / 143.2 ms | ❌❌ |
| lbug-disk | rust_bfs | 10.1 ms / 10.6 ms | 33.7 ms / 34.3 ms | 35.4 ms / 36.4 ms | 39.0 ms / 41.8 ms | 44.7 ms / 46.3 ms | ✅✅ |
| lbug-mem | cypher_shortest | 741 µs / 872 µs | 895 µs / 1.0 ms | 6.4 ms / 6.7 ms | 6.5 ms / 7.0 ms | 6.7 ms / 7.2 ms | ✅✅ |
| lbug-mem | cypher_varlen | 1.1 ms / 1.2 ms | 2.7 ms / 2.8 ms | 882.8 ms / 900.5 ms | 232.6 ms / 238.1 ms | 152.0 ms / 156.7 ms | ❌❌ |
| lbug-mem | rust_bfs | 10.6 ms / 11.3 ms | 35.0 ms / 36.9 ms | 36.9 ms / 39.5 ms | 40.6 ms / 42.5 ms | 47.6 ms / 51.0 ms | ✅✅ |
| diy-ascent-redb | ascent_per_query | 445 µs / 463 µs | 467 µs / 524 µs | 834 µs / 908 µs | 842 µs / 940 µs | 826 µs / 935 µs | ✅✅ |
| diy-ascent-redb | rust_bfs | 0.7 µs / 0.8 µs | 2.7 µs / 2.8 µs | 63 µs / 72 µs | 57 µs / 60 µs | 76 µs / 82 µs | ✅✅ |

##### Everything else (best correct strategy for blast)

| engine | best strategy | build | disk | reopen | cold 1st query | shortest path | SCC | Louvain | PageRank | update p50 / p95 | read while writing p50 / p99 / max | write while reading p50 / p99 | peak RSS |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cozo-mnestic-mem | rust_bfs | 33 ms | in-mem | — | 596 µs | 8.2 ms | 13.8 ms | 316.3 ms | 16.9 ms | 180 µs / 204 µs | 303 µs / 595 µs / 6.6 ms | 298 µs / 584 µs | 119 MB |
| cozo-mnestic-sqlite | rust_bfs | 56 ms | 6.1 MB | 0 ms | 1.1 ms | 13.2 ms | 18.8 ms | 320.1 ms | 21.7 ms | 1.2 ms / 1.5 ms | 1.6 ms / 2.1 ms / 5.7 ms | 1.6 ms / 2.1 ms | 109 MB |
| cozo-mnestic-newrocksdb | rust_bfs | 105 ms | 3.0 MB | 41 ms | 881 µs | 10.1 ms | 15.5 ms | 315.3 ms | 18.7 ms | 437 µs / 520 µs | 377 µs / 962 µs / 2.6 ms | 605 µs / 775 µs | 357 MB |
| lbug-disk | cypher_shortest | 51 ms | 2.4 MB | 7 ms | 6.6 ms | 1.3 ms | unsupported | unsupported | unsupported | 15.9 ms / 18.2 ms | 896 µs / 1.3 ms / 1.9 ms | 18.0 ms / 23.9 ms | 2121 MB |
| lbug-mem | cypher_shortest | 13 ms | in-mem | — | 8.3 ms | 1.3 ms | unsupported | unsupported | unsupported | 11.8 ms / 14.2 ms | 850 µs / 1.1 ms / 1.3 ms | 13.7 ms / 17.6 ms | 2137 MB |
| diy-ascent-redb | rust_bfs | 6 ms | 1.0 MB | 5 ms | 13 µs | 3.0 µs | 128 µs | unsupported | 222 µs | 4.0 ms / 4.2 ms | 0.8 µs / 1.4 µs / 22 µs | 3.0 ms / 4.1 ms | 103 MB |

Notes:
- cozo-mnestic-mem shortest_path: expected Some(9), got Some(9)
- cozo-mnestic-mem scc: nontrivial SCCs expected 6, got Some(6)
- cozo-mnestic-sqlite shortest_path: expected Some(9), got Some(9)
- cozo-mnestic-sqlite scc: nontrivial SCCs expected 6, got Some(6)
- cozo-mnestic-newrocksdb shortest_path: expected Some(9), got Some(9)
- cozo-mnestic-newrocksdb scc: nontrivial SCCs expected 6, got Some(6)
- lbug-disk shortest_path: expected Some(9), got Some(9)
- lbug-disk scc: nontrivial SCCs expected 6, got None
- lbug-disk correct_incremental_eq_full[cypher_varlen]: after 200 updates: 0/46 differ from full rebuild, 1/46 differ from reference
- lbug-disk correct_vs_reference_initial[cypher_varlen]: 2 mismatching queries of 12
- lbug-mem shortest_path: expected Some(9), got Some(9)
- lbug-mem scc: nontrivial SCCs expected 6, got None
- lbug-mem correct_incremental_eq_full[cypher_varlen]: after 200 updates: 0/46 differ from full rebuild, 1/46 differ from reference
- lbug-mem correct_vs_reference_initial[cypher_varlen]: 2 mismatching queries of 12
- diy-ascent-redb shortest_path: expected Some(9), got Some(9)
- diy-ascent-redb scc: nontrivial SCCs expected 6, got Some(6)

#### medium: 50,000 symbols / 240,465 edges

Targets (depth-10 reach): hub = 49,890, p99 = 49,886, median = 230 nodes.

##### Blast radius by traversal strategy (p50 / p95)

| engine | strategy | d3 median | d10 median | d10 p99 | d10 hub | d10 hub trusted | correct |
|---|---|---|---|---|---|---|---|
| cozo-mnestic-mem | recursive | 69 µs / 75 µs | 365 µs / 392 µs | 129.0 ms / 133.4 ms | 124.8 ms / 126.4 ms | 128.7 ms / 130.6 ms | ✅✅ |
| cozo-mnestic-mem | rust_bfs | 64 µs / 67 µs | 393 µs / 414 µs | 79.2 ms / 82.9 ms | 78.5 ms / 79.2 ms | 85.1 ms / 87.7 ms | ✅✅ |
| cozo-mnestic-mem | unrolled | 232 µs / 345 µs | 2.2 ms / 2.5 ms | 371.3 ms / 374.5 ms | 379.1 ms / 383.6 ms | 391.1 ms / 398.0 ms | ✅✅ |
| cozo-mnestic-sqlite | recursive | 110 µs / 122 µs | 874 µs / 994 µs | 313.4 ms / 331.3 ms | 323.5 ms / 339.5 ms | 331.3 ms / 341.6 ms | ✅✅ |
| cozo-mnestic-sqlite | rust_bfs | 120 µs / 132 µs | 1.0 ms / 1.0 ms | 254.0 ms / 258.9 ms | 250.9 ms / 251.9 ms | 265.1 ms / 272.3 ms | ✅✅ |
| cozo-mnestic-sqlite | unrolled | 294 µs / 400 µs | 3.7 ms / 3.9 ms | 1084.3 ms / 1091.2 ms | 1170.6 ms / 1224.2 ms | 1100.9 ms / 1110.6 ms | ✅✅ |
| cozo-mnestic-newrocksdb | recursive | 79 µs / 86 µs | 589 µs / 622 µs | 177.7 ms / 179.2 ms | 179.6 ms / 181.0 ms | 181.9 ms / 184.6 ms | ✅✅ |
| cozo-mnestic-newrocksdb | rust_bfs | 81 µs / 86 µs | 649 µs / 681 µs | 129.7 ms / 130.9 ms | 131.3 ms / 136.5 ms | 134.6 ms / 138.7 ms | ✅✅ |
| cozo-mnestic-newrocksdb | unrolled | 262 µs / 366 µs | 2.9 ms / 3.1 ms | 593.8 ms / 599.9 ms | 616.2 ms / 621.2 ms | 615.2 ms / 627.6 ms | ✅✅ |
| lbug-disk | cypher_shortest | 714 µs / 810 µs | 903 µs / 998 µs | 62.3 ms / 63.6 ms | 61.8 ms / 62.9 ms | 66.0 ms / 66.7 ms | ✅✅ |
| lbug-disk | rust_bfs | 61.6 ms / 62.3 ms | 205.5 ms / 206.5 ms | 268.1 ms / 270.5 ms | 267.3 ms / 271.0 ms | 316.6 ms / 317.7 ms | ✅✅ |
| lbug-mem | cypher_shortest | 721 µs / 797 µs | 891 µs / 960 µs | 63.2 ms / 65.3 ms | 62.8 ms / 63.7 ms | 68.4 ms / 69.0 ms | ✅✅ |
| lbug-mem | rust_bfs | 63.4 ms / 64.5 ms | 209.8 ms / 210.9 ms | 274.1 ms / 276.3 ms | 275.2 ms / 280.7 ms | 324.2 ms / 327.8 ms | ✅✅ |
| diy-ascent-redb | ascent_per_query | 6.6 ms / 7.1 ms | 6.7 ms / 7.2 ms | 13.9 ms / 15.8 ms | 13.7 ms / 15.2 ms | 13.4 ms / 15.9 ms | ✅✅ |
| diy-ascent-redb | rust_bfs | 1.8 µs / 1.8 µs | 4.1 µs / 4.3 µs | 1.3 ms / 1.6 ms | 1.3 ms / 1.4 ms | 1.5 ms / 1.7 ms | ✅✅ |

##### Everything else (best correct strategy for blast)

| engine | best strategy | build | disk | reopen | cold 1st query | shortest path | SCC | Louvain | PageRank | update p50 / p95 | read while writing p50 / p99 / max | write while reading p50 / p99 | peak RSS |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| cozo-mnestic-mem | rust_bfs | 338 ms | in-mem | — | 712 µs | 89.8 ms | 142.1 ms | 19266.1 ms | 138.7 ms | 192 µs / 224 µs | 628 µs / 808 µs / 1.5 ms | 620 µs / 794 µs | 789 MB |
| cozo-mnestic-sqlite | rust_bfs | 598 ms | 62.9 MB | 0 ms | 1.4 ms | 141.4 ms | 191.5 ms | 18618.0 ms | 185.8 ms | 1.4 ms / 1.9 ms | 2.2 ms / 3.3 ms / 7.0 ms | 2.2 ms / 3.3 ms | 599 MB |
| cozo-mnestic-newrocksdb | rust_bfs | 1209 ms | 30.2 MB | 251 ms | 962 µs | 108.0 ms | 159.5 ms | 18551.8 ms | 152.5 ms | 486 µs / 571 µs | 1.0 ms / 1.4 ms / 3.3 ms | 580 µs / 756 µs | 884 MB |
| lbug-disk | cypher_shortest | 136 ms | 9.1 MB | 7 ms | 7.3 ms | 8.2 ms | unsupported | unsupported | unsupported | 23.0 ms / 25.2 ms | 1.8 ms / 2.1 ms / 2.3 ms | 24.4 ms / 29.6 ms | 332 MB |
| lbug-mem | cypher_shortest | 67 ms | in-mem | — | 7.3 ms | 8.3 ms | unsupported | unsupported | unsupported | 19.9 ms / 22.2 ms | 970 µs / 1.3 ms / 1.6 ms | 21.3 ms / 26.7 ms | 312 MB |
| diy-ascent-redb | rust_bfs | 23 ms | 4.0 MB | 21 ms | 17 µs | 29 µs | 2.3 ms | unsupported | 4.6 ms | 3.9 ms / 4.1 ms | 3.8 µs / 5.5 µs / 279 µs | 3.0 ms / 5.0 ms | 99 MB |

Notes:
- cozo-mnestic-mem shortest_path: expected Some(11), got Some(11)
- cozo-mnestic-mem scc: nontrivial SCCs expected 53, got Some(53)
- cozo-mnestic-sqlite shortest_path: expected Some(11), got Some(11)
- cozo-mnestic-sqlite scc: nontrivial SCCs expected 53, got Some(53)
- cozo-mnestic-newrocksdb shortest_path: expected Some(11), got Some(11)
- cozo-mnestic-newrocksdb scc: nontrivial SCCs expected 53, got Some(53)
- lbug-disk shortest_path: expected Some(11), got Some(11)
- lbug-disk scc: nontrivial SCCs expected 53, got None
- lbug-mem shortest_path: expected Some(11), got Some(11)
- lbug-mem scc: nontrivial SCCs expected 53, got None
- diy-ascent-redb shortest_path: expected Some(11), got Some(11)
- diy-ascent-redb scc: nontrivial SCCs expected 53, got Some(53)

##### Process exit codes

```
2026-09-24T01:25:21Z cozo-bench mem real:continuum-strict exit=0 secs=30
2026-09-24T01:26:02Z cozo-bench sqlite real:continuum-strict exit=0 secs=41
2026-09-24T01:26:39Z cozo-bench newrocksdb real:continuum-strict exit=0 secs=37
2026-09-24T01:27:32Z lbug-bench disk real:continuum-strict exit=0 secs=53
2026-09-24T01:28:24Z lbug-bench mem real:continuum-strict exit=0 secs=52
2026-09-24T01:28:35Z diy-bench redb real:continuum-strict exit=0 secs=11
2026-09-24T01:29:12Z cozo-bench mem real:sensemesh-strict exit=0 secs=37
2026-09-24T01:29:58Z cozo-bench sqlite real:sensemesh-strict exit=0 secs=46
2026-09-24T01:30:39Z cozo-bench newrocksdb real:sensemesh-strict exit=0 secs=41
2026-09-24T01:31:48Z lbug-bench disk real:sensemesh-strict exit=0 secs=69
2026-09-24T01:32:56Z lbug-bench mem real:sensemesh-strict exit=0 secs=68
2026-09-24T01:33:11Z diy-bench redb real:sensemesh-strict exit=0 secs=15
2026-09-24T01:33:42Z cozo-bench mem real:continuum-permissive exit=0 secs=31
2026-09-24T01:34:20Z cozo-bench sqlite real:continuum-permissive exit=0 secs=38
2026-09-24T01:34:55Z cozo-bench newrocksdb real:continuum-permissive exit=0 secs=35
2026-09-24T01:35:48Z lbug-bench disk real:continuum-permissive exit=0 secs=53
2026-09-24T01:36:41Z lbug-bench mem real:continuum-permissive exit=0 secs=53
2026-09-24T01:36:52Z diy-bench redb real:continuum-permissive exit=0 secs=11
2026-09-24T01:37:36Z cozo-bench mem real:sensemesh-permissive exit=0 secs=44
2026-09-24T01:38:27Z cozo-bench sqlite real:sensemesh-permissive exit=0 secs=51
2026-09-24T01:39:13Z cozo-bench newrocksdb real:sensemesh-permissive exit=0 secs=46
2026-09-24T01:40:22Z lbug-bench disk real:sensemesh-permissive exit=0 secs=69
2026-09-24T01:41:29Z lbug-bench mem real:sensemesh-permissive exit=0 secs=67
2026-09-24T01:41:44Z diy-bench redb real:sensemesh-permissive exit=0 secs=15
2026-09-24T01:42:34Z cozo-bench mem small exit=0 secs=50
2026-09-24T01:43:33Z cozo-bench sqlite small exit=0 secs=59
2026-09-24T01:44:29Z cozo-bench newrocksdb small exit=0 secs=56
2026-09-24T01:45:47Z lbug-bench disk small exit=0 secs=78
2026-09-24T01:47:04Z lbug-bench mem small exit=0 secs=77
2026-09-24T01:47:15Z diy-bench redb small exit=0 secs=11
2026-09-24T01:49:28Z cozo-bench mem medium exit=0 secs=133
2026-09-24T01:52:16Z cozo-bench sqlite medium exit=0 secs=168
2026-09-24T01:54:35Z cozo-bench newrocksdb medium exit=0 secs=139
2026-09-24T01:56:39Z lbug-bench disk medium exit=139 secs=124
2026-09-24T01:58:42Z lbug-bench mem medium exit=138 secs=123
2026-09-24T01:59:22Z diy-bench redb medium exit=0 secs=40
2026-09-24T02:01:28Z lbug-bench disk medium (LBUG_SKIP_VARLEN=1) exit=0
2026-09-24T02:02:56Z lbug-bench mem medium (LBUG_SKIP_VARLEN=1) exit=0
```
