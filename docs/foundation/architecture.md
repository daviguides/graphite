# Graphite — Architecture

> The graph your agent reads before it acts.

Embedded code-graph engine. Rust CLI + MCP server. Tree-sitter extracts, CozoDB stores facts and rules, an in-memory adjacency serves hot traversals.

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
│                 │                                │
│        ┌────────┴────────┐                      │
│        ▼                 ▼                      │
│   ┌─────────┐     ┌───────────┐                 │
│   │  CLI    │     │MCP Server │                 │
│   │  Human  │     │  Agents   │                 │
│   └─────────┘     └───────────┘                 │
└─────────────────────────────────────────────────┘
```

## Optimization Targets

Graphite is not optimized for ease of setup. It is optimized for making the AI agent complete tasks **faster**, with **more confidence**, and with **fewer mistakes**.

### 1. Agent Execution Speed

Not query latency — total wall-clock time the agent takes to finish a task. Every Read/Grep the agent skips, every turn eliminated, every tool call avoided. The graph exists so the agent acts instead of exploring.

Background file watcher keeps CozoDB hot at all times. MCP queries return in microseconds from an already-indexed graph. No probe, no rebuild in the query path. The agent never waits.

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

### 5. MCP Server

Tools exposed to AI agents:

| Tool | Input | Output |
|------|-------|--------|
| `blast_radius` | symbol name or file path | list of affected files/symbols with depth |
| `dependents` | symbol name | who calls/uses this symbol |
| `dependencies` | symbol name | what this symbol calls/uses |
| `symbol_search` | query string | matching symbols with file:line |
| `file_overview` | file path | symbols defined, imports, dependency counts |
| `path_between` | source, target | shortest dependency path |
| `hot_symbols` | top_n | most-depended-on symbols (high-impact change targets) |

Each tool queries CozoDB and returns a compact, token-efficient response.

### 6. Sync Engine

Two modes: always-hot (server) and on-demand (CLI).

#### Server mode (primary): background watcher

When the MCP server runs (`graphite serve`), a background thread watches the filesystem and keeps CozoDB current. Queries never pay sync cost.

```
graphite serve
  │
  ├── thread 1: MCP handler
  │   └── query CozoDB (microseconds, always fresh)
  │
  ├── thread 2: file watcher (FSEvents / inotify / ReadDirectoryChangesW)
  │   └── file changed → content-hash check → tree-sitter parse → update CozoDB
  │       (incremental: only changed files, only affected edges)
  │
  └── CozoDB instance (shared across threads)
      ├── reads: lock-free, concurrent, from any thread
      └── writes: serialized by watcher thread, non-blocking for readers
```

The watcher uses content hashes (BLAKE3) to skip files whose bytes haven't changed (rename, touch, save-without-edit). Only files with actual content changes trigger a re-parse. Edge updates are incremental: delete old entries for the changed file, insert new ones. Dependents of changed symbols are not rebuilt — their edges still point to the same symbol IDs.

#### CLI mode (fallback): probe + sync

When no server is running, CLI commands probe the filesystem before querying:

```
graphite blast <symbol>
  │
  ├─ probe: compare file mtimes against last sync timestamp
  ├─ if stale: incremental sync (changed files only)
  └─ query CozoDB
```

This adds a few milliseconds per invocation. Acceptable for human-driven CLI use, but the server mode exists because agents make dozens of queries per session and cannot afford per-query overhead.

### 7. CLI

```
graphite init           # Parse entire repo, build initial graph
graphite sync           # Incremental update (changed files only)
graphite query <datalog> # Run raw Datalog query
graphite blast <symbol>  # Shorthand for blast radius
graphite deps <symbol>   # Direct dependencies
graphite overview <file> # File summary
graphite serve           # Start MCP server (stdio or HTTP)
graphite stats           # Graph stats: files, symbols, edges
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
│   │   ├── mod.rs            # CozoDB wrapper
│   │   ├── schema.rs         # Relation definitions
│   │   └── queries.rs        # Named Datalog queries
│   ├── sync/
│   │   ├── mod.rs            # Sync orchestration
│   │   ├── watcher.rs        # Background file watcher (FSEvents/inotify)
│   │   ├── hasher.rs         # BLAKE3 content hashing
│   │   ├── probe.rs          # CLI fallback: mtime-based staleness check
│   │   └── incremental.rs    # Incremental parse + edge update
│   ├── mcp/
│   │   ├── mod.rs            # MCP server
│   │   ├── tools.rs          # Tool definitions
│   │   └── transport.rs      # stdio / HTTP transport
│   └── types.rs              # Symbol, Dependency, SymbolKind, etc.
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
| Graph storage | CozoDB (Rust, embedded, Datalog) |
| CLI | clap |
| MCP | rmcp or custom stdio JSON-RPC |
| Serialization | serde + serde_json |
| File watching | notify (cross-platform FSEvents/inotify/ReadDirectoryChangesW) |
| Content hashing | blake3 |
| Git integration | gix (pure Rust git) |
| Async | tokio |

## Design Principles

1. **Agent speed above all** — every design decision is measured by whether it reduces the agent's total execution time. Setup convenience is secondary.
2. **Always-hot graph** — background watcher keeps CozoDB current. Zero sync cost in the query path. The agent never waits for a rebuild.
3. **Source in results** — MCP tools return verbatim source with line numbers alongside graph context. The agent doesn't need a follow-up Read to see the code.
4. **Confidence signals** — every edge carries a confidence tag (EXTRACTED / INFERRED). The agent knows when to trust the graph and when to verify.
5. **Complete flows** — dynamic dispatch, framework routes, and cross-module edges are resolved so the agent sees the full execution path, not just static imports.
6. **Incremental always** — content-hash-based extraction cache. Only re-parse files whose bytes actually changed. Only update edges for affected symbols.
7. **Single binary (consequence, not rule)** — Tree-sitter grammars, CozoDB, MCP server, file watcher compile into one executable because Rust makes that cheap. Optional accelerators (e.g. Laya) run as sidecars when that serves the targets better.
8. **Concurrent-safe** — multiple agents query simultaneously while the watcher writes. CozoDB handles isolation.
9. **Language-extensible** — adding a language means implementing one trait.
