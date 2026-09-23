# Graphite — Positioning

## Core Phrase

**The graph your agent reads before it acts.**

Code understanding doesn't need more tokens. It needs a map — structured, queryable, instant.

## Pitches

### For the engineer
"Embedded code graph that extracts dependencies via Tree-sitter, stores them in CozoDB, and serves blast radius queries to your AI agent in microseconds. One binary, zero config."

### For the AI tooling builder
"MCP server that gives Claude Code, Cursor, or Codex a surgical view of your codebase. Instead of reading 40 files to understand a change, the agent calls one tool and gets exactly the dependency chain."

### For the performance-minded
"Graft loads megabytes of JSON per query. Graphite queries an indexed graph database. Concurrent reads, incremental sync, recursive traversals in microseconds. Rust, not Node."

## Voice

| Layer | Register |
|-------|----------|
| Primary | Technical, direct, no fluff. Show the mechanism. |
| Examples | Real queries, real output, real repos. |
| Comparisons | Measured, specific. Numbers, not adjectives. |

**Tone**: engineering log, not marketing page. Confident because measured. Show the query, show the result, let the reader decide.

## Use / Avoid

| Use | Avoid |
|-----|-------|
| graph, traversal | index, search |
| blast radius | impact analysis |
| embedded, in-process | serverless, lightweight |
| surgical context | smart context |
| Datalog query | database query |
| incremental sync | auto-update |
| symbol, dependency | node, edge (in user-facing text) |
| tool call | API call (for MCP) |
| zero-config | easy setup |

## Differentiators

1. **Embedded graph, not flat JSON.** CozoDB runs in-process — no daemon, no port, no config. But it's a real graph database with indexing, recursive queries, and concurrent reads. JSON can't do any of that.

2. **Datalog for code.** Transitive closure in 3 lines. "Every file that transitively depends on this function, at any depth" is a native operation, not a BFS loop in JavaScript.

3. **Rust single binary.** No runtime, no npm, no Python venv. Download, run. Compile CozoDB and Tree-sitter into one executable.

4. **Incremental by default.** Only re-parse files that changed since last sync. Git-aware: use `git diff` to know what moved.

5. **MCP-native.** Not an afterthought adapter. The MCP tools are the primary interface. CLI is for humans, MCP is for agents.

6. **Privacy absolute.** No cloud, no telemetry, no embeddings server. The graph lives next to your code, readable as CozoDB files.

## Landscape

| Tool | Language | Storage | Query | Approach |
|------|----------|---------|-------|----------|
| Graft | Node.js | JSON files | CLI/MCP (full JSON parse) | Tree-sitter AST to JSON |
| GitNexus | ? | Graph DB | MCP | AST + community clusters |
| Aider RepoMap | Python | In-memory | Injected prompt | PageRank on dependency graph |
| Goldfish | Go | In-memory | Token budget | PageRank, binary search budget |
| **Graphite** | **Rust** | **CozoDB (embedded)** | **MCP (Datalog)** | **Tree-sitter + persistent indexed graph** |
