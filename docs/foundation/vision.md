# Graphite — Vision

## What

Embedded code-graph engine: a Rust daemon with CLI, MCP and hook clients that extracts dependency graphs from source code via Tree-sitter, persists them in CozoDB (embedded Datalog database), and serves surgical context to AI agents. Built to make the agent finish tasks faster, act with more confidence, and make fewer mistakes.

## Optimization Targets

1. **Agent execution speed.** Not query latency — total wall-clock time the code assistant takes to complete a task. Every file read eliminated, every turn skipped, every exploration phase removed. The graph exists so the agent acts instead of wandering.

2. **Assertiveness.** The agent acts with confidence because it has the right context upfront. No "let me check a few more files", no hesitation, no backtracking. Edge confidence tags tell the agent when to trust the graph and when to verify. Community boundaries tell it where subsystem borders are.

3. **Correctness.** The agent makes fewer mistakes because blast radius is visible, dynamic dispatch flows are mapped, framework routes are connected, and nothing is silently missed. Source code is returned inline so the agent sees what the graph knows without a second round-trip.

These are not balanced equally. Speed of the code assistant's execution is the primary target. Assertiveness and correctness serve speed — a hesitant agent is a slow agent, an incorrect agent repeats work.

**These three targets are the only absolutes in this project.** Every other rule in these docs (storage choice, sync model, scope boundaries, integrations) is a derived decision, valid only while it serves the targets, and revisable when evidence shows it hurts one. No technology, dependency, or approach is excluded by principle — only by measured effect on speed, assertiveness, or correctness.

## Pain

1. **Blind agents.** AI assistants read dozens of files to understand architecture. Each file read is a tool call, each tool call is latency, each wrong file is a wasted turn. The agent explores when it should be acting.
2. **Hesitant agents.** Without structural context, agents hedge. "Let me verify by reading one more file." Five more files later, the agent finally acts — often on stale understanding from the first file it read.
3. **Incorrect agents.** Changes break downstream code the agent never saw. Blast radius is invisible. Dynamic dispatch hides call flows. Framework routes aren't connected to handlers. The agent patches one file and misses its siblings.
4. **Cold start on every session.** Each new conversation re-discovers the same codebase. No persistent, queryable understanding. The understanding dies with the session.
5. **Flat context tools.** Competing tools serialize AST relationships into JSON or markdown files. Works for small repos, but parsing megabytes per tool call doesn't scale. No indexed queries, no concurrent reads, no incremental updates at edge level.

## Thesis

The agent's execution time is dominated by exploration, not action. Graphite eliminates exploration.

Tree-sitter extracts structure. A per-repo daemon keeps the graph always hot: CozoDB holds the facts and rules, an in-memory adjacency answers traversals in microseconds, and the watcher indexes eagerly — no rebuild in the query path. Responses return verbatim source code with dependency context — the agent gets the answer AND the code in one call, through the CLI it already uses or MCP, and hooks deliver the answer at the moment it would otherwise grep or edit blind.

The agent starts every session already knowing the architecture. It acts immediately, with confidence, and with full visibility of what its changes will affect.

## Scope (v1)

Every feature is placed on the versioned [roadmap in features.md](features.md#roadmap) (v1–v8, each with goal, dependencies and exit criteria; v1 and its waves are a proposal pending confirmation). v1 ships in four vertical waves, each usable and measured on its own:

- **v1.0 — thesis slice:** Python extractor, daemon + watcher, `diff_impact` + blast radius with source inline and confidence, CLI `--json`, with-vs-without bench on real Continuum tasks. Go / no-go on "the agent finishes faster".
- **v1.1 — full agent surface:** Rust / TS / Go, `context` + search + grep, pre-grep and pre-edit hooks, MCP shim, routing bench.
- **v1.2 — runner integration:** socket bridge, per-mode injection, targeted tests.
- **v1.3 — contract completion:** compression, risk verdict, depth labels, CLI fallback without daemon.

v1 as a whole covers:

- Tree-sitter extraction for Rust, TypeScript, Python, Go: symbols, calls, imports, inheritance, references, tests
- Edge confidence (EXTRACTED / INFERRED / AMBIGUOUS) with provenance, derived by rule
- One daemon per repo: CozoDB (`mnestic`, RocksDB) facts + in-memory adjacency, eager watcher, freshness barrier, `graph_rev` watermark
- Queries: `diff_impact`, `context`, blast radius, search — source inline, honest disclosure, risk verdict
- CLI via Bash as primary agent surface, MCP shim secondary, steering hooks (pre-grep answer, pre-edit impact)
- Runner integration: socket bridge, per-mode prompt injection, targeted tests in VALIDATING
- Measurement: trace, conversion, turns and wall-clock with vs without Graphite

## Not a Target

- Convenience is a consequence of good engineering, not a design target. Single binary and zero-config happen because Rust compiles everything in, not because we optimized for setup experience.

Scope beyond v1 is decided feature by feature against the three targets — see the feature inventory.

## Audience

Engineers running AI code assistants (Claude Code, Cursor, Codex) on medium-to-large codebases where agent execution time matters. Orchestration builders (like Continuum runner) who drive autonomous agent workflows and need surgical context injection per mode. Value speed and correctness over convenience.
