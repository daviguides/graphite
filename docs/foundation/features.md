# Graphite — Feature Inventory

> One deduplicated list of every capability found across the studies, judged only by the three targets: **S** = agent execution speed, **A** = assertiveness, **C** = correctness. Verdict: **v1**, **v2**, **later**. This is the backlog; the storage decision (hybrid) is settled in [architecture.md](architecture.md) and not reopened here.

**Source keys:** GN = [gitnexus.md](../../references/studies/gitnexus.md) · CGM = [code-graph-mcp.md](../../references/studies/code-graph-mcp.md) · CT = [cozo-based-tools.md](../../references/studies/cozo-based-tools.md) (fg = ferrograph, ig = infigraph, lk = LeanKG) · SB = [storage-benchmark.md](../../references/studies/storage-benchmark.md) · RG = [real-graph-shape.md](../../references/studies/real-graph-shape.md) · LS = [landscape.md](../../references/landscape.md) (Graft / CodeGraph / Graphify) · FE = [frontend.md](frontend.md) · LY = [laya-integration.md](laya-integration.md) · RI = [runner-integration.md](runner-integration.md)

Verdicts are revisable: a v2 item moves up when measurement shows it cuts agent turns, and a v1 item moves down when it doesn't.

---

## 1. Extraction & resolution

| Feature | What | Target | Verdict | Sources | Why |
|---|---|---|---|---|---|
| Languages Rust / TS-JS / Python / Go | Tree-sitter grammars compiled in | S C | **v1** | vision, RG | Covers Continuum and sensemesh, the two real test repos. |
| `.scm` query files with unified capture tags | One query set per language, capture names shared (`@definition.function`, `@call.name`…); downstream code never branches on language | C | **v1** | GN, CGM | CGM's #1 recurring bug is "a missing match arm is a silently absent edge"; data-driven captures + wiring tests prevent it. |
| Single AST pass emitting node facts + reference facts | Definitions, unresolved references with qualifiers, imports — per file | S C | **v1** | CGM | Two walks per file (CGM) cost time; per-file facts are the basis of incremental == full. |
| Deterministic symbol IDs | Hash/readable id from path + qualified name + kind + disambiguator (arity) | C | **v1** | CGM, GN, CT | Rowid/position IDs caused CGM's `node_id_renumbered` class and fg's instability under edits. |
| Import-bound call resolution | A call to `foo` in a file importing `foo` from X binds to X; edges contradicted by imports pruned | C | **v1** | CGM, GN | Difference between 1 true edge and N phantom edges. |
| Callee qualifiers | How each call was bound: `path` / `self` / `stype` / `rtype` / `recv` / `chain` | C | **v1** | CGM | Structural qualifiers protect edges from being downgraded to ambiguous. |
| Ambiguity refinement | Same-name candidates: prefer non-test for non-test caller, then longest common path prefix; keep all if tied | C | **v1** | CGM | Over-report beats silently dropping a real caller. |
| `<external>` sentinels + `<module>` scope node | Unresolved/std imports bind to external nodes; top-level statements attach to a per-file module node | C | **v1** | CGM | Without them, `read`/`spawn`/`swap` produce phantom edges and top-level-only files look dead. |
| Noise filters as data tables | Cross-file call noise (`get`, `run`, `new`…), per-language type-ref noise | C | **v1** | CGM | Cheap precision; RG saw noise inflate hubs (`Expect.IsZero`). |
| Test detection | AST flags (`#[test]`, `describe/it`…) + path/name heuristics | S C | **v1** | CGM, GN | Required for prod/test partition and targeted tests. |
| Unresolved references recorded, not dropped | Store unresolved receivers/dispatch with cause | A C | **v1** | GN, CGM | Feeds `epistemic` / `causes` (§5). |
| Parse-parallel, write-serial pipeline | Rayon parse, single writer thread | S | **v1** | CGM, SB | Target ≥1,000 files/s cold; writes stay atomic per file. |
| Per-file parse timeout + recorded parse errors | 5 s cap; failures surfaced in health | C | **v1** | CGM | Feeds Graph Health; no silent holes. |
| Framework routes (Axum, FastAPI, Express, net/http…) | Route → handler edges, synthetic route nodes | C | **v2** | CGM, GN, LS | High value for web backends, not core for first speed win. Precision over recall ("an invented route is a lie", GN). |
| Dynamic dispatch synthesis (callbacks, EventEmitter, React chains, interface fan-out, DI) | Synthetic INFERRED edges for indirect flows | C | **v2** | LS (CodeGraph), GN | Largest correctness gain after v1, but heuristic and costly; v1 compensates with honest `lower-bound` reporting. |
| Python MRO, receiver-chain typing | Deeper resolution | C | **v2** | GN | Years of edge cases in GN; ship after the core is measured. |
| LSP / SCIP compiler-grade edges | Optional precise edges from language servers | C | **v2** | CT (ig), LS (Graft `--lsp`) | Strongest precision upgrade available; opt-in layer on top of Tree-sitter. |
| Doc comments / docstrings | Attach to symbols | A | **v2** | CGM | Useful for summaries, not traversal. |
| Rust ownership/borrow edges, macro expansion | `owns` / `borrows` / edges through macros | C | **later** | CT (fg) | Rust-specific; valuable for Rust-heavy repos. |
| Non-code ingestion (SQL schemas, configs, docs) | Same graph as code | C | **later** | LS (Graphify) | Measure whether it cuts turns before building. |

## 2. Graph model & edges

| Feature | What | Target | Verdict | Sources | Why |
|---|---|---|---|---|---|
| Edge kinds v1 | `contains`, `calls`, `imports`, `extends`/`inherits`, `implements`, `has_member`, `overrides`, `references` | C | **v1** | GN, CGM | Union of what both mature tools need for blast radius. |
| Edge kinds v2 | `exports`, `routes_to`/`handles_route`, `accesses` r/w, `injects`, `fetches` | C | **v2** | GN, CGM | Tied to framework/dispatch work. |
| Three-tier confidence + categorical provenance | Tier: EXTRACTED / INFERRED / AMBIGUOUS; provenance: `extracted`, `resolved`, `inferred`, `name_guess` | A C | **v1** | CGM, GN, LS (Graphify) | GN proved a number alone can't exclude name guesses (0.5 = threshold). Default traversal floor = INFERRED; ambiguous counted and disclosed. |
| Confidence derived by rule, never stored | Evaluated at query time from facts | C | **v1** | CGM, SB | Stored confidence drifted in CGM; SB measured the rule at +3–5% cost. |
| Git co-change edges (`changes_with`) | Mined from git history; participate in blast radius | C | **v2** | CT (fg), LS gap, LY | CT marked v1; kept v2 because it needs history mining and its main consumer (ranker signal) is phase 2. Moves up if blast radius misses co-changed files in measurement. |
| Communities (Louvain → Leiden) | Subsystem clusters over calls/extends/implements, name guesses excluded, singletons dropped, folder labels, deterministic seed | A | **v2** | GN, LS (Graphify), SB | Assertiveness for EXPLORING, not needed for the first speed win. CozoDB Louvain cost 1–4.7 s / GBs (SB) → background Rust crate per architecture. |
| PageRank / hot symbols, SCC / cycles | Centrality and cycle detection | A C | **v2** | GN, CGM, SB | Cheap in Rust (SB: ~50 lines, 0.1–4.6 ms). Serves PLANNING and architecture views. |
| Execution flows (processes) | Scored entry points → bounded DFS → flows, truncation published | A S | **v2** | GN | High EXPLORING value, heuristic and costly. Schema slot reserved. |
| Test coverage relation (`covers`) | Test symbol → symbols it reaches | S C | **v1** | GN, CGM, CT (ig), LS gap | Enables targeted tests for runner VALIDATING and covering tests in pre-edit hook. |
| Betweenness centrality, "surprising" edges, reading-order tour | Analytics | A | **later** | CGM | Frontend architecture views. |

## 3. Freshness & sync

| Feature | What | Target | Verdict | Sources | Why |
|---|---|---|---|---|---|
| One daemon per repo | Owns CozoDB, watcher, in-memory adjacency; CLI and MCP are thin clients over a unix socket | S C | **v1** | CGM, CT (lk) | Every Claude session / runner task spawns its own MCP process; one daemon = one hot graph for N clients, no writer election. |
| Eager indexing on the watcher thread | File change → hash → parse → atomic per-file fact replace → adjacency update | S | **v1** | CGM, GN, SB | CGM indexes on the next query; GN re-analyze takes 31.7 s. Eager keeps the query path free. |
| Freshness barrier | A query waits ≤~200 ms for pending indexing up to its arrival seqno, else answers with `stale: true` | S C | **v1** | CGM | Never blocks, never silently stale. |
| `graph_rev` watermark in DB, returned in every response | Monotonic revision of the fact base | A C | **v1** | CT (lk) | Kills cache-race bugs; agent and Observatory know what state answered. |
| Change ladder: mtime+size stamp → BLAKE3 | Skip unchanged bytes (touch, save-without-edit) | S | **v1** | CGM, LS (Graft) | Watcher overhead stays minimal. |
| Watcher safety nets | Unknown events = content change; periodic backstop rescan | C | **v1** | CGM | Watchers fail silently (inotify limits, network FS). |
| Post-edit nudge hook | PostToolUse Write/Edit tells the daemon "file X changed now" | S C | **v1** | CGM | Closes the gap between the agent's edit and its next query at ~0 cost with a resident daemon. |
| Crash marker + rebuild in one transaction | `index_run_in_flight`; readers see old complete graph until commit | C | **v1** | CGM | Cheap crash consistency. |
| Stale-element sweep | Remove facts of files that left the tracked set | C | **v1** | CT (lk) | Deleted files must disappear. |
| Incremental == full rebuild, tested | Fixture tests comparing incremental result with rebuild | C | **v1** | CGM, SB | CGM bumped its index format 71 times over this bug class. |
| Schema fingerprint → rebuild on mismatch | Hash of the schema definition | C | **v1** | GN | Safe upgrades. |
| CLI probe fallback | When no daemon runs, probe mtimes and sync before answering | S | **v1** | architecture | For CI and one-off use. |
| Worktree seeding | Seed a new worktree's graph from the main checkout's graph, then incremental-diff | S | **v2** | CGM | Runner creates a fresh worktree per task; cold start matters there. |

## 4. Query & traversal

| Feature | What | Target | Verdict | Sources | Why |
|---|---|---|---|---|---|
| Blast radius (upstream / downstream) | Transitive dependents with shallowest depth per node; default depth 3, max 10 | S C | **v1** | all | Core. RG: depth 3 already reaches 63–76% of the depth-10 set on real hubs. SB: 19–80 µs in-memory. |
| Depth labels | d1 WILL BREAK / d2 LIKELY AFFECTED / d3+ MAY NEED TESTING | A | **v1** | GN | Tells the agent what to do, not just what exists. |
| Class-target seeding | Impact on a type seeds constructors, owning file, typed properties | C | **v1** | GN | Without it, class impact finds nothing in some languages. |
| `context` / symbol view | One symbol: signature, source, callers/callees, references by category, tests | S A | **v1** | GN, CGM (`get_ast_node`), CT (fg `node_info`) | Replaces 3–6 Read/Grep calls; the "before editing X" payload. |
| `diff_impact` | Git diff hunks → changed symbols (range overlap) → blast radius → covering tests, in one call | S C | **v1** | GN (`detect_changes`), CT (ig), CGM (`affected`), RI | Merged: `detect_changes` == `affected` == `semantic_diff` == `diff_impact`. Graphite's graph already matches the working tree. |
| Find all references | Every import/inherit/implement/call/reference site, grouped by file | C | **v1** | CGM, CT (ig) | Rename/remove safety; call graph alone misses it. |
| Symbol search (lexical) | Exact → fuzzy (FTS/BM25) with disambiguation | S | **v1** | CGM, CT (lk), GN | Replaces grep loops when the agent has a name. |
| Search ladder with provenance | exact → fuzzy → (semantic) with `retrieval{rung, reason}` on each answer | A | **v1** (exact + fuzzy) | CT (lk) | Agent knows how the answer was found. |
| Path between two symbols (`trace`) | Shortest directed path with file:line per hop | S | **v1** | GN, CGM | Answers in one call what takes 3–8 hops. |
| Repo / module overview | Modules, deps, entry points, hot symbols, token-budgeted | S A | **v1** | CGM (`project_map`), LS (Graft `repo_map`), RI | The runner EXPLORING payload; skips 10–15 exploratory reads. |
| Skeleton with fan-in / complexity | Signatures of a file annotated with fan-in and complexity | S A | **v1** | CT (ig), LS (Graft `file_api`) | API surface at ~1/10 tokens; fan-in says how careful to be. |
| AST-context grep | Regex hits grouped by enclosing symbol, ranked by coupling | S | **v1** | CGM, LS (Graft) | It is what the pre-grep hook answers with. |
| Batch lookups | N symbols in one query (inline relation) | S | **v1** | CT (ig) | Fewer round trips for multi-symbol questions. |
| Semantic / hybrid search (BM25 + vector, RRF) | Concept search when no exact name | S A | **v2** (measure) | CGM, GN, CT (ig) | GN skipped it only because of the old "no embeddings" line, now removed. Ship if a recall bench on real agent queries shows fewer turns; embed on the watcher thread, never at query time. CozoDB HNSW before adding a store. |
| Typed enumeration (`ast_search`) | "All fns returning Result<T>" | S | **v2** | CGM | Cheap, low call frequency. |
| Import cycles check | SCC lint on imports | C | **v2** | GN, CGM | PLANNING input. |
| Dead-code candidates | Unreachable from entry points, framed as "verify" | C | **v2** | CGM, CT (fg) | Not on the speed path. |
| Raw Datalog query | `graphite query <datalog>` | — | **v2** (CLI/UI only) | GN, CT (fg) | LLMs don't know Datalog, and a raw-query tool's description taxes every session. |
| Route map / API impact | Route → handler → consumer | C | **v2** | GN, CGM | With framework routes. |
| Cross-repo groups | Contracts across repos | C | **later** | GN, CT (ig), LS (Graphify) | Microservices; after single-repo core. |

## 5. Response contract

| Feature | What | Target | Verdict | Sources | Why |
|---|---|---|---|---|---|
| Source inline | Verbatim line-numbered source per symbol, capped (~4 KB); read from disk by byte range + body hash, not duplicated in the DB | S | **v1** | CGM, GN, LS (CodeGraph) | Eliminates the follow-up Read. Disagreement: CGM stores code in DB, GN recommends byte ranges — byte ranges chosen because the watcher keeps files current and the DB stays small. |
| `compact` flag | Signature + location only | S | **v1** | CGM | Every tool. |
| Tiered auto-compression | Over budget → node summaries → grouped by file → by directory, with ids to expand | S | **v1** | CGM, CT (ig), LY | Names always present, detail tiered; bounded payloads. |
| Rank-based budget | Drop lowest-ranked items to fit, never cut bytes | S A | **v1** | GN | Byte truncation cuts structures mid-way. |
| Deterministic ranking of results | Order by confidence, depth, fan-in (co-change when available) | S A | **v1** | LY (deterministic baseline), SB | Agent reads the most relevant first; this is Laya phase 2 and the baseline any ML ranker must beat. |
| Disclosure fields | `limit_hit`, `depth_capped`, `*_hidden`, `*_truncated`, `confidence_filtered`, `partial` | A C | **v1** | CGM, GN, CT (ig) | Three independent tools converged on it: never a silent partial answer. |
| `epistemic` + `causes` | Result says `exact` or `lower-bound`, with counts of why (unresolved receivers, dispatch boundary, external) | A C | **v1** (3 causes) | GN | Honest uncertainty compensates v1's shallower resolution. |
| Risk verdict | LOW / MEDIUM / HIGH / CRITICAL from direct/total counts, modules; **UNKNOWN** (never LOW) on zero callers | A C | **v1** | GN, CGM | One word the agent can act on; UNKNOWN stops deleting dynamically-reached code. |
| Prod / test caller partition + covering tests | Tests split out, runnable test names listed | S C | **v1** | CGM, GN | Source of targeted tests. |
| Disambiguation in the envelope | Several matches → ranked candidates with `uid`, retry by uid/path/kind; no match → suggestion + fuzzy candidates, not an error | A S | **v1** | GN, CGM | Accept a name, disambiguate in the response — avoids the two-step lookup turn (ig). |
| Not-indexed / not-found as guidance | Success-shaped response naming the next action | S | **v1** | LS (CodeGraph), CGM | `isError` teaches agents to abandon the toolset. |
| `stale` + `graph_rev` + `graph_lag_ms` | Freshness state in every response | A | **v1** | CT (lk), GN (replaced), CGM | See §3. |
| Bytes/3 token estimator | Conservative, one estimator | S | **v1** | CGM | Predictable budgets. |

## 6. Agent surface & steering

| Feature | What | Target | Verdict | Sources | Why |
|---|---|---|---|---|---|
| CLI as primary agent surface | `graphite <cmd> --json` via Bash | S | **v1** | CGM | In Claude Code MCP tools are deferred (ToolSearch load first) while Bash is live; measured conversions came through the CLI. |
| MCP server (secondary) | Same handlers as CLI, thin client to the daemon | S | **v1** | architecture, CGM | For hosts that don't defer tools, and for runner's SDK sessions. |
| Few listed tools, capability via flags | ~3–5 listed tools; management tools unlisted | S | **v1** | CGM (7 listed), CT (lk 3 tools), LS (CodeGraph 1 tool) | Agents under-pick extra tools; ig's ~90 tools and GN's 17 tax every session. Disagreement on count (1 vs 3 vs 7): start with few, let the routing bench decide. |
| Tool schema limits | No `anyOf` (client silently drops the tool); descriptions ≤200 chars, positive phrasing; instructions ≤1.5 KB | S | **v1** | CGM | Measured client behavior. |
| Pre-grep hook: block and answer | Deny raw grep for identifier patterns, return the graph's answer in the deny reason | S | **v1** | CGM, GN | Hint-only steering measured ~0% uptake; deny-with-answer converts. GN's augmentation variant served over the socket in ms. |
| Pre-edit hook: impact + covering tests | When an edit touches a signature with ≥2 prod callers, inject impact summary and runnable covering tests | C | **v1** | CGM | Blast radius at the moment of the edit, zero agent turns. |
| Hook fail-open, registered in settings.json | Hook errors never break the tool call; plugin `hooks.json` only honors SessionStart | C | **v1** | CGM | Measured constraint. |
| Skill file teaching the tool | Ships with the binary | S | **v1** | CT (fg) | Cheap onboarding for the agent. |
| Compound-grep inject, read fan-out hint, per-prompt context push | Post-hoc steering variants | S | **v2** | CGM | Runner prompt composition covers most of this. |
| Per-community generated skills, AGENTS.md rules | Area knowledge and "impact before edit" rules | A | **v2** | GN | After communities. |

## 7. Runner integration

| Feature | What | Target | Verdict | Sources | Why |
|---|---|---|---|---|---|
| Runner talks to the daemon socket | Bridge queries the daemon directly (CLI fallback) | S | **v1** | RI | No per-query process spawn. |
| Prompt injection per mode | EXPLORING overview, IMPLEMENTING diff_impact, VALIDATING targeted tests | S A C | **v1** | RI | Upfront context, zero turns. |
| Targeted tests first in VALIDATING | Run covering tests before full suite | S | **v1** | RI, CGM (`affected`) | Seconds of feedback instead of minutes; full suite still runs. |
| Mid-session access in SDK sessions | Agent queries Graphite during a mode (CLI via Bash or MCP) | S A | **v1** | RI | Layer 2 of the integration. |
| RESEARCHING / PLANNING context (communities, coupling, cycles) | Mode-specific payloads | A | **v2** | RI | Depend on §2 v2 features. |
| Context pack preview / task scoping | Human trims context before spend; scope boundary violations in trace | S | **later** | FE | Needs frontend. |

## 8. Ranking / Laya

| Feature | What | Target | Verdict | Sources | Why |
|---|---|---|---|---|---|
| Deterministic ranker | confidence × depth × fan-in (+ co-change, test adjacency when available) | S A | **v1** | LY | Captures most of the value with no model; see §5. |
| Instrumentation for triage | Trace fields: candidates, must/verify/skip counts, engine, model sha | S | **v1** (fields) | LY | Without them no later ranker can be evaluated. |
| Git-history label mining | Training data from past commits | A C | **v2** | LY | Needed before any model. |
| Laya sidecar, shadow mode then ranked mode | Encoder ranks residual INFERRED items; ranker never filters; VALIDATING never ranked | S A C | **later** | LY | Activate only if it beats the deterministic baseline on recall@token-budget. |

## 9. Frontend

| Feature | What | Target | Verdict | Sources | Why |
|---|---|---|---|---|---|
| Agent Observatory, Blast Radius Review, Savings Dashboard | FE P1 | S C | **v2** | FE | Core engine first; the data behind them (trace, conversion) is v1 (§10). |
| Graph Health, Overrides / curation | FE P2 | A C | **v2** | FE, CGM | Overrides compound; needs health data from v1 extraction. |
| Architecture views (DSM, snapshot diff) | FE P3 | C | **later** | FE, GN | After communities/cycles. |
| Context Workbench, Smart Search, Runner Control Center | FE P4–P5 | S | **later** | FE | Depend on runner maturity. |

## 10. Measurement & observability

| Feature | What | Target | Verdict | Sources | Why |
|---|---|---|---|---|---|
| Trace relation | Every tool call: session, tool, args, result/source tokens, latency, symbols, graph_rev | S | **v1** | FE, CGM, LY | Feeds savings, observatory and ranker evaluation. |
| Conversion metric from transcripts | Did the agent Read/Edit what Graphite returned? | A | **v1** | CGM | The "was the context used" signal. |
| Routing bench (release gate) | Does the agent pick the right tool from live schemas? | A | **v1** | CGM | Tool descriptions are code; regress-test them. |
| Effectiveness bench (turns, wall-clock) | With vs without Graphite on real tasks | S | **v1** | CGM, LS (Graft SWE-bench) | The only proof of the primary target. |
| Storage bench kept runnable | `bench/` on real graphs | S | **v1** | SB | Guards the hot path against regressions. |
| Health check | Parse errors, unresolved ratio, staleness | C | **v1** | CGM | Trust calibration. |
| Recommendation funnel | Hook deny/hint/bypass joined to later use | S | **v2** | CGM | Tuning steering. |

---

## Counts

v1: 75 · v2: 24 · later: 8 (107 features after deduplication)

## v1 candidate cut — proposal for the user to confirm

The minimal set that already makes the agent finish faster. Not a decision.

1. **Extractor** for Rust / TS / Python / Go: `.scm` captures, single pass, deterministic IDs, import-bound calls, qualifiers, sentinels, test detection, unresolved refs recorded.
2. **Daemon per repo**: CozoDB (mnestic, RocksDB) facts + in-memory adjacency, eager watcher, BLAKE3 ladder, freshness barrier, `graph_rev`, post-edit nudge, incremental == full test.
3. **Four queries**: `diff_impact`, `context`, blast radius (depth-labelled, default depth 3), symbol search (exact + fuzzy). Plus AST-context grep for the hook.
4. **Response contract**: source inline, compact + tiered compression, deterministic ranking, disclosure fields, `epistemic`/`causes`, risk incl. UNKNOWN, prod/test partition with covering tests, disambiguation in the envelope.
5. **Agent surface**: CLI `--json` first, MCP secondary with the same handlers, ≤5 listed tools, pre-grep block-and-answer hook, pre-edit impact hook, skill file.
6. **Runner**: socket bridge, IMPLEMENTING `diff_impact` injection, VALIDATING targeted tests.
7. **Measurement**: trace relation, conversion metric, effectiveness bench (turns and wall-clock with vs without).

Everything else in v1 above (search ladder, trace/path, references, overview, skeleton, batch lookups, safety nets) is the next slice once this cut is measured.
