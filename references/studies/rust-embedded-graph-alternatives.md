# Rust-Embeddable Graph Storage — CozoDB Health & Alternatives

> Research 2026-09-23. Judged only by Graphite's targets (agent execution speed > assertiveness > correctness) plus maintenance risk. Stars/dates from GitHub API and crates.io on this date.

## 1. CozoDB health

| Signal | Value |
|--------|-------|
| Last release | v0.7.6 — 2023-12-11 (crate `cozo`, 113K downloads) |
| Last commit on main | 2024-12-04 (merged PRs from `cozo-community`) |
| Open issues | 49 |
| Stars | 4.1K |
| Community takeover | `cozo-community` org (Julep's maintainer) published `cozo-ce` 0.7.13-alpha.3 on 2024-12-08, then stopped. All `cozo-community` repos are forks, last push Dec 2024. |
| Other forks | ~a dozen with 2026 activity, all 0 stars, personal (e.g. `lawless-m/cozo-redb` redb backend, `Ryan-AI-Studios/cozo-redux`, `putao520/frogdb`). No credible successor. |

**Verdict:** abandoned upstream, no successor. Adopting it means **owning a fork**.

**What we'd inherit (still unmatched in Rust):** embedded Datalog with recursion + stratified negation + aggregation; persistent backends (mem / SQLite / RocksDB / sled); fixed rules built in: PageRank, Louvain, SCC, connected components, BFS/DFS, Dijkstra, Yen k-shortest, betweenness/closeness, label propagation, topological sort; FTS index and HNSW vector index (0.7); MVCC transactions; time travel. Pure Rust.

**Cost of owning it:** dependency refresh (old rocksdb/sqlite bindings), 49 unresolved issues, no upstream bugfixes, performance tuning is ours. Mitigation: vendor at a pinned commit, hide it behind a `GraphStore` trait so a swap stays possible, run the benchmark in §4 before committing.

## 2. Comparison

| Candidate | Lang / embed | Maintenance | Query model | Recursion | Algorithms | Concurrency | FTS / vector | Fit |
|---|---|---|---|---|---|---|---|---|
| **CozoDB** | Rust crate | Dead since 2024-12 (4.1K★) | Datalog (rules, recursion, aggregation) | Native recursive rules, shallowest-depth via `min` agg | PageRank, Louvain, SCC, shortest paths, centralities built in | Single process, MVCC; concurrent reads with RocksDB | FTS + HNSW | Best feature fit; maintenance is the risk |
| **LadybugDB** (Kuzu fork, crate `lbug`) | C++ core, Rust bindings | Very active (release 2026-09-10, 1.8K★, crate 825K dl) | Cypher | Var-length `*1..N` (default max 30, configurable), `SHORTEST`, `ALL SHORTEST`, weighted; WALK/TRAIL/ACYCLIC | Algo extension: PageRank, WCC/SCC, Louvain (parallel, Grappolo), k-core | One READ_WRITE Database per file in one process, many connections, serializable txns — fits daemon | Native FTS + vector | Strong; Cypher not Datalog → derived edges are queries/Rust, not rules |
| Kuzu (original) | C++ / crate `kuzu` | **Archived 2025-10** (Apple acquisition) | Cypher | as above | as above | as above | yes | Dead; use LadybugDB |
| Vela kuzu fork | C++ | 44★, 2026-07 | Cypher | as above | as above | Adds concurrent writers | yes | Niche; watch only |
| **Ascent** (+ own persistence) | Rust macro-compiled Datalog | Active (2026-08, crate 538K dl) | Datalog compiled to Rust at build time | Very fast in-memory fixpoint; lattices for min-depth | None built in (write as rules or use petgraph) | In-memory; we own it | none (add tantivy) | Fastest rule engine; persistence, FTS, algos all DIY |
| Datafrog | Rust lib (Polonius) | Active, minimal | Datalog-ish joins, manual | Fast semi-naive loops | none | in-memory | none | Lower-level Ascent; only as engine core |
| Differential Dataflow | Rust lib | Active (3K★) | Incremental dataflow / Datalog | Incremental recursion (iterate) | build yourself | in-memory, multi-worker | none | Only engine giving **true incremental == full**; high complexity; v2 option |
| crepe | Rust macro Datalog | Low (2025-12) | Datalog | in-memory | none | in-memory | none | Ascent does more |
| DDlog | Rust/Haskell | **Archived 2023** | incremental Datalog | — | — | — | — | Dead |
| Soufflé | C++ compiler | Active | Datalog → C++ | Very fast batch | none | batch, not live store | none | Not embeddable as live store |
| **petgraph + redb / fjall** (DIY) | Rust | petgraph 2026-09 (4K★), redb 2026-09 (4.8K★), fjall 2026-09 (2.3K★) | Imperative Rust | BFS/Dijkstra/SCC in memory, microseconds | Dijkstra, SCC, toposort, PageRank (petgraph); no Louvain | redb: MVCC, 1 writer + concurrent readers | add tantivy | Maximum speed & control; no declarative rules |
| SurrealDB embedded | Rust crate (kv-rocksdb / surrealkv) | Very active (33K★) | SurrealQL | `{1..N}` recursion exists; vendor guidance "2–3 hops comfortable" | few | MVCC | FTS + vector | Heavy dependency, weak deep recursion |
| Oxigraph | Rust crate | Active (2026-09) | SPARQL / RDF | Property paths `+`/`*` (no depth, no shortest path) | none | RocksDB, concurrent reads | none | Wrong model |
| TerminusDB | Prolog + Rust store | Active | WOQL/GraphQL | recursion via Prolog | few | server | no | Server-oriented; no |
| IndraDB | Rust lib/server | Low (release 2025-08) | Imperative pipe queries | Manual hops | none | RocksDB | no | Too thin |
| DuckDB + DuckPGQ | C++ via `duckdb-rs` | Active | SQL + SQL/PGQ | `ANY SHORTEST`, recursive CTE `USING KEY` | few (PGQ path finding) | 1 writer process | FTS ext + vss ext | Community extension distribution/bundling risk; analytical engine, OK read perf |
| SQLite recursive CTE (rusqlite) | C via rusqlite | Active | SQL | code-graph-mcp measured **22.8 s at depth 10** on a 55-node graph → they moved traversal to Rust | none | WAL: 1 writer + readers | FTS5 | Rejected by evidence |
| HelixDB | Rust | Active (6K★) | HelixQL | graph traversal | — | **Server-only now** (HTTP, object storage) | vector + FTS | Not embeddable |
| Minigraf | Rust crate | 35★, 1.0 in 2026-05 | Bi-temporal Datalog | recursive rules | none | single file | none | Too young |

## 3. Top 3

### 1. CozoDB — vendored, owned fork
Only candidate that delivers everything Graphite's design already assumes: persistent embedded Datalog, recursive rules for "incremental == full" (store per-file facts, derive edges/confidence at read), built-in Louvain/PageRank/shortest path, FTS + vector. Serves correctness most directly (derived-at-read means no drift, the code-graph-mcp bug class that caused 71 index bumps). Risk is maintenance, not capability. Adopt only if the benchmark hits the <10 ms depth-10 target; keep it behind a `GraphStore` trait.

### 2. LadybugDB (`lbug`)
The actively maintained embedded graph DB with the strongest engine: columnar CSR adjacency, parallel recursive joins, full shortest-path family, algo extension with Louvain/PageRank, FTS + vector. Concurrency model (one read-write Database, many connections) matches the one-daemon-per-repo decision. Costs: C++ build in the Rust binary (heavier compile, larger binary), Cypher instead of Datalog (derived edges must be computed in Rust or materialized, so incremental == full needs discipline), young governance after Kuzu's archival; GitNexus's LadybugDB pain (509 typed rel-table pairs, crashes on missing pairs) applies unless schema uses one generic node/edge table.

### 3. DIY: Ascent + redb (+ petgraph, tantivy)
Fastest possible query path: graph and rules in memory, compiled Datalog, microsecond traversals; redb gives durable per-file facts with MVCC; petgraph covers SCC/Dijkstra/PageRank; Louvain written ourselves; tantivy for symbol search. Every component is actively maintained pure Rust. Cost: we build persistence mapping, rule re-evaluation strategy, and algorithms — most engineering, least external risk. Differential Dataflow is the upgrade path if full recompute of derived rules becomes too slow.

## 4. What the benchmark must compare

Candidates: CozoDB (mem, SQLite, RocksDB backends), LadybugDB, Ascent + redb. Optional baseline: petgraph in memory (floor).

Datasets:
- Synthetic code-shaped graph: 50K symbols / 250K edges, power-law fan-in, 3 edge kinds, ~10% INFERRED.
- One real indexed repo (e.g. a cloned reference repo) for realism.

Queries (p50 / p95, warm and cold):
1. Blast radius upstream, depth 10, shallowest depth per node — target < 10 ms
2. Direct callers of one symbol (depth 1)
3. Shortest path between two symbols
4. Cycle detection (SCC) over whole graph
5. Louvain and PageRank over whole graph
6. Derived-at-read query: edge confidence computed by rule, joined into blast radius
7. Symbol-name search (FTS)

Write-path:
8. Incremental update of one file (delete + insert ~50 symbols / ~250 edges): latency
9. Read p95 while the watcher writes continuously (1 file/100 ms)
10. Incremental result == full rebuild result (correctness check)

Operational: cold open time, RSS, on-disk size, binary size delta, clean-build time.

Decision rule: pick the fastest candidate that passes #1 and #10; maintenance risk breaks ties.

## Sources

- GitHub API / crates.io (2026-09-23): cozodb/cozo, cozo-community/*, LadybugDB/ladybug, kuzudb/kuzu, oxigraph, surrealdb, HelixDB/helix-db, s-arash/ascent, TimelyDataflow/differential-dataflow, rust-lang/datafrog, petgraph, redb, fjall, duckdb-rs, duckpgq-extension, rusqlite, project-minigraf/minigraf
- [LadybugDB README](https://github.com/LadybugDB/ladybug), [Concurrency](https://docs.ladybugdb.com/concurrency/), [Algo extension](https://docs.ladybugdb.com/extensions/algo/), [Louvain](https://docs.ladybugdb.com/extensions/algo/louvain/), [MATCH / recursive](https://docs.ladybugdb.com/cypher/query-clauses/match/)
- [From Kuzu to Ladybug](https://thedataquarry.com/blog/from-kuzu-to-ladybug/), [Kuzu archived: pin, fork or migrate](https://oneuptime.com/blog/post/2026-08-12-kuzu-archived-pin-0-11-3-fork-or-migrate/view), [dbdb.io LadybugDB](https://dbdb.io/db/ladybugdb)
- [HelixDB README](https://github.com/HelixDB/helix-db)
- [SurrealDB multi-hop traversal](https://surrealdb.com/blog/multi-hop-graph-traversal-inside-surrealdb)
- [Minigraf 1.0](https://dev.to/adityamukho/minigraf-10-an-embedded-bi-temporal-datalog-database-in-rust-4bg5)
- `references/studies/code-graph-mcp.md` (SQLite recursive CTE 22.8 s evidence), `references/studies/gitnexus.md` (LadybugDB schema pain)
