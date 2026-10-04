---
title: Architecture
order: 1
---

> The graph your agent reads before it acts.

Embedded code-graph engine. One Rust daemon per repo; CLI, MCP shim, hooks and runner are thin clients. Tree-sitter extracts, CozoDB stores facts and rules, an in-memory adjacency serves hot traversals.

## Hybrid Engine

Two layers, each doing what it's measured best at:

| Layer | Owns | Why |
|---|---|---|
| **CozoDB** (embedded Datalog, RocksDB) | Facts, persistence, confidence rules, search | Ready-made features, fast per-file writes (~0.2 ms), no read-blocking during writes |
| **In-memory adjacency** | Hot traversals: blast radius, callers/callees, paths | Plain Rust BFS: 19–80 µs on worst real hubs vs 3–13 ms through DB |

A background crate handles community detection (Louvain/Leiden) and PageRank off the query path.

### One source of truth

CozoDB is the only thing written by the watcher. The in-memory adjacency is derived from CozoDB facts, rebuilt on daemon start, updated from the same per-file delta after each committed write. Incremental result must equal a full rebuild (fixture-tested).

## How it flows

```
Source code ──► Tree-sitter ──► CozoDB (facts + rules, RocksDB)
                                       │
                                       ▼ derived
                              In-memory adjacency (hot traversals)
                                       │
                              ONE DAEMON PER REPO
                                       │ unix socket
                    ┌──────────┬───────┴────────┬──────────┐
                    CLI      MCP shim      Hooks       Runner
                  (Bash)     (stdio)    (grep/edit)    bridge
```

## Daemon

One daemon per repo, started with `graphite daemon`. Owns CozoDB, the watcher, and the in-memory adjacency. CLI, MCP shim, hooks, and runner bridge are thin clients over a local unix socket. One hot graph serves N sessions.

### Watcher

File system events trigger: `seqno++`, mtime+size stamp, BLAKE3 content hash, Tree-sitter parse (rayon), one atomic CozoDB transaction replacing that file's facts, same delta applied to in-memory adjacency, `graph_rev++`.

### Freshness barrier

Each query records the watcher seqno at arrival and waits up to ~200 ms for indexing to reach it. If it doesn't, the query answers from the last committed graph with `stale: true`. Every response carries `graph_rev`.

### Probe mode (fallback)

When no daemon runs, CLI commands probe the filesystem before querying. Compares file mtimes against last sync, incrementally syncs changed files, then queries CozoDB. Adds per-invocation cost; acceptable for CI and one-off use.

## Agent surface

All surfaces are thin clients of the daemon, sharing the same handlers.

- **Transparent interception** (primary): PreToolUse hook rewrites the agent's `grep`/`find`/`cat` via `updatedInput`; the daemon runs the search with embedded ripgrep and answers with enriched grep: integrated `path:line:` lines annotated by the graph, a verdict header, and a short footer.
- **CLI via Bash**: for explicit graph questions. `graphite blast` / `diff-impact` answer in graph-centric text; `--json` for programs.
- **MCP shim** (stdio): for hosts that don't defer tools and for runner's SDK sessions.
- **Hooks**: pre-edit impact + covering tests injection; post-edit nudge to the daemon.

## Tech stack

| Component | Technology |
|-----------|------------|
| Language | Rust |
| AST parsing | Tree-sitter (compiled in) |
| Graph storage | CozoDB via `mnestic` fork (RocksDB) + in-memory adjacency |
| Daemon IPC | Unix socket |
| Content hashing | BLAKE3 |
| CLI | clap |
| File watching | notify (cross-platform) |
| Serialization | serde + serde_json |
