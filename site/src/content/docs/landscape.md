---
title: Landscape
order: 6
---

How Graphite compares to other code-graph tools for AI agents.

## The field

| Tool | Stars | Language | Storage | Key approach |
|------|-------|----------|---------|-------------|
| **Graphify** | 120.9K | Python | NetworkX | Knowledge graph, Leiden communities, non-code ingestion |
| **CodeGraph** | 71.9K | TS + Rust | SQLite WAL + FTS5 | Best resolution layer, 20+ framework synthesizers, real-time watcher |
| **GitNexus** | 47.5K | TypeScript | KuzuDB | Execution flow tracing, embedded graph DB |
| **codebase-memory** | 44.5K | C | Knowledge graph | 162 languages, sub-ms queries, single binary |
| **Graft** | 9.1K | TypeScript | JSON + markdown | LLM semantic tier, pre-query freshness, SWE-bench +12pts |
| **Graphite** | - | **Rust** | **CozoDB (Datalog)** | **Always-hot daemon, source inline, edge confidence, runner integration** |

## Architecture comparison

| Aspect | Graft | CodeGraph | Graphify | Graphite |
|--------|-------|-----------|----------|----------|
| Extraction | Tree-sitter + LLM | WASM tree-sitter + Rust kernel | Python tree-sitter | Tree-sitter compiled into Rust binary |
| Resolution | AST-based + optional LSP | Multi-layer: imports, names, 20+ frameworks, dynamic dispatch | Tree-sitter + cross-file | AST-based + confidence rules |
| Storage | Flat JSON + markdown | SQLite (WAL + FTS5) | NetworkX DiGraph | CozoDB (Datalog, RocksDB) + in-memory adjacency |
| Freshness | Pre-query fingerprint (~3ms) | Real-time file watcher (~1s) | Manual or watchdog | Eager background watcher, freshness barrier |
| Output | Markdown nodes + source excerpts | Verbatim line-numbered source | Graph context (labels, edges only) | Verbatim source + graph annotations + confidence |

## Feature matrix

| Feature | Graft | CodeGraph | Graphify | Graphite |
|---------|-------|-----------|----------|----------|
| Languages (depth) | 9 | 34+ | 45+ | 3 (Python, Rust, TS) |
| Non-code support | No | No | PDF, images, video | No |
| LLM required | Optional | Never | Optional | Never |
| Real-time watching | No (probe) | Yes | Optional | Yes (eager daemon) |
| Community detection | No | No | Yes (Leiden) | Yes (background) |
| Framework resolution | No | Yes (20+) | Partial | Planned (v3) |
| Dynamic dispatch | No | Yes | No | Planned (v3) |
| Source in results | Yes | Yes (verbatim) | No | Yes (verbatim + annotations) |
| Blast radius | Yes | Yes | Yes | Yes (with depth labels + confidence) |
| Edge confidence | No | No | Yes | Yes (EXTRACTED/INFERRED/AMBIGUOUS) |
| Transparent interception | No | No | No | Yes (hooks rewrite grep/find/cat) |
| Runner integration | No | No | No | Yes (per-mode context injection) |
| Agent benchmarks | SWE-bench +12pts | 44% cost, 62% tokens | LOCOMO recall 0.497 | 21% fewer turns, 54% fewer file reads |

## What sets Graphite apart

### Always-hot daemon

One daemon per repo indexes eagerly via file watcher. Every agent session queries the same hot graph through a unix socket. No rebuild in the query path; at most a short freshness barrier. Competing tools either probe on each query (Graft), require manual sync (Graphify), or watch but don't share across sessions (CodeGraph).

### Source inline with graph annotations

Every response returns verbatim source code with line numbers, annotated by graph context (callers, confidence tags, depth labels). CodeGraph also returns source; Graft returns excerpts; Graphify returns labels and edges without source. Graphite's responses also carry epistemic disclosure: whether the result is a lower bound and why.

### Edge confidence

Every edge tagged EXTRACTED, INFERRED, or AMBIGUOUS with categorical provenance. The agent knows when to trust the graph and when to verify with a file read. Graphify has a similar concept but exposes it differently; Graft and CodeGraph don't tag edges at all.

### Transparent interception

PreToolUse hooks rewrite the agent's habitual `grep`/`find`/`cat` commands into enriched graph-aware answers. The agent gets structural context without changing its workflow. No other tool in the landscape does this; Graphify's strict mode denies Read access to force graph-first, which breaks agent workflows instead of enriching them.

### Hybrid engine (benchmarked)

CozoDB (Datalog) for facts, rules, and search. In-memory adjacency for hot traversals (19-80 us on worst real hubs). Community detection and PageRank run in the background. The choice was made after benchmarking CozoDB, LadybugDB (Kuzu fork), and a DIY stack (redb + Ascent) on real and synthetic graphs.

### Runner integration

Only tool in the space that integrates with an orchestration layer. Per-mode context injection: EXPLORING gets architecture map, IMPLEMENTING gets blast radius, VALIDATING gets targeted tests.

## Gaps in the landscape

Things no tool does well yet:

1. **Incremental edge resolution**: all rebuild edges from scratch on change
2. **Type-aware resolution**: none use actual type inference for edge resolution
3. **Cross-language interop edges**: C FFI, JNI, WASM boundaries remain invisible
4. **Build system integration**: Makefile/CMake/Bazel dependencies affect blast radius but aren't modeled
5. **Git-aware temporal edges**: co-change history as graph edges ("these files always change together")
6. **Diff-aware query mode as first-class**: "what changed + what does it affect" in one operation (Graphite's `diff-impact` addresses this)
7. **Multi-agent coordination**: no shared write-through cache for concurrent agents querying the same graph (Graphite's daemon model addresses this)

## MCP tool philosophy

| Tool | MCP tools | Philosophy |
|------|-----------|-----------|
| Graft | 6 | One tool per operation |
| CodeGraph | 1 primary | Consolidated (data shows agents under-pick extra tools) |
| Graphify | 11 | One tool per graph operation |
| Graphite | CLI + hooks | Agent uses its existing commands; hooks enrich transparently |

CodeGraph's finding matters: agents under-pick from large toolsets. Graphite's approach avoids the problem entirely by intercepting commands the agent already uses.
