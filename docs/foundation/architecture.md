# Graphite — Architecture

> The graph your agent reads before it acts.

Embedded code-graph engine. Rust CLI + MCP server. Tree-sitter extracts, CozoDB stores, Datalog queries.

## Why CozoDB

CozoDB was chosen over alternatives for specific architectural reasons:

- **Over KuzuDB** — KuzuDB uses Cypher (pattern matching). CozoDB uses Datalog (recursive rules). Transitive closure, PageRank, community detection are native Datalog operations, not query-language extensions. CozoDB also has a Rust crate (`cozo`); KuzuDB's Rust bindings are less mature.
- **Over SQLite** — Relational, not graph-native. Code dependency traversal requires recursive CTEs that are verbose and slow compared to Datalog's native recursion. SQLite has no concept of graph edges or traversal.
- **Over JSON/markdown files** — No indexing, no concurrent reads, entire graph loaded per query. Works for small repos, collapses at scale. Graft's approach.
- **Over NetworkX** — Python in-memory graph. No persistence, no indexing, no concurrent access. Graphify's approach.

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

### 3. Storage Layer (CozoDB)

Embedded, in-process. Two backend options:
- **SQLite** (default): persistent, survives restarts, good for repos you work on daily
- **Memory**: ephemeral, fastest, good for one-shot analysis

```rust
use cozo::DbInstance;

let db = DbInstance::new("sqlite", "graphite.db", "")?;

// Create relations
db.run_script(r#"
    :create file { path: String => lang: String, mtime: Int }
    :create symbol { id: String => name: String, kind: String, file: String, line: Int, visibility: String, signature: String }
    :create depends { from_sym: String, to_sym: String => kind: String }
    :create imports { file: String, target: String => symbols: [String] }
"#, Default::default())?;
```

### 4. Query Layer (Datalog)

Recursive queries are CozoDB's strength. Examples:

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
7. **Single binary** — Tree-sitter grammars, CozoDB, MCP server, file watcher all compile into one executable.
8. **Privacy absolute** — no network, no telemetry, no cloud, no LLM. Graph lives beside code.
9. **Concurrent-safe** — multiple agents query simultaneously while the watcher writes. CozoDB handles isolation.
10. **Language-extensible** — adding a language means implementing one trait.
