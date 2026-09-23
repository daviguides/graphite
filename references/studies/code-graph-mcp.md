# Study: code-graph-mcp (sdsrss)

> Source study of `references/repos/code-graph-mcp/` — v0.153.0, schema v10, INDEX_VERSION 71. Rust, ~96K LOC in `src/` (134 files) + ~40K LOC of integration tests + a Node.js Claude Code plugin (`claude-plugin/`). Studied 2026-09-23.
>
> Lens: Graphite's targets — **S** = agent execution speed, **A** = assertiveness, **C** = correctness. Paths below are relative to the cloned repo.

Despite 77 stars, this is the most mature Rust implementation in the space: 153 releases, dozens of dated internal audits recorded in code comments, and measured data on how Claude Code actually uses (or ignores) code-graph tools. Its most valuable content is **empirical**. It shows what agents do with these tools, and which failure classes a by-name SQLite graph keeps producing.

---

## 1. Feature Inventory

Every capability an agent or human would notice. Verdict = what Graphite should do with it.

### 1.1 MCP tool surface (7 listed, ~11 hidden)

| Feature | What it does | Target | Verdict |
|---|---|---|---|
| `get_ast_node` | ONE symbol → signature + verbatim source (+ `context_lines`) + opt `include_references` (calls / called_by), `include_impact` (risk summary), `include_similar`. Positioned as "use BEFORE editing X". `src/mcp/tools.rs:79-101` | S A C | **v1** — the core "before edit" tool. Graphite's `explore` must subsume this shape. |
| `get_call_graph` | Multi-hop callers/callees, `depth` (default 3, cap 10), `direction`, `file_path` disambiguation, `min_confidence` floor (default `inferred`), `include_tests`, `route_path` mode (`GET /api/x` → handler → downstream). `tools.rs:53-77`, `graph/query.rs:11-16` | S C | **v1** (callers/callees + depth + confidence floor). Route mode **v2**. |
| `find_references` | Every site that imports / inherits / implements / calls / references / exports / routes_to a symbol. Relation filter, per-row `confidence` tag, `confidence_filtered` count. Framed as "rename/remove audits". `tools.rs:183-213` | C | **v1** — rename safety is a correctness case the call graph alone misses. |
| `module_overview` | Symbols in a dir/file grouped by type + caller count. `include_deps` (file-level import graph, direction + depth), `include_dead` (unreferenced candidates). `tools.rs:143-159` | S | **v1** (overview + deps). Dead code **v2**. |
| `project_map` | Architecture map: modules, module deps, entry points (incl. HTTP routes), hot functions, key symbols. `include_centrality` = betweenness chokepoints. `tools.rs:102-142` | S A | **v1** — this is the runner EXPLORING-mode payload. Centrality **v2**. |
| `semantic_code_search` | Concept search: FTS5 BM25 + vector (MiniLM-L6, 384-d) fused with weighted RRF. Degrades to FTS-only when no model is loaded (`search_mode` tells the agent). `tools.rs:37-52`, `search/fusion.rs` | S A | **v1 lexical (BM25), v2 hybrid if measured.** Concept search replaces multi-round Grep when the agent has no exact symbol (speed), and a right first hit avoids backtracking (assertiveness). Whether hybrid vector+RRF ships depends only on whether it saves agent turns on real queries. Their weighted RRF with the bounded tie-break (`fusion.rs:11-45`) is the design to copy if it does. Cost to weigh: the model must be loaded before search runs, so first-query latency rises unless the embedding is precomputed on the watcher thread. |
| `ast_search` | Typed enumeration: `type` / `returns` / `params` substring filters ("all fns returning Result<T>"). `tools.rs:160-182` | S | **v2** — cheap in Datalog, low call frequency. |
| Hidden but callable: `impact_analysis`, `dependency_graph`, `find_similar_code`, `find_dead_code`, `trace_http_chain`, `find_http_route`, `start_watch`, `stop_watch`, `get_index_status`, `rebuild_index`, `read_snippet` | Folded into flags on the 7 listed tools (v0.18.4) to save listing tokens. Still dispatch when named. `domain.rs:74-86` | S | **Adopt the principle**: few listed tools, capability via flags. Management tools stay unlisted. |

### 1.2 Response shaping (what the agent reads)

| Feature | What it does | Target | Verdict |
|---|---|---|---|
| Verbatim source inline | `code_content` stored per node (capped at 4 KB, `CODE_GRAPH_MAX_CODE_LEN`) and returned with line range, so the agent doesn't need a follow-up Read. `domain.rs:590`, `ast_node.rs:291-319` | S | **v1** — already a Graphite principle. Confirmed here. |
| `compact` flag on every tool | Signature + location only, no code. Every tool has it. | S | **v1**. |
| Auto-compression tiers | If the estimated tokens pass a threshold, results collapse to L1 node summaries (≤3×), L2 grouped by file (≤8×), or L3 grouped by directory, each with `node_id`s for expansion. `sandbox/compressor.rs:64-106` | S | **v1** — bounded payloads make latency predictable. |
| Token estimate = bytes/3 | One estimator, CJK-safe, overestimates on purpose. `domain.rs:540-549` | S | **v1** (adopt as is). |
| Disclosure fields | `clamped_arguments`, `limit_hit`, `depth_capped`, `ambiguous_edges_hidden`, `ambiguous_callers_excluded`, `test_callers_hidden`, `callers_truncated`, `hot_functions_truncated`, `confidence_filtered`. Every truncation or filter is announced, never silent. `graph/query.rs:55-70`, `graph/routes.rs` header | A C | **v1** — an agent that knows a result is partial doesn't over-trust it. Cheap to build, high assertiveness value. |
| Risk verdict | `risk_level` = HIGH / MEDIUM / LOW from prod-caller count, affected routes, and whether the change is breaking. `UNKNOWN` + warning for non-function symbols, where call edges understate usage. `domain.rs:618-650`, `graph/impact.rs` | A | **v1** — one word the agent can act on. |
| Prod/test caller partition | Test callers split out and deduped (AST `is_test` flag OR path/name heuristic), with `test_callers` listed for targeted re-runs. `graph/impact.rs:1-60` | S C | **v1** — the source for runner VALIDATING's targeted tests. |
| Ambiguity response | Several exact definitions → refuse + list `node_id` candidates. No exact match → `suggestion` + ≤5 fuzzy `candidates` inside the tool's normal envelope (not an error). `resolve.rs`, `tools/callgraph.rs:196-222` | A | **v1**. |
| Not-found as guidance | Errors name the next tool to try ("Use semantic_code_search to find the correct symbol name"). `callgraph.rs:224` | S | **v1**. |
| `node_id_renumbered` | Warns when a re-index during the call invalidated the node_id the caller passed. `ast_node.rs:182-190` | C | **Skip** — this only exists because rowids aren't stable. Graphite should use deterministic IDs. |

### 1.3 Edge kinds and resolution coverage

| Feature | What it does | Target | Verdict |
|---|---|---|---|
| 7 relations | `calls`, `imports`, `inherits`, `implements`, `exports`, `references` (type position + path-qualified consts), `routes_to`. `domain.rs:89-98` | C | **v1**: calls, imports, inherits, implements, references. **v2**: exports, routes_to. |
| Edge confidence | `extracted` (same-file precise or structural), `inferred` (cross-file, unique same-language name, or import-bound), `ambiguous` (by-name fan-out to more than one definition). Tool floors default to `inferred`. `domain.rs:162-190`, `resolve.rs:973-1090` | A | **v1** — confirms Graphite's EXTRACTED/INFERRED decision. Add AMBIGUOUS as the third tier. |
| Callee qualifiers | Each call edge carries how it was bound: `path` (`a::b::f`), `self`, `stype`, `rtype` (Python receiver-type inference), `recv`, `chain`. Structural qualifiers are exempt from being downgraded to ambiguous. `parser/relations/mod.rs:60-95` | C | **v1** — the difference between 1 edge and N phantom edges. |
| Ambiguity refinement | For N same-name candidates: prefer non-test when the caller is non-test, then prefer the longest common path prefix with the caller. If still tied, keep all (bias to over-report over dropping). `pipeline/resolve.rs:89-170` | C | **v1** (adopt the heuristic as a Datalog rule). |
| Import-bound call precision | A call to `foo` in a file that imports `foo` from X binds to X. Edges contradicted by imports are pruned. `bind_calls_to_imported_targets`, `prune_import_contradicted_call_edges` | C | **v1**. |
| `<external>` sentinels | Unresolved imports/implements bind to synthetic external nodes, e.g. `use std::…`. External-typed imports use `IMPORT_EXTERNAL_META`, so project symbols with the same name don't capture them. `domain.rs:100-120, 707` | C | **v1** — without it, `swap` / `read` / `spawn` produce phantom edges. |
| `<module>` scope node | Top-level statements (imports, module-level calls) attach to a per-file module node, so files called only from top level aren't reported as dead. `domain.rs:712` | C | **v1**. |
| Noise filters | Cross-file call noise list (`get`, `run`, `new`…), plus per-language type-reference noise and framework-decorator lists. `domain.rs:1191-1430` | C | **v1** (as data tables). |
| HTTP routes | Express / Fastify / Koa, Go net/http, Flask / FastAPI decorators, axum builder chains (incl. `.nest` prefixes). Inline handlers become synthetic nodes `GET /x#L12`. `parser/relations/routes.rs`, `parser/mod.rs:12-98` | C | **v2** — valuable for web repos. Not core. |
| Test detection | AST flags (`#[cfg(test)]`, `#[test]`, Jest `describe/it`, gtest `TEST()`) plus path/name heuristics, pinned by a parity corpus. `treesitter.rs:84-200`, `domain.rs:653-1100` | S C | **v1** — required for impact partition and targeted tests. |
| Doc comments | Preceding comments with wrapper climbing (export / decorator / attribute), Python docstrings, Rust inner-doc rules. `treesitter.rs:1396-1660` | A | **v2** (nice for summaries, not for traversal). |
| 20 languages | TS/TSX/JS, Go, Python, Rust, Java, C, C++, C#, Kotlin, Ruby, PHP, Swift, Dart, HTML, CSS, Markdown, Bash, JSON. `parser/languages.rs` | — | Graphite v1: Rust / TS / Python / Go as planned. |

### 1.4 Freshness guarantees

| Feature | What it does | Target | Verdict |
|---|---|---|---|
| File watcher (`notify`) | Signals only. Events go into a bounded channel (4096). Indexing happens on the **next tool call** (`ensure_indexed` drains events, then runs a synchronous incremental). `indexer/watcher.rs:50-135`, `mcp/server/mod.rs:1572-1610` | S | **Reject the design**: it puts indexing in the query path. Graphite indexes eagerly on the watcher thread. |
| Watcher-deaf backstop | Even with a watcher, a periodic full rescan bounds staleness, because watchers fail silently (inotify limits, network FS, bind mounts). Unknown event kinds (`Any` / `Other`) count as content changes. `mod.rs:1586-1606`, `watcher.rs:8-24` | C | **v1 adopt** — a watcher cannot be the only freshness source. |
| Query-time per-file refresh | A tool called with `file_path` re-hashes that file and reindexes it before answering. Result-set refresh: run the query, hash the returned files (cap 32), reindex stale ones, re-run. `mcp/server/freshness.rs:1-60` | C | **Adapt** — Graphite uses a freshness **barrier** instead (wait for the watcher queue to drain up to the query's arrival seqno), not a re-hash per query. |
| Edit hook reindex | PostToolUse `Write\|Edit` → reindex the edited file synchronously. Now off by default (query-time refresh made it redundant; ~80 ms cold start per edit). `claude-plugin/scripts/incremental-index.js` | S C | **v1 adapt** — with a resident daemon it costs ~0. Closes the gap between the agent's own edit and its next query. |
| Change detection | Directory-mtime skip, per-file (mtime, size) stamp, then BLAKE3 content hash. Flat HashMap diff. Marketed as a "Merkle tree" but it isn't one. `indexer/merkle.rs:452-660` | S | **v1 adopt the stamp→hash ladder**. Ignore the "Merkle" label. |
| Noop incremental | ~28 ms on 283 files. `README.md:63-69` | S | Baseline to beat. |
| Snapshot bootstrap | Prebuilt zstd index artifacts downloaded from GitHub Releases, then an incremental drift-check. `snapshot/mod.rs` | S | **v2 adapt** — the cold-start cost matters for Graphite because runner creates a **fresh worktree per task**. Better source than a release artifact: seed the worktree's graph from the main checkout's graph, then incremental-diff. Remote artifact seeding for CI / fresh clones is also acceptable. |

### 1.5 Error and failure semantics

| Feature | What it does | Target | Verdict |
|---|---|---|---|
| Indexing-in-progress | Waits ≤2 s for startup indexing, then returns "retry shortly" instead of blocking the stdio loop. `mod.rs:1504-1533` | S | **Adapt**: Graphite answers from the last committed graph and flags `stale: true`, never blocks. |
| Per-request `catch_unwind` | A handler panic becomes JSON-RPC -32603 and the session survives. `panic = "abort"` is forbidden in the release profile for this reason. `main.rs:814-848`, `Cargo.toml` profile | C | **v1 adopt**. |
| Explicit thread stack | Index threads get 8 MiB. A stack overflow is an abort, which bypasses `catch_unwind`. `domain.rs:552-570` | C | **v1 adopt**. |
| Tree-sitter parse timeout | 5 s per file. Parse-error files are recorded in `meta.parse_error_files` and surfaced by health-check. `treesitter.rs:32-55`, `schema.rs:15-37` | C | **v1** — feeds Graphite's Graph Health view. |
| Crash-consistent runs | An `index_run_in_flight` marker in `meta`; if present at startup, the next run escalates to a full reindex. `schema.rs:6-13` | C | **v1 adopt** (cheap). |
| Rebuild inside one tx | Readers keep seeing the old complete index under WAL until commit. `mod.rs:1612-1660` | C | **v1 adopt** (Cozo transactions). |

### 1.6 Agent steering (the adoption layer)

| Feature | What it does | Target | Verdict |
|---|---|---|---|
| PreToolUse(Bash) grep **deny + answer** | Blocks a raw `grep/rg/ag` for an identifier-shaped pattern on indexed source, and returns the graph's answer (hits grouped by enclosing symbol, or symbol bodies) in the deny reason. Measured: agents reach for `grep -rn` ~13× more than the tool; **hint-only had ~0% transfer**, deny-with-answer converts. `claude-plugin/scripts/pre-grep-guide.js:10-45` | S | **v1** — without steering the tools don't get used and no speed is gained. |
| PostToolUse(Bash) compound-grep inject | A grep buried in `a && grep X` can't be denied, so the graph answer is injected as `additionalContext` after it runs. `post-grep-inject.js` | S | **v2**. |
| PreToolUse(Edit) impact inject | When `old_string` touches a function signature and the symbol has ≥2 prod callers, injects the impact summary plus covering tests (with a runnable `cargo test <names>` for Rust). 2-minute per-symbol cooldown. `pre-edit-guide.js`, `covering-tests.js` | C | **v1** — blast radius delivered at the moment of the edit, zero agent turns. |
| PreToolUse(Read) fan-out hint | On the 5th+ Read into one directory, suggests `module_overview` (5-minute cooldown). `pre-read-guide.js` | S | **v2**. |
| UserPromptSubmit context push | Injects relevant graph context per prompt. `user-prompt-context.js` | S | **v2** (the runner does this via prompt composition). |
| Hooks in settings.json, not plugin hooks.json | Plugin-cache `hooks.json` only honors SessionStart; every other event is silently ignored (measured 2026-05-24), so lifecycle writes them into `~/.claude/settings.json`. `claude-plugin/hooks/hooks.json`, `lifecycle.js:906-1030` | — | **Non-obvious constraint — record it.** |
| CLI-first instructions | In Claude Code, MCP tools are **deferred** (a ToolSearch round trip is needed before first use) while Bash is always live. The only conversions observed were CLI calls seconds after a deny. Instructions lead with CLI commands. `mcp/server/mod.rs:36-58` | S | **v1** — Graphite's CLI must be a first-class agent surface, not a human fallback. |
| Hook fail-open | Any hook error exits 0 (never breaks the user's tool call), with a budget-aware child timeout. `hook-fail-open.js`, `incremental-index.js:13-30` | C | **v1 adopt**. |

### 1.7 Measurement

| Feature | What it does | Target | Verdict |
|---|---|---|---|
| Session metrics | Per-tool stats, search quality (zero-result rate, FTS-only vs hybrid), index timings → `.code-graph/usage.jsonl`. `mcp/metrics.rs:134-175` | S | **v1** — feeds the Savings Dashboard. |
| Outcome (conversion) | Parses Claude Code transcripts (`~/.claude/projects/<slug>`). Measures whether files a tool returned were then Read/Edited by the agent (conversion), plus field MRR for ranked tools. `outcome.rs:1-140` | A | **v1 for Graphite** — this is the "was the context actually used?" metric the Agent Observatory needs. |
| Recommendation funnel | Every hook recommendation is logged (`recommendations.jsonl`) with action `deny` / `hint` / `bypass`, joined to later use. | S | **v2**. |
| Routing bench | For NL queries, asks a Claude model which tool it would pick given the live schemas. Threshold 0.70. `tests/routing_bench.rs` | A | **v1** — tool descriptions are code; regress-test them. |
| Effectiveness bench | Response bytes vs a hand-set Grep+Read baseline, failing above a 0.60 ratio (fixture: 942 / 23000 = 0.04×). `tests/effectiveness_bench.rs` | S | **v1** (Graphite version measures turns and wall-clock, not just bytes). |

### 1.8 Analytics (CLI-only there)

| Feature | What it does | Target | Verdict |
|---|---|---|---|
| `affected` | Changed files (positional or `git diff --name-only \| … --stdin`) → test files that transitively depend on them + the full affected set. `cli/commands/affected.rs` | S | **v1** — this *is* runner VALIDATING targeted tests. |
| `cycles` | File-level import SCCs (Tarjan) + shortest representative cycle. Rust intra-crate cycles excluded as idiomatic. `graph/cycles.rs` | C | **v2** (runner PLANNING). |
| `centrality` | Brandes betweenness over calls, i.e. chokepoints. `graph/centrality.rs` | A | **v2**. |
| `surprising` | Cross-file edges scored by low confidence + cross-module + sole bridge. `graph/surprising.rs` | A | **v2** (frontend architecture views). |
| `tour` | Kahn topological reading order of modules (Entry / Foundational / Core / Mid roles). `graph/reading_order.rs` | S | **v2** (EXPLORING). |
| `dead-code` | Orphans + exported-unused, with macro/shell entry points pre-filtered. Results framed as "candidates to verify". | C | **v2**. |
| `grep` (AST-context) | ripgrep hits grouped by enclosing function/class. `cli/grep.rs` | S | **v1** — it's what the grep-deny hook answers with. |
| `report`, `stats`, `health-check`, `benchmark` | Consolidated health, usage stats, index status, perf numbers. | — | health-check **v1**. The rest v2. |

---

## 2. Non-obvious constraints this repo paid to learn

These are the findings that change Graphite's design. Each comes from dated audits in the code.

### 2.1 Incremental ≠ full is their #1 bug class

`INDEX_VERSION` has been bumped 71 times. The v58–v71 entries alone (`domain.rs:365`) are a catalogue of one failure: **an incrementally grown index silently diverging from a rebuild of the same tree**. Examples: cross-batch phantoms, inbound edges destroyed by cascade delete, deleted-file dependents pinned to old resolutions, a second definition of a name never reaching unchanged callers.

The root cause is structural, and it has three parts:

1. **Node ids are SQLite rowids**, with `ON DELETE CASCADE` from files → nodes → edges (`schema.rs:63-141`). Re-indexing one file deletes every inbound edge from *unchanged* files.
2. **Relations are extracted as names** (`ParsedRelation{source_name, target_name}`, `relations/mod.rs:117-127`) and resolved against a *global* name map at write time. Whether an edge is right depends on the whole corpus, but only changed files get re-resolved.
3. So the pipeline grew compensators: `pending_unresolved_calls` buffer, `restore_inbound_edges`, deferred cross-batch pass, sentinel reaping, `existence_change_dependents`, `fan_out_to_new_duplicate_definitions`, plus scoped post-passes (`index_files.rs`, 3,873 lines, Phases 0-pre / 0 / 1a / 1b / 2 / 2b / 2b-final / 2c / 2e / 3).

**Graphite must make incremental = full by construction.** Deterministic symbol IDs (hash of path + qualified name + kind + disambiguator). Store only **per-file facts** (definitions, unresolved references with their qualifiers, imports). Compute resolution (`calls`, `confidence`) as **Datalog rules over those facts**. Re-indexing a file then replaces exactly that file's facts, and every derived edge is recomputed from the current fact base. No buffers, no restoration, no divergence. This is the single strongest argument for CozoDB in the whole landscape.

### 2.2 Recursive SQL blew up; they moved traversal into Rust

The `WITH RECURSIVE` call-graph CTE guarded cycles with a per-path visited string. It therefore enumerated all simple paths: **22.8 s at depth 10 on a 55-node / 250-edge graph**, and 66 nodes didn't finish in 2 minutes (`graph/query.rs:215-240`). They replaced it with level-by-level BFS in Rust, one SQL query per level. The file dependency CTEs in `storage/queries/imports.rs:29, 332` have the same shape.

Datalog semi-naive evaluation computes set fixpoints, not paths, so this class doesn't arise. For depth and parent, use Cozo's aggregation in recursive rules (`min(depth)`). **Graphite must still benchmark Cozo recursion on dense graphs at depth ≥ 10 before committing** (see §4).

### 2.3 Confidence is derived state, and storing it created drift

`classify_edge_confidence` recomputes a stored `edges.confidence` column in a scoped post-pass (`resolve.rs:973-1090`). Its own comment names the risk: a scoped copy that drifts from the global one "would label the same edge two ways depending on how the index happened to be grown". In Graphite, confidence is a rule (`same_name_count > 1 ∧ ¬import_bound ∧ ¬structural_qualifier → ambiguous`), evaluated at query time. It is never stored, so it is never stale.

### 2.4 The watcher does not index; the query does

`FileWatcher` only pushes paths into a channel (`watcher.rs:57-135`). `ensure_indexed` drains it at the start of the **next tool call** and runs the incremental synchronously (`mcp/server/mod.rs:1572-1583`). The first query after an edit therefore pays the whole incremental. Result-set refresh then adds hash-and-rerun on top (`freshness.rs`). Graphite's always-hot design (index on the watcher thread, and let queries wait only on a freshness barrier) is exactly the fix. The architecture doc is confirmed.

### 2.5 One process per Claude session, so single-writer election

Every Claude Code session (and every runner parallel task) spawns its **own** stdio MCP server. They coordinate with an `flock` index lock: one primary indexes and watches; secondaries open read-only and retry promotion, throttled (`mcp/server/mod.rs:432-490`, `indexer/lock.rs`).

**Graphite's "background thread in the same process" assumption breaks under parallel agents.** Graphite needs a process model decision:

- **Option A**: `graphite serve` as a per-repo daemon (unix socket), with `graphite mcp` as a thin stdio shim that forwards to it.
- **Option B**: flock-elected primary, as here.

Option A keeps one hot graph and one watcher for N agents, which is also Graphite's multi-agent-coordination gap. **Recommended: A.**

### 2.6 How Claude Code consumes MCP (measured)

- **MCP tools are deferred.** Each needs a ToolSearch load before first use; Bash is always live, so the CLI converts first (`mod.rs:36-41`).
- **`anyOf` in an input schema makes the client silently drop the tool** (measured 2026-09-02). Use `"required": []` and enforce disjunctions in the handler (`tools.rs:287-334`).
- **Instructions are truncated at ~2 KB.** Compile-time assert ≤1500 bytes (`mod.rs:52-66`).
- **Descriptions ≤200 chars** (test). Negative phrasing ("don't call this unless…") measured **20 pp worse** in routing than a positive cue (`tools.rs:104-113`).
- **Hint-only steering ~0% transfer; deny-with-answer works** (`pre-grep-guide.js`).
- **Plugin `hooks.json` honors SessionStart only.** Other hooks must go in `settings.json`.

### 2.7 Batching and memory

Files are processed in batches of 500 plus a byte cap (`index_files.rs:79-110`), because Phase 1a materializes whole batches (source + tree). The rayon parse (1a) is separate from the sequential DB insert (1b). Graphite should adopt this parse-parallel / write-serial split.

---

## 3. Implementation notes (only where non-obvious)

- **Crates.** `rusqlite 0.38` (bundled-full), vendored `sqlite-vec` C compiled in `build.rs` with a **BLAKE3 supply-chain pin** on the vendored source. Also `tree-sitter 0.24` + per-language grammar crates (compiled in), `notify 6`, `blake3`, `ignore` (gitignore-aware walk), `rayon`, `clap`, `anyhow`, `zstd`. Candle + tokenizers behind the `embed-model` feature (~120 MB vs ~10 MB binary).
- **No MCP crate.** JSON-RPC 2.0 over stdio is hand-rolled (`mcp/protocol.rs`, `utils/stdio.rs` framing with a size cap). A shared, mutex-guarded stdout serializes responses and notifications. Single-threaded request loop.
- **Extraction.** An imperative recursive `match node.kind()` shared across languages (`treesitter.rs:103-893`), then a **second walk** for relations. There are **no tree-sitter queries (`.scm`)**. Relation extraction is being migrated to **data tables** (`CALL_PASSES`, `IMPORT_PASSES`, `HERITAGE_PASSES`, `REFERENCE_PASSES`, `EXPORT_PASSES`: rows of (language, node kinds, extractor fn)), with tests asserting no row overlap and no inert row (`relations/calls.rs:1-80, 800-850`). Their stated top recurring bug: "a missing arm is not a compile error but a silently absent edge".
- **Schema** (`storage/schema.rs:63-172`):
  - `files(path, blake3_hash, last_modified, language)`
  - `nodes(file_id, type, name, qualified_name, start/end_line, code_content, signature, doc_comment, context_string, name_tokens, return_type, param_types, is_test)`
  - `edges(source_id, target_id, relation, metadata JSON, confidence)`, unique on `(src, tgt, rel, metadata)`
  - `pending_unresolved_calls`
  - `meta`
  - FTS5 external-content table (porter stemmer) kept in sync by triggers
  - `node_vectors` vec0 (384-d)
  - `embedding_cache` keyed by context hash
- **Pragmas.** WAL, `synchronous=NORMAL`, 64 MB cache, `mmap_size=0` (a SIGBUS on truncate-under-mmap was observed), `busy_timeout=5000`. `PRAGMA optimize` / ANALYZE after bulk writes. Version guards via `user_version` (schema) and `application_id` (INDEX_VERSION, which triggers a rebuild).
- **Parser cache.** A `thread_local!` `HashMap<lang, Parser>` with a timeout set per parser (`treesitter.rs:26-55`).
- **Search.** Weighted RRF (k=30, FTS 1.0 / vec 1.2, acronym-aware reweighting), with a provably bounded raw-score tie-break blend (`fusion.rs:11-45`). There's also a stack of hand-tuned penalties and boosts (exact-name +100, size dampening, markdown penalty, sparsity penalties; `domain.rs:398-470`). This is heavy tuning on top of a fundamentally lexical/vector recall problem.
- **Testing strategy worth copying:**
  - **Architecture tests** that scan source for forbidden module edges (`tests/hardening.rs:446`)
  - **Parity tests**: CLI↔MCP freshness, compact↔full key sets, predicate SQL↔Rust
  - **Wiring tests**: every extractor appears in its pass table
  - **Incremental-vs-rebuild equivalence** fixtures
  - **Routing bench** and **effectiveness bench** as `#[ignore]` release gates
  - A negative control proving each guard can fire

---

## 4. Where CozoDB / Datalog is strictly better

| Their mechanism | Problem | Datalog equivalent |
|---|---|---|
| Rowid + cascade delete + restore buffers | Incremental ≠ full (71 index bumps) | Per-file fact replacement; derived relations recomputed by rules |
| Stored `confidence` + scoped post-pass | Drift between scoped and global classification | `confidence[e] := …` rule, evaluated on read |
| `pending_unresolved_calls` (+ attempts eviction) | Unresolved call becomes resolvable when a later file adds the target | Unresolved refs are just facts; the `calls` rule matches when the definition appears |
| Recursive CTE, then Rust BFS | Path explosion, then N round trips per level | Semi-naive fixpoint with `min` aggregation in one query |
| Import corroboration via temp tables (`cg_imports`, `cg_namecount`) | Materialized per run, indexed per run | Plain rule joins |
| `refine_ambiguous_targets` in Rust | Heuristic lives outside the store | Expressible as ranking rules. Keep in Rust only if Cozo lacks the prefix-length function (verify) |

**Where SQLite still wins, so check Cozo for gaps:**

1. **FTS5 BM25 text search.** Cozo has FTS indices (verify tokenizer, stemming, ranking quality).
2. **Mature WAL multi-process readers.** With the daemon model (§2.5, Option A), Graphite only needs single-process concurrency.
3. **Recursion performance on dense graphs.** No measurement exists for Cozo yet. **Benchmark before v1**: 50K nodes / 250K edges, blast radius at depth 10, target < 10 ms.

---

## 5. Weaknesses and bottlenecks

1. **Indexing in the query path** (§2.4). The first call after an edit pays the incremental.
2. **Two AST walks per file** (nodes, then relations) over the same tree. A single-pass extractor emitting both facts is cheaper.
3. **Global name-map rebuild per batch.** O(nodes × batches), measured 0.26% of a full index and left alone (`index_files.rs:1120-1140`). Not a problem at their scale.
4. **Full index ~2.0 s for 283 files (~139 files/s)**, reported single-threaded in the benchmark (`README.md:63-69`). Graphite should target ≥ 1,000 files/s with parallel parse + a serial writer.
5. **P50 query 655 µs / P99 2.1 ms.** Fine, but this is from a single-threaded stdio loop: one slow call blocks every other request.
6. **Complexity.** 96K LOC, and much of it compensates for §2.1. The pipeline file is 3,873 lines. The MCP server module is 6,954 lines.
7. **Embeddings cost.** An optional 120 MB stack, background model download, and a dimension-guard migration, with FTS-only degradation until the model is ready. The gain has to be measured: their own penalty stack (sparsity, vec-only, OR-fallback) shows vector recall is noisy for code identifiers.
8. **Language coverage via hand-written match arms.** Every grammar quirk becomes an `if config.name == …`. Tree-sitter queries (`.scm` tags) plus pass tables would cut this down.

---

## 6. Decisions for Graphite

### Adopt (as is)

1. **Three-tier edge confidence** (`extracted` / `inferred` / `ambiguous`). Default traversal floor = `inferred`, and every hidden-ambiguous count is disclosed.
2. **Disclosure fields on every response.** `clamped_*`, `limit_hit`, `depth_capped`, `*_hidden`, `*_truncated`. Never a silent partial answer.
3. **Risk verdict + prod/test partition + covering tests** in the impact payload.
4. **`compact` flag everywhere + tiered auto-compression** (node → file → directory) with ids for expansion. Bytes/3 token estimator.
5. **Few listed tools, capability via flags.** No `anyOf`, descriptions ≤200 chars, instructions ≤1.5 KB, positive phrasing. A routing bench as a release gate.
6. **Callee qualifiers on call edges** (`path` / `self` / `stype` / `rtype` / `recv` / `chain`), plus import-bound precision, `<external>` sentinels and the `<module>` scope node.
7. **Freshness safety nets**:
   - unknown watcher events = content change
   - periodic backstop rescan
   - mtime+size stamp → BLAKE3 ladder
   - `index_run_in_flight` crash marker
   - rebuild inside one transaction
8. **Robustness**: per-request `catch_unwind`, unwind panic strategy, 8 MiB index thread stacks, per-file parse timeout, recorded parse-error files.
9. **Hook fail-open** + registering hooks in `settings.json`, not plugin `hooks.json`.
10. **Test strategy**: architecture edge tests, pass-table wiring tests, incremental≡rebuild fixtures, `#[ignore]` benches.

### Adapt

1. **Freshness.** Replace query-time rehash/rerun with a **seqno barrier**. The watcher thread indexes eagerly; a query waits (bounded, e.g. ≤200 ms) for the indexed seqno to reach the event seqno at arrival, and otherwise answers with `stale: true`. Keep the PostToolUse `Write|Edit` hook, pointed at the resident daemon: an explicit "file X changed now" nudge that costs about 0.
2. **Process model.** One **per-repo daemon** owns CozoDB + the watcher. `graphite mcp` (stdio) and the `graphite <cmd>` CLI are thin clients over a unix socket. This solves N sessions / N runner tasks with one hot graph (their flock primary/secondary is the fallback if a daemon is unavailable).
3. **CLI as a first-class agent surface.** Claude Code defers MCP tools, so the fastest path is Bash → `graphite <cmd> --json`, with MCP for hosts that don't defer. Same handlers behind both.
4. **Steering.** Adopt **PreToolUse grep deny-with-answer** and **PreToolUse Edit impact injection** in v1. Read-fanout and compound-grep injection go in v2. For runner sessions, prompt composition replaces most hooks.
5. **Outcome measurement.** Port transcript-based conversion ("returned file → later Read/Edit") into the Agent Observatory / Savings Dashboard. Measure **turns and wall-clock**, not only bytes.
6. **Extraction design.** A single AST pass emits node facts + reference facts. Language differences live in pass tables (or tree-sitter queries). Wiring tests guard the tables.
7. **`affected`** becomes part of `diff_impact`: changed files → reverse closure → test files, fed straight into runner VALIDATING.

### Reject

1. **Rowid identities + FK cascades + write-time by-name resolution.** Use deterministic IDs + per-file facts + Datalog-derived edges (§2.1).
2. **Stored derived state** (confidence, pending buffers, sentinel reaping). Use rules.
3. **Watcher-signals-only / index-on-query.** Graphite indexes on the watcher thread.
4. **Recursive SQL with path strings.** Moot with Datalog, but benchmark Cozo recursion first.
5. **`node_id_renumbered` class of warnings.** Unnecessary with stable IDs.

### Deferred to measurement (v2)

- **Hybrid vector + FTS concept search (sqlite-vec-style ANN + BM25, weighted RRF).** Decide purely on speed / assertiveness / correctness. Run a routing+recall bench on real agent queries and ship if it cuts turns versus BM25 + graph proximity. If it ships, embed on the watcher thread, never in the query path. Cozo has HNSW vector indices; check them before adding a second store.
- **Index seeding.** Seed from the main checkout's graph for runner worktrees; remote snapshot artifacts for CI / fresh clones.
