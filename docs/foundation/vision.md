# Graphite — Vision

## What

Embedded code-graph engine: a Rust CLI that extracts dependency graphs from source code via Tree-sitter, persists them in CozoDB (embedded Datalog database), and serves surgical context to AI agents via MCP tools.

## Pain

1. **Blind agents.** AI assistants read dozens of files to understand architecture. Token-expensive, slow, often hallucinates relationships.
2. **Flat context.** JSON/text dumps lack structure. The agent gets a haystack, not a map. No blast radius, no transitive dependencies, no call chains.
3. **Cold start on every session.** Each new conversation re-discovers the same codebase. No persistent, queryable understanding.
4. **JSON ceiling.** Tools like Graft serialize AST relationships into flat JSON. Works for small repos, but parsing megabytes of JSON per tool call doesn't scale — no indexing, no incremental queries, no concurrent reads.
5. **Infrastructure tax.** Graph databases (Neo4j, Dgraph) require daemons, ports, configuration. Developers won't adopt tools that demand setup.

## Thesis

Tree-sitter extracts structure. CozoDB (embedded, Datalog) stores it as a proper graph with recursive queries in microseconds. MCP exposes surgical tools — the agent asks "who depends on this function?" and gets exactly that, not a file dump.

The graph is the context layer. Classical graph traversal for the majority, full-file reads only when the agent genuinely needs source. The cost ladder applied to code understanding.

## Scope (v1)

- Tree-sitter parsing for Rust, TypeScript, Python, Go (extensible to 20+ languages)
- Symbol extraction: functions, structs/classes, traits/interfaces, imports, exports
- Dependency graph: who-calls-who, who-imports-who, type references
- CozoDB persistence: embedded SQLite backend, zero-config
- Recursive Datalog queries: blast radius, transitive closure, shortest path between symbols
- MCP server: tools for `blast_radius`, `dependents`, `dependencies`, `symbol_search`, `file_overview`
- CLI commands: `graphite init`, `graphite sync`, `graphite query`, `graphite serve`
- Incremental sync: only re-parse changed files (via file mtime or git diff)
- Single binary, zero dependencies, zero configuration

## Not Scope

- Not an IDE plugin. CLI + MCP server, agents consume it.
- Not a linter or formatter. Reads structure, doesn't judge it.
- Not a code search engine. Understands relationships, not full-text content.
- Not cloud. Everything local, no telemetry, no network calls.
- No embeddings. Graph relationships, not semantic similarity.

## Audience

Engineers using AI code assistants (Claude Code, Cursor, Codex) on medium-to-large codebases who want faster, cheaper, more accurate agent responses. Comfortable with CLI tools. Value performance and privacy over cloud convenience.
