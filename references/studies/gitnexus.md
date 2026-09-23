# GitNexus — Source Study

> Deep source study of `references/repos/gitnexus/` (47.5K★, TypeScript, embedded graph DB). Written 2026-09-23 for Graphite.
> All paths are relative to `references/repos/gitnexus/`. `gitnexus/src/...` is the CLI/MCP package.
> Lens: Graphite targets — **agent execution speed > assertiveness > correctness**. Convenience is a consequence, not a target.

**TL;DR** — GitNexus is the most feature-complete code graph for agents in the landscape and the closest storage model to Graphite (embedded property-graph DB). Its best ideas are in the **response shape**: honest uncertainty (`epistemic`, `causes`, `risk: UNKNOWN`), depth-labeled blast radius, disambiguation by uid, provenance on every edge. Its weakest point is exactly Graphite's thesis: freshness is commit-based and a **one-file edit takes 31.7s to re-analyze** (51K nodes); working-tree edits are invisible to the agent until then. Storage lessons point strongly toward CozoDB: typed node tables forced 509 declared FROM/TO relation pairs (which crashed `analyze` four separate times), there is no index on relation properties, and blast radius runs one Cypher round-trip per depth from application code.

---

## 1. Feature Inventory

Every capability an agent or human would notice. Target: **S** = speed, **A** = assertiveness, **C** = correctness. Verdict: **v1** (must-have), **v2**, **v3+**, **skip**.

### 1.1 MCP tools

| Feature | What it does | Target | Verdict | Notes for Graphite |
|---|---|---|---|---|
| `impact` | Blast radius from a symbol, upstream or downstream. Results grouped by depth: d=1 "WILL BREAK", d=2 "LIKELY AFFECTED", d=3 "MAY NEED TESTING". Returns risk, affected processes and modules, per-depth pagination, `summaryOnly`, `minConfidence`, `relationTypes`, `includeTests`. (`gitnexus/src/mcp/tools.ts:506-704`) | C, A | **v1** | Core. Keep the depth labels: they tell the agent what to do, not just what exists. Collapse into Graphite's primary tool. |
| `context` | 360° view of one symbol: incoming and outgoing refs by category (calls, imports, extends, implements, methods, properties, overrides, reads/writes), process participation, routes, optional `chain_depth` BFS. (`tools.ts:307-388`) | S, A | **v1** | Replaces 3–6 Read/Grep calls. Must return source inline, which GitNexus makes opt-in (`include_content: false` by default). |
| `detect_changes` | Maps `git diff` hunks (unstaged/staged/all/compare-to-ref) to indexed symbols by line-range overlap, then to affected processes, then to a risk level. Reports `partial` and `truncated` so a degraded run cannot read as clean. (`tools.ts:389-428`, `local-backend.ts:6141`) | S, C | **v1** | Same idea as Graphite's `diff_impact`. Graphite's version is better because the graph is already current with the working tree: no stale-index mismatch. |
| `trace` | Shortest directed path between two symbols over call and member edges, with file:line and edge type/confidence per hop. "Answers in one call what takes 3–8 hops." (`tools.ts:940`) | S | **v1** | Cheap: one CozoDB fixed rule (`ShortestPathBFS`). |
| `query` | Search that groups hits by execution flow ("process"): BM25 + vector + RRF, plus `task_context`/`goal` ranking hints. (`tools.ts:141-228`) | S | **v1 (lite)** | v1: FTS symbol search. Process grouping comes with flows in v2. Vectors: skip in v1 (vision.md Not Scope: no embeddings). |
| Disambiguation | Ambiguous name returns ranked candidates with `uid`. Caller retries with `target_uid`, `file_path` or `kind`. `totalCandidates` is the true count. | S, C | **v1** | Prevents the agent from acting on the wrong `save()`. Cheap to build. |
| `check` | Structural lint: import cycles that force module-initialization order. | C | **v2** | One SCC fixed rule in CozoDB. |
| `route_map` / `api_impact` / `shape_check` | HTTP route → handler → consumer map. Pre-change report for a route handler. Response keys compared with the properties consumers access. | C | **v2** (routes) / **v3+** (shape_check) | Route edges matter for web backends (Axum, FastAPI, Express). Shape drift is niche. |
| `rename` | Multi-file rename combining graph refs (high confidence) and regex (low confidence), with a dry run. | C | **skip** | Editing is the agent's job. Graphite can expose "all references" and let the agent edit. |
| `cypher` | Raw query tool with a schema cheat sheet embedded in the description. | — | **v2 (CLI only)** | Keep `graphite query <datalog>` for humans. As an MCP tool its description costs tokens on every session. |
| `explain` / `pdg_query` | Taint findings (source→sink), control dependence and reaching definitions at basic-block level. Opt-in `--pdg`, TS/JS only. | C | **v3+** | Heavy and narrow. Out of scope until the core is fast. |
| `tool_map` | MCP/RPC tool definitions and their handlers. | — | **skip** | Niche. |
| `group_*`, cross-repo `@group` mode | Contract registry that joins HTTP consumer→provider across repos. | C | **v3+** | Matches "cross-repo" in the landscape gaps. Later. |
| `list_repos` | Multi-repo registry. | — | **skip** | Graphite runs one server per repo. |

### 1.2 Response-shape features (the most valuable part of GitNexus)

| Feature | What it does | Target | Verdict |
|---|---|---|---|
| **`epistemic: exact \| lower-bound`** | Every impact/context result says whether the caller set is complete or a floor. (`tools.ts:528`) | A | **v1** |
| **`causes {...}`** | Machine-readable reasons a count may be short, each counting missing things: `receiverTyping` (call sites dropped because the receiver type is unknown), `dispatchBoundary` (DI/interface), `externalBoundary` (calls leaving the program, not a defect), `callableValueReferences`, `scopeExtractionFiles`, `undecidedSatisfaction`. (`tools.ts:530-537`) | A, C | **v1 (3 causes)** — unresolved receivers, dispatch boundary, external. Others v2. |
| **`risk: UNKNOWN` on zero callers** | Upstream impact with 0 resolved callers returns UNKNOWN, never LOW, plus a `riskNote`: "no callers" may mean unused or unresolvable. (`gitnexus-shared/src/impact-risk.ts:35`) | C | **v1** — stops an agent from deleting "unused" code that is actually reached dynamically. |
| Risk ladder | LOW/MEDIUM/HIGH/CRITICAL from direct count (5/15/30), total (30/100/200), processes (3/5), modules (3/5). (`impact-risk.ts:35-50`) | A | **v1** — thresholds are a good default. |
| `partial` / `truncated` flags | A capped or failed step can never produce a false all-clear. | C | **v1** |
| Per-edge `confidence` + `reason` | Confidence tiers: same-file 0.95, import-scoped 0.9, global 0.5 (`ARCHITECTURE.md:418-424`). `reason` names how the edge was resolved. | A | **v1** — as Graphite's `provenance`. |
| `staleness` envelope | Every result says which commit answered and whether it is `current/behind/diverged/unknown`. | A | **Replace** — with a watcher, report `graph_lag_ms` / `pending_files` instead. |
| Pagination per depth, `summaryOnly` | Avoids output blow-up on hub symbols. | S | **v1** |
| `maxTokens` | Truncates bytes at bytes/4, which can cut through the middle of a structure. (`gitnexus/src/mcp/output-budget.ts`) | S | **Improve** — Graphite should drop the lowest-ranked items to fit a budget, never cut bytes. |

### 1.3 Graph features

| Feature | What it does | Target | Verdict |
|---|---|---|---|
| Edge kinds (28) | CONTAINS, DEFINES, CALLS, IMPORTS, EXTENDS, IMPLEMENTS, HAS_METHOD, HAS_PROPERTY, ACCESSES (read/write), METHOD_OVERRIDES, METHOD_IMPLEMENTS, USES, INJECTS, HANDLES_ROUTE, FETCHES, QUERIES, DECORATES, MEMBER_OF, STEP_IN_PROCESS, ENTRY_POINT_OF, … (`ARCHITECTURE.md:511`) | C | **v1**: contains, calls, imports, extends, implements, has_member, overrides, references. **v2**: accesses r/w, injects, handles_route, fetches. **skip**: Spring AOP/conditional, COBOL. |
| Node kinds (31 tables) | File, Folder, Function, Class, Interface, Method, Struct, Enum, Trait, Impl, TypeAlias, Const, Property, Route, Community, Process, … | — | **v1** as a `kind` column, not as tables (see §3). |
| Communities (Leiden) | Clustering over CALLS/EXTENDS/IMPLEMENTS among Function/Class/Method/Interface. Name-guessed edges excluded. Heuristic label from the dominant folder, cohesion = internal-edge ratio, singletons dropped. | A | **v1-lite** (CozoDB `CommunityDetectionLouvain`) → Leiden in v2 if quality matters. |
| Processes (execution flows) | Scored entry points (call ratio, exported, name patterns like `handle*`, framework paths) → bounded DFS over CALLS (depth 10, branching 4, max 75 flows, ≥3 steps) → dedupe → `intra_community` / `cross_community`. Truncation counters published. | A, S | **v2** — high value for EXPLORING, but costly and heuristic. |
| Resolution: 3-tier imports + scope resolution + MRO | Named/wildcard/namespace import semantics per language, C3 MRO for Python, receiver chains (`a.b().c()`), interface dispatch fan-out (generic-aware, cap 32), callable-value flow (cap 32). | C | **v1**: tiers 1–2 and import resolution for Rust/TS/Py/Go, simple receiver typing. **v2**: MRO, fan-out, callable flow. |
| Framework routes | Next.js/Expo filesystem routes, Laravel, Django urlpatterns, decorators (Spring, FastAPI, NestJS), raw `node:http` dispatch guards. Precision over recall: "a missing route is a coverage limit; an invented one is a lie" (`ARCHITECTURE.md:197-200`). | C | **v2** |
| Test-file detection | `includeTests: false` by default. Test paths are excluded from impact. | S | **v1** — plus Graphite's test-to-code mapping (a landscape gap nobody fills). |
| Embeddings + hybrid search | Arctic-embed-xs (384D), HNSW, RRF K=60. | S | **skip** per current vision.md Not Scope ("No embeddings"). If that is revisited, note that CozoDB has native HNSW indices, so it would not need a separate store. |
| PDG/taint layer | CFG, REACHING_DEF, CDG, taint. Opt-in. | C | **v3+** |

### 1.4 Integration and UX features

| Feature | What it does | Target | Verdict |
|---|---|---|---|
| **PreToolUse hook on Grep/Glob/Bash** | Before the agent greps, the hook adds callers, callees and process participation for the pattern as extra context. BM25 only, target <200ms warm. (`gitnexus-claude-plugin/hooks/hooks.json`, `gitnexus/src/core/augmentation/engine.ts:1-14`) | S | **v1** — improves native tools without a new tool call. GitNexus spawns `node` on every hook call; Graphite's hook can hit the running server over a Unix socket in milliseconds. |
| PostToolUse freshness check | After Bash runs, check whether the index went stale. | — | **skip** — the watcher makes this unnecessary. |
| Installed skills (exploring, debugging, impact, refactoring, plan/work/review) | Workflow prompts that tell the agent when to call which tool. | A | **v2** — runner already owns orchestration. |
| **Per-community generated skills** (`analyze --skills`) | One skill per detected functional area: key files, entry points, flows, cross-area links. | A, S | **v2** — pre-loaded area knowledge. Maps to runner EXPLORING. |
| AGENTS.md/CLAUDE.md generation | Writes tool rules into agent instruction files ("MUST run impact before editing"). | A | **v2** — the rule "impact before edit, diff check before commit" is sound. |
| MCP resources | `context` (overview + staleness), `clusters`, `cluster/{name}`, `processes`, `process/{name}`, `schema`. | S | **v1**: repo overview. **v2**: clusters/processes. |
| MCP prompts | `detect_impact`, `generate_map`. | — | **skip** |
| Web UI | Sigma.js graph canvas (graphology + ForceAtlas2), file tree, code references panel, process flow as a Mermaid modal, AI chat, ops dashboard for analyze jobs, i18n. | — | **v2**: Mermaid flow view. **skip**: chat. No agent observability exists (Graphite's frontend gap confirmed). |
| Wiki generation (LLM) | Per-community docs written by an LLM. | A | **v2** — judged on value, not on the LLM: an LLM-written summary per community is pre-digested orientation for runner EXPLORING (same role as Graft's semantic tier). It must run off the query path and be cached by community membership hash. |
| Auto-sync | Polls remote git every N minutes, clone/pull, re-analyze. **No local file watching** (`gitnexus watch` is reserved and does nothing). (`README.md:486-491`) | S | **skip** — replaced by Graphite's watcher. |
| Multi-branch indexes | `analyze --branch` pins an index per branch. | — | **v3+** |

### 1.5 Performance facts (their own benchmark)

From `gitnexus/bench/analyze-phase-breakdown.md` (51,286 nodes, 163,092 edges, 2,106 clusters, 759 flows):

| Operation | Time | Dominant cost |
|---|---|---|
| Cold analyze | 63.6s | parse 36s, scope resolution 16s |
| **Re-analyze after one-file leaf edit** | **31.7s** | scope resolution walks every reference site in the repo (~13.9s, 44%); FTS rebuild (7.5s, 24%) because LadybugDB FTS is not incremental; subgraph write 3.6s |

This is the number Graphite's architecture beats. In GitNexus the agent's edit loop pays tens of seconds per re-index, or queries a stale graph. Graphite's goal: **per-file sub-second update, off the query path**.

---

## 2. Architecture (what they built)

### 2.1 Layout

Monorepo: `gitnexus/` (CLI + MCP stdio + HTTP API + pipeline, ~2,600 TS files), `gitnexus-web/` (Vite/React thin client), `gitnexus-shared/` (types/constants), `gitnexus-claude-plugin/` (hooks + skills). (`ARCHITECTURE.md:5-15`)

Flow: `analyze` → `runFullAnalysis` (`gitnexus/src/core/run-analyze.ts`) → a DAG of 19 phases builds an **in-memory `KnowledgeGraph`** → bulk load into the DB via CSV COPY → FTS indexes → metadata. (`ARCHITECTURE.md:460-476`)

Phase DAG (`ARCHITECTURE.md:83-113`): `scan → structure → parse → routes/tools/orm → crossFile → scopeResolution → pruneLocalSymbols → mro → di → communities → processes`. Static phase list, Kahn topological validation, typed per-phase outputs.

**Lesson:** the graph is computed in RAM by application code and the DB is a **sink**, not the compute engine. Communities, processes and resolution never run in the DB.

### 2.2 Storage: KuzuDB → LadybugDB

They migrated from KuzuDB to **LadybugDB** (a Kuzu fork; `run-analyze` step 4 still "cleans up legacy KuzuDB files"). The Kuzu upstream question is itself a data point: embedded graph DB projects are fragile dependencies. **This applies to CozoDB too** (see §5 risks).

On-disk layout: `.gitnexus/lbug` + WAL + shadow + single-writer lock + checkpoint lock files, with recovery code for interrupted checkpoints (`ARCHITECTURE.md:480-501`). Operational complexity is real: `gitnexus/src/core/lbug/` is 13K lines, including `sidecar-recovery.ts`, `wal-checkpoint-driver.ts` and `conn-lock.ts`.

### 2.3 Schema (`gitnexus/src/core/lbug/schema.ts`)

- **31 node tables, one per kind**, mostly sharing `id, name, filePath, startLine, endLine, isExported, content, description`. Kind-specific extras: `Method.parameterCount/returnType`, `Class.frameworkAnnotations`, `Property.declaredType/isDetail`, `Route.method/responseKeys/middleware`, `Community.cohesion/symbolCount/keywords`, `Process.processType/stepCount/communities/entryPointId/terminalId`. (`schema.ts:27-343`)
- **One relation table `CodeRelation`** with `type, confidence, reason, step, staticGated`. (`schema.ts:683-692`)
- **The pair problem:** Kuzu relation tables must declare every `FROM <NodeTable> TO <NodeTable>` pair. With typed node tables, that came to **509 pairs**, part hand-written (`STRUCTURAL_PAIR_DDL`, `schema.ts:572-633`) and part generated by cross products (`schema.ts:509-518, 664-681`). An undeclared pair **aborts `analyze` on the user's repo**; this happened 4+ times (`Method→Annotation`, `Method→File`, `Namespace→Record`, `Class→Tool`, `schema.ts:445-450`). More pairs also make anchored untyped queries slower (450 pairs ≈1.0×, 1024 pairs ≈2.2×; `schema.ts:487-505`).
- **No index on relation properties** (`schema.ts:326-331`, `ARCHITECTURE.md:313`). Filtering `r.type IN [...]` scans.
- `content` (source text) is stored **in the DB** on every node.
- Schema versioning: SHA-256 fingerprint of the DDL, with a forced full rebuild on mismatch (`schema.ts:830-846`). A good pattern to copy.

### 2.4 Tree-sitter extraction

- One S-expression query set per language with **unified capture tags**: `@definition.function`, `@definition.struct`, `@call.name`, `@import.source`, `@reference.inherits`, `@assignment.property`. Code downstream never branches on language. (`ARCHITECTURE.md:410-412`; Rust: `gitnexus/src/core/ingestion/tree-sitter-queries.ts:2014-2090`)
- A second query channel per language drives scope resolution (`languages/<lang>/query.ts`), and the two channels differ on anchor conventions (see the comment at `tree-sitter-queries.ts:2020-2029`). Two channels is drift risk.
- Parse runs in a worker pool over ~20MB byte chunks. Workers serialize `ParsedFile` artifacts to a disk-backed store to avoid native-memory leaks (tree-sitter buffers are not GC'd). (`ARCHITECTURE.md:433-447`)
- A parse cache replays unchanged files by content hash, with `PARSE_CACHE_VERSION` bumps whenever the artifact format changes.

**Lesson for Graphite:** use query files (`.scm`) with unified capture names and one channel per language. Rust has none of the worker-serialization pain: no GC, and trees can move across threads.

### 2.5 Resolution

- `ScopeResolver` interface per language (`ARCHITECTURE.md:318-341`): `resolveImportTarget`, `buildMro`, `mergeBindings`, `arityCompatibility`, `importEdgeReason`, and so on. Adding a language means one interface plus one registry entry.
- 3-tier lookup with confidence (`ARCHITECTURE.md:418-424`): same-file 0.95 → import-scoped 0.9 → global unique-name 0.5.
- Import semantics per language: `named` (TS/JS/Java/C#/Rust/PHP/Kotlin), `wildcard-leaf` (Go/Ruby/Swift/Dart), `wildcard-transitive` (C/C++), `namespace` (Python).
- **Unresolved receivers are recorded, not dropped**: `ResolutionOutcome` with shape (`chain-call`, `chain-field`, …) and origin (`in-program`/`external`), aggregated per member name into `unresolvedReceiverMembers`. `impact`/`context` read this to set `epistemic`. (`ARCHITECTURE.md:303`)
- **Provenance beats numbers:** `global-name-fallback` edges sit at exactly 0.5, the same value as the process/community thresholds, so `confidence < 0.5` could not exclude them. They had to add a categorical `reason` check (`gitnexus/src/core/graph/edge-reasons.ts:1-40`). → Graphite needs **categorical provenance plus numeric confidence**.
- Overload IDs: `Method:file:Class.method#arity`, then `~type1,type2` on collision, then `$const` for C++. IDs change when an overload is added (`ARCHITECTURE.md:525-533`).

### 2.6 Communities (`gitnexus/src/core/ingestion/community-processor.ts`)

- Vendored graphology Leiden (never published to npm), deterministic seed `0xc0de` (`:112`), resolution 1.0, or 2.0 when there are more than 10K symbols (`:482`), 60s timeout with a fallback. An optional native engine (`@ladybugmem/icebug`) runs in a worker so a native crash cannot kill analyze.
- Projection (`:304-382`): only Function/Class/Method/Interface (`:431-435`); only CALLS/EXTENDS/IMPLEMENTS (`:437-438`); name-guessed edges always excluded; on large graphs, low-confidence edges and degree-1 nodes are dropped.
- Output: `Community{id: comm_N, heuristicLabel, cohesion, symbolCount}` and `MEMBER_OF` edges. Label = most common non-generic parent folder, else a common name prefix, else `Cluster_N` (`:755-815`). Cohesion = sampled internal-edge ratio (`:843-867`). Singletons dropped.
- Used in: `impact` (affected modules → risk), process classification (intra vs cross community), resources, generated skills, hook ranking (internal only, never in output).

### 2.7 Processes / execution flows (`gitnexus/src/core/ingestion/process-processor.ts`)

- Entry-point scoring (`entry-point-scoring.ts`): callees/(callers+1) ratio, an export bonus, name patterns (`main|init|start`, `handle*`, `on*`, `*Handler`, `*Controller`, `process*`, `execute*`, …), utility penalties (`get*/set*/is*`, `format*/parse*`, `*Util`), framework path detection, test files excluded.
- Forward DFS over CALLS, excluding name-guessed edges. Budgets: depth 10, branching 4, 75 processes, at least 3 steps, 200 entry candidates (`process-detection-budget.ts:15-20`).
- Output: `Process{processType, stepCount, communities[], entryPointId, terminalId}` plus `STEP_IN_PROCESS{step}` edges.
- **Truncation is published** (`ProcessTruncationStats`, `process-processor.ts:62-120`): the number of entry candidates dropped, unexplored entry points, depth-capped traces, dropped callees. The principle: "a silently truncating cap reads as 'this is everything'."
- Dynamic/indirect calls: covered in resolution rather than here, through callable-value flow (singleton 0.8, multi-target 0.7, target set capped at 32), interface dispatch fan-out, property-key dispatch (cap 32) and DI `INJECTS`. Everything past a cap is reported, not guessed.

### 2.8 Query path

- `impact` BFS (`gitnexus/src/mcp/local/local-backend.ts:7964-8340`): **application-driven, one Cypher round-trip per depth level**:
  ```cypher
  MATCH (caller)-[r:CodeRelation]->(n)
  WHERE n.id IN $frontierIds AND r.type IN $relTypes [AND r.confidence >= $minConfidence]
  RETURN n.id AS sourceId, caller.id, caller.name, labels(caller)[0], caller.filePath, r.type, r.confidence, r.staticGated
  ```
  (`local-backend.ts:8238-8241`). The per-node argmax edge and the deterministic ordering are done in JS. `ORDER BY` was removed because sorting every frontier edge was too slow (`:8204-8237`).
- Class targets seed the frontier with constructors, the owning File, and properties typed with that class (`:8072-8140`). Without the constructor seed, Java impact on a class found nothing.
- Epistemic probe runs concurrently with the BFS (`:8034-8046`).
- `detect_changes`: hunks are coalesced per file, then one unlabeled `MATCH (n)` scan per file batch with range overlap `n.startLine <= hi AND n.endLine >= lo`, then STEP_IN_PROCESS lookup (`local-backend.ts:6141-6460`, query near `:6313`).
- Tool descriptions are **very long** (the `impact` description alone is ~3K tokens, `tools.ts:507-559`). The MCP client loads all 17 into the system prompt, which taxes every agent session whether the tools are used or not.

### 2.9 Incremental / freshness

- Early exit when `lastCommit == HEAD` (`ARCHITECTURE.md:466`).
- The incremental path still **runs the full pipeline** (the parse cache replays unchanged files; scope resolution walks every reference site), then writes only a subgraph: nodes of changed files plus any edge touching them (`gitnexus/src/core/incremental/subgraph-extract.ts`). If the write set exceeds 50% of files and there are ≥50 files, it escalates to wipe-and-bulk-COPY (`incremental/escalation-gate.ts`).
- Communities and processes are fully recomputed on **any** content change; reuse is allowed only when the diff is empty (`incremental/derived-writeback.ts`).
- LadybugDB cannot write to a table with a live FTS index, so the index is dropped and rebuilt (24% of incremental time).
- No local watcher. Freshness is classified as `current/behind/diverged/unknown` against git HEAD (`gitnexus/src/core/git-staleness.ts`).

---

## 3. Cypher → CozoDB Datalog

| Pattern | GitNexus (Cypher/LadybugDB) | CozoDB | Verdict |
|---|---|---|---|
| Typed node tables + FROM/TO pair DDL | 509 declared pairs; missing pair = analyze abort | Relations are untyped tuples; `kind` is a column | **Much better** — the whole failure class disappears. |
| Filter by edge type | `r.type IN $list`, no rel-property index → scan | `edge{src, dst, kind}` key order + `::index` on `{dst, kind, src}` → prefix seek | **Better** |
| Blast radius (multi-hop) | App-level loop, one query per depth, argmax in JS | One recursive query with semi-lattice `min(depth)` in the head | **Better** — zero round-trips, depth-minimal by construction |
| Shortest path | BFS in app / Cypher | `ShortestPathBFS` / `ShortestPathDijkstra` fixed rules | **Better** |
| Import cycles / SCC | Custom code | `StronglyConnectedComponent` fixed rule | **Better** |
| Communities | graphology Leiden in RAM, then persisted | `CommunityDetectionLouvain` fixed rule, in-DB | **Mixed** — Louvain (not Leiden) can produce badly connected communities. Fine for v1; revisit. |
| PageRank / hot symbols | Not present | `PageRank` / `DegreeCentrality` fixed rules | **Better** (free feature) |
| Diff hunk → symbol range overlap | Unlabeled `MATCH (n)` scan over every node table | `symbol:by_file{path, start_line, ...}` index + range predicate | **Better** |
| Bounded DFS flow enumeration with branching caps and score ordering | Imperative DFS | Path enumeration in Datalog blows up; top-k-per-node needs care | **Worse** → keep in Rust over adjacency pulled from CozoDB |
| Deterministic pagination / capped fan-out / truncation stats | JS | Awkward in Datalog | **Worse** → Rust post-processing |
| Full-text search | Kuzu FTS, not incremental, blocks writes | CozoDB FTS index (`::fts create`), maintained on write per docs | **Likely better** — verify incremental cost under the watcher |
| Ad-hoc queries by LLM | Cypher is familiar to LLMs | Datalog is not | **Worse for LLM-authored queries** → don't expose raw query as an MCP tool (already decided) |

---

## 4. What to steal / avoid (Graphite lens)

**Steal (v1):**
1. The honest-uncertainty envelope: `epistemic`, 2–3 `causes`, `risk: UNKNOWN` on zero callers, `partial`/`truncated`. → assertiveness.
2. Depth semantics in impact output (WILL BREAK / LIKELY / MAY NEED TESTING). → correctness, fewer tokens spent interpreting.
3. Categorical edge provenance plus numeric confidence. Exclude name guesses from flows and communities.
4. Disambiguation: candidates + `uid` round-trip.
5. Class-target seeding (constructors, owning file, typed properties) in impact.
6. PreToolUse Grep/Glob augmentation, served by the always-running server over a socket.
7. Unified capture tags in `.scm` query files, one channel per language.
8. Schema fingerprint → rebuild on mismatch.
9. Deterministic community seed; drop singletons; folder-based labels.

**Avoid:**
1. Graph computed in RAM with the DB as a sink → Graphite computes traversals **in** CozoDB.
2. Full-pipeline incremental → Graphite re-parses per file and updates only that file's symbols and edges.
3. Commit-based freshness → watcher.
4. Byte-truncation budgets → rank-based budgets.
5. ~3K-token tool descriptions → keep each short (Graphite's single-tool philosophy already points here).
6. Source content duplicated in the DB → store byte ranges + body hash and read from disk (the file is current thanks to the watcher).
7. Many opt-in flags that change the shape of results (`--pdg`, `--embeddings`, `--skills`) → fewer modes, one shape.

---

## 5. Risks surfaced for Graphite

- **Embedded DB longevity.** GitNexus left KuzuDB for a fork. CozoDB's last tagged release I know of is 0.7.6 (late 2023), and development activity has been low since. **Verify maintenance status before committing.** Mitigation: keep all DB access behind a thin `store` module (already in architecture.md) so a swap stays possible. Keep queries in named Datalog files.
- **Louvain vs Leiden** quality for communities: measure on real repos before exposing communities as "subsystems" in runner EXPLORING context.
- **Concurrent write + read under the watcher.** The CozoDB SQLite backend is effectively single-writer. RocksDB may handle a continuous writer plus MCP readers better. Benchmark both.
- **Resolution depth is where correctness lives.** GitNexus's resolution layer is years of edge cases (receiver chains, MRO, generic-aware dispatch). Graphite v1 must ship with honest `lower-bound` reporting precisely because its resolution will be shallower.

---

## 6. Proposed CozoDB schema for Graphite

Schema follows the v1 features above: `graphite_explore`/context, impact, diff_impact, trace, communities-lite, confidence/epistemic, test mapping, the agent trace for the frontend, and the PreToolUse augmentation.

```datalog
# ── Files ────────────────────────────────────────────────────────────────
# Watcher unit of work. content_hash (BLAKE3) short-circuits no-op saves.
:create file {
    path: String
    =>
    lang: String,
    content_hash: Bytes,
    size: Int,
    mtime_ns: Int,
    parse_status: String,        # 'ok' | 'partial' | 'failed'  -> feeds epistemic
    is_test: Bool,
    graph_rev: Int               # monotonically increasing sync counter
}

# ── Symbols ──────────────────────────────────────────────────────────────
# One relation for every kind (no per-kind tables -> no FROM/TO pair problem).
# id = "path::qualified#arity" (human-readable; the agent can pass it back as uid).
:create symbol {
    id: String
    =>
    name: String,
    qualified: String,
    kind: String,                # function|method|struct|class|trait|interface|impl|enum|type|const|field|module|route
    path: String,
    start_line: Int, end_line: Int,
    start_byte: Int, end_byte: Int,   # source read from disk -> no content in DB
    exported: Bool,
    signature: String,           # body-less; what explore returns by default
    parent: String?,             # owning class/impl/module id (containment without an edge scan)
    body_hash: Bytes             # validates the disk read against the indexed version
}
::index create symbol:by_file { path, start_line, id }   # diff hunk -> symbol overlap
::index create symbol:by_name { name, id }              # disambiguation / lookup
::fts create symbol:fts { extractor: concat(name, ' ', qualified), tokenizer: Simple, filters: [Lowercase] }

# ── Edges ────────────────────────────────────────────────────────────────
# Collapsed per (src, dst, kind); call sites kept as a list for "line-numbered" output.
# provenance is categorical (GitNexus lesson: a number alone cannot exclude guesses).
#   extracted  - direct from the AST, same file or explicit import      -> EXTRACTED
#   resolved   - scope/type resolution                                   -> EXTRACTED
#   inferred   - dispatch fan-out, callable-value flow, DI               -> INFERRED
#   name_guess - unique global name fallback                             -> INFERRED (excluded from flows/communities)
:create edge {
    src: String, dst: String, kind: String
    =>
    provenance: String,
    confidence: Float,
    sites: [Int],                # call-site lines in src's file
    reason: String?
}
::index create edge:by_dst { dst, kind, src }           # upstream (callers) seek

# ── Unresolved references (drives `epistemic` + `causes`) ────────────────
:create unresolved {
    path: String, line: Int, name: String
    =>
    cause: String,               # 'receiver_typing' | 'dispatch_boundary' | 'external'
    origin: String               # 'in_program' | 'external'
}
::index create unresolved:by_name { name, cause, path, line }

# ── Tests ────────────────────────────────────────────────────────────────
# Materialized from reachability from test-file symbols. Serves runner VALIDATING.
:create covers {
    test: String, target: String
    =>
    depth: Int, provenance: String
}
::index create covers:by_target { target, test }

# ── Communities (v1-lite) ────────────────────────────────────────────────
:create community {
    id: Int
    =>
    label: String, cohesion: Float, size: Int, algo: String, graph_rev: Int
}
:create member_of { sym: String => community: Int }
::index create member_of:by_comm { community, sym }

# ── Flows (v2, schema reserved) ──────────────────────────────────────────
:create flow {
    id: String
    =>
    label: String, entry: String, terminal: String,
    scope: String,               # 'intra' | 'cross' community
    steps: Int, truncated: Bool
}
:create flow_step { flow: String, step: Int => sym: String }
::index create flow_step:by_sym { sym, flow, step }

# ── Agent trace (frontend observatory + savings dashboard) ───────────────
:create trace {
    session: String, seq: Int
    =>
    ts: Int, tool: String, args: Json,
    result_tokens: Int, source_tokens: Int, latency_us: Int,
    symbols: [String], graph_rev: Int
}

# ── Meta ─────────────────────────────────────────────────────────────────
:create meta { key: String => value: Any }   # schema_fingerprint, graph_rev, lang versions
```

**Key design choices, justified:**
- **One `symbol` relation with `kind`**: removes GitNexus's 509-pair DDL and its abort class. Kind-specific extras (return type, declared type) can go in a `symbol_ext{id => props: Json}` side relation later.
- **`edge` keyed `{src, dst, kind}`**: downstream is a prefix seek on `src`; `edge:by_dst` makes upstream a prefix seek on `dst`. Collapsing per site keeps traversal fan-out minimal (speed), and `sites` keeps line-level precision (correctness).
- **No source text in the DB**: the watcher keeps disk and graph in sync. `body_hash` catches the rare race. This cuts DB size and write amplification on every save.
- **`unresolved` as first-class data**: `epistemic`/`causes` becomes a cheap aggregate query instead of an index-time summary blob.
- **`covers` materialized**: runner VALIDATING needs "which tests exercise these symbols" in one seek, not a traversal per call.
- **`graph_rev` everywhere**: replaces commit-based staleness. A response carries the rev it answered from, and the trace records it.

### 6.1 Datalog translations of GitNexus's key queries

**(1) `impact` upstream: blast radius with depth, relation filter, and guesses excluded.** Replaces an app-level loop that ran one Cypher query per depth.
```datalog
reach[s, min(d)] := *edge:by_dst{dst: $target, kind, src: s, provenance},
                    is_in(kind, $kinds), provenance != 'name_guess', d = 1
reach[s, min(d)] := reach[m, d0], d0 < $max_depth,
                    *edge:by_dst{dst: m, kind, src: s, provenance},
                    is_in(kind, $kinds), provenance != 'name_guess', d = d0 + 1

?[depth, id, name, kind, path, start_line] :=
    reach[id, depth],
    *symbol{id, name, kind, path, start_line},
    *file{path, is_test}, or(!is_test, $include_tests)
:order depth, path, start_line
```
Epistemic companion (one aggregate query, run concurrently):
```datalog
?[cause, count(line)] := *symbol{id: $target, name}, *unresolved:by_name{name, cause, line}
```

**(2) `context`: categorized incoming/outgoing for one symbol.**
```datalog
?[dir, kind, other, name, path, line, provenance] :=
    *edge:by_dst{dst: $id, kind, src: other}, *edge{src: other, dst: $id, kind, provenance, sites},
    line in sites, dir = 'in', *symbol{id: other, name, path}
?[dir, kind, other, name, path, line, provenance] :=
    *edge{src: $id, dst: other, kind, provenance, sites},
    line in sites, dir = 'out', *symbol{id: other, name, path}
```

**(3) `detect_changes` / `diff_impact`: diff hunks → changed symbols** (hunks passed as `[[path, lo, hi], ...]`).
```datalog
changed[id] := h in $hunks, path = get(h, 0), lo = get(h, 1), hi = get(h, 2),
               *symbol:by_file{path, start_line, id}, start_line <= hi,
               *symbol{id, end_line}, end_line >= lo
```
Feed `changed[]` into query (1) as a multi-seed (`dst` bound by `changed[t]`) and join `covers:by_target` to get tests. That makes Graphite's consolidated `diff_impact` a **single Datalog program**.

**(4) `trace`: shortest path A → B over call and member edges.**
```datalog
g[a, b]      := *edge{src: a, dst: b, kind}, is_in(kind, ['calls', 'has_member', 'implements'])
start[]      <- [[$from]]
goal[]       <- [[$to]]
?[s, t, cost, path] <~ ShortestPathBFS(g[], start[], goal[])
```

**(5) Communities (v1-lite) persisted.**
```datalog
g[a, b] := *edge{src: a, dst: b, kind, provenance},
           is_in(kind, ['calls', 'extends', 'implements']), provenance != 'name_guess',
           *symbol{id: a, kind: ka}, is_in(ka, ['function', 'method', 'class', 'struct', 'trait', 'interface'])
comm[c, n] <~ CommunityDetectionLouvain(g[])
?[sym, community] := comm[community, sym]
:replace member_of { sym => community }
```
(Label and cohesion are computed in Rust from `member_of` + `symbol.path`, with GitNexus's folder heuristic. Singletons are dropped there.)

**Keep in Rust, not Datalog:** process/flow enumeration (bounded DFS with branching caps and entry scoring), per-depth pagination, rank-based token budgets, truncation counters.
