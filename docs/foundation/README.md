# Foundation Docs

Graphite: embedded code-graph engine in Rust. Makes AI code assistants finish tasks faster, act with more confidence, and make fewer mistakes.

**Only absolutes:** agent execution speed, assertiveness, correctness (defined in [vision.md](vision.md)). Everything else in these docs is a derived, revisable decision. Do not add "no X / never X / out of scope by design" rules unless derived from a target with the reasoning written down.

## Documents

| Doc | What it covers |
|-----|---------------|
| [vision.md](vision.md) | What Graphite is, optimization targets (speed > assertiveness > correctness), pain points, thesis, v1 summary |
| [features.md](features.md) | Deduplicated feature inventory (109 features) placed on a versioned roadmap v1–v8 with goals, dependencies, exit criteria; parking lot with revisit triggers. v1 is a proposal pending confirmation. |
| [architecture.md](architecture.md) | Hybrid storage decision (CozoDB/mnestic facts + in-memory traversal, benchmarked), one daemon per repo with thin clients (CLI, MCP shim, hooks, runner), freshness barrier, sync engine, module map, tech stack, design principles |
| [positioning.md](positioning.md) | Core phrase, pitches by persona, voice, differentiators, landscape comparison |
| [runner-integration.md](runner-integration.md) | Continuum runner integration: daemon socket bridge, upfront prompt injection, mid-session CLI-first access, steering hooks, integration points per workflow mode |
| [laya-integration.md](laya-integration.md) | Laya encoder as optional ranker (never filter) for blast radius, INFERRED edges and tests; deterministic baseline first |
| [frontend.md](frontend.md) | Local web UI features, priority phases mapped to optimization targets |

## Research

| Doc | What it covers |
|-----|---------------|
| [landscape.md](../../references/landscape.md) | 19 competing projects. Deep comparison of Graft, CodeGraph, Graphify. What to steal, what to avoid, gaps in the space. |
| [studies/gitnexus.md](../../references/studies/gitnexus.md) | GitNexus source study: feature inventory, response-shape lessons, proposed CozoDB schema |
| [studies/code-graph-mcp.md](../../references/studies/code-graph-mcp.md) | code-graph-mcp (Rust) source study: feature inventory, measured Claude Code behavior, incremental == full lessons |
| [studies/cozo-based-tools.md](../../references/studies/cozo-based-tools.md) | infigraph, LeanKG, ferrograph: CozoDB in practice |
| [studies/cozodb-health.md](../../references/studies/cozodb-health.md) | CozoDB maintenance state; `mnestic` fork as successor |
| [studies/rust-embedded-graph-alternatives.md](../../references/studies/rust-embedded-graph-alternatives.md) | Rust-embeddable graph storage alternatives |
| [studies/real-graph-shape.md](../../references/studies/real-graph-shape.md) | Real graph shape of Continuum and sensemesh (hub size, depth, density) |
| [studies/storage-benchmark.md](../../references/studies/storage-benchmark.md) | CozoDB vs LadybugDB vs DIY benchmark on real and synthetic graphs |

Cloned repos for reference live in `references/repos/` (gitignored). The benchmark harness lives in `bench/`.

## Reading Order

For a new session picking up this project: vision → features (roadmap) → architecture → positioning → landscape + studies → runner-integration → laya-integration → frontend.
