# CozoDB Health — Adopt, Fork, Own, or Reject?

> Research 2026-09-23. Evidence from gh API, crates.io API, OSV, shallow clones of upstream + forks, and a local build on Rust 1.94.0. Judged only by Graphite's three targets (agent execution speed > assertiveness > correctness) plus maintenance risk.

## Verdict

**Adopt the `mnestic` fork, pinned exactly, behind a thin Graphite storage module. Fallback if mnestic stalls: vendor and own (from mnestic or upstream `481af05`).**

Do **not** depend on upstream `cozo` 0.7.6: it builds and works today, but it is dormant and takes no fixes. Do not reject CozoDB: the engine is the right shape for Graphite, and the core is small enough to own if forced.

Gate before writing Graphite code on top of it: recursion benchmark (depth-10 blast radius, ~50K nodes / 250K edges, target <10 ms) plus a concurrent read-during-write test. None of this research measured either.

## 1. Upstream state (cozodb/cozo)

| Fact | Evidence |
|---|---|
| Last release | `v0.7.6`, 2023-12-11 (confirmed; crates.io `cozo` max_version 0.7.6, same date) |
| Last commit on `main` | `481af05`, 2024-12-04 — merge of PR #286 (prefix_join fix) and #290 from `cozo-community` (rust-rocksdb backend) |
| Commits since | None in ~22 months |
| Activity profile | Original author Ziyang Hu (`zh217`): 1,735 commits, heavy 2022 to early 2023, a trickle through 2024, then silence |
| Issues | 49 open. Recent ones unanswered: #312 (2026-07, silent validity-timestamp corruption), #307/#306 (2026-04), #298 (rayon breaks build), #285 (SQLite perf suggestions) |
| Abandonment signal | #301 "Is cozo still being maintained?" (2025-12-04): only 🕯️ replies, no maintainer answer. One reply points to mnestic. |
| Stars / forks | 4,123 / 167; not archived |
| crates.io | `cozo` 113.6K total downloads, 21.9K recent (90 days). Still used: 9 reverse deps, most published in 2026 |

**Reading:** maintenance mode with no maintainer. Not formally archived, but effectively abandoned since Dec 2024.

## 2. Forks and successors

167 forks. Of those pushed after Dec 2023, most are 0–2 commits ahead. Real divergence:

| Fork | Ahead | Last push | crates.io | What changed | Credible successor? |
|---|---|---|---|---|---|
| **shuruheel/mnestic** (hard fork, own repo) | 282 commits by the maintainer, 18 releases May–Sep 2026 (v0.10 → v0.18.0, 2026-09-05) | 2026-09-08 | `mnestic` 0.18.0, 1.8K downloads | Planner correctness fixes; a recursive-workload test suite (transitive closure, seeded reachability, same-generation, semi-naive chains); HNSW/FTS hardening; hybrid search; bitemporality; `:mem_limit`; RocksDB options that were silently discarded now apply; fixes the upstream #312 corruption (0.12.2); concurrency contract doc for SQLite vs RocksDB; its own `mnestic-mcp`. Keeps `[lib] name = "cozo"`, so the API is drop-in. MPL-2.0, provenance documented (`FORK.md`, fork base `481af05`). | **Yes — the only one.** Risks: bus factor 1 (one maintainer, 45 stars), very fast churn (breaking changes likely), aimed at agent memory rather than code graphs. |
| Ryan-AI-Studios/cozo-redux | 51 | 2026-09-03 | not published | Parallel joins (rayon), allocation cuts, HNSW PQ, sled → fjall, pyo3 CVE fixes, v0.8.1. Mostly bulk-generated "tracks" by one author in a few days. | No — unpublished, one author, big unreviewed diffs |
| lawless-m/cozo-redb | 43 | 2026-04-16 | not published | Pure-redb backend only; deletes rocksdb/sqlite/tikv/sled, HTTP server, FFI, callbacks, native FTS (swapped for tantivy). Rebranded, version reset to 0.1.0. | No — stalled since April; interesting as a slim "Rust-only" reference |
| aramallo/cozo | 34 | 2026-06-30 | not published | Temporal functions, RocksDB 10.9 upgrade + jemalloc, **RocksDB read-only queries on snapshots instead of transactions ("avoid lock_timeout errors")**, S3/Parquet archiving | No — app-specific; the snapshot-read fix is worth noting |
| fluminis-scientiae-oraculum/retia | 18 | 2026-06-28 | `retia` 0.2.0, 678 downloads | Rust-only rebrand, dependency refresh, drops sled/tikv, clears RUSTSEC-2026-0041, SQLite `busy_timeout` fix, CI | Minor — small, clean, but low activity |
| cozo-community/cozo | merged upstream | 2024-12-12 | `cozo-ce` 0.7.13-alpha.3 (2024-12) | rust-rocksdb backend (merged into upstream as #290) | No — dormant since Dec 2024 |

**Prior art on the same stack (outside this study's scope, worth a look):** crates depending on `cozo` include `infigraph-core` (intuit/infigraph, Rust, "code intelligence engine, 62 languages", 89★, active Sep 2026), `leankg` (FreePeak/LeanKG, 220★, active) and `ferrograph` (Rust code intelligence with CLI + MCP, 3★). All are code-graph tools built on CozoDB — direct precedent for Graphite's choice, and they may already hit the same limits.

## 3. crates.io, build, advisories

- **Builds on current stable.** `cozo = "=0.7.6"` with `storage-sqlite, graph-algo, rayon` builds clean on rustc 1.94.0 in 39 s (release, fresh) and runs a query. Issue #298 (rayon breaking `graph_builder`) did not reproduce: `rayon 1.12.0` + `graph_builder 0.4.2` resolved fine today. Latent risk remains: no upstream will fix the next break.
- **Stale dependencies:** `sqlite` 0.32 + `sqlite3-src` 0.5.1 (#261 asks for SQLite 3.45+), `base64` 0.21, `miette` 5, `env_logger` 0.10, `ndarray` 0.15, `chrono-tz` 0.8, `tikv-client` 0.3, `sled` 0.34, `cozorocks` (C++ RocksDB binding).
- **OSV advisories in the resolved tree (210 crates):**
  - RUSTSEC-2026-0041 `lz4_flex` 0.10.0 — decompressing invalid data can leak uninitialized memory. It comes in via `swapvec`, which only handles data the engine wrote itself: low risk for Graphite.
  - Unmaintained notices: `adler` (RUSTSEC-2025-0056), `bincode` 1.3.3 (RUSTSEC-2025-0141), `fxhash` (RUSTSEC-2025-0057), `smartstring` (RUSTSEC-2026-0249).
  - No exploitable issue in Graphite's use. Every one is fixed by a dependency bump that upstream will never ship. mnestic and retia have done this work.

## 4. Things that matter for Graphite

| Concern | Finding | Target hit |
|---|---|---|
| Recursion on dense graphs | Semi-naive evaluation plus magic sets. No upstream issue reports recursion blowups, but no benchmark exists either. mnestic added a recursive-workload planner test suite, which protects plan shape but doesn't measure our graph size. **Must benchmark.** | Speed |
| Concurrent read during write | **SQLite and mem backends take a process-wide lock:** `transact(write=true)` holds a `ShardedLock` write guard for the whole write transaction, so every read waits (`storage/sqlite.rs:27,72-76`, `storage/mem.rs:53-69`). RocksDB reads use snapshot transactions (`rocks.rs:133`). A fork (aramallo) moved read-only queries to snapshots to stop lock timeouts. Implication: the watcher must commit in small per-file transactions, or Graphite uses RocksDB/newrocks. Needs a read-during-write test. | Speed |
| SQLite backend performance | #285: prepared statements rebuilt per transaction, rowid tables (upstream never answered). mnestic tracks WITHOUT ROWID (#6). | Speed |
| Memory | `:mem_limit` exists only in mnestic. RocksDB memory limits requested upstream (#302), unanswered. | Speed |
| Graph algorithms built in | PageRank, Louvain, label propagation, SCC, topological sort, BFS/DFS, shortest path (BFS, Dijkstra, A*, Yen k-shortest), all-pairs shortest path, degree centrality, triangles, random walk, MST. **No Leiden.** | Assertiveness (communities), correctness (cycles, paths) |
| FTS / vectors | Native FTS (the 29K-line `fts/` directory is mostly tokenizer dictionaries) and HNSW. mnestic hardened both and added hybrid search. | Speed |
| Known correctness bug | #312: an integral float in a validity timestamp is silently written as 1970. Unfixed upstream, fixed in mnestic 0.12.2. Irrelevant unless Graphite uses validity/time travel. | Correctness |

## 5. Owning it ourselves

- **Size:** upstream `cozo-core/src` is 65.9K lines, of which about 29K is `fts/`, mostly tokenizer dictionary data. The engine proper is ~36K lines: `data` 11K, `runtime` 7.6K, `query` 6.8K, `fixed_rule` 4.4K, `parse` 3.2K, `storage` 3.1K. mnestic's `cozo-core` is 96K lines, so it has grown ~46%.
- **Complexity:** a real Datalog engine (pest grammar, stratification, magic-set rewrite, semi-naive fixpoint, relational algebra, storage trait). Hard to extend deeply, but Graphite would only touch a narrow surface: one storage backend, recursive rules, a few fixed rules, and dependency bumps.
- **Dependency upgrade effort:** moderate. retia and mnestic show that one person can clear the advisories and refresh the dependency tree in days.
- **Verdict on owning:** feasible as a **fallback**, not a first choice. Owning a Datalog engine takes attention away from Graphite's actual job.

## Recommendation, concretely

1. **Depend on `mnestic` pinned exactly (`=0.18.x`)**, `default-features = false`, only the backend + `graph-algo` we need. Since `[lib] name = "cozo"`, switching back to upstream or to our own vendored copy is a Cargo.toml change.
2. **Isolate the engine:** every Datalog script and every `DbInstance` call lives in one Graphite module (`store/`). Nothing else imports `cozo`. This keeps the vendor-and-own path cheap.
3. **Gate before committing:**
   - recursion benchmark: 50K nodes / 250K edges, depth-10 blast radius, target <10 ms;
   - read latency while the watcher writes, per backend (sqlite / mem / newrocks);
   - Louvain quality on a real repo, since there's no Leiden.
4. **Trigger for vendoring:** mnestic makes no release for 90 days, or a breaking change hits the surface Graphite uses, or it drifts away from what Graphite needs. Then vendor from the last good mnestic tag.
5. **Look at infigraph / LeanKG / ferrograph** before scaffolding. They are code-graph tools already on CozoDB and may have hit these limits already.
