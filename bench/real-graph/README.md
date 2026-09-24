# real-graph

Quick tree-sitter extractor that builds a real code graph from a repo and measures its shape
(fan-in, blast radius, cycles). Used to check whether the storage benchmark's synthetic dataset
looks like real code. Standalone crate: not a member of the `bench/` workspace.

Good enough for shape measurement, not a production resolver (see caveats in
`references/studies/real-graph-shape.md`).

## Run

```bash
cargo build --release
# strict: default resolver (bounded name guesses, stdlib/third-party receivers excluded)
./target/release/real-graph ~/work/sources/continuum continuum-strict tools=3 gradients=2
./target/release/real-graph ~/work/sources/sensemesh/sensemesh sensemesh-strict cloud=2 aux=2 tools=2
# permissive: unbounded name guesses (upper bound on edges and blast radius)
RG_PERMISSIVE=1 ./target/release/real-graph ~/work/sources/continuum continuum-permissive tools=3 gradients=2
RG_PERMISSIVE=1 ./target/release/real-graph ~/work/sources/sensemesh/sensemesh sensemesh-permissive cloud=2 aux=2 tools=2
python3 summarize.py
```

`prefix=depth` groups files under `prefix/` by their first `depth` path components
(e.g. `tools=3` -> `tools/orch/runner`); other files group by their first directory.
The target repos are only read, never modified.

## Outputs

- `results/<label>.json` — shape report (committed): counts, edges by kind/provenance,
  resolution outcome of every call site, fan-in distribution, top 20 hubs, blast radius
  distributions at depth 3 / 10 / unbounded for two edge sets, SCC stats, per-group stats.
- `data/<label>/symbols.jsonl` and `data/<label>/edges.jsonl` — the graph itself (gitignored).

## Data format

`symbols.jsonl`, one symbol per line, `i` is the dense node index used by edges:

```json
{"i": 42, "id": "tools/orch/runner/runner/core/engine.py#run_task:function",
 "name": "run_task", "kind": "function", "parent": null,
 "file": "tools/orch/runner/runner/core/engine.py", "line": 173, "lang": "python"}
```

- `kind`: `module` (one per file, `name` = path), `function`, `method`, `class`, `struct`,
  `enum`, `trait`, `interface`, `type`, `union`.
- `parent`: enclosing type for methods (class / impl type / Go receiver), else null.
- `id`: deterministic, `path#[Parent.]name:kind`, with `~N` appended on collision; modules use the path.

`edges.jsonl`, one directed edge per line (`src` depends on / contains `dst`):

```json
{"src": 42, "dst": 97, "kind": "calls", "prov": "extracted"}
```

- `kind`: `calls` (caller -> callee), `imports` (module -> module), `implements`
  (type -> base/trait/interface), `contains` (container -> member).
- `prov`: `extracted` (resolved through scope or imports) or `inferred` (name guess:
  unknown receiver type, or unique name outside the file's scope).
- Blast radius of `t` = nodes reaching `t` over reversed edges. `blast_calls` uses calls+implements;
  `blast_all` uses every kind, the same semantics as the synthetic benchmark (`common::Reference::blast`).

Loading into a benchmark engine: map `i` to the engine's node id, load every edge; the
`trusted` rule of the synthetic dataset corresponds to `prov == "extracted"`.
