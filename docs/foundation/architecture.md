# Graphite — Architecture

> The graph your agent reads before it acts.

Embedded code-graph engine. One Rust daemon per repo; CLI, MCP shim, hooks and runner are its thin clients. Tree-sitter extracts, CozoDB stores facts and rules, an in-memory adjacency serves hot traversals.

## Storage Decision: Hybrid (benchmarked 2026-09-24)

Decided after a measured benchmark on real graphs (Continuum, sensemesh) and synthetic stress graphs. Evidence: [storage-benchmark.md](../../references/studies/storage-benchmark.md), [cozodb-health.md](../../references/studies/cozodb-health.md), [rust-embedded-graph-alternatives.md](../../references/studies/rust-embedded-graph-alternatives.md), [real-graph-shape.md](../../references/studies/real-graph-shape.md), [cozo-based-tools.md](../../references/studies/cozo-based-tools.md).

### The split

| Layer | Owns | Why |
|---|---|---|
| **CozoDB via `mnestic` fork, RocksDB backend** | Source of truth: per-file facts (files, symbols, edges), persistence, confidence rules, flexible Datalog queries, full-text search | Ready-made features, shared maintenance, fast per-file writes (~0.2 ms), no crashes under concurrent read/write. RocksDB because SQLite/mem backends block reads during writes. |
| **In-memory adjacency in the daemon** | Hot traversals: blast radius, callers/callees, paths, cycles | Plain Rust BFS over adjacency lists: 19–80 µs on the worst real hub vs 3–13 ms through the DB. Traversal is the part where own code is trivial and a DB pays a fixed per-query cost. |
| **Dedicated Rust crate, background** | Community detection (Louvain/Leiden), PageRank | CozoDB's Louvain cost 1–4.7 s and several GB; run off the query path on the call graph only. |

### Invariant: one source of truth

CozoDB is the only thing written by the watcher. The in-memory adjacency is **derived** from CozoDB facts — rebuilt on daemon start, updated from the same per-file delta after each committed write, never written independently. Incremental result must equal a full rebuild from CozoDB (checked by test). Without this invariant the hybrid becomes two diverging stores — the bug class code-graph-mcp bumped its index format 71 times for.

### Why not the alternatives

- **CozoDB pure** — traversals 40–150× slower than in-memory BFS on hubs (still under target on strict real graphs, 13 ms on permissive); Louvain too heavy for the daemon; a correctness bug in mnestic's cached graph projection (edge kind treated as weight) — use Rust-driven traversal, not its projection.
- **DIY pure (Ascent + redb)** — fastest everywhere, but we would write persistence, rules, search and algorithms ourselves. Speed gain is invisible to an agent whose turns take seconds.
- **LadybugDB** — two native crashes (SIGSEGV/SIGBUS) on query timeout, algorithm extension doesn't load on macOS arm64, ~1 ms per-query floor, 5–23 ms per-file writes. Better fit for large analytical graphs (Cypher), not this workload.
- **SQLite / JSON / NetworkX** — see landscape; recursion too slow or no indexing/concurrency.

### Risks

- `mnestic` has one maintainer. Mitigation: pin exact version; keep CozoDB behind a `GraphStore` trait; the engine (~36K lines) is small enough to own if it stalls.
- Durability settings differ from the benchmark's DIY candidate (fsync per commit); tune RocksDB sync before relying on crash recovery.

## How It Works

```
Source code (any language)
       │
       ▼
┌─────────────────────────────────────────────────┐
│              GRAPHITE ENGINE                      │
│                                                  │
│  ┌──────────────┐     ┌──────────────────┐      │
│  │  Tree-sitter  │     │  Language Grammars│      │
│  │  Parser       │────▶│  Rust/TS/Py/Go   │      │
│  └──────┬───────┘     └──────────────────┘      │
│         │                                        │
│         ▼                                        │
│  ┌──────────────┐                                │
│  │  Symbol       │  functions, structs, imports,  │
│  │  Extractor    │  exports, type references      │
│  └──────┬───────┘                                │
│         │                                        │
│         ▼                                        │
│  ┌──────────────────────────────────────┐       │
│  │           CozoDB (embedded)           │       │
│  │                                       │       │
│  │  Relations:                           │       │
│  │    file(path, lang, mtime)            │       │
│  │    symbol(id, name, kind, file, line) │       │
│  │    depends(from_sym, to_sym, kind)    │       │
│  │    imports(file, target, symbols)     │       │
│  │                                       │       │
│  │  Datalog:                             │       │
│  │    blast_radius[x] := depends(x, y)  │       │
│  │    blast_radius[x] := depends(x, z), │       │
│  │                       blast_radius[z] │       │
│  └──────────────┬───────────────────────┘       │
│                 │ derived (never written directly)│
│                 ▼                                │
│  ┌──────────────────────────────────────┐       │
│  │  In-memory adjacency (hot traversals) │       │
│  └──────────────┬───────────────────────┘       │
│   ONE DAEMON PER REPO — owns all of the above    │
└─────────────────┼───────────────────────────────┘
                  │ local unix socket
     ┌────────────┼──────────────┬──────────────┐
     ▼            ▼              ▼              ▼
┌─────────┐ ┌───────────┐ ┌────────────┐ ┌──────────┐
│  CLI    │ │ MCP shim  │ │   Hooks    │ │  Runner  │
│ (Bash)  │ │  (stdio)  │ │ grep/edit  │ │  bridge  │
└─────────┘ └───────────┘ └────────────┘ └──────────┘
 thin clients — N agent sessions share one hot graph
```

## Optimization Targets

Graphite is not optimized for ease of setup. It is optimized for making the AI agent complete tasks **faster**, with **more confidence**, and with **fewer mistakes**.

### 1. Agent Execution Speed

Not query latency — total wall-clock time the agent takes to finish a task. Every Read/Grep the agent skips, every turn eliminated, every tool call avoided. The graph exists so the agent acts instead of exploring.

One daemon per repo indexes eagerly on its watcher thread and keeps the graph hot. Queries from the CLI, MCP shim, hooks and runner hit an already-indexed graph through a local socket. No probe, no rebuild in the query path; at most a short freshness barrier.

### 2. Assertiveness

The agent acts with confidence because it has the right context upfront. No "let me check a few more files", no hesitation, no backtracking. Edge confidence tags (EXTRACTED vs INFERRED) tell the agent exactly when to trust the graph and when to verify with a file read. Community boundaries tell it where subsystem borders are.

### 3. Correctness

The agent makes fewer mistakes because blast radius is visible, dynamic dispatch flows are mapped, framework routes are connected, and nothing is silently missed. Source code is returned inline with line numbers so the agent doesn't need a second round-trip to read what the graph already knows about.

## Core Components

### 1. Parser Layer

Tree-sitter grammars compiled into the binary. Each language gets a `LanguageExtractor` that maps AST nodes to Graphite's symbol model.

```rust
pub trait LanguageExtractor {
    fn language(&self) -> &str;
    fn extensions(&self) -> &[&str];
    fn extract_symbols(&self, tree: &Tree, source: &[u8]) -> Vec<Symbol>;
    fn extract_dependencies(&self, tree: &Tree, source: &[u8]) -> Vec<Dependency>;
}
```

**Supported (v1)**: Rust, TypeScript/JavaScript, Python, Go
**Extensible**: implement `LanguageExtractor` for any Tree-sitter grammar

### 2. Symbol Model

```rust
pub struct Symbol {
    pub id: SymbolId,           // deterministic hash of file + name + kind + line
    pub name: String,
    pub kind: SymbolKind,       // Function, Struct, Trait, Interface, Class, Type, Const, Enum
    pub file: PathBuf,
    pub line: u32,
    pub visibility: Visibility, // Public, Private, Crate
    pub signature: String,      // condensed type signature (no body)
}

pub struct Dependency {
    pub from: SymbolId,
    pub to: SymbolId,
    pub kind: DepKind,          // Calls, Imports, Implements, References, Contains
}
```

### 3. Storage Layer (CozoDB + in-memory adjacency)

See [Storage Decision](#storage-decision-hybrid-benchmarked-2026-09-24). CozoDB (`mnestic` fork, RocksDB backend) holds the facts; the daemon derives an in-memory adjacency from them for traversals. The schema below is a starting sketch — the proposed schema in [gitnexus.md](../../references/studies/gitnexus.md) §6 supersedes it.

```rust
use cozo::DbInstance;

let db = DbInstance::new("rocksdb", "graphite.db", "")?;

// Create relations
db.run_script(r#"
    :create file { path: String => lang: String, mtime: Int }
    :create symbol { id: String => name: String, kind: String, file: String, line: Int, visibility: String, signature: String }
    :create depends { from_sym: String, to_sym: String => kind: String }
    :create imports { file: String, target: String => symbols: [String] }
"#, Default::default())?;
```

### 4. Query Layer (Datalog)

Datalog serves rules (confidence, derived edges) and flexible/ad-hoc queries. Hot traversals (blast radius, callers, paths) run on the in-memory adjacency in production; the Datalog forms below remain the reference semantics used to verify it. Examples:

**Blast radius** — everything that transitively depends on a symbol:
```datalog
?[file, name, kind, line] :=
    *depends[from_sym, target_sym, _],
    target_sym = $target,
    *symbol[from_sym, name, kind, file, line, _, _]

?[file, name, kind, line] :=
    *depends[from_sym, mid, _],
    blast[mid],
    *symbol[from_sym, name, kind, file, line, _, _]

blast[x] := *depends[x, $target, _]
blast[x] := *depends[x, y, _], blast[y]
```

**Shortest path** between two symbols:
```datalog
path[s, t, 1] := *depends[s, t, _], s = $source
path[s, t, n] := path[s, m, n1], *depends[m, t, _], n = n1 + 1, n < 10

?[t, min(n)] := path[$source, t, n], t = $target
```

**File overview** — all symbols in a file with their dependency count:
```datalog
?[name, kind, line, count(dep)] :=
    *symbol[id, name, kind, $file, line, vis, _],
    *depends[dep, id, _]
```

### 5. Agent Surface (CLI first, MCP second, hooks)

All surfaces are thin clients of the daemon and share the same handlers, so answers are identical whichever path the agent takes.

- **CLI via Bash is the primary agent surface.** In Claude Code, MCP tools are deferred (a ToolSearch load is needed before first use) while Bash is always live; code-graph-mcp measured its conversions coming through the CLI. `graphite <cmd> --json`.
- **MCP shim (stdio) is secondary** — for hosts that don't defer tools and for runner's SDK sessions. Few listed tools (≤5), capability via flags; no `anyOf` in schemas, descriptions ≤200 chars, instructions ≤1.5 KB (measured client limits). A routing bench guards tool descriptions.
- **Steering hooks** (registered in `settings.json`, fail-open): pre-grep **block and answer** with the graph's result; pre-edit **impact + covering tests** injection; post-edit nudge to the daemon. Hint-only steering measured ~0% uptake.

Core queries (full list and verdicts in [features.md](features.md)):

| Query | Input | Output |
|------|-------|--------|
| `diff_impact` | working tree / staged / ref | changed symbols → blast radius → covering tests, with source |
| `context` | symbol (name, disambiguated in response) | signature, source, callers/callees, references, tests |
| `blast_radius` | symbol, depth (default 3), direction | dependents with depth labels, risk, prod/test split |
| `search` | name or text | exact → fuzzy matches with retrieval provenance |

Every response follows the contract in [features.md §5](features.md#5-response-contract): source inline, compact/tiered compression, disclosure fields, `epistemic`, risk, `stale` + `graph_rev`.

### 6. Sync Engine

Two modes: always-hot daemon (primary) and on-demand probe (fallback when no daemon runs).

#### Daemon mode (primary): one daemon per repo

Every Claude Code session and every runner task spawns its own MCP process, so a watcher inside the MCP server would mean N watchers racing on one store. Instead, `graphite daemon` runs once per repo and owns CozoDB, the watcher and the in-memory adjacency. The CLI, the MCP shim, hooks and the runner bridge are thin clients over a local unix socket. One hot graph serves N sessions.

```
graphite daemon  (one per repo)
  │
  ├── watcher thread (FSEvents / inotify / ReadDirectoryChangesW)
  │   └── event → seqno++ → mtime+size stamp → BLAKE3 → parse (rayon)
  │       → ONE atomic CozoDB transaction replacing that file's facts
  │       → apply same delta to in-memory adjacency → graph_rev++
  │
  ├── socket server (N concurrent clients)
  │   └── query → freshness barrier → adjacency (traversals) / CozoDB (rules, search)
  │
  ├── background jobs: communities, PageRank (off the query path)
  │
  └── safety nets: periodic backstop rescan, unknown events = content change,
      index_run_in_flight crash marker, stale-file sweep
```

**Freshness barrier.** Each query records the watcher seqno at arrival and waits up to ~200 ms for indexing to reach it. If it doesn't, the query answers from the last committed graph with `stale: true`. Every response carries `graph_rev` (a watermark stored in the DB) so the agent and the Observatory know which state answered. The post-edit hook nudges the daemon with the edited path so the agent's own edit is indexed before its next query.

**Incremental == full.** The watcher replaces exactly one file's facts; derived edges and confidence are recomputed from facts, and the adjacency is derived from the same delta. A fixture test compares incremental results with a full rebuild.

#### CLI probe mode (fallback): no daemon running

For CI and one-off use, CLI commands probe the filesystem before querying:

```
graphite blast <symbol>
  │
  ├─ probe: compare file mtimes against last sync timestamp
  ├─ if stale: incremental sync (changed files only)
  └─ query CozoDB
```

This adds per-invocation cost. Acceptable when no daemon runs; agents make dozens of queries per session, which is why the daemon is the primary mode.

### 7. CLI

Every query command talks to the daemon when one runs, otherwise uses probe mode. `--json` is the agent format.

```
graphite daemon          # Start the per-repo daemon (watcher + socket)
graphite init            # Parse entire repo, build initial graph
graphite diff-impact     # Changed symbols → blast radius → covering tests
graphite context <sym>   # One symbol: source, callers/callees, refs, tests
graphite blast <sym>     # Blast radius (default depth 3)
graphite search <text>   # Exact → fuzzy symbol search
graphite grep <regex>    # Hits grouped by enclosing symbol (used by the grep hook)
graphite mcp             # MCP stdio shim (thin client of the daemon)
graphite hooks install   # Register steering hooks in settings.json
graphite query <datalog> # Raw Datalog (humans / UI)
graphite stats | health  # Graph stats, parse errors, staleness
```

## Module Map

```
graphite/
├── src/
│   ├── main.rs              # CLI entry point (clap)
│   ├── lib.rs               # Public API
│   ├── parser/
│   │   ├── mod.rs            # Parser orchestration
│   │   ├── extractor.rs      # LanguageExtractor trait
│   │   ├── rust.rs           # Rust extractor
│   │   ├── typescript.rs     # TypeScript/JS extractor
│   │   ├── python.rs         # Python extractor
│   │   └── go.rs             # Go extractor
│   ├── store/
│   │   ├── mod.rs            # GraphStore trait + CozoDB (mnestic, RocksDB) impl
│   │   ├── schema.rs         # Relation definitions + schema fingerprint
│   │   └── queries.rs        # Named Datalog queries (rules, search)
│   ├── graph/
│   │   ├── adjacency.rs      # In-memory adjacency derived from store facts
│   │   ├── traverse.rs       # Blast radius, paths, callers/callees (Rust BFS)
│   │   └── algos.rs          # Communities, PageRank, SCC (background)
│   ├── daemon/
│   │   ├── mod.rs            # Per-repo daemon lifecycle
│   │   ├── socket.rs         # Unix socket server, N concurrent clients
│   │   ├── freshness.rs      # Seqno barrier, graph_rev watermark
│   │   └── jobs.rs           # Background jobs
│   ├── sync/
│   │   ├── watcher.rs        # File watcher + safety nets
│   │   ├── hasher.rs         # mtime/size stamp → BLAKE3 ladder
│   │   ├── probe.rs          # Fallback when no daemon runs
│   │   └── incremental.rs    # Atomic per-file fact replacement
│   ├── query/                # diff_impact, context, blast, search, grep handlers
│   ├── response/             # Contract: source inline, compression, disclosure, risk
│   ├── client/
│   │   ├── cli.rs            # CLI commands (thin client, --json)
│   │   ├── mcp.rs            # MCP stdio shim (thin client)
│   │   └── hooks.rs          # pre-grep / pre-edit / post-edit hook entry points
│   └── types.rs              # Symbol, Edge, Provenance, SymbolKind, etc.
├── grammars/                  # Tree-sitter grammar .so files (or compiled in)
├── tests/
│   ├── fixtures/             # Sample repos for testing
│   └── integration/
├── Cargo.toml
└── docs/
```

## Tech Stack

| Component | Technology |
|-----------|------------|
| Language | Rust |
| AST parsing | Tree-sitter (C, compiled in) |
| Graph storage | CozoDB via `mnestic` fork (RocksDB backend) + in-memory adjacency |
| Daemon IPC | Local unix socket |
| CLI | clap |
| MCP | rmcp or custom stdio JSON-RPC (thin shim over the socket) |
| Serialization | serde + serde_json |
| File watching | notify (cross-platform FSEvents/inotify/ReadDirectoryChangesW) |
| Content hashing | blake3 |
| Git integration | gix (pure Rust git) |
| Async | tokio |

## Design Principles

1. **Agent speed above all** — every design decision is measured by whether it reduces the agent's total execution time. Setup convenience is secondary.
2. **Always-hot graph, one daemon per repo** — the daemon's watcher indexes eagerly; every client (CLI, MCP shim, hooks, runner) shares the same hot graph. At most a short freshness barrier in the query path, never a rebuild.
3. **Source in results** — responses return verbatim source with line numbers alongside graph context. The agent doesn't need a follow-up Read to see the code.
4. **Confidence signals** — every edge carries a tier (EXTRACTED / INFERRED / AMBIGUOUS) plus categorical provenance, derived by rule at query time. The agent knows when to trust the graph and when to verify.
5. **Complete flows** — dynamic dispatch, framework routes, and cross-module edges are resolved so the agent sees the full execution path, not just static imports.
6. **Incremental always** — content-hash-based extraction cache. Only re-parse files whose bytes actually changed. Only update edges for affected symbols.
7. **Single binary (consequence, not rule)** — Tree-sitter grammars, CozoDB, daemon, clients and file watcher compile into one executable because Rust makes that cheap. Optional accelerators (e.g. Laya) run as sidecars when that serves the targets better.
8. **Concurrent-safe** — multiple agents query the daemon simultaneously while the watcher writes. RocksDB snapshot reads don't wait for the writer; the adjacency is updated from committed deltas only.
9. **Agent surface where the agent already is** — CLI via Bash first, MCP second, steering hooks that answer instead of hinting.
10. **Language-extensible** — adding a language means a `.scm` query file with unified capture tags plus a registry entry.
