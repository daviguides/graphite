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

**From Graft:**
- Pre-query freshness probe (~3ms fingerprint)
- Content-hash extraction cache
- Two-tier model: structural (free) + optional semantic (cached)
- Token-budgeted repo map (graft_repo_map)
- Regex search grouped by enclosing symbol + ranked by coupling

**From CodeGraph:**
- Dynamic dispatch synthesizers (callbacks, React, EventEmitter, tier-crossing)
- Framework-aware resolution (Express, Django, Axum, etc.)
- Single-tool philosophy (agents under-pick multiple tools)
- NotIndexedError as success-shaped response (not isError — prevents agents abandoning toolset)
- Adaptive explore budget scaled to project size
- Real-time file watcher with native FSEvents

**From Graphify:**
- Leiden community detection for automatic subsystem clustering
- Edge confidence tagging (EXTRACTED/INFERRED/AMBIGUOUS)
- PR impact analysis tools
- Cross-repo graph support
- Non-code ingestion (docs, configs, SQL schemas)
- Hook-based strict mode (deny Read to force graph-first)

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

## Graphite Positioning in Landscape

| Differentiator | Why it matters |
|---------------|----------------|
| **CozoDB Datalog** | No one else uses Datalog. Recursive queries (transitive closure, PageRank, community detection) are native, not hand-coded BFS loops. |
| **Rust single binary** | code-graph-mcp proves Rust+Tree-sitter+MCP works. CozoDB adds indexed persistent graph DB. |
| **Zero LLM** | Pure structural. No provider lock-in, no cost, no API keys. |
| **Embedded graph DB** | Not JSON (Graft), not SQLite (CodeGraph), not NetworkX (Graphify). A real graph database with indexing and concurrent reads. |
| **Runner integration** | No competitor integrates with an orchestration layer. Surgical context injection per workflow mode. |
| **Frontend observatory** | Agent trace, savings dashboard, blast radius review — none offer observability of agent behavior. |
