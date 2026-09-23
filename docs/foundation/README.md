# Foundation Docs

Graphite: embedded code-graph engine in Rust. Makes AI code assistants finish tasks faster, act with more confidence, and make fewer mistakes.

## Documents

| Doc | What it covers |
|-----|---------------|
| [vision.md](vision.md) | What Graphite is, optimization targets (speed > assertiveness > correctness), pain points, thesis, scope |
| [positioning.md](positioning.md) | Core phrase, pitches by persona, voice, differentiators, landscape comparison |
| [architecture.md](architecture.md) | CozoDB choice rationale, components, Datalog queries, sync engine (background watcher + CLI probe fallback), module map, tech stack, design principles |
| [runner-integration.md](runner-integration.md) | Two-layer integration with Continuum runner: upfront prompt injection + mid-session MCP access. Integration points per workflow mode. |
| [frontend.md](frontend.md) | Local web UI features (10), priority phases mapped to optimization targets. Agent observatory, savings dashboard, blast radius review, curation/overrides. |

## Research

| Doc | What it covers |
|-----|---------------|
| [../references/landscape.md](../../references/landscape.md) | 16 competing projects analyzed. Deep source-code comparison of Graft, CodeGraph, Graphify. What to steal, what to avoid, gaps in the space. |

Cloned repos for reference live in `references/repos/` (gitignored).

## Reading Order

For a new session picking up this project: vision → architecture → positioning → landscape → runner-integration → frontend.
