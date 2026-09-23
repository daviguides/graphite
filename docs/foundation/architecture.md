# Graphite — Architecture

> The graph your agent reads before it acts.

Embedded code-graph engine. Rust CLI + MCP server. Tree-sitter extracts, CozoDB stores, Datalog queries.

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

Incremental updates:

```
graphite sync
  │
  ├─ git diff --name-only HEAD~1  (or mtime comparison)
  │
  ├─ For each changed file:
  │    ├─ Tree-sitter parse
  │    ├─ Extract symbols + dependencies
  │    ├─ Delete old entries for this file
  │    └─ Insert new entries
  │
  └─ Total time: ~ms for typical commits
```

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
│   │   ├── git.rs            # Git diff integration
│   │   └── incremental.rs    # File change detection
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
| Git integration | gix (pure Rust git) |
| Async | tokio |

## Design Principles

1. **Single binary** — everything compiles into one executable, including Tree-sitter grammars and CozoDB
2. **Zero config** — `graphite init` in any repo, works immediately
3. **Incremental first** — never re-parse what hasn't changed
4. **Graph queries, not file reads** — the agent gets relationships, not raw source
5. **Token-efficient responses** — MCP tools return compact summaries, not dumps
6. **Privacy absolute** — no network, no telemetry, no cloud. Graph lives beside code
7. **Language-extensible** — adding a language means implementing one trait
8. **Concurrent-safe** — multiple agents can query simultaneously (CozoDB handles this)
