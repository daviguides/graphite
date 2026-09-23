# Graphite — Vision

## What

Embedded code-graph engine: a Rust CLI + MCP server that extracts dependency graphs from source code via Tree-sitter, persists them in CozoDB (embedded Datalog database), and serves surgical context to AI agents. Built to make the agent finish tasks faster, act with more confidence, and make fewer mistakes.

## Optimization Targets

1. **Agent execution speed.** Not query latency — total wall-clock time the code assistant takes to complete a task. Every file read eliminated, every turn skipped, every exploration phase removed. The graph exists so the agent acts instead of wandering.

2. **Assertiveness.** The agent acts with confidence because it has the right context upfront. No "let me check a few more files", no hesitation, no backtracking. Edge confidence tags tell the agent when to trust the graph and when to verify. Community boundaries tell it where subsystem borders are.

3. **Correctness.** The agent makes fewer mistakes because blast radius is visible, dynamic dispatch flows are mapped, framework routes are connected, and nothing is silently missed. Source code is returned inline so the agent sees what the graph knows without a second round-trip.

These are not balanced equally. Speed of the code assistant's execution is the primary target. Assertiveness and correctness serve speed — a hesitant agent is a slow agent, an incorrect agent repeats work.

## Pain

1. **Blind agents.** AI assistants read dozens of files to understand architecture. Each file read is a tool call, each tool call is latency, each wrong file is a wasted turn. The agent explores when it should be acting.
2. **Hesitant agents.** Without structural context, agents hedge. "Let me verify by reading one more file." Five more files later, the agent finally acts — often on stale understanding from the first file it read.
3. **Incorrect agents.** Changes break downstream code the agent never saw. Blast radius is invisible. Dynamic dispatch hides call flows. Framework routes aren't connected to handlers. The agent patches one file and misses its siblings.
4. **Cold start on every session.** Each new conversation re-discovers the same codebase. No persistent, queryable understanding. The understanding dies with the session.
5. **Flat context tools.** Competing tools serialize AST relationships into JSON or markdown files. Works for small repos, but parsing megabytes per tool call doesn't scale. No indexed queries, no concurrent reads, no incremental updates at edge level.

## Thesis

The agent's execution time is dominated by exploration, not action. Graphite eliminates exploration.

Tree-sitter extracts structure. CozoDB stores it as a persistent indexed graph with recursive Datalog queries in microseconds. A background watcher keeps the graph always hot — no rebuild in the query path. MCP tools return verbatim source code with dependency context — the agent gets the answer AND the code in one call, not a pointer it has to follow.

The agent starts every session already knowing the architecture. It acts immediately, with confidence, and with full visibility of what its changes will affect.

## Scope (v1)

- Tree-sitter parsing for Rust, TypeScript, Python, Go (extensible to 20+ languages)
- Symbol extraction: functions, structs/classes, traits/interfaces, imports, exports
- Dependency graph: who-calls-who, who-imports-who, type references
- Edge confidence tagging (EXTRACTED vs INFERRED)
- CozoDB persistence: embedded SQLite backend
- Recursive Datalog queries: blast radius, transitive closure, shortest path, community detection
- Background file watcher: graph always hot, zero query-time sync cost
- Content-hash extraction cache (BLAKE3): only re-parse files whose bytes changed
- MCP server with source code in results (verbatim, line-numbered)
- Consolidated `diff_impact` query: changed files + blast radius + affected tests + source in one call
- CLI commands: `graphite init`, `graphite serve`, `graphite blast`, `graphite query`
- Single binary: Tree-sitter, CozoDB, MCP server, file watcher compiled into one executable
- Runner integration: prompt injection + mid-session MCP access for orchestrated workflows

## Not Scope

- Convenience is a consequence of good engineering, not a design target. Single binary and zero-config happen because Rust compiles everything in, not because we optimized for setup experience.
- Not an IDE plugin. CLI + MCP server, agents consume it.
- Not a linter or formatter. Reads structure, doesn't judge it.
- Not a code search engine. Understands relationships, not full-text content.
- No embeddings. Graph relationships, not semantic similarity.

## Audience

Engineers running AI code assistants (Claude Code, Cursor, Codex) on medium-to-large codebases where agent execution time matters. Orchestration builders (like Continuum runner) who drive autonomous agent workflows and need surgical context injection per mode. Value speed and correctness over convenience.
