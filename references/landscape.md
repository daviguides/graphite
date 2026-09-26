# Code Graph for AI Agents — Landscape Research

> Research conducted 2026-09-23 via Opus 5.5 consultation (two agents: landscape search + deep source-code analysis).

## Repos Cloned (references/repos/)

| Repo | Stars | Lang | Storage | Key Feature |
|------|-------|------|---------|-------------|
| **Graphify** | 120.9K | Python | NetworkX → JSON | Knowledge graph, Leiden communities, non-code (PDF/images/video) |
| **CodeGraph** | 71.9K | TS + Rust kernel | SQLite WAL + FTS5 | Best resolution layer, 20+ framework synthesizers, real-time watcher |
| **GitNexus** | 47.5K | TypeScript | KuzuDB (embedded graph DB) | Closest storage model to CozoDB, execution flow tracing |
| **codebase-memory** | 44.5K | C | Knowledge graph | 162 languages, academic paper, single binary |
| **Graft** | 9.1K | TypeScript | JSON + markdown files | LLM semantic tier, pre-query freshness, SWE-bench +12pts |
| **code-graph-mcp** | 77 | Rust | SQLite + FTS5 + sqlite-vec | BLAKE3 Merkle incremental, dirty propagation. Closest tech stack. |
| **llm-context-mgr** | 61 | Rust | Petgraph + LanceDB | Rust core, reasoning-aware retrieval |
| **repomap-rs** | 0 | Rust | In-memory | Aider PageRank algorithm in Rust |
| **LeanKG** | 220 | Go (was Rust) | SQLite WAL + PG/pgvector (was CozoDB 0.7.6) | Exact→fuzzy→semantic query ladder with provenance on every response; dropped CozoDB (single-writer, FFI abort, zero recursive Datalog used) |
| **infigraph** (intuit) | 89 | Rust | LadybugDB `lbug =0.16.0` active; CozoDB-sqlite parked backend | 62 languages, SCIP compiler-grade edges, ~90 MCP tools, output compression, `cozo_vs_kuzu` bench harness |
| **ferrograph** | 3 | Rust | CozoDB 0.7 (mem / sqlite) | Only tool with true recursive Datalog blast radius; git change-coupling edges; Rust ownership/borrow edges |

## Not Cloned (reference only)

| Project | Lang | Notes |
|---------|------|-------|
| **Aider RepoMap** | Python | Pioneer. Personalized PageRank with conversation biasing. Part of aider. |
| **Goldfish** | Go | Go port of Aider's algorithm. Token budget via binary search. |
| **RepoMapper** | Python | NetworkX, Louvain clustering, SCC detection, topological sort. |
| **CodeGraphContext** | Python/TS | 6+ graph DB backends (FalkorDB, KuzuDB, Neo4j). |
| **mcp-context-graph** | Python | Minimal MCP graph server. Good reference for MVP. |
| **Diffbot mcp-code-graph** | Python | Cloud-hosted (DeepGraph). Enterprise. |
| **Sourcegraph MCP** | Go | Enterprise platform. SCIP-based, not Tree-sitter. |
| **mcp-server-tree-sitter** | Python | Raw Tree-sitter access over MCP. Lowest level. |

## Deep Comparison: Graft vs CodeGraph vs Graphify

### Architecture

| Aspect | Graft | CodeGraph | Graphify |
|--------|-------|-----------|----------|
| Extraction | Native tree-sitter + LLM | WASM tree-sitter + Rust kernel | Python tree-sitter |
| Resolution | AST-based + optional LSP | Multi-layer: imports, names, 20+ frameworks, dynamic dispatch synthesizers | Tree-sitter + cross-file |
| Storage | Flat JSON + markdown files | SQLite (WAL + FTS5, schema) | NetworkX DiGraph → JSON |
| Query | In-memory load + keyword scoring | SQL + BFS/DFS + FTS5 | BFS/DFS + trigram + IDF |
| Freshness | Pre-query fingerprint (~3ms) | Real-time file watcher (~1s) | Manual or optional watchdog |
| Output | Markdown nodes + source excerpts | Verbatim line-numbered source | Graph context (labels, edges only) |
| Philosophy | "Explain like a senior engineer" | "Return exact code agent needs" | "Build queryable knowledge graph" |

### Feature Matrix

| Feature | Graft | CodeGraph | Graphify |
|---------|-------|-----------|----------|
| Languages (depth) | 9 | 34+ | 45+ |
| Non-code support | No | No | PDF, images, video, docs |
| LLM required | Optional (Tier-2) | Never | Optional |
| Real-time watching | No (probe) | Yes (native) | Optional |
| Community detection | No | No | Yes (Leiden) |
| Framework resolution | No | Yes (20+) | Partial |
| Dynamic dispatch | No | Yes | No |
| Source in results | Yes (crux) | Yes (verbatim) | No |
| Blast radius | Yes | Yes | Yes |
| PR analysis | No | No | Yes |
| Browser UI | Yes | Yes (Svelte) | Yes |
| Edge confidence | No | No | Yes (EXTRACTED/INFERRED/AMBIGUOUS) |
| Cross-repo | No | No | Yes |
| Agent targets | 4 | 11+ | 15+ |
| Benchmarks | SWE-bench +12pts | 44% cost, 62% tokens | LOCOMO recall 0.497 |

### MCP Tools

**Graft (6):** graft_find_code, graft_file_api, graft_check_freshness, graft_trace_calls, graft_find_all, graft_repo_map

**CodeGraph (1 primary):** codegraph_explore (consolidated — data shows agents under-pick additional tools)

**Graphify (11):** query_graph, get_node, get_neighbors, get_community, god_nodes, graph_stats, shortest_path, list_prs, get_pr_impact, triage_prs + project_path support

### What to Steal for Graphite

Evaluated through Graphite's optimization targets: does it make the AI agent's total execution **faster**, more **assertive**, or more **correct**?

**Agent speed** (reduce total task execution time):

- Source code in results (CodeGraph) — agent gets verbatim line-numbered source in MCP response. Eliminates Read round-trips. Directly cuts turns and wall-clock time.
- Real-time file watcher with native FSEvents (CodeGraph) — graph always hot, zero sync cost per query. Agent never waits for a rebuild.
- Content-hash extraction cache (Graft) — BLAKE3 hash means unchanged files replay their last parse. Watcher overhead stays minimal.
- Token-budgeted repo map (Graft) — agent gets architecture orientation in one call. Eliminates the 10-15 exploratory Reads at session start.
- Adaptive explore budget scaled to project size (CodeGraph) — prevents context bloat on large repos while giving enough on small ones.
- Diff-aware query mode (gap — none have it) — "what changed + blast radius" as a single operation. Agent gets actionable scope instantly.
- Test-to-code mapping (gap — none have it) — which tests cover changed symbols. Runner VALIDATING runs only relevant tests, minutes become seconds.

**Assertiveness** (agent acts with confidence, no hesitation):

- Edge confidence tagging: EXTRACTED/INFERRED/AMBIGUOUS (Graphify) — agent knows which edges are certain vs guessed. Reads files only where confidence is low.
- Leiden community detection (Graphify) — automatic subsystem boundaries. Agent understands module ownership without reading READMEs.
- Single-tool philosophy (CodeGraph) — data shows agents under-pick additional tools. One powerful `graphite_explore` tool that returns everything beats 11 specialized tools the agent ignores.
- NotIndexedError as success-shaped response (CodeGraph) — returning `isError: true` teaches agents to abandon the toolset entirely. Success-shaped "not indexed yet, run graphite init" keeps the agent engaged.

**Correctness** (agent makes fewer mistakes):

- Dynamic dispatch synthesizers (CodeGraph) — callbacks, EventEmitter, React re-render chains, tier-crossing edges. Without these, blast radius misses entire flow branches. "The flow must exist in the graph end-to-end."
- Framework-aware resolution (CodeGraph) — Express/Django/Axum route-to-handler edges. HTTP request → route → handler → service is one connected path, not four orphaned symbols.
- Git-aware temporal edges (gap — none have it) — co-change history as graph edges. "These files always change together" prevents the agent from editing one and forgetting the other.

**Deprioritized** (ease-of-use, not performance):

- Pre-query freshness probe (Graft) — elegant for zero-config but adds latency per query. Replaced by background watcher in server mode. Kept only as CLI fallback.
- Two-tier model with LLM semantic layer (Graft) — adds provider dependency and cost.
- Non-code ingestion (Graphify) — PDFs, images, video in the graph. Broadens scope but doesn't make code editing faster or more correct. Future consideration.
- Cross-repo support (Graphify) — valuable for microservices but adds complexity. Not v1.
- Hook-based strict mode (Graphify) — deny Read to force graph-first. Superseded for Graphite by transparent interception: hooks let the agent's habitual commands (grep/rg/find/cat/sed -n/ls) run and enrich them with the graph, dropping only lines the graph provably explains (see features.md v1 steering).

### What to Avoid

- **Graft**: JSON-only storage doesn't scale. LLM-dependent semantic tier adds cost and provider coupling.
- **CodeGraph**: Over-complexity in resolution (40+ framework files). WASM tree-sitter was too slow (hence Rust kernel). Context bloat (80% more residual context acknowledged).
- **Graphify**: NetworkX doesn't scale. Loading entire graph.json per query. Too many MCP tools (11). Concept-level nodes without source code in results.

### Gaps — What None of Them Do

1. **Incremental edge resolution** — all rebuild edges from scratch
2. **Type-aware resolution** — none use actual type inference
3. **Cross-language interop edges** — C FFI → Python, JNI, WASM boundaries
4. **Build system integration** — Makefile/CMake/Bazel dependencies affect blast radius
5. **Test-to-code mapping** — which tests cover which functions
6. **Git-aware temporal edges** — co-change history as graph edges
7. **Diff-aware query mode** — "what changed + what does it affect" as first-class
8. **Streaming/progressive results** — all return complete results
9. **Multi-agent coordination** — no shared write-through cache for concurrent agents

## Business Models

| Aspect | Graft | CodeGraph | Graphify |
|--------|-------|-----------|----------|
| Core | Open-source CLI | Open-source CLI | Open-source CLI |
| Cloud | Trail Brain (CLAUDE.md mgmt) | Platform (PR intelligence) — waitlist | Hosted graph + verification + PR review |
| Pricing | Not public | Not launched | $0-$29/seat/mo + Enterprise |
| YC backed | No (NanoNets) | No | Yes (S26) |

## Notable Findings from Documentation Sites

### Graphify — Enterprise Features (not in open-source)

**Differential Formal Verification** — six-tier verification ladder:
- Tier 1: SMT (Z3) — sound proofs over all inputs for pure Python
- Tier 2: Loop-invariant tier — Z3-checked coupling invariants
- Tier 3: CrossHair — concolic testing
- Tier 4: Property tier — deterministic corpus in network-denied sandbox
- Tier 5: Trace-carving — real pytest suite data
- Tier 6: Honest abstain — explicit "unsupported" when verification can't help

Verdicts: `equivalent`, `distinguished` (concrete divergence), `may_equivalent` (empirical only), `unsupported`. ~17-45% decisive verdicts with full ladder + test suite. No LLM in verification path.

**CI/CD Gate**: `graphify gate --verify-edits --base origin/main --block-behavior-change` — blocks PRs that change behavior. Neither Graft nor CodeGraph has CI integration.

**Hosted MCP**: `api.graphify.com/mcp` — OAuth-authenticated cloud endpoint. Agent queries pre-built graph without running locally.

**Memory System**: 2K ingests/day, 10K stored memory turns/repo. Conversational memory beyond graph.

### CodeGraph — Platform Vision
"Change-impact intelligence for AI-written code" — platform (waitlist) will focus on PR analysis: what to test, what could break, which flows affected.

### Graft — Trail Brain
Cloud product at app.trailhq.com: converts agent corrections into persistent rules, manages AGENTS.md automatically, adds hooks that block rule violations.

## Graphite Positioning in Landscape

| Differentiator | Why it matters |
|---------------|----------------|
| **CozoDB Datalog** | No one else uses Datalog. Recursive queries (transitive closure, PageRank, community detection) are native, not hand-coded BFS loops. |
| **Rust single binary** | code-graph-mcp proves Rust+Tree-sitter+MCP works. CozoDB adds indexed persistent graph DB. |
| **Embedded graph DB** | Not JSON (Graft), not SQLite (CodeGraph), not NetworkX (Graphify). A real graph database with indexing and concurrent reads. |
| **Runner integration** | No competitor integrates with an orchestration layer. Surgical context injection per workflow mode. |
| **Frontend observatory** | Agent trace, savings dashboard, blast radius review — none offer observability of agent behavior. |
