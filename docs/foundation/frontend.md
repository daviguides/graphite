# Graphite — Frontend

> The graph visualizer is the canvas, not the feature.

Local web UI served by `graphite ui`. Static bundle embedded in the same Rust binary as CLI/MCP (rust-embed + axum), reading the same CozoDB instance. No second process — this follows from the single-binary architecture, not from optimizing for setup.

## Thesis

The frontend serves the same three optimization targets as the engine:

- **Speed** — Savings dashboard proves ROI (tokens, cost, time saved). Context workbench eliminates prompt assembly time. Agent finishes faster because the human prepared better context.
- **Assertiveness** — Agent observatory shows where agents hesitate, backtrack, or query excessively. Touch map reveals blind edits vs informed edits. The human sees what the agent saw.
- **Correctness** — Blast radius review catches missed impact before merge. Graph health shows where extractors fail (and agents will be blind). Override curation fixes the graph for all future sessions.

Nobody opens a "graph viewer" daily. People open tools that answer questions they have every day: "what will this change break?", "is the agent using good context?", "where should I refactor next?". The node-and-edge view is the background; blast radius highlights, agent traces, coupling heatmaps are the foreground.

## Features

### 1. Agent Observatory

Live feed of every MCP tool call agents make against the graph.

**Views:**
- **Tool call stream** — timestamp, agent session ID, tool name, input params, response size (rows + tokens), latency. Color-coded by agent type (Claude Code, Cursor, runner mode).
- **Touch map** — graph with heatmap overlay of "what the agent looked at this session." Combined with `git diff` reveals:
  - **Blind edits**: files changed but never queried (agent edited without context)
  - **Ignored impact**: files in blast radius but not changed (potential missed regression)
- **Tool call replay** — click any past call to see exact input and output. Debug bad agent decisions by tracing them to the context received.
- **Session timeline** — horizontal swimlanes showing sessions over time. Reveals patterns: "this agent called `symbol_search` 14 times in 2 minutes — it's lost."
- **Mode overlay** (runner) — color tool calls by runner mode. If IMPLEMENTING sessions do RESEARCHING-style wide sweeps, that's a prompt-composition bug — visible immediately.

**Storage:** append-only `trace` relation in CozoDB:
```
trace(session, ts, tool, args, result_tokens, source_tokens, latency_ms, symbols: [String])
```

### 2. Savings Dashboard

Token, cost, and speed savings from using Graphite vs raw file reads.

**Metrics (measured, not estimated):**
- **Token savings** — per tool call: `source_tokens` (sum of original file sizes) minus `result_tokens` (compact graph response). Accumulated per session, day, week, lifetime.
- **Cost savings** — token savings × model input pricing. Configurable per model.
- **Speed savings** — estimated tool calls eliminated × average tool call latency (~2-5s each). Based on files in result that agent didn't need to Read.
- **Compression ratio** — "Graphite served 2.1K tokens of context replacing 28K tokens of file reads (13x compression)."

**Views:**
- **Counter** — lifetime / today / this session savings (tokens, USD, seconds)
- **Per-session breakdown** — "this IMPLEMENTING session saved 34K tokens ($0.12) and ~45s of tool calls"
- **Trend chart** — savings over time. Shows compounding value as graph improves (more overrides, better extraction)
- **Per-mode comparison** (runner) — tokens via Graphite vs estimated tokens without, per workflow mode

**Data source:** every MCP tool call already produces `result_tokens` and `source_tokens` naturally. Zero additional cost.

### 3. Blast Radius Review

Input is a diff, not a symbol. Interactive change-impact analysis.

**Interactions:**
- Select diff source: working tree, commit, PR range, branch comparison
- For every changed symbol: transitive dependents grouped by file/module, with depth
- Toggle depth (1 / 2 / ∞). Depth-1 = "definitely affected", deeper = "review candidates"
- **Multi-select mode** — select several symbols changing together (refactor). Combined blast radius with overlap highlighted.
- **Risk heatmap** — color files by how many blast radii they appear in. File in every radius = fragile coupling.
- **Test coverage** — which test files exercise affected symbols, which affected symbols have no test. Actionable, not available from linters.
- **Mark reviewed** — checklist for human reviewing agent output
- **"Generate agent prompt"** — export dependency chain as context-optimized prompt for runner/Claude Code
- **Export as VALIDATING checklist** — hand to runner as validation brief

### 4. Graph Health / Extraction Confidence

The graph is only as good as the extractor. Surface where it's incomplete.

**Metrics:**
- **Unresolved references** per file — calls whose target couldn't be resolved (dynamic dispatch, reflection, macros, generated code). Ranked by frequency.
- **Orphan symbols** — defined but never referenced, not exported, not entrypoint. Dead code or extraction miss.
- **Parse failures / partial trees** per file after sync.
- **Sync staleness** — files with mtime newer than last sync; git HEAD vs graph HEAD.
- **Per-language coverage** — % of symbols with at least one resolved edge.

**Interaction:** click unresolved reference → opens Override editor (§5).

**Why daily:** trust calibration. If `auth/` is 40% unresolved because of a plugin registry, you know to tell the agent to read files there directly.

### 5. Curation: Overrides and Annotations

First-class `override` relation, editable in UI, persisted as committable `.graphite/overrides.toml`.

**Operations:**
- **Add edge** — "`Router.dispatch` calls every `*Handler.handle`" (dynamic dispatch, DI, event buses). One click from unresolved-reference row.
- **Suppress edge** — "ignore `log::*` / `tracing::*` as dependencies." They inflate every blast radius to the whole repo.
- **Boundary tags** — mark modules `public-api`, `internal`, `deprecated`, `generated`, `do-not-touch`. Tags flow into MCP results (`blast_radius` marks "crosses public-api boundary") so agents get architectural intent, not just structure.
- **Symbol notes** — short human annotations ("hot path; benchmark before changing") returned by `symbol_search` / `file_overview`. Solves cold-start for intent, not just structure.

**Why daily:** every override makes every future agent session smarter. Compounding value.

### 6. Architecture Views (derived, not drawn)

Specific questions rendered as purpose-built views. Each is one Datalog query plus a rendering.

**Views:**
- **Module matrix (DSM)** — modules × modules, cell = edge count. Cycles visible as symmetric off-diagonal blocks. More legible than force-directed for >200 nodes.
- **Coupling matrix** — highlights unexpected couplings ("why does `billing` depend on `notifications`?")
- **Circular dependency detector** — SCCs with size > 1, ranked. Click → minimal cycle path. "Break here" suggestion (edge with lowest fan-out).
- **Fan-in/fan-out scatter** — high-in + high-out = god object; high-in + low-out = stable core. Quadrant tells you (and agent) how careful to be.
- **Layer inference** — topological depth from entrypoints. Actual layering vs what directory names claim.
- **Module boundary violations** — user-defined boundaries ("nothing in `core/` should import from `api/`"). Violations shown as red edges. Architectural lint powered by graph.
- **Change velocity overlay** — git history + graph. Color nodes by change frequency. High-change + high-fan-out = riskiest code.
- **Snapshot diff** (commit A vs B) — edges added/removed, new cycles, fan-in changes. "What did this week of agent PRs do to the architecture." Stored per commit hash; CozoDB makes this cheap.

### 7. Context Workbench

Manual context assembly. The step between "I have a task" and "I paste it into Claude."

**Interactions:**
- **Symbol picker** — search and select symbols. Preview signature and doc comment.
- **Auto-expand** — pull first-order dependencies, click again for second-order.
- **Token counter** — real-time token count of assembled context. "23 symbols, ~4,200 tokens (8% of context window)."
- **"Copy as context"** — serializes selection exactly as MCP tool would. See what the agent sees, or paste into prompt.
- **Saved context sets** — named collections ("auth-module-core", "billing-integration-points"). Reusable knowledge packages.
- **Diff-aware mode** — paste git diff or branch name, auto-select touched symbols + blast radius.
- **Query REPL** — editable Datalog. Visual is rendering of current result set. Debugging tool for queries.

### 8. Runner Control Center

Visual interface for runner workflow orchestration.

**Features:**
- **Context pack preview** — before runner launches a mode, show the Graphite context that will be injected with token estimate. Human trims/pins/excludes before spend.
- **Task scoping** — select symbols/files in workbench → "create runner task with this scope." Scope becomes boundary; tool calls outside it appear as violations in Agent Trace.
- **Live mode tracker** — which mode runner is in, current artifacts, progress.
- **Session comparison** — side-by-side of two sessions. Compare: files touched, graph context used, tool calls, cost, outcome. Learn which context strategies work.
- **VALIDATING checklist** — generated from Blast Radius Review. Affected symbols, covering tests, uncovered symbols. Human ticks items off in UI.
- **Per-mode cost report** — tokens via Graphite vs estimated without, per mode and session.

### 9. Smart Search

Structural queries, not text search. Query builder over Datalog.

**Example queries:**
- "Functions that call `authenticate` AND are called from `api/` handlers"
- "All public types in `core/` not re-exported from `lib.rs`"
- "Symbols with fan-out > 10"
- "Shortest path from `UserService` to `DatabasePool`"
- "Show query" toggle for raw Datalog

### 10. Minor but Sticky

- **Pin board** — watched symbols/files. On open, shows fan-in deltas and sync staleness.
- **Command palette** (Cmd-K) — symbol search as launcher. Fastest path in.
- **Deep links** (`graphite://symbol/...`) — from CLI output and MCP responses, so agent answers can include "open in UI."

## What NOT to Build

- Whole-repo force-directed graph as landing page. Past 500 nodes it's a hairball.
- Source editing. Not an IDE. Read-only peek only.
- Semantic or embedding search. Out of scope by design.
- Cloud sync or share links. Privacy absolute.

## Priority

| Phase | Features | Target | Rationale |
|-------|----------|--------|-----------|
| **P1** | Agent Observatory + Blast Radius Review + Savings Dashboard | Speed + Correctness | Observatory shows where agents waste turns (speed). Blast radius catches missed impact (correctness). Savings dashboard proves ROI in wall-clock time and tokens. |
| **P2** | Graph Health + Overrides/Curation | Assertiveness + Correctness | Confidence calibration: human sees where graph is blind (assertiveness). Overrides compound — each one makes every future agent session more correct. |
| **P3** | Architecture Views (snapshot diff) | Correctness | Defense against architectural drift from many small agent PRs. Catches coupling creep and cycle introduction. |
| **P4** | Context Workbench + Smart Search | Speed | Eliminates prompt assembly time. Human curates context in seconds, agent starts faster. |
| **P5** | Runner Control Center | Speed | Visual steering of orchestrated workflows. Context pack preview before spend. Depends on runner maturity. |

## Tech Stack

| Component | Technology |
|-----------|------------|
| Embedding | rust-embed (static files compiled into binary) |
| Server | axum (same process as CLI/MCP) |
| Data | CozoDB (same instance, read-only from UI) |
| Frontend | Vanilla JS or Svelte (small bundle, no framework tax) |
| Graph rendering | d3-force or cytoscape.js (local subgraph only, never whole-repo) |
| Charts | lightweight (savings trends, DSM heatmap, scatter plots) |

## Sources

Analysis synthesized from consultations with Fable 5.1 and Opus 5.5 (2026-09-23). Savings dashboard from project requirements. See [landscape.md](../../references/landscape.md) for full competitor analysis including CodeGraph's UI, Graft's viz, and Graphify's export formats.
