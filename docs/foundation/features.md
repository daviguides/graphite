# Graphite — Feature Inventory & Roadmap

> One deduplicated list of every capability found across the studies, each placed on exactly one version of the roadmap. Judged only by the three targets: **S** = agent execution speed, **A** = assertiveness, **C** = correctness. The storage decision (hybrid) is settled in [architecture.md](architecture.md) and not reopened here.

**Source keys:** GN = [gitnexus.md](../../references/studies/gitnexus.md) · CGM = [code-graph-mcp.md](../../references/studies/code-graph-mcp.md) · CT = [cozo-based-tools.md](../../references/studies/cozo-based-tools.md) (fg = ferrograph, ig = infigraph, lk = LeanKG) · SB = [storage-benchmark.md](../../references/studies/storage-benchmark.md) · RG = [real-graph-shape.md](../../references/studies/real-graph-shape.md) · LS = [landscape.md](../../references/landscape.md) (Graft / CodeGraph / Graphify) · FE = [frontend.md](frontend.md) · LY = [laya-integration.md](laya-integration.md) · RI = [runner-integration.md](runner-integration.md)

Versions are ordered by value to the targets and by dependency. Placement is revisable: an item moves earlier when measurement shows it cuts agent turns, later when it doesn't. **v1 and its waves are a proposal for the user to confirm.**

Every wave is a vertical slice: usable end-to-end by an agent and measurable on its own, never a single layer.

---

## Roadmap

| Version | Theme | Primary target |
|---|---|---|
| **v1** | Core graph + agent surface — shipped in four waves: | Speed |
| ↳ **v1.0** | Thesis slice: one language, daemon, `diff_impact` + blast radius, CLI, **interception + enriched grep**, with-vs-without bench | Speed |
| ↳ **v1.1** | Full agent surface: all v1 languages, `context` + search, pre-edit hook, MCP shim | Speed, Assertiveness |
| ↳ **v1.2** | Runner integration | Speed |
| ↳ **v1.3** | Response contract completion + operational fallback | Speed, Assertiveness |
| **v2** | Query breadth + hardening | Speed, Correctness |
| **v3** | Resolution depth | Correctness |
| **v4** | Structure & orientation | Assertiveness |
| **v5** | Frontend core (observability + curation) | Assertiveness, Correctness |
| **v6** | Learned ranking (Laya) | Speed, Assertiveness |
| **v7** | Advanced views & orchestration UI | Speed, Assertiveness |
| **v8** | Scope expansion | Correctness |

### v1 — Core graph + agent surface *(proposal)*

- **Goal:** the agent stops exploring. It gets the change's blast radius, the code and the covering tests in one call or before it greps or edits, from a graph that is always current.
- **Shipped as four waves** (v1.0 → v1.3, below). 62 features (the language row is split in two: Python in v1.0, the rest in v1.1).
- **Depends on:** nothing.
- **Exit criteria (v1 as a whole, checked at the end of v1.3):** (1) effectiveness bench on real tasks in Continuum (Python, Rust and TS/Tauri tools) shows fewer turns and less wall-clock with Graphite than without; (2) incremental == full rebuild on Continuum; (3) depth-10 blast radius < 10 ms on real graphs; (4) routing bench above threshold; (5) hooks show measurable conversion (graph answer used instead of grep/Read).

#### v1.0 — Thesis slice *(proposal)*

- **Goal:** prove or refute the thesis with the thinnest usable slice — **does the agent finish real tasks faster with Graphite than without?** Speed first; correctness must not regress.
- **Language: Python** *(confirmed by the user 2026-09-24)*. Language priority for analyzed code: **Python → Rust → TypeScript**.
  - **Python:** 909 of Continuum's 1,146 files, and Continuum is where runner and dao tasks run — the bench uses real tasks on a real repo the user works in daily. It is also the hardest to resolve (dynamic, duck typing), so v1.0 exercises confidence, `lower-bound` and `causes` for real instead of hiding them.
  - If Python's resolution noise makes the bench inconclusive, narrow the task set to Continuum's statically clearer Python modules before changing language.
- **Features (34):**
  - *Extraction:* Python grammar, `.scm` captures, single AST pass, deterministic IDs, import-bound calls, `<external>`/`<module>` sentinels, test detection, unresolved refs recorded.
  - *Graph:* core edge kinds, three-tier confidence + provenance, confidence derived by rule, `covers`.
  - *Daemon:* one daemon per repo (CozoDB mnestic/RocksDB + in-memory adjacency), eager watcher, freshness barrier, `graph_rev`, mtime→BLAKE3 ladder, stale sweep, incremental == full test.
  - *Queries:* `diff_impact`, blast radius (default depth 3).
  - *Response:* source inline, rank-based budget, deterministic ranking (and ranker), disclosure fields, `epistemic` + `causes`, prod/test partition + covering tests, `stale` + `graph_rev`, bytes/3 estimator.
  - *Surface:* CLI (model text format by default, `--json` for programs), skill file (so the agent knows the CLI exists).
  - *Steering (moved in from v1.1 on 2026-09-27 — see [interception.md](interception.md)):* embedded ripgrep search in the daemon, residue judgment, PreToolUse rewrite hook for read-only `grep`/`rg`/`ack`/`find`/`ls`/`cat`/`sed -n`/`head`/`tail`, enriched-grep answer (integrated `path:line:` lines + header verdict + footer), head/tail as budget, filters by intent, hook fail-open in settings.json + `graphite hooks install`, post-edit nudge, `hooks.jsonl` log.
  - *Measurement:* effectiveness bench (turns + wall-clock, with vs without), storage bench kept runnable.
  - **Why the move:** pilot B (CLI + prompt line, no hooks) hit this wave's own "did not use" rethink rule — one task never called Graphite, and every run that did still grepped after a complete answer. Without steering the thesis can't be tested.
- **Depends on:** nothing.
- **Exit criteria — go / no-go on the thesis:**
  - **Bench:** a fixed set of real Python tasks in Continuum (bug fixes and changes that cross files, taken from real history), each run with and without Graphite, same model, several repeats. The deciding arm is **C** (enriched-grep format + interception hooks); A = no Graphite, B = CLI + prompt line with the old JSON output (pilot B: turns ratio 1.05, wall-clock 0.88, 10/10 success both arms, see `references/studies/effectiveness-bench-design.md`).
  - **Go:** with Graphite, median wall-clock and median turns per task are clearly lower (target ≥20%), task success rate is equal or better, and zero silent-stale answers were observed. Plus incremental == full on Continuum and depth-10 blast radius < 10 ms.
  - **Rethink if:**
    - the agent **used** Graphite (Graphite CLI calls in the transcripts) and was **not** faster, or got less correct → the thesis or the answer shape is wrong; revisit what `diff_impact` returns before building more surface.
    - the agent **did not use** Graphite → a surface problem, not a thesis refutation; pull the v1.1 hooks forward and rerun before judging. *(Fired in pilot B; interception pulled into v1.0 on 2026-09-27.)*
    - correctness dropped (tasks failed or regressed that passed without Graphite) → stop and investigate before any further wave.

#### v1.1 — Full agent surface

- **Goal:** the agent gets graph answers everywhere it would otherwise explore — on every language of Continuum (Python, Rust, TS/Tauri), at the moment it greps or edits, and through MCP where the host prefers it.
- **Features:** Rust / TS-JS grammars (in that order), callee qualifiers, parse-parallel/write-serial pipeline; `context`, lexical symbol search; disambiguation in the envelope; MCP shim, ≤5 listed tools, tool schema limits; pre-edit hook (impact + covering tests); trace relation, conversion metric, routing bench. *(AST-context grep, the grep hook, hook fail-open and post-edit nudge moved to v1.0 on 2026-09-27.)*
- **Depends on:** v1.0 (daemon, `diff_impact`, response envelope).
- **Exit criteria:** effectiveness bench rerun on Continuum including its Rust and TS/Tauri tools shows turns and wall-clock at least as good as v1.0, with a further drop from the pre-edit hook; conversion metric shows the hooks' answers are used instead of the grep/Read they replaced; routing bench above threshold; incremental == full across all three languages.

#### v1.2 — Runner integration

- **Goal:** runner-driven workflows get the same gain without the agent having to ask — context injected per mode, tests targeted.
- **Features (4):** socket bridge, IMPLEMENTING injection (`diff_impact` with source), VALIDATING targeted tests, mid-session access in SDK sessions (CLI + hooks in worktree settings, MCP shim).
- **Depends on:** v1.0 (`diff_impact`, `covers`), v1.1 (hooks, MCP shim, mid-session surface).
- **Exit criteria:** runner tasks in Continuum, with vs without Graphite: fewer turns and less wall-clock in IMPLEMENTING; time to first test failure in VALIDATING drops from minutes to seconds; SDK sessions show Graphite calls mid-session in their transcripts.

#### v1.3 — Response contract completion + operational fallback

- **Goal:** large and edge-case answers stay bounded and honest; Graphite works where no daemon runs.
- **Features (7):** `compact` flag, tiered auto-compression, risk verdict (UNKNOWN on zero callers), depth labels, not-indexed/not-found as guidance, CLI probe fallback, triage trace fields.
- **Depends on:** v1.0 (response envelope, ranking), v1.1 (trace relation, for triage fields).
- **Exit criteria:** hub-symbol queries on both repos return bounded payloads with every cut disclosed; no regression on the v1.1 effectiveness bench; CLI works in CI with no daemon; triage fields populated in the trace. Then the v1 exit criteria above are checked as a whole.

### v2 — Query breadth + hardening

- **Goal:** cover the rest of the agent's everyday questions in one call each, and make freshness failure-proof in long sessions.
- **Features:** find all references, search ladder with provenance, path between symbols, repo/module overview, skeleton with fan-in, batch lookups, class-target seeding; ambiguity refinement, noise filters, per-file parse timeout; watcher safety nets, crash marker + transactional rebuild, schema fingerprint, worktree seeding; EXPLORING-mode injection; health check; steering variants (compound-grep inject, read fan-out hint, per-prompt push) + recommendation funnel.
- **Depends on:** v1.0 daemon + envelope, v1.1 trace, v1.2 runner bridge (for EXPLORING injection), v1.3 contract completion.
- **Exit criteria:** EXPLORING turns down vs v1 on the same tasks; worktree cold start fast enough that runner's first query doesn't wait on a full index; a multi-hour soak with the watcher shows zero silent staleness.

### v3 — Resolution depth

- **Goal:** fewer `lower-bound` answers and fewer missed dependents — the agent stops patching one file and missing its siblings.
- **Features:** framework routes + route map / API impact, dynamic dispatch synthesis, Python MRO and receiver-chain typing, LSP/SCIP compiler-grade edges, v2 edge kinds, git co-change edges.
- **Depends on:** v1.0 provenance + `epistemic`/`causes` (to measure the gain), v2 references.
- **Exit criteria:** on real repos, the share of `lower-bound` results and unresolved references drops; blast radius recall against historical commits (files actually co-modified) rises.

### v4 — Structure & orientation

- **Goal:** the agent knows subsystem boundaries and architecture before reading files — confident planning, no backtracking.
- **Features:** communities (Louvain → Leiden, background crate), PageRank / SCC, execution flows, import-cycle check, dead-code candidates, doc comments, typed enumeration, semantic/hybrid search (ships only if measured), RESEARCHING / PLANNING injection, per-community skills and AGENTS.md rules.
- **Depends on:** v3 edges (communities and flows exclude name guesses; better edges → better clusters).
- **Exit criteria:** RESEARCHING/PLANNING turns down; community quality checked on real repos; semantic search ships only if a recall bench on real agent queries shows fewer turns than lexical + graph.

### v5 — Frontend core

- **Goal:** the human sees what the agent saw and fixes the graph where it is blind; savings are visible.
- **Features:** Agent Observatory, Blast Radius Review, Savings Dashboard, Graph Health view, Overrides / curation, raw Datalog query (humans/UI).
- **Depends on:** v1.1 trace + conversion data, v2 health check, v3 provenance.
- **Exit criteria:** overrides measurably reduce unresolved / lower-bound results; dashboard savings match the effectiveness bench.

### v6 — Learned ranking (Laya)

- **Goal:** within a large blast radius, the agent reads the right items first — fewer investigation turns on INFERRED noise.
- **Features:** git-history label mining, Laya sidecar in shadow mode, ranked mode activation.
- **Depends on:** v1.0 deterministic ranker, v1.3 triage trace fields, v3 co-change edges (label source and ranker signal), v5 Observatory (evaluation).
- **Exit criteria:** Laya beats the deterministic baseline on recall@token-budget; filtered-then-accessed (false negative) rate stays under threshold; VALIDATING never ranked.

### v7 — Advanced views & orchestration UI

- **Goal:** less human time assembling context and steering runner; architectural drift caught early.
- **Features:** architecture views (DSM, snapshot diff), centrality / surprising edges / reading-order tour, Context Workbench, Smart Search, Runner Control Center, context pack preview + task scoping.
- **Depends on:** v4 structure, v5 frontend, runner maturity.
- **Exit criteria:** runner tasks started from a curated context pack finish with fewer turns than uncurated; snapshot diff flags coupling introduced by agent PRs.

### v8 — Scope expansion

- **Goal:** correctness across boundaries a single-repo code graph doesn't see.
- **Features:** cross-repo groups / contracts, non-code ingestion (SQL schemas, configs, docs), Rust ownership / borrow and macro-expansion edges, session memory across agent sessions.
- **Depends on:** v3 resolution, v4 structure.
- **Exit criteria:** each item ships only when a measured task class (microservice change, schema change, Rust-heavy repo) shows fewer turns or fewer misses with it.

---

## 1. Extraction & resolution

| Feature | What | Target | Version | Sources | Why this version |
|---|---|---|---|---|---|
| Language: Python | Tree-sitter grammar compiled in | S C | **v1.0** | RG, RI | Proposed first language: 909 of Continuum's 1,146 files, and the repo where runner runs — the thesis bench runs on real tasks. See [v1.0](#v10--thesis-slice-proposal). |
| Languages Rust / TS-JS | Remaining grammars compiled in, Rust first | S C | **v1.1** | vision, RG | User priority Python → Rust → TypeScript. Brings in Graphite itself and Continuum's Rust tools, then Continuum's TS/Tauri parts. |
| `.scm` query files with unified capture tags | One query set per language, shared capture names; downstream never branches on language | C | **v1.0** | GN, CGM | CGM's top recurring bug is "a missing match arm is a silently absent edge"; data-driven captures + wiring tests prevent it. |
| Single AST pass emitting node + reference facts | Definitions, unresolved references with qualifiers, imports — per file | S C | **v1.0** | CGM | Per-file facts are the basis of incremental == full. |
| Deterministic symbol IDs | From path + qualified name + kind + arity | C | **v1.0** | CGM, GN, CT | Rowid/position IDs caused CGM's renumbering class and fg's instability. |
| Import-bound call resolution | A call to `foo` in a file importing `foo` from X binds to X; contradicted edges pruned | C | **v1.0** | CGM, GN | Difference between 1 true edge and N phantom edges. |
| Callee qualifiers | `path` / `self` / `stype` / `rtype` / `recv` / `chain` per call edge | C | **v1.1** | CGM | Structural qualifiers keep edges from being downgraded to ambiguous. |
| `<external>` sentinels + `<module>` scope node | Unresolved/std imports bind to external nodes; top-level statements attach to a module node | C | **v1.0** | CGM | Without them, common names produce phantom edges and top-level-only files look dead. |
| Test detection | AST flags + path/name heuristics | S C | **v1.0** | CGM, GN | Needed for prod/test partition and targeted tests. |
| Unresolved references recorded | Store unresolved receivers/dispatch with cause | A C | **v1.0** | GN, CGM | Feeds `epistemic` / `causes`. |
| Parse-parallel, write-serial pipeline | Rayon parse, single writer | S | **v1.1** | CGM, SB | Cold index speed; atomic per-file writes. |
| Ambiguity refinement | Prefer non-test for non-test caller, then longest common path prefix; keep all if tied | C | **v2** | CGM | Precision gain on top of the v1 confidence tiers. |
| Noise filters as data tables | Cross-file call noise, type-ref noise | C | **v2** | CGM, RG | RG saw noise inflate hubs; tune after measuring v1 graphs. |
| Per-file parse timeout + recorded parse errors | 5 s cap; failures surfaced in health | C | **v2** | CGM | Pairs with the v2 health check. |
| Framework routes | Axum, FastAPI, Express, net/http route → handler | C | **v3** | CGM, GN, LS | Correctness for web backends; precision over recall. |
| Dynamic dispatch synthesis | Callbacks, EventEmitter, React chains, interface fan-out, DI | C | **v3** | LS (CodeGraph), GN | Largest correctness gain after the core; v1 compensates with honest `lower-bound`. |
| Python MRO, receiver-chain typing | Deeper resolution | C | **v3** | GN | Years of edge cases; after the core is measured. |
| LSP / SCIP compiler-grade edges | Optional precise edges from language servers | C | **v3** | CT (ig), LS (Graft) | Strongest precision upgrade; opt-in layer. |
| Doc comments / docstrings | Attached to symbols | A | **v4** | CGM | Orientation, not traversal. |
| Rust ownership/borrow edges, macro expansion | `owns` / `borrows` / through macros | C | **v8** | CT (fg) | Rust-heavy repos only. |
| Non-code ingestion | SQL schemas, configs, docs in the graph | C | **v8** | LS (Graphify) | Ships when schema/config changes are measured as a miss source. |

## 2. Graph model & edges

| Feature | What | Target | Version | Sources | Why this version |
|---|---|---|---|---|---|
| Edge kinds (core) | `contains`, `calls`, `imports`, `extends`/`inherits`, `implements`, `has_member`, `overrides`, `references` | C | **v1.0** | GN, CGM | What blast radius needs. |
| Three-tier confidence + categorical provenance | EXTRACTED / INFERRED / AMBIGUOUS; `extracted`, `resolved`, `inferred`, `name_guess` | A C | **v1.0** | CGM, GN, LS (Graphify) | A number alone can't exclude guesses (GN). Default floor INFERRED; ambiguous counted and disclosed. |
| Confidence derived by rule | Evaluated at query time from facts, never stored | C | **v1.0** | CGM, SB | Stored confidence drifted in CGM; SB measured +3–5% cost. |
| Test coverage relation (`covers`) | Test symbol → symbols it reaches | S C | **v1.0** | GN, CGM, CT (ig), LS | Targeted tests and pre-edit covering tests. |
| Edge kinds (extended) | `exports`, `routes_to`, `accesses` r/w, `injects`, `fetches` | C | **v3** | GN, CGM | Come with routes and dispatch. |
| Git co-change edges (`changes_with`) | Mined from history; participate in blast radius | C | **v3** | CT (fg), LS, LY | Correctness signal; also the ranker/label source for v6. CT suggested v1 — placed v3 because it needs history mining and its main consumers come later. |
| Communities (Louvain → Leiden) | Clusters over calls/extends/implements, name guesses excluded, deterministic seed | A | **v4** | GN, LS, SB | CozoDB Louvain cost 1–4.7 s / GBs (SB) → background Rust crate. Needs v3 edge quality. |
| PageRank / hot symbols, SCC | Centrality, cycles | A C | **v4** | GN, CGM, SB | Cheap in Rust; PLANNING input. |
| Execution flows | Scored entry points → bounded DFS → flows, truncation published | A S | **v4** | GN | High EXPLORING value, heuristic. |
| Betweenness, "surprising" edges, reading-order tour | Analytics | A | **v7** | CGM | Architecture views. |

## 3. Freshness & sync

| Feature | What | Target | Version | Sources | Why this version |
|---|---|---|---|---|---|
| One daemon per repo | Owns CozoDB, watcher, adjacency; CLI/MCP/hooks/runner are thin socket clients | S C | **v1.0** | CGM, CT (lk) | Every session/runner task spawns its own MCP process; one daemon = one hot graph. |
| Eager indexing on the watcher thread | Change → hash → parse → atomic per-file replace → adjacency update | S | **v1.0** | CGM, GN, SB | CGM indexes on the next query; GN takes 31.7 s per edit. |
| Freshness barrier | Wait ≤~200 ms for indexing up to arrival seqno, else `stale: true` | S C | **v1.0** | CGM | Never blocks, never silently stale. |
| `graph_rev` watermark | Stored in DB, returned in every response | A C | **v1.0** | CT (lk) | Kills cache races; tells agent and Observatory what state answered. |
| mtime+size stamp → BLAKE3 ladder | Skip unchanged bytes | S | **v1.0** | CGM, LS | Minimal watcher overhead. |
| Post-edit nudge hook | Tell the daemon "file X changed now" | S C | **v1.0** | CGM | Agent's own edit indexed before its next query; installed with the interception hooks. |
| Stale-element sweep | Remove facts of files that left the tracked set | C | **v1.0** | CT (lk) | Deleted files must disappear. |
| Incremental == full, tested | Fixture comparing incremental with rebuild | C | **v1.0** | CGM, SB | CGM bumped its index format 71 times over this bug class. |
| CLI probe fallback | Probe + sync when no daemon runs | S | **v1.3** | architecture | CI and one-off use. |
| Watcher safety nets | Unknown events = content change; periodic backstop rescan | C | **v2** | CGM | Watchers fail silently in long sessions. |
| Crash marker + transactional rebuild | `index_run_in_flight`; readers see the old graph until commit | C | **v2** | CGM | Crash consistency. |
| Schema fingerprint → rebuild on mismatch | Hash of the schema definition | C | **v2** | GN | Safe upgrades once v1 ships. |
| Worktree seeding | Seed a worktree's graph from the main checkout, then diff | S | **v2** | CGM | Runner uses a fresh worktree per task. |

## 4. Query & traversal

| Feature | What | Target | Version | Sources | Why this version |
|---|---|---|---|---|---|
| Blast radius | Transitive dependents, shallowest depth; default depth 3, max 10 | S C | **v1.0** | all | Core. RG: depth 3 reaches 63–76% of the depth-10 set; SB: 19–80 µs. |
| Depth labels | d1 WILL BREAK / d2 LIKELY AFFECTED / d3+ MAY NEED TESTING | A | **v1.3** | GN | Tells the agent what to do. |
| `context` | One symbol: signature, source, callers/callees, references, tests | S A | **v1.1** | GN, CGM, CT (fg) | Replaces 3–6 Read/Grep calls before an edit. |
| `diff_impact` | Diff hunks → changed symbols → blast radius → covering tests | S C | **v1.0** | GN (`detect_changes`), CT (ig), CGM (`affected`), RI | Merged duplicates; the graph already matches the working tree. |
| Lexical symbol search | Exact → fuzzy with disambiguation | S | **v1.1** | CGM, CT (lk), GN | Replaces grep loops for names. |
| AST-context grep | Regex hits grouped by enclosing symbol | S | **v1.0** | CGM, LS (Graft) | The intercepted answer for non-identifier patterns ([interception.md](interception.md)). |
| Embedded ripgrep search | ripgrep crates in the daemon; gitignore + hidden/default excludes, omitted counts disclosed | S C | **v1.0** | user, interception | Faster than `grep -r`, drops binary/worktree noise; always line numbers. |
| Residue judgment | Classify each match: definition / call site / import / mock / docs / string-comment / unresolved / other language / graph-only | A C | **v1.0** | interception | Drop only what the graph provably explains; nothing silently omitted. |
| Find all references | Every import/inherit/implement/call/reference site | C | **v2** | CGM, CT (ig) | Rename/remove safety. |
| Search ladder with provenance | `retrieval{rung, reason}` on each answer | A | **v2** | CT (lk) | Builds on v1 search. |
| Path between two symbols | Shortest directed path, file:line per hop | S | **v2** | GN, CGM | Answers 3–8 hops in one call. |
| Repo / module overview | Modules, deps, entry points, hot symbols, token-budgeted | S A | **v2** | CGM, LS (Graft), RI | EXPLORING payload. |
| Skeleton with fan-in / complexity | Annotated signatures of a file | S A | **v2** | CT (ig), LS (Graft) | API surface at ~1/10 tokens. |
| Batch lookups | N symbols in one query | S | **v2** | CT (ig) | Fewer round trips. |
| Class-target seeding | Impact on a type seeds constructors, owning file, typed properties | C | **v2** | GN | Fixes empty class impact in some languages. |
| Route map / API impact | Route → handler → consumer | C | **v3** | GN, CGM | With framework routes. |
| Semantic / hybrid search | BM25 + vector with RRF | S A | **v4** | CGM, GN, CT (ig) | Ships only if a recall bench shows fewer turns; embed on the watcher thread. |
| Typed enumeration | "All fns returning Result<T>" | S | **v4** | CGM | Low call frequency. |
| Import-cycle check | SCC lint | C | **v4** | GN, CGM | PLANNING input. |
| Dead-code candidates | Unreachable from entry points, "verify" framing | C | **v4** | CGM, CT (fg) | Needs entry-point detection from flows. |
| Raw Datalog query | `graphite query` for humans / UI | — | **v5** | GN, CT (fg) | Human surface; LLMs don't know Datalog. |
| Cross-repo groups | Contracts across repos | C | **v8** | GN, CT (ig), LS | After single-repo core. |

## 5. Response contract

All **v1** — the contract is what turns a correct graph into fewer agent turns. The core (source inline, budgets, disclosure, honesty) ships in v1.0; convenience layers on top of it in v1.1 and v1.3.

| Feature | What | Target | Version | Sources | Why this version |
|---|---|---|---|---|---|
| Source inline | Verbatim line-numbered source (~4 KB cap), read from disk by byte range + body hash | S | **v1.0** | CGM, GN, LS | Eliminates the follow-up Read. Disagreement: CGM stores code in DB, GN reads by range — range chosen, the watcher keeps files current. |
| `compact` flag | Signature + location only | S | **v1.3** | CGM | Every query. |
| Tiered auto-compression | Node summaries → by file → by directory, ids to expand; names always present | S | **v1.3** | CGM, CT (ig), LY | Bounded payloads. |
| Rank-based budget | Drop lowest-ranked items, never cut bytes | S A | **v1.0** | GN | Byte truncation breaks structure. |
| Deterministic ranking | confidence × depth × fan-in | S A | **v1.0** | LY, SB | Most relevant first; baseline for v6. |
| Disclosure fields | `limit_hit`, `depth_capped`, `*_hidden`, `*_truncated`, `partial` | A C | **v1.0** | CGM, GN, CT (ig) | Three tools converged: never a silent partial answer. |
| `epistemic` + `causes` | `exact` / `lower-bound` + counted reasons | A C | **v1.0** | GN | Honest uncertainty compensates shallower v1 resolution. |
| Risk verdict | LOW…CRITICAL; UNKNOWN (never LOW) on zero callers | A C | **v1.3** | GN, CGM | One word the agent can act on. |
| Prod / test partition + covering tests | Tests split out, runnable names | S C | **v1.0** | CGM, GN | Targeted tests. |
| Disambiguation in the envelope | Ranked candidates with `uid`; no match → suggestion, not error | A S | **v1.1** | GN, CGM | Avoids the two-step lookup turn. |
| Not-indexed / not-found as guidance | Success-shaped, names the next action | S | **v1.3** | LS (CodeGraph), CGM | `isError` teaches agents to abandon the toolset. |
| `stale` + `graph_rev` + `graph_lag_ms` | Freshness state in every response | A | **v1.0** | CT (lk), CGM | See §3. |
| Bytes/3 token estimator | Conservative, single estimator | S | **v1.0** | CGM | Predictable budgets. |

## 6. Agent surface & steering

| Feature | What | Target | Version | Sources | Why this version |
|---|---|---|---|---|---|
| CLI as primary agent surface | `graphite <cmd>` via Bash; default = model text format, `--json` for programs | S | **v1.0** | CGM | MCP tools are deferred in Claude Code; measured conversions came via CLI. Pilot B: 14–25 KB one-line JSON got `head -c`-cut by agents. |
| Output modes | Default (exact agent view), `--human` (grouped, colored), `--json`, `--explain` — one record, parity-tested renderers | A | **v1.0** | interception | Debug what the agent actually saw; no TTY auto-switch. |
| MCP shim (secondary) | Same handlers, thin client | S | **v1.1** | architecture, CGM | Hosts that don't defer tools; runner SDK sessions. |
| Few listed tools, capability via flags | ≤5 listed | S | **v1.1** | CGM, CT (lk), LS | Agents under-pick extra tools. Count disagreement (1 vs 3 vs 7): start few, the routing bench decides. |
| Tool schema limits | No `anyOf`; descriptions ≤200 chars, positive; instructions ≤1.5 KB | S | **v1.1** | CGM | Measured client behavior. |
| Transparent interception → enriched grep | PreToolUse rewrites read-only grep/rg/ack/find/ls/cat/sed -n/head/tail via `updatedInput`; Graphite answers with integrated `path:line:` lines + verdict header + footer; head/tail = budget; filters by intent | S A C | **v1.0** | CGM, GN, RTK, format eval | Hint-only ~0% uptake (CGM); pilot B non-use + post-answer greps. Replaces CGM's "deny and answer": the agent sees its own command's output, not a refusal. Format chosen by experiment (lines 100%, fewest tokens). |
| Pre-edit hook: impact + covering tests | Inject when a signature with ≥2 prod callers is touched | C | **v1.1** | CGM | Blast radius at the moment of the edit. |
| Hook fail-open, in settings.json | Errors never break the tool call; `graphite hooks install` preserves foreign hooks and key order | C | **v1.0** | CGM | Plugin `hooks.json` only honors SessionStart. |
| Hook log + debug views | `.graphite/hooks.jsonl`; `graphite hooks log` / `hooks show <n>` side by side (command, model answer, `--human`, `--explain`) | A | **v1.0** | interception | Debug real sessions. |
| Skill file | Teaches the agent the tool | S | **v1.0** | CT (fg) | Cheap onboarding. |
| Steering variants | Compound-grep inject, read fan-out hint, per-prompt push | S | **v2** | CGM | Tune with the v2 recommendation funnel. |
| Per-community skills, AGENTS.md rules | Area knowledge, "impact before edit" | A | **v4** | GN | Needs communities. |
| Session memory across agent sessions | Save/search prior session context | S | **v8** | CT (ig, lk) | Overlaps runner/dao task state today; ships if cross-session tasks measure repeated rediscovery. |

## 7. Runner integration

| Feature | What | Target | Version | Sources | Why this version |
|---|---|---|---|---|---|
| Socket bridge | Runner queries the daemon; CLI fallback; starts the worktree daemon | S | **v1.2** | RI | No per-query spawn. |
| IMPLEMENTING injection | `diff_impact` with source in the prompt | S C | **v1.2** | RI | Highest-value mode. |
| VALIDATING targeted tests | Covering tests before the full suite | S | **v1.2** | RI, CGM | Seconds of feedback instead of minutes. |
| Mid-session access | CLI (primary) / MCP in SDK sessions, hooks in worktree settings | S A | **v1.2** | RI | Layer 2. |
| EXPLORING injection | Repo overview in the prompt | S | **v2** | RI | Needs v2 overview. |
| RESEARCHING / PLANNING injection | Communities, coupling, cycles | A | **v4** | RI | Needs v4 structure. |
| Context pack preview / task scoping | Human trims context; scope violations in trace | S | **v7** | FE | Needs the UI. |

## 8. Ranking / Laya

| Feature | What | Target | Version | Sources | Why this version |
|---|---|---|---|---|---|
| Deterministic ranker | confidence × depth × fan-in (+ co-change once v3) | S A | **v1.0** | LY | Most of the value with no model. |
| Triage trace fields | Candidates, must/verify/skip, engine, model sha | S | **v1.3** | LY | Without them no later ranker can be evaluated. |
| Git-history label mining | Training data from past commits | A C | **v6** | LY | Needs v3 co-change. |
| Laya sidecar: shadow → ranked | Ranks residual INFERRED items; never filters; VALIDATING never ranked | S A C | **v6** | LY | Activates only if it beats the deterministic baseline. |

## 9. Frontend

| Feature | What | Target | Version | Sources | Why this version |
|---|---|---|---|---|---|
| Agent Observatory, Blast Radius Review, Savings Dashboard | FE P1 | S C | **v5** | FE | Data exists from v1; UI after the engine is measured. |
| Graph Health view, Overrides / curation | FE P2 | A C | **v5** | FE, CGM | Needs v2 health data; overrides compound. |
| Architecture views (DSM, snapshot diff) | FE P3 | C | **v7** | FE, GN | Needs v4 structure. |
| Context Workbench, Smart Search, Runner Control Center | FE P4–P5 | S | **v7** | FE | Needs runner maturity. |

## 10. Measurement & observability

| Feature | What | Target | Version | Sources | Why this version |
|---|---|---|---|---|---|
| Trace relation | Every call: session, tool, args, result/source tokens, latency, symbols, `graph_rev` | S | **v1.1** | FE, CGM, LY | Feeds every later measurement. |
| Conversion metric | Did the agent Read/Edit what Graphite returned? (from transcripts) | A | **v1.1** | CGM | The "was it used" signal. |
| Routing bench | Right tool picked from live schemas; release gate | A | **v1.1** | CGM | Tool descriptions are code. |
| Effectiveness bench | Turns and wall-clock with vs without Graphite | S | **v1.0** | CGM, LS (Graft) | The only proof of the primary target. |
| Storage bench kept runnable | `bench/` on real graphs | S | **v1.0** | SB | Guards the hot path. |
| Health check | Parse errors, unresolved ratio, staleness | C | **v2** | CGM | With parse-error recording. |
| Recommendation funnel | Hook deny/hint/bypass joined to later use | S | **v2** | CGM | Tunes steering. |

---

## Parking lot

Not scheduled because no measured effect on the targets justifies them now. Each has a trigger that brings it back.

| Item | Source | Why parked | Revisit when |
|---|---|---|---|
| Go grammar | RG, vision | No active Go repo (sensemesh was only a reference and is no longer active). | A Go repo becomes an active target for agent work. |
| `rename` tool (graph + regex multi-file edit) | GN | Editing is the agent's job; v2 references give it the sites. | Agents repeatedly miss sites in multi-file renames despite references. |
| Many listed tools (~90 / 17) | CT (ig), GN | Measured to cost turns and context every session. | Routing bench shows a specific extra tool is picked correctly and saves turns. |
| Two-step `symbol_id` lookups | CT (ig) | Costs a turn; v1 disambiguates in the envelope. | Disambiguation in the envelope measurably fails. |
| `list_repos` / multi-repo registry | GN | One daemon per repo. | v8 cross-repo work needs a registry. |
| MCP prompts (`detect_impact`, `generate_map`) | GN | No measured use by agents. | A host surfaces prompts and agents use them. |
| Remote auto-sync polling | GN | The local watcher supersedes it. | Hosted/CI deployments with no local watcher. |
| Post-command freshness check hook | GN | The watcher + barrier supersede it. | Watcher proves unreliable on some platform. |
| `node_id_renumbered` warnings | CGM | Deterministic IDs make them unnecessary. | IDs ever become unstable. |
| Taint / PDG / security analysis passes | GN, CT (ig) | Security-scanner scope, heavy. | Agent correctness failures trace to data-flow the graph can't see. |
| Structured TOML ingestion (JSON/YAML → tables) | CT (ig) | No agent task needs it yet. | v8 non-code ingestion needs a generic loader. |
| Hash-chained audit ledger | CT (lk) | No target served. | A compliance requirement appears. |
| AI chat inside the UI | GN | The agent already is the chat. | Humans ask questions the agent can't route to Graphite. |
| Byte-truncating token budgets | GN | Rank-based budgets (v1) keep structure intact. | A measured case where rank-based budgets lose information that byte truncation keeps. |
| Storing source text in the DB | GN, CGM | Byte ranges + disk reads keep the DB small; watcher keeps files current. | Reading from disk becomes a measured latency problem. |

---

## Counts

114 features after deduplication (the v1 language row is split in two; 2026-09-27: +4 interception features — embedded search, residue judgment, output modes, hook log) — v1: 66 (v1.0: 42 · v1.1: 13 · v1.2: 4 · v1.3: 7; AST-context grep, grep interception, hook fail-open and post-edit nudge moved v1.1 → v1.0) · v2: 18 · v3: 7 · v4: 10 · v5: 3 · v6: 2 · v7: 4 · v8: 4 · parking lot: 15 (Go grammar moved to the parking lot on 2026-09-24; the v1.1 language row now covers Rust and TS-JS)
