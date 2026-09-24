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
"One daemon per repo keeps the graph hot for every agent session. Blast radius in microseconds from an in-memory adjacency, facts and rules in CozoDB. Source code inline in results — zero follow-up reads. BLAKE3 content hashing, atomic per-file updates, reads never wait for the writer. Rust, not Node."

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
| per-repo daemon, background watcher | service, server (for the daemon) |
| surgical context | smart context |
| source inline | code snippets |
| Datalog query | database query |
| EXTRACTED / INFERRED | certain / uncertain |
| agent finishes faster | agent gets better context |
| exploration eliminated | exploration reduced |

## Differentiators

1. **Speed target is the agent, not the tool.** Every design decision is measured by whether it reduces the agent's total task execution time. Not query latency, not setup time — how fast the agent finishes. Always-hot daemon, source inline, consolidated queries, steering hooks — all serve this.

2. **Always-hot graph, shared.** One daemon per repo indexes eagerly and serves every session and runner task over a local socket. No rebuild in the query path — at most a short freshness barrier, and every answer carries the graph revision that produced it.

3. **Source code in results.** Responses return verbatim source with line numbers alongside graph context. The agent doesn't need a follow-up Read. One call = dependency context + the actual code. CodeGraph proved this eliminates round-trips.

4. **Hybrid engine, measured.** CozoDB (Datalog) holds facts and derives confidence by rule, so incremental always equals a full rebuild; hot traversals run on an in-memory adjacency (19–80 µs on the worst real hub, benchmarked on real repos). Each part does what it's measured best at.

5. **Edge confidence.** Every edge tagged EXTRACTED, INFERRED or AMBIGUOUS, with provenance. Responses say when a result is a lower bound and why. The agent knows when to trust the graph vs when to verify with a file read. Assertiveness from transparency.

6. **Answers where the agent already is.** CLI via Bash first (always live in Claude Code, where MCP tools must be loaded first), MCP second, and hooks that answer a grep with the graph's result and inject impact before an edit. Hint-only steering measured ~0% uptake; answering converts.

7. **Rust single binary.** Tree-sitter grammars, CozoDB, daemon, clients, file watcher — one executable. No runtime, no npm, no Python venv.

8. **Runner-native.** Only tool in the space that integrates with an orchestration layer. Context injection per workflow mode. EXPLORING gets architecture map, IMPLEMENTING gets blast radius, VALIDATING gets targeted tests.

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

Competing tools optimize for **ease of adoption** (npm install, zero config, works immediately). Graphite optimizes for **agent execution speed** (background watcher, source inline, consolidated queries, runner integration). Convenience follows from good engineering — single binary and zero-config are consequences of compiling everything into Rust, not design targets. Different priorities produce different architectures.
