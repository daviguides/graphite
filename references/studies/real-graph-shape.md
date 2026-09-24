# Real Code Graph Shape — Continuum and sensemesh

> 2026-09-24. Validates the synthetic dataset used by the storage benchmark
> (`references/studies/storage-benchmark.md`, `bench/common`) against two real repos.
> Extractor and raw results: `bench/real-graph/` (commit `323b03f`).

## Question

The storage benchmark's "hub" case is a symbol whose depth-10 blast radius reaches
**49,886 of 50,000 nodes (99.8%)**. Only the in-memory Rust BFS met the <10 ms target on it;
CozoDB took 125–300 ms and LadybugDB 62 ms. If real code never produces such a hub,
that case should not decide the engine.

## Method

Quick tree-sitter extractor (`bench/real-graph`), run read-only on:

- **Continuum** (`~/work/sources/continuum`): monorepo of independent tools. 1,146 files:
  909 Python, 177 TS/TSX, 60 Rust.
- **sensemesh** (`~/work/sources/sensemesh/sensemesh`): production, mostly Go. 3,131 files:
  1,385 Go, 1,390 TS/TSX/JS, 356 Python. **501 generated files skipped** (ent ORM,
  protobuf; `Code generated ... DO NOT EDIT` headers).

Graph: one node per symbol (function, method, class/struct/enum/trait/interface/type) plus one
`module` node per file. Edges `calls`, `imports`, `implements`, `contains`, each tagged
`extracted` (resolved through scope or imports) or `inferred` (name guess).

Blast radius is measured over two edge sets:

- **calls**: calls + implements, reversed. Symbol-level dependents, what `blast_radius` should return.
- **all**: every kind reversed, including `contains` and `imports`. Same semantics as the
  synthetic benchmark (`common::Reference::blast`). This is an upper bound: a changed function
  makes its module dependent, and every importer of that module follows.

Two resolver modes bound resolution error:

- **strict** (primary): name guesses for receivers of unknown type stay inside the package unit,
  skip ~100 very common method names (`get`, `Lock`, `Now`, `append`...), and calls through
  stdlib / third-party import aliases are external.
- **permissive**: unbounded name guesses. Reproduces the noisy first pass; upper bound on edges.

All ~11K (Continuum) and ~21K (sensemesh) non-module symbols were measured as targets (no sampling).

## Results

### Size and density

| | Continuum | sensemesh | Synthetic medium |
|---|---|---|---|
| Nodes (strict) | 12,373 | 23,741 | 50,000 |
| Edges (strict) | 27,931 | 76,139 | 240,465 |
| Edges per node | 2.3 | 3.2 | 4.8 |
| Call edges extracted / inferred | 13,230 / 769 | 27,745 / 8,392 | ~85% / 15% |
| Call sites: resolved / ambiguous dropped / external | 16,779 / 494 / 38,338 | 50,718 / 3,942 / 135,207 | — |
| Fan-in (calls) max / p99 / p90 / median | 228 / 15 / 3 / 0 | 315 / 19 / 4 / 1 | power-law, hubs far higher |
| Nontrivial SCCs (all edges), largest | 10, size 3 | 42, size 15 | seeded cycles |

Most call sites are external (stdlib, third-party). Those don't create in-repo edges and
correctly don't count.

### Blast radius at depth 10 (strict)

| | Continuum calls | Continuum all | sensemesh calls | sensemesh all |
|---|---|---|---|---|
| max | 1,266 | 1,792 | 800 | 1,796 |
| p99 | 96 | 197 | 95 | 326 |
| p90 | 12 | 46 | 19 | 94 |
| median | 0 | 3 | 1 | 13 |
| max % of repo | 10.2% | 14.5% | 3.4% | 7.6% |
| targets reaching >10% of repo | 1 | 2 | 0 | 0 |
| targets reaching >50% of repo | 0 | 0 | 0 | 0 |

Permissive upper bound: max 3,695 nodes (29.9%) in Continuum, 6,433 (27.1%) in sensemesh.
Its top hubs are resolution artifacts: `dict.get` resolved to `MeetingRegistry.get`,
`mu.Lock()` to `integrationLocker.Lock`, `time.Now()` to a test fake `fakeClock.Now`.

Depth barely matters past 3. The worst Continuum hub reaches 1,355 nodes at depth 3 and
1,792 at depth 10; the worst sensemesh hub 1,130 and 1,796. Unbounded is within 1% of depth 10
everywhere.

### Real hubs (strict, top by fan-in)

| Repo | Symbol | Fan-in | Blast d10 calls / all |
|---|---|---|---|
| Continuum | `load_yaml` (dao-cli `core/yaml_io.py`) | 228 | 1,266 / 1,792 |
| Continuum | `save_yaml` (same file) | 160 | 968 / 1,426 |
| Continuum | `load_session` (refiner `core/state.py`) | 72 | 131 / 232 |
| sensemesh | `cn` (`ui/src/lib/cn.ts`) | 315 | 800 / 1,175 |
| sensemesh | `errors.Wrap` (`cloud/internal/errors`) | 140 | 279 / 932 |
| sensemesh | `ParseUUID` (`cloud/internal/uuid.go`) | 136 | 217 / 854 |
| sensemesh | `RequirePrincipal` (`cloud/pkg/security`) | 125 | 299 / 991 |

These are the hubs one would expect: a YAML I/O helper shared by the orchestrator tools,
the Tailwind class-merge helper used by every UI component, and error, UUID and auth helpers.

### Monorepo structure

Continuum's tools are nearly isolated: most keep 89–100% of their edges inside the tool (lowest
among tools with >200 edges: 73%). The Continuum-wide maximum comes from dao-cli's `load_yaml`
(1,792), because other tools import dao-cli. Next are tabclean (881, inflated by the `list()`
builtin caveat below) and refiner (495).
sensemesh `cloud/*` services are less isolated: 64–98% internal for most, but `mission_insight`
(22%) and `server` (24%) are mostly glue into other services. Blast radius stays bounded anyway.

## Verdict: the synthetic hub case is not realistic

1. **No real symbol reaches more than 15% of its repo** at depth 10 under benchmark semantics
   (strict), and none reaches 30% even with the noisiest resolution. The synthetic hub reaches
   99.8%. In absolute terms the synthetic hub returns ~28× more nodes than the worst real hub
   (49,886 vs 1,796).
2. **The synthetic "median" symbol is really a p99 symbol.** Its 243-node blast radius sits at the
   real p99 (197–326). Real medians are 3–13 nodes.
3. **The synthetic graph is 1.5–2× denser** than real code (4.8 vs 2.3–3.2 edges per node) with a
   heavier fan-in tail, which is what makes one node reach almost everything.
4. Cycles are small and rare in real code (largest SCC: 3 in Continuum, 15 in sensemesh).

## What the benchmark should target

Replace the synthetic hub/median pair with real-shaped targets:

| Case | Blast size (nodes, depth 10) | Where it comes from |
|---|---|---|
| typical | 3–15 | real median |
| p90 | 50–100 | real p90 (all edges) |
| p99 | 200–350 | real p99 (all edges); ≈ the old synthetic "median" |
| worst real hub | ~1,800 | max in both repos, strict |
| stress (keep, labelled as stress) | ~6,500 or 25–30% of repo | permissive upper bound |

Graph sizes: 12K–24K nodes and 28K–76K edges for these repos. Also run 5× (~120K nodes) for a
large-monorepo projection. Density: ~3 edges per node.

**Better still: run the three engines on the exported real graphs** (`bench/real-graph/data/*`,
JSONL format in `bench/real-graph/README.md`), not only on synthetic data. The storage-benchmark
fork owns `bench/`; this study did not modify it.

### Rough projection (to be confirmed by running on real graphs)

If query time scales roughly linearly with result size, measured cost per returned node at the
synthetic hub gives, for the worst real hub (~1,800 nodes, strict):

| Engine (strategy) | Synthetic hub (49,886) | Projected worst real hub (~1,800) | Projected stress (~6,400) |
|---|---|---|---|
| Ascent+redb (Rust BFS) | 1.3 ms | <0.1 ms | ~0.2 ms |
| LadybugDB (Cypher shortest) | 62 ms | ~2–3 ms (≥~1 ms floor) | ~8 ms |
| CozoDB mem (recursive) | 125 ms | ~4.5 ms | ~16 ms |
| CozoDB RocksDB (recursive) | 179 ms | ~6.5 ms | ~23 ms |
| CozoDB SQLite (recursive) | 298 ms | ~11 ms | ~38 ms |

Under that assumption, **every candidate except CozoDB-SQLite meets <10 ms on the worst real hub**,
and all of them are sub-millisecond on the real median. The engine choice then turns on the other
criteria (incremental == full correctness, reads while the watcher writes, write latency,
maintenance, features) rather than hub traversal speed. Linear scaling is an assumption: fixed
per-query overhead and different density can move these numbers. Hence the recommendation to
rerun on the real graphs.

## Extractor caveats

Accurate enough for distribution shape, not for per-edge truth:

- **Name-based resolution, no types.** Calls on receivers of unknown type are guessed by method
  name (`inferred`). Strict mode bounds this, but noise remains: e.g. sensemesh `Expect.IsZero`
  (fan-in 108) probably absorbs `time.Time.IsZero` calls, and Continuum `tabclean list` (95)
  absorbs the `list()` builtin within its unit. Both inflate their hub's blast radius.
- **Missed edges.** Dynamic dispatch, callbacks, decorators, dependency injection, Go interface
  satisfaction (structural, not extracted), framework routes, Rust trait-method calls through
  generics, macros (skipped), and re-exports through `__init__.py` / `index.ts` barrels. These
  make real blast radius *larger* than measured. The permissive run is a partial upper bound.
- **Import resolution** is heuristic: Python by module path suffix, TS relative paths plus `@/`
  alias, Go via `go.mod` module prefix, Rust `crate::`/`super::`/`self::` paths only.
- **Implements** resolves only in-repo bases: 181/371 in Continuum, 30/154 in sensemesh; the
  rest are external (`BaseModel`, `Exception`, React types).
- **Scope**: generated files, `node_modules`, `vendor`, `target`, `dist`, hidden directories and
  files over 1 MB are skipped. Tests are included.
- **Parse errors**: 4 files (Continuum) and 11 (sensemesh) parsed with error nodes; their
  symbols are still extracted.

Net effect: true worst-case blast radius probably sits between the strict (1.8K, 8–15%) and
permissive (3.7K–6.4K, 27–30%) numbers. Both are far from the synthetic 99.8%.
