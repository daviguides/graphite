# CozoDB-Based Code-Graph Tools — Study

> Study date: 2026-09-23. Three tools that use (or used) CozoDB for code graphs. Judged only by Graphite's targets: agent execution speed > assertiveness > correctness.
> Cloned (depth 1) under `references/repos/`: `infigraph/` (intuit/infigraph, 89★, last commit 2026-09-13), `leankg/` (FreePeak/LeanKG, 220★, last commit 2026-09-21), `ferrograph/` (GarthDB/ferrograph, 3★, last commit 2026-09-15).

## TL;DR — what these three tell us about CozoDB

| Tool | CozoDB today | Recursion in Datalog? | Verdict on Cozo |
|---|---|---|---|
| **ferrograph** | Active store, `cozo = "0.7"`, `mem` or `sqlite` | **Yes** — real recursive rules for blast radius, callers, dead code, module reachability | Works for a small Rust-only tool; no perf data |
| **infigraph** | **Parked** second backend (`cozo 0.7.6`, `storage-sqlite`); production runs on LadybugDB `lbug =0.16.0` | **No** — recursion manually unrolled into `layer_1..layer_N` rules "for index use" | Evaluated head-to-head (`cozo_vs_kuzu` bin), chose Kuzu/Ladybug; results not published |
| **LeanKG** | **Removed.** Rust+Cozo (216 Datalog call sites) → Go + plain SQL (SQLite WAL / PG) | **No** — "the recursive Datalog class is EMPTY (measured)"; all traversal was Rust-side BFS | Left because of single-writer file ownership, FFI-abort crashes under concurrent write+read, translator tax |

**The uncomfortable finding:** two of three serious CozoDB users never used recursive Datalog in production — the one feature that justifies CozoDB for Graphite. infigraph unrolled it by hand for index use; LeanKG did BFS in Rust. Only ferrograph (3★, Rust-only, no benchmarks) runs real recursion. **No tool in the landscape has published evidence that CozoDB recursion meets our <10 ms depth-10 target.** The benchmark is not optional.

---

## 1. Feature Inventory

Verdicts: **v1** / **v2** / **skip**. Target: **S** speed, **A** assertiveness, **C** correctness.

### ferrograph (Rust, 10 MCP tools)

| Feature | What it does | Target | Verdict |
|---|---|---|---|
| `blast_radius` | Seeds immediate neighbors both directions (calls/contains/refs/changes_with), then recursive forward expansion over calls/refs/changes_with; `:limit 500` (`src/graph/query.rs:248-276`) | S, C | v1 (shape) — but add depth + shallowest-depth + truncation disclosure |
| `callers` with depth | Transitive callers to N hops, capped at 100 depth / 500 rows (`query.rs:284-309`) | S, C | v1 |
| **Git change-coupling edges** (`changes_with`) | Mines git history, adds co-change edges that participate in blast radius (`src/pipeline/git_coupling.rs:16-110`) | C | **v1** — this is the "git-aware temporal edges" gap from landscape, already solved here; also the co-change signal Laya's deterministic baseline needs |
| Dead-code detection | Datalog fixed point: reachable from entry points (main, pub, test, bench) via calls+refs; `not reachable` (`query.rs:198-239`) | C | v2 |
| `trait_implementors`, `module_graph` | Trait→impl and module containment views | A | v1 (as part of explore/context) |
| Rust ownership edges | `owns`, `borrows`, `borrows_mut` edges (`pipeline/owns.rs`, `borrows.rs`) | C | v2 (Rust-specific; valuable for Rust repos) |
| Macro expansion phase | Edges through macro-generated code | C | v2 |
| `query` (raw Datalog) | Read-only; mutation directives blocked at parse time (`src/mcp/mod.rs:654`) | A | v2 (human/UI only, not agent tool) |
| `reindex`, `status`, `search`, `node_info` | Basics | S | v1 (`node_info` ≈ context) |
| Watch mode | Debounced full pipeline re-run on change (`src/watch.rs:17-82`) | S | skip (full re-run; our watcher is per-file incremental) |
| Claude skill + Datalog cookbook | Ships `.claude/skills/ferrograph/SKILL.md` | S | v1 (skill that teaches the agent the tool) |

### infigraph (Rust, ~90 MCP tools)

| Feature | What it does | Target | Verdict |
|---|---|---|---|
| Unified `search` "PRIMARY" | BM25+vector hybrid + semantic hybrid + regex grep in one call, merged/deduped, auto-escalates when weak; compact by default, `detail=true` for snippets (`crates/infigraph-mcp/src/lib.rs:378-379`) | S | **v1** — one search call replaces grep loops |
| `trace_callers` / `trace_callees` | Direct callers with file:line; `include_tests=false`; **warns when sibling-method callers are excluded**, `expand_interface=true` to aggregate (`lib.rs:390-392`) | C, A | **v1** — the "response says what it didn't include" pattern |
| `transitive_impact` | Blast radius to depth N | S, C | v1 |
| `find_all_references` | Every reference, grouped by file, `detail` per-line context (`lib.rs:456`) | C | v1 (rename safety) |
| `detect_changes` / `semantic_diff` | Diff → affected symbols | S, C | v1 (= our `diff_impact`) |
| `get_skeleton` with quality annotations | Signatures + `# complexity \| nesting \| stmts \| fan-in` per function | S, A | v1 — fan-in on the skeleton tells the agent how careful to be |
| `get_test_coverage`, `tested_by` derived edges, `generate_test_context` | Test↔symbol mapping, derived edge pass (`cozo_store.rs:1440`) | S, C | v1 (runner VALIDATING) |
| SCIP import / LSP→SCIP bridge | Compiler-grade reference edges on top of tree-sitter (`crates/lsp-to-scip`) | C | v2 — strongest precision upgrade available |
| Output compression with levels + stats | `compress.rs`, `CompressionLevel`, `get_compression_stats` | S | v1 (tiered output) |
| Session memory | `save_session`, `get_latest_session`, `search_sessions`, `memory_context` | S | v2 (cross-session continuity) |
| Multi-repo groups | `group_*` tools, cross-service contracts, `calls_service` edges with method/path/protocol | C | v2 |
| Analysis passes | taint, concerns, config bindings, reflection, dynamic URLs, path traversal, clones | C | skip for v1 (security scanner scope) |
| Structured ingestion | TOML-defined node/edge tables from JSON/YAML | — | skip |
| Custom edge DDL per plugin | Schema extensible per language plugin (`create_custom_edge`, `cozo_store.rs:1361`) | C | v2 |
| ~90 tools listed | Every capability is a tool | — | **avoid** — contradicts measured agent behavior (few tools win) |
| Two-step symbol lookup | Tools require exact `symbol_id` from `search` first | — | **avoid** — costs a turn; accept name + disambiguate in response |

### LeanKG (Go now; Rust+Cozo until v0.30)

| Feature | What it does | Target | Verdict |
|---|---|---|---|
| **3-tool surface** | `import` / `query` / `status`, verbs as action namespace inside each tool | S | v1 (validates few-tools) |
| **Query ladder with provenance** | L1 exact → L2 fuzzy → L3 semantic (ANN) + L0 cold; every response carries `retrieval{rung, reason}`; below-confidence-floor degrades gracefully | A, S | **v1** — agent knows *how* an answer was found |
| Router L1-first | Identifier queries answer from exact before ANN (latency 14–31 s → 2.3 s warm) | S | v1 |
| Token budget with post-truncation accounting | Budget enforced after truncation, reported | S, A | v1 |
| **DB-resident freshness watermarks** | `write_watermark(seq, at)` in the DB replaces per-process TTL caches; kills cache-race bug class | C | **v1** — fits one-daemon design; watermark goes in every response |
| Writer/reader role split | `serve` (readers) vs `writer` (watcher/indexer/embedder owns RW) | S, C | v1 (same conclusion as code-graph-mcp study: one writer daemon) |
| Stale-element sweep | Incremental sync removes elements for files that left the collection set | C | v1 |
| Audit ledger (hash-chained) | Records writes | — | skip |
| Agent memory banks | mnemopi-compatible JSONL | — | v2 |

---

## 2. CozoDB Schemas and Queries

### ferrograph — minimal generic schema (`src/graph/store.rs:47-73`)

```
:create nodes { id: String => type: String, payload: String? }
:create edges { from_id: String, to_id: String, edge_type: String }
:create dead_functions { id: String }
```

- Node IDs are position-based: `./src/main.rs#line:col` — **not stable across edits** (a line shift re-keys every symbol below it).
- Edge type is a string column inside the key; recursion filters with `et in [...]`.
- Backend: `mem` or `sqlite`. Writes chunked at 100 rows per script (`store.rs:120+`).

Blast radius (`query.rs:259-268`):
```
seed[to]   := *edges[from, to, et], from = $from, et in [calls, contains, refs, changes_with]
seed[from] := *edges[from, to, et], to = $from,   et in [...]
reachable[id] := seed[id]
reachable[to] := reachable[n], *edges[n, to, et], et in [calls, refs, changes_with]
?[id, type, payload] := reachable[id], *nodes[id, type, payload], id != $from
:limit 500
```

Callers with depth (`query.rs:293-299`):
```
callers[caller, depth] := *edges[caller, callee, "calls"], callee = $target, depth = 1
callers[caller, d1] := *edges[caller, prev, "calls"], callers[prev, d], d1 = d + 1, d1 <= $max_depth
```
⚠️ Keying `callers` by `(caller, depth)` materializes every depth at which a node is reachable — on dense graphs this multiplies rows. Graphite should keep **shallowest depth per node** (aggregate `min(depth)` or semi-naive with a `seen` guard).

### infigraph — rich typed schema (`crates/infigraph-core/src/graph/cozo_store.rs:1697-1775`)

Nodes: `symbol{id => name, kind, file, start_line, end_line, signature_hash, language, visibility, parent, docstring, complexity, parameters, return_type, category}`, `module{id => name, file, language, content_hash, summary}`, `file`, `folder`, `cluster`, `dependency`, `statement`, `concern`, `config_binding`, `externalref`.

Edges (one relation per kind, key = both ends): `calls{caller, callee, line}`, `imports`, `contains`, `inherits`, `tested_by`, `reads_rel`, `writes_rel`, `member_of`, `similar_to{score}`, `bridge_to`, `defines{file_id, symbol_id}`, `calls_service{method, path, protocol, ...}`, `has_statement`, `resolves_to{mechanism}`, `taint_flow`, ...

Reverse indexes on every edge's second column (`::index create calls:calls_by_callee {callee}` etc., `cozo_store.rs:1751-1775`) — **required**, since Cozo stored relations are ordered by key prefix only.

Materialized helpers: `meta_cache{key => val}`, `testable_cache{id}` rebuilt with `:replace` (`cozo_store.rs:1322-1359`).

Transitive impact — **recursion unrolled by hand** (`cozo_store.rs:253-281`):
```
layer_1[caller] := *calls{caller, callee: $target}
layer_2[caller] := layer_1[mid], *calls{caller, callee: mid}
...
layer_N[caller] := layer_{N-1}[mid], *calls{caller, callee: mid}
?[id, name, file, kind] := layer_d[id], *symbol{id, name, file, kind}   # union over d
```
Comment: "Unroll recursion: bind callee from previous layer first for index use." Implies the recursive form did not bind to the `calls_by_callee` index as they wanted. No dedup across layers — a node reachable at depth 2 and 5 appears twice.

Batch lookups via inline relations: `targets[id] <- [["a"], ["b"]]` then join (`cozo_store.rs:1262-1320`) — one script for N symbols instead of N round-trips.

Incremental: `upsert_file = delete_file_data + insert_file_data` (`cozo_store.rs:993-1086`). Delete is **~12 separate `:rm` scripts**, each a separate transaction, errors ignored (`let _ =`). Not atomic — a crash mid-delete leaves a half-removed file.

### LeanKG — Cozo era (from `docs/prd.md:399-425` and issue #365)

- `cozo = 0.7.6` on `storage-sqlite`, one DB per project `.leankg/leankg.db`, HNSW vectors via Cozo's own `::hnsw` (cosine, dim 384).
- 216 Datalog call sites; **none recursive**. Impact radius, shortest path and NL-query traversal were Rust BFS over indexed per-hop lookups.

---

## 3. Problems They Hit With CozoDB

| Problem | Where | Consequence for Graphite |
|---|---|---|
| **Single-writer file ownership**, no WAL control | LeanKG #365 §4 ("CozoDB owns the SQLite file single-process") | Confirms cozodb-health finding (write lock blocks reads on sqlite/mem). One writer daemon + RocksDB or tiny per-file txns |
| **Process abort, no panic trace, during concurrent write + read** | LeanKG #286, #321 (exit code 1 mid-embed, index left partial) | Must stress-test concurrent watcher writes + queries in the benchmark; keep embedding/ML out of the DB process |
| Recursive rules did not use the reverse index as desired → hand-unrolled layers | infigraph `cozo_store.rs:256` | Benchmark must compare recursive rule vs unrolled layers vs Rust BFS over indexed lookups |
| `:insert` rejects duplicate keys | LeanKG PR #284 | Use `:put` for upserts |
| Short positional `*rel[col]` invalid on multi-column relations | LeanKG #288 | Always use named `*rel{col}` form |
| `\"` rejected inside double-quoted strings (0.7.6) | LeanKG `prd.md:393` | Always pass values as `$params`, never string-interpolate (infigraph's `batch_callers` interpolates — injection/escaping risk) |
| No `str_starts_with` | ferrograph `query.rs:202` | Some filtering moves to Rust; minor |
| Rayon ≥1.11 breaks `graph_builder 0.4.1` (Cozo graph algos) | ferrograph `Cargo.toml:17` | Pin rayon or use mnestic fork; verify in benchmark build |
| Non-atomic multi-script delete | infigraph `cozo_store.rs:1004-1086` | One script per file update (chained queries in one transaction) |
| Translator tax (Datalog ↔ SQL for dual backend) | LeanKG (~5.7k LOC dead weight) | Don't run two query languages; one engine |

Backend choice: all three chose **sqlite** (ferrograph also `mem`). Nobody ran RocksDB — yet RocksDB is the backend that reads from snapshots. Untested territory for code graphs.

---

## 4. What to Steal / What to Avoid

### Steal
1. **Git change-coupling edges** (ferrograph) — co-change as a first-class edge kind in blast radius. v1. Also feeds the deterministic ranker.
2. **"Response says what it excluded"** (infigraph `trace_callers` sibling-method warning) — same family as GitNexus `epistemic` and code-graph-mcp truncation disclosure. Three independent tools converged on it.
3. **Query ladder with provenance** (LeanKG `retrieval{rung, reason}`) — every answer says how it was found (exact / fuzzy / semantic / graph).
4. **DB-resident freshness watermark** (LeanKG) — `graph_rev`/watermark in DB, returned in every response; no per-process caches.
5. **Fan-in / complexity annotations on skeletons** (infigraph) — cheap assertiveness signal.
6. **Batch lookups via inline relations** (infigraph `targets[id] <- [...]`) — one script, N symbols.
7. **Reverse index on every edge** (infigraph) — mandatory in Cozo.
8. **Unified search** (infigraph) — one call, all retrieval modes, compact default.
9. **SCIP / LSP edges as precision layer** (infigraph) — v2 path to compiler-grade `EXTRACTED` edges.
10. **Skill file that teaches the agent the tool** (ferrograph) — ships with the binary.

### Avoid
1. ~90 MCP tools (infigraph) and two-step `symbol_id` lookups — cost turns.
2. Position-based node IDs (ferrograph `file#line:col`) — unstable under edits; breaks incremental == full.
3. Multi-script non-atomic file updates with ignored errors (infigraph).
4. String-interpolated Datalog (infigraph batch helpers) — use `$params`.
5. `(node, depth)` keyed recursion without min-depth (ferrograph) — row explosion.
6. Two query languages behind one interface (LeanKG translator).
7. Full pipeline re-run on file change (ferrograph watch).

---

## 5. Implications for the Benchmark (hand-off to `bench/`)

The benchmark must answer questions none of these tools answered:

1. **Recursive Datalog vs unrolled layers vs Rust BFS over indexed lookups** — same CozoDB instance, depth 10, ~50K nodes / 250K edges. infigraph and LeanKG both avoided recursion; we need to know whether that was necessary.
2. **Shallowest-depth semantics** — measure `min(depth)` aggregation cost vs `(node, depth)` key.
3. **Concurrent watcher writes + queries** — reproduce LeanKG #286 shape (sustained writes + reads) on sqlite, mem and RocksDB; check for aborts, not just latency.
4. **RocksDB backend** — nobody in the landscape measured it for code graphs.
5. **Atomic per-file update** — single chained script delete+insert; measure latency.
