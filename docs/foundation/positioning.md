# Graphite — Positioning

## Core Phrase

**The graph that makes your agent finish faster.**

Not "understand your codebase" — finish the task. Faster, with fewer mistakes, without hesitation.

## Pitches

### For the engineer
"Your AI agent spends 60% of its time exploring before it acts. Graphite gives it the dependency graph upfront — blast radius, call chains, source code inline. The agent acts in turn 1, not turn 15."

### For the orchestration builder
"Runner composes prompts for 7 workflow modes. Graphite injects surgical context per mode: architecture map for EXPLORING, coupling matrix for PLANNING, blast radius for IMPLEMENTING, targeted tests for VALIDATING. 40-60% faster end-to-end."

### For the performance-minded
"Background watcher keeps CozoDB hot. Datalog queries return in microseconds. Source code inline in results — zero follow-up reads. BLAKE3 content hashing, incremental edge updates, concurrent reads. Rust, not Node."

## Voice

| Layer | Register |
|-------|----------|
| Primary | Technical, direct, no fluff. Show the mechanism. |
| Examples | Real queries, real output, real repos. |
| Comparisons | Measured, specific. Wall-clock time, not adjectives. |
| Claims | Always about agent execution time, not tool features. |

**Tone**: engineering log, not marketing page. Confident because measured. Show the before/after in turns and seconds, not in feature lists.

## Use / Avoid

| Use | Avoid |
|-----|-------|
| execution speed, wall-clock time | fast, quick, lightweight |
| assertive, confident | smart, intelligent |
| correct, complete | accurate, precise |
| blast radius, transitive dependents | impact analysis |
| always-hot graph | real-time sync |
| background watcher | daemon, service |
| surgical context | smart context |
| source inline | code snippets |
| Datalog query | database query |
| EXTRACTED / INFERRED | certain / uncertain |
| agent finishes faster | agent gets better context |
| exploration eliminated | exploration reduced |

## Differentiators

1. **Speed target is the agent, not the tool.** Every design decision is measured by whether it reduces the agent's total task execution time. Not query latency, not setup time — how fast the agent finishes. Background watcher, source inline, consolidated queries — all serve this.

2. **Always-hot graph.** Background file watcher keeps CozoDB current. MCP queries hit an already-indexed graph. No probe, no rebuild, no "freshness check" in the query path. The agent never waits.

3. **Source code in results.** MCP tools return verbatim source with line numbers alongside graph context. The agent doesn't need a follow-up Read. One tool call = dependency context + the actual code. CodeGraph proved this eliminates round-trips.

4. **Datalog for code.** Transitive closure in 3 lines. Blast radius, shortest path, community detection, PageRank — native Datalog operations, not hand-coded BFS loops. CozoDB is the only embedded Datalog DB in this space.

5. **Edge confidence.** Every edge tagged EXTRACTED (certain from AST) or INFERRED (resolved heuristically). The agent knows when to trust the graph vs when to verify with a file read. Assertiveness from transparency.

6. **Two-layer integration.** Not just prompt injection (upfront context) — agents also get MCP tool access mid-session. They query blast radius after each change, in real time. Layer 1 gives speed, Layer 2 gives assertiveness.

7. **Rust single binary.** Tree-sitter grammars, CozoDB, MCP server, file watcher — one executable. No runtime, no npm, no Python venv.

8. **Runner-native.** Only tool in the space that integrates with an orchestration layer. Context injection per workflow mode. EXPLORING gets architecture map, IMPLEMENTING gets blast radius, VALIDATING gets targeted tests.

9. **Privacy absolute.** No cloud, no telemetry, no LLM, no API keys. Zero cost per query. The graph lives next to your code.

## Landscape

| Tool | Stars | Lang | Storage | Speed Focus | Correctness Focus |
|------|-------|------|---------|------------|-------------------|
| Graphify | 120.9K | Python | NetworkX → JSON | Token reduction | Edge confidence, community detection, verification (enterprise) |
| CodeGraph | 71.9K | TS + Rust | SQLite + FTS5 | Tool call reduction | Dynamic dispatch, framework resolution |
| GitNexus | 47.5K | TypeScript | KuzuDB | Community clustering | Execution flow tracing |
| codebase-memory | 44.5K | C | Knowledge graph | Sub-ms queries | 162 languages |
| Graft | 9.1K | TypeScript | JSON + markdown | Pre-query freshness | SWE-bench +12pts, LLM semantic tier |
| **Graphite** | — | **Rust** | **CozoDB (Datalog)** | **Always-hot graph, source inline, zero exploration** | **Edge confidence, blast radius, framework flows, runner integration** |

### Key distinction

Competing tools optimize for **ease of adoption** (npm install, zero config, works immediately). Graphite optimizes for **agent execution speed** (background watcher, source inline, consolidated queries, runner integration). Different priorities produce different architectures.
