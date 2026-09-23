# Graphite — Laya Integration

> Typed decision engine as ranking/annotation layer for graph output.

Laya sits between Graphite's graph queries and the AI agent's context window. It ranks and annotates blast radius results so the agent processes the most relevant items first — without ever hiding information.

## Background: What Is Laya

Laya is ConvAI Innovations' open-weight System One decision engine. Unlike LLMs, it generates zero text. It accepts typed questions evaluated against a provided state and returns structured decisions with calibrated probabilities.

### Model Specifications

| Checkpoint | Encoder | Parameters | Context | Target |
|---|---|---|---|---|
| `laya` | ModernBERT-large | 421M | 512 tokens | English text classification |
| `laya-multilingual` | mmBERT-base (256k vocab) | 322M | 1024-8k | 100+ languages |
| `laya-typed-decisions` | ModernBERT-large | 421M | 1024 tokens | Agent observability, structured decisions |

Download sizes: English (~808 MB), multilingual (~647 MB), total bundle (2.5 GB).

### Decision Primitives

Three question types, combinable in a single call:

- **Choice**: selects from predefined options, returns selected key + probability distribution + confidence
- **Score**: evaluates state against ordered rubric (levels 0, 1, 2...), returns expected level + distribution
- **Noul**: binary truth assessment, returns P(true) from 0.0 to 1.0

### Performance

| Metric | Laya (MLX, M3 Max) | TypeSafe Jev (API) |
|---|---|---|
| Single question P50 | 13.4 ms | 236-276 ms |
| Single question P95 | 13.9 ms | — |
| 50-question throughput | 146.8 q/s | — |
| Batched (10 questions) | 7.2 ms/q | — |
| Accuracy (typed-decisions, fine-tuned) | 0.766 | 0.727 |
| Accuracy (zero-shot) | 0.35 (near random) | 0.727 |
| Calibration Error (ECE) | 0.081 | 0.246 |
| Cost | $0 (self-hosted) | $0.042/M input tokens |
| License | Apache 2.0 | Proprietary |

### Apple Silicon: laya-mlx

Native MLX port exists (`pip install laya-mlx`). Runs on Apple Silicon without PyTorch or cloud API.

| Metric | laya 421M | multilingual 322M |
|---|---|---|
| Single question P50 | 13.42 ms | 7.39 ms |
| Single question P95 | 13.92 ms | 7.79 ms |
| Peak memory | 943.6 MiB | 687.6 MiB |
| 50-question throughput | 146.8 q/s | 395.0 q/s |

MLX conversion:
```bash
uv run laya-mlx convert \
  --model convaiinnovations/laya \
  --dtype float16 \
  --output models/laya-mlx-fp16
```

### Key Constraints

- **Zero-shot is not shippable.** Base models score ~0.35 (near random). Fine-tuning required.
- **>20 options degrades.** Choice questions with more than 20 candidates lose accuracy. Not a concern for Graphite's use cases (3-8 options max).
- **512 token context window.** Requires per-item scoring design, not full list input.
- **Apple Silicon only (MLX).** Linux requires candle (Rust) port or PyTorch CPU fallback.

### Laya Is Not an LLM

Laya is a bidirectional encoder (ModernBERT) with typed decision heads. It is non-autoregressive — no text generation, no token-by-token output, no hallucination by construction. It returns typed, structured decisions with calibrated probabilities.
### Why Laya Over Jev

| Dimension | Laya | Jev |
|-----------|------|-----|
| Latency | 13ms | 236ms (18x slower) |
| Cost | $0 | $0.042/M tokens |
| Calibration (ECE) | 0.081 (3x better) | 0.246 |
| Offline/air-gapped | Yes | No |
| Vendor lock-in | Apache 2.0, weights open | Proprietary API |
| Fine-tune on domain data | Yes | Impossible |
| Zero-shot | 0.35 (bad) | 0.727 (good) |

Zero-shot irrelevant because Graphite will fine-tune on domain-specific data (graph structure, code dependencies). Fine-tuned Laya on a narrow domain outperforms generic Jev. >20 options irrelevant because Graphite's decision space is 3-8 options.

### Historical Note

Laya predates Jev. ConvAI published the arXiv paper (2503.23303) and released open weights in March 2025. TypeSafe AI launched Jev from stealth in September 2026 claiming "first System One model" — marketing, not fact.

---

## The Problem Laya Solves

Graphite's graph queries return complete, structurally correct results. But completeness creates a secondary problem:

1. **Blast radius noise.** A signature change on a utility function returns 200 transitively dependent files. Most are irrelevant passthrough. The agent reads 30, acts on 8, wastes ~10 turns investigating noise.

2. **INFERRED edge uncertainty.** Graphite infers edges via heuristic (dynamic dispatch, duck typing, string-based routing). Some are real, some are noise. The agent cannot distinguish them — both carry the same INFERRED tag. It either investigates all (slow) or trusts all (wrong).

3. **Flat test ordering.** 30 affected test files returned in graph order. The agent or runner executes sequentially. No signal about which tests are most likely to catch the specific change.

These are not graph problems. The graph is correct. These are **relevance ranking problems** — which items matter most for *this specific change*. That is where a fine-tuned decision encoder earns its value.

---

## Design Principle: Ranker, Never Filter

**Laya may reorder and label. It may never delete. It may never be the only path to an item.**

Worst case of a ranking error: agent processes items in suboptimal order. Same as today. Correctness never regresses.

Worst case of a filtering error: agent misses a broken downstream dependency. Bug ships silently. This is the exact failure mode Graphite exists to prevent.

Every blast radius response with Laya annotation includes the complete deterministic list. Laya adds `attention` tags and `laya_score` to each item. The agent sees everything; Laya tells it what to look at first.

Output shape:
```
blast_radius response:
  items:
    - symbol: "auth_middleware::validate_token"
      file: "src/auth/middleware.rs"
      line: 42
      edge: EXTRACTED
      depth: 1
      attention: must        # deterministic (EXTRACTED depth-1)
      
    - symbol: "session::refresh"
      file: "src/session/manager.rs" 
      line: 118
      edge: INFERRED
      depth: 2
      attention: verify      # Laya scored
      laya_score: 0.87
      
    - symbol: "logging::access_log"
      file: "src/logging/access.rs"
      line: 15
      edge: INFERRED  
      depth: 3
      attention: skip        # Laya scored
      laya_score: 0.12
      
  triage:
    engine: "laya-mlx"
    model_sha: "a1b2c3..."
    applied: true
    
  summary:
    total: 47
    must: 5
    verify: 12
    skip: 30
    deterministic: 5
    laya_scored: 42
```

Footer always present: `47 total, 5 must, 12 verify, 30 skip. Call with detail=all for full source.`

---

## Three Use Cases (Where Laya Adds Value)

### 1. Blast Radius Ranking

**Problem:** Agent receives 50 files flat, reads 30, acts on 8. ~15 turns.
**With Laya:** Agent receives 50 ranked, reads top 12, acts on 8. ~6 turns.

| Metric | Without Laya | With Laya | Gain |
|--------|-------------|-----------|------|
| Agent turns | ~15 | ~6 | -60% |
| Wall-clock time | ~75s (15 × 5s) | ~30s (6 × 5s) | -60% |
| Tokens consumed | ~45k | ~18k | -60% |
| Correctness | Baseline | Same (nothing hidden) | Neutral |

Laya questions per item (~400 tokens each):
```
Template: caller_affected
State: "Symbol `authenticate(token: &str) -> Result<User>` changed to 
       `authenticate(token: &str, scope: Scope) -> Result<User>`"
Question: {
  "type": "choice",
  "instructions": "Does this call site need to change?",
  "criteria": {
    "must_change": "call passes args that no longer match",
    "should_verify": "call may be affected depending on defaults or wrappers",
    "unaffected": "call does not use the changed parameter path"
  }
}
```

### 2. INFERRED Edge Validation (Highest Value)

**Problem:** 20 INFERRED edges — agent investigates all, 10 are noise. ~10 wasted turns.
**With Laya:** Each edge scored. Agent focuses on high-probability edges. ~3 turns.

| Metric | Without Laya | With Laya | Gain |
|--------|-------------|-----------|------|
| Investigation turns | ~10 | ~3 | -70% |
| Assertiveness | Low (all INFERRED equal) | High (scored 0-1) | Major improvement |
| Correctness | Baseline | Better (noise distinguished) | Improvement |

This is the highest-value template because INFERRED edges are the largest noise source and the one thing the graph structurally cannot resolve. Ground truth is also cleanest: the edge is real or it isn't.

```
Template: inferred_edge_real
State: "Graphite inferred `UserController.handle_request` depends on 
       `AuthService.validate` via dynamic dispatch through trait `Handler`."
Question: {
  "type": "score",
  "instructions": "Is this inferred dependency real?",
  "criteria": ["no_relationship", "possible_but_unlikely", "likely", "certain"]
}
```

### 3. Test Relevance Ranking

**Problem:** 30 affected test files, executed sequentially. Minutes for feedback.
**With Laya:** Tests ranked by relevance to the specific change. Top 5 run first (seconds), rest after.

| Metric | Without Laya | With Laya | Gain |
|--------|-------------|-----------|------|
| Time to first failure | Minutes (sequential) | Seconds (prioritized) | Major |
| Assertiveness | Run everything equally | Run most relevant first | Improvement |
| Correctness | Same (all tests run) | Same (all tests run) | Neutral |

```
Template: test_covers_change
State: "Function `authenticate` signature changed. Diff hunk: 
       -fn authenticate(token: &str) -> Result<User>
       +fn authenticate(token: &str, scope: Scope) -> Result<User>"
Question: {
  "type": "score",
  "instructions": "Does this test exercise the changed behavior?",
  "criteria": ["unrelated", "tangential", "exercises_path", "directly_tests"]
}
```

---

## What Does NOT Need Laya (Deterministic Graph Queries)

These scenarios are solved by CozoDB Datalog queries with 100% precision. Adding Laya would be slower and less accurate:

| Scenario | Graph Query | Why ML Is Worse |
|----------|-------------|-----------------|
| "File deleted, who imports it?" | `dependents` query | Exact answer, zero ambiguity |
| "New dependency creates cycle?" | `path_between` query | Exact answer, zero ambiguity |
| "Export removed from public surface?" | `imports` relation | Every external importer affected, 100% rule |
| "Body-only change, callers affected?" | Edge type check | EXTRACTED callers unaffected by definition |
| "Rename detected?" | Content hash comparison | Same body hash, different name/path = rename |
| "Config/build file changed?" | Outside Tree-sitter scope | Emit "unanalyzable, full test run recommended" |
| "Multi-file refactor dedup?" | Diff membership check | Files already in diff are not triage candidates |

**Rule:** if the answer exists in the graph structure, use the graph. Laya only where the question is about **relevance to an intent**, not about **structural fact**.

---

## Architecture: Sidecar Over Unix Socket

### Why Not Embedded

| Option | Problem |
|--------|---------|
| PyO3 (embed Python) | Breaks single binary. Python runtime dependency. Python crash kills graph server. Worst of both worlds. |
| Native Rust (candle) | ModernBERT exists in candle-transformers, but Laya's decision heads need porting. candle Metal is 2-3x slower than MLX (~30-40ms). Only justified when Linux demand + trusted checkpoint exist. |
| Embedded in watcher | Laya in write path makes indexing non-deterministic and non-reproducible. Never. |

### Sidecar Design

Graphite stays a pure Rust single binary. Laya runs as a separate Python process communicating via unix domain socket.

```
┌────────────────────────────────┐     ┌──────────────────────────┐
│  GRAPHITE (Rust, single binary)│     │  LAYA SIDECAR (Python)   │
│                                │     │                          │
│  MCP Server                   │     │  laya-mlx (MLX runtime)  │
│    ↓                          │     │  ModernBERT 421M fp16    │
│  blast_radius query           │     │  Decision heads           │
│    ↓                          │     │  Fine-tuned checkpoint    │
│  CozoDB result (47 items)     │     │                          │
│    ↓                          │     │  Unix socket listener     │
│  Triage trait                 │     │    /run/graphite-laya.sock│
│    ├── NoopTriage (default)   │     │                          │
│    └── SocketTriage ──────────┼─────┤  Receives: template +    │
│         50ms connect timeout  │     │    state + items          │
│         Noop on failure       │     │  Returns: scores +       │
│    ↓                          │     │    attention tags         │
│  Annotated result (47 items   │     └──────────────────────────┘
│    + attention + laya_score)  │
│    ↓                          │
│  MCP response to agent        │
└────────────────────────────────┘
```

### Key Properties

1. **Graphite does not spawn Laya.** User or runner starts `laya-serve --socket /run/graphite-laya.sock`. Graphite probes socket with 50ms connect timeout. Absent = `NoopTriage`.

2. **Laya is an accelerator, never a requirement.** "Zero dependency" remains honest. Graphite works identically without Laya — just no ranking annotations.

3. **Concurrent-safe.** One Laya process serves multiple Graphite instances (multiple agents querying simultaneously). Socket allows this naturally.

4. **Laya only in read path.** Never in the watcher/write path. Indexing stays deterministic and reproducible.

5. **Every response carries triage metadata.** `triage: {engine, model_sha, applied}` so Observatory traces distinguish triaged vs untriaged sessions.

### Triage Trait

```rust
pub trait Triage: Send + Sync {
    fn triage(
        &self, 
        template: &Template, 
        change_context: &ChangeContext,
        items: &[BlastRadiusItem],
    ) -> Vec<TriageDecision>;
}

pub struct TriageDecision {
    pub item_id: SymbolId,
    pub attention: Attention,      // Must, Verify, Skip
    pub score: Option<f32>,        // 0.0-1.0, from Laya
    pub source: TriageSource,      // Deterministic | LayaModel { sha, version }
}

pub enum Attention {
    Must,     // EXTRACTED depth-1 callers, or Laya score >= high threshold
    Verify,   // Laya score in middle range
    Skip,     // Laya score below low threshold (still shown, just deprioritized)
}

// Two implementations
pub struct NoopTriage;   // Returns all items with attention: Must, no scores
pub struct SocketTriage; // Connects to Laya sidecar, applies templates
```

### Protocol (Newline-Delimited JSON over Unix Socket)

Request:
```json
{
  "template": "caller_affected",
  "change_context": {
    "symbol": "authenticate",
    "file": "src/auth/mod.rs",
    "before_signature": "fn authenticate(token: &str) -> Result<User>",
    "after_signature": "fn authenticate(token: &str, scope: Scope) -> Result<User>",
    "diff_hunk": "@@ -42,3 +42,3 @@..."
  },
  "items": [
    {
      "id": "sym_a1b2c3",
      "symbol": "session::refresh",
      "file": "src/session/manager.rs",
      "edge_type": "INFERRED",
      "edge_mechanism": "dynamic_dispatch",
      "depth": 2,
      "snippet": "let user = auth.authenticate(token)?;"
    }
  ]
}
```

Response:
```json
{
  "decisions": [
    {
      "id": "sym_a1b2c3",
      "attention": "verify",
      "score": 0.87,
      "distribution": {"must_change": 0.12, "should_verify": 0.75, "unaffected": 0.13}
    }
  ],
  "model_sha": "a1b2c3d4...",
  "inference_ms": 14.2
}
```

### Per-Item Scoring Design (512-Token Constraint)

Laya's 512-token context window cannot fit a full blast radius list. Each item is scored independently as a (change_context, candidate_item) pair:

```
[template question                                    ~40 tokens]
[change context: before/after signature or diff hunk  ≤150 tokens]
[candidate: path, edge type, confidence, snippet      ≤200 tokens]
≈ 400 tokens with margin
```

50 items = 50 inferences. Sequential: 50 × 13ms = 650ms. Tensor-batched (batch size 32 on M-series): ~100-200ms. Either is invisible against multi-second agent turns.

**Truncation rule:** Cut snippets at Tree-sitter node boundaries (whole call expression, whole signature), never mid-expression at a token count. Start from the call-site line and expand outward until budget. Graphite already has the AST — use it.

**Residual only:** Send only items that need ML scoring. EXTRACTED depth-1 callers get `attention: must` deterministically. Typically 5-20 items go to Laya, not the full blast radius.

---

## Three Templates (v1)

Most of the original 6 templates are deterministic graph queries. v1 needs only three ML templates — the ones where the graph cannot resolve relevance:

### Template 1: `caller_affected` (Choice)

**When:** Signature change, type change, rename with INFERRED call sites.

**Question:** "Symbol S changed from `<before>` to `<after>`. Does the call at D (`<snippet>`) need to change?"

**Options:** `must_change | should_verify | unaffected`

**Scope:** Only INFERRED edges. EXTRACTED call sites of a renamed/changed-signature symbol are `must_change` deterministically — no ML needed.

**Ground truth:** Whether the call site was modified in the commit (or a follow-up commit within 48h on the same branch).

### Template 2: `inferred_edge_real` (Score) — Highest Value

**When:** Any INFERRED edge in blast radius.

**Question:** "Graphite inferred D depends on S via `<mechanism: dynamic dispatch | string route | duck typing>`. Given `<D snippet>` and `<S signature>`, is this real?"

**Rubric:** `[no_relationship, possible_but_unlikely, likely, certain]`

**Why highest value:** INFERRED edges are the largest noise source in the graph. They are the one thing the graph structurally cannot resolve. Every false INFERRED edge wastes an agent turn investigating. Every missed real INFERRED edge risks a silent break. This template directly improves both speed AND correctness.

**Ground truth:** Cleanest of all three — the edge is real or it isn't. Verifiable from code inspection.

### Template 3: `test_covers_change` (Score)

**When:** Ranking affected test files for execution priority.

**Question:** "Test T (`<snippet>`) references symbol S. Does it exercise the behavior in `<diff hunk>`?"

**Rubric:** `[unrelated, tangential, exercises_path, directly_tests]`

**Scope:** Never drops tests. All affected tests run. Laya only determines execution order.

**Ground truth:** Whether the test fails when the change is introduced without its fix. Derivable from CI history.

### Templates NOT Needed (Deterministic)

| Scenario | Why No Template | Graph Solution |
|----------|----------------|----------------|
| `file_deleted` | Every importer affected, 100% rule | `dependents` query |
| `new_dependency` circular check | Exact structural answer | `path_between` query |
| `export_change` removed | Every external importer affected | `imports` relation |
| `body_only_change` | EXTRACTED callers unaffected by definition | Edge type check |
| Rename/move (EXTRACTED refs) | Deterministic for EXTRACTED | Content hash + `dependents` |
| Config/build files | Outside Tree-sitter scope | Emit "unanalyzable — full test run" |
| Multi-file refactor dedup | Files in diff = not triage candidates | Diff membership check |

---

## Fine-Tuning Pipeline

### Zero-Shot Is Unusable

Laya base models score 0.35 on typed-decisions benchmark (near random). Fine-tuning raises this to 0.766 on the generic benchmark, but domain-specific fine-tuning on code dependency data should exceed this — narrow domain with consistent structure.

### Bootstrap: Git History Mining (Primary Training Source)

No agent traces needed to start. Every git repository contains thousands of natural training examples:

```
For each historical commit that changes a function signature:
  1. Run diff_impact at parent commit on changed symbols
  2. Blast radius at parent = candidate set
  3. Files actually modified in commit = must_change (positive)
  4. Files modified in follow-up fix commits ≤48h = late_positive
  5. Remaining = weak negative (may be should_verify, not skip)
```

This produces thousands of labeled (change_context, candidate, outcome) triples per repository. Run across the user's repos plus OSS for scale.

### Agent Observatory Traces (Eval Set, Not Training Set)

Observatory traces have fundamental labeling problems for training:

1. **"Agent used" ≠ "needed."** Agent's git diff reflects what it changed, not what it needed to see. Files read for understanding are labeled negative — wrong.

2. **Survivorship bias.** If the agent never saw an item (truncated, tool not called), it couldn't have edited it. Labeling it negative trains Laya to reproduce the agent's blind spots.

3. **Feedback loop.** Once Laya ranks, the agent follows the ranking, confirming it. The model reinforces its own errors without exploration.

**Use traces for evaluation, not training.** Traces reflect real agent behavior and are the right test set for measuring whether Laya's ranking actually helps.

### Label Specification

For sessions with terminal outcome ∈ {tests_pass, merged}:

| Label | Definition | Signal |
|-------|-----------|--------|
| `must_change` | Symbol span overlaps a hunk in final diff, or in follow-up commit ≤48h on same branch | Strong positive |
| `should_verify` | File was Read by agent (Read / `file_overview` appears in trace) but not changed | Informative — agent judged worth inspecting |
| `skip` | Shown to agent, not Read, not changed | Weak negative (noisy) |

**Critical:** Train and evaluate only on the residual — items that went to Laya. EXTRACTED depth-1 callers are in the diff ~100% of the time. Including them inflates accuracy ("EXTRACTED → positive" is trivial) without adding signal.

### Label at Symbol Level, Not File Level

"File in diff" is too coarse when blast radius is symbol-level. A one-line unrelated edit in a 2000-line file poisons the label. Use hunk ranges overlapping the dependent symbol's span for positive labels.

### Fine-Tuning Infrastructure

| Aspect | Detail |
|--------|--------|
| Training hardware | 2x T4 GPUs (Kaggle free tier) or M-series MacBook with PyTorch MPS |
| Training time | ~4-5 hours for 4 epochs over ~30k questions |
| Framework | Upstream Laya PyTorch (RLCD training pipeline) |
| Post-training | Temperature calibration per (question_type, option_count) to achieve ECE < 0.1 |
| Deployment | Convert to MLX: `laya-mlx convert` → deploy to sidecar |
| Dataset size target | ≥5000 labeled examples per template from git history |

### Shadow Mode (Pre-Activation)

Before activating Laya as ranker:

1. Laya scores every residual item but does NOT affect output ordering
2. Scores logged in Observatory traces alongside actual agent behavior
3. Offline analysis: does Laya ranking correlate with agent usefulness?
4. Activate only when Laya beats deterministic baseline (fan-in × confidence × depth) on recall@token-budget

---

## Runner Integration

### Per-Mode Behavior

Modes have different needs. One model, mode as input feature to threshold selection:

| Mode | Laya Behavior | Rationale |
|------|--------------|-----------|
| **BRIEFING** | Rank-only, names only, no detail compression | Injected every session; agent cannot know what was filtered; highest risk |
| **EXPLORING** | Aggressive detail compression, keep all names | Breadth matters, depth doesn't |
| **RESEARCHING** | Moderate ranking, keep all names | Targeted depth, Laya helps focus on relevant subsystems |
| **PLANNING** | Conservative, full direct blast radius, compress depth ≥3 only | Plan omission propagates through every later mode |
| **IMPLEMENTING** | Full ranking, direct callers complete | Highest Laya value; but also where false negatives most dangerous |
| **VALIDATING** | **No Laya. Unfiltered graph only.** | Independence: if Laya filtered callers AND tests, same error hides bug + the test that catches it. Correlated failure. |
| **FINALIZING** | Not applicable | — |

### Integration Points

```python
# In graphite_bridge.py

def implementing_context(cwd: Path) -> str | None:
    impact = query("diff_impact", changed_files, mode="ranked")  # Laya-annotated
    
    # Always include EXTRACTED depth-1 (deterministic must)
    # Laya-ranked residual follows
    # Nothing hidden — just ordered by attention tag
    
    if not impact or impact.affected_count < 5:
        return None
    return format_ranked_impact(impact)

def targeted_tests(cwd: Path) -> list[str]:
    changed_files = get_changed_files(cwd)
    tests = query("test_coverage", changed_files, mode="ranked")  # Laya-ranked
    
    # Run order: directly_tests → exercises_path → tangential → unrelated
    # All run — Laya only determines order
    return tests.ordered_by_relevance()
```

### MCP Tool Changes

Two tools gain Laya annotation: `blast_radius` and `diff_impact`. All others unchanged.

```
# Existing tool, new optional parameter
blast_radius:
  input:
    symbol: string
    mode: "full" | "ranked"    # NEW — default "full" (no Laya)
  output:
    items: [...]               # same structure
    triage: {...}              # NEW — present only when mode="ranked" and sidecar available
```

### Observatory Schema Extension

Current trace relation:
```
trace(session, ts, tool, args, result_tokens, source_tokens, latency_ms, symbols)
```

Extended for Laya:
```
trace(session, ts, tool, args, result_tokens, source_tokens, latency_ms, symbols,
      triage_applied, triage_engine, triage_model_sha, triage_template,
      candidates_count, must_count, verify_count, skip_count,
      triage_latency_ms)
```

Without these fields, you cannot measure false negative rate or evaluate whether Laya is helping.

### Online Monitoring: Filtered-Then-Accessed Rate

Track: agent later Reads or edits a file that Laya tagged as `skip`. This is the **false negative signal**.

```
FN_rate = count(skip items later accessed) / count(skip items)
```

A rising rate per template or per mode means threshold is too aggressive. Alert and widen.

---

## Deterministic Baseline (Build First)

Before any ML, build a deterministic ranker using graph metrics:

```rust
pub fn deterministic_score(item: &BlastRadiusItem) -> f32 {
    let mut score = 0.0;
    
    // Edge confidence
    score += match item.edge_confidence {
        Confidence::Extracted => 1.0,
        Confidence::Inferred => 0.5,
    };
    
    // Depth (closer = more important)
    score += 1.0 / (item.depth as f32);
    
    // Fan-in (more dependents = more central)
    score += (item.fan_in as f32).ln().min(3.0) / 3.0;
    
    // Co-change frequency from git history
    score += item.co_change_frequency.min(1.0);
    
    // Test adjacency (file has associated test = more important)
    if item.has_test_coverage { score += 0.3; }
    
    score
}
```

This captures 70-80% of the value with zero ML, zero latency, zero dependency. Laya must beat this baseline on recall@token-budget to earn activation.

---

## Estimated Impact

### Per-Task Gains

| Metric | Without Laya | With Laya (ranked) | Gain |
|--------|-------------|-------------------|------|
| Blast radius investigation turns | ~15 | ~6 | -60% |
| INFERRED edge investigation turns | ~10 | ~3 | -70% |
| Time to first test failure | Minutes | Seconds | Order of magnitude |
| Total turns per task | ~20 | ~12 | -40% |
| Total wall-clock per task | ~15 min | ~8 min | -45% |
| Token spend per task | Baseline | -40% estimated | Significant |
| Correctness | Baseline | Same or better (INFERRED edge validation) | Neutral to positive |
| Assertiveness | Baseline | Higher (scored confidence, not flat list) | Positive |

### Where Gains Come From

Not from Laya's 13ms latency (irrelevant vs agent turn time). From **turns eliminated** — agent stops investigating irrelevant items and acts on the right ones first.

---

## Implementation Sequence

```
Phase 1: Instrument
  - Add triage fields to Observatory trace schema
  - Measure blast_radius token distribution (is "too much context" real?)
  - Measure agent Read patterns on blast radius items
  - Measure turn counts per task

Phase 2: Deterministic Ranker
  - Implement deterministic_score() using edge confidence, depth, fan-in, co-change
  - Ship tiered output (top-k detailed, rest one-liner, footer with totals)
  - Measure improvement vs flat output

Phase 3: Git History Mining
  - Run diff_impact on historical commits to generate training labels
  - Target ≥5000 examples per template
  - Build evaluation set from Observatory traces

Phase 4: Laya Shadow Mode
  - Fine-tune Laya on git history labels
  - Deploy sidecar, score everything, affect nothing
  - Compare Laya ranking vs deterministic baseline on recall@token-budget

Phase 5: Laya Activation (only if beats baseline)
  - Enable ranked mode as opt-in on blast_radius / diff_impact
  - Monitor filtered-then-accessed rate
  - Never gate VALIDATING or EXTRACTED depth-1 callers

Phase 6: Candle Port (future, conditional)
  - Only with trusted fine-tuned checkpoint
  - Only with Linux user demand
  - ModernBERT exists in candle-transformers; Laya decision heads are small MLPs
  - Expect 2-3x slower than MLX (~30-40ms) but still invisible vs agent turns
```

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Laya zero-shot is useless (0.35) | Cannot ship without fine-tune | Git history mining for bootstrap; shadow mode before activation |
| 512-token truncation produces arbitrary decisions | Silent bad scores | Truncate at AST node boundaries; log truncation events; flag items scored under truncation |
| Template rewording invalidates fine-tune | Retrain needed | Version templates; pin template_version in trace |
| Feedback loop (Laya ranking → agent follows → confirms ranking) | Model reinforces own errors | Exploration policy: bypass ranking on X% of sessions; shadow mode for clean eval data |
| Sidecar dies mid-session | Ranking disappears | `NoopTriage` fallback; 50ms timeout per call; logged but silent degradation |
| Apple Silicon only (MLX) | Linux devs get no ranking | Explicit: Laya is macOS-first accelerator; candle port is Phase 6 |
| Correlated failure (ranking callers AND tests) | Same error hides bug + test | VALIDATING never uses Laya; test ranking is execution order only, never drops |
| Model version drift | Same query, different ranking between retrains | Pin model_sha in every response and trace |
| Cold start latency | 2-5s first inference | `laya-serve` warms up on start; Graphite never blocks on cold sidecar |
| Co-change frequency may capture most value without ML | Laya adds little | This is fine — deterministic baseline wins, ML is optional accelerator |

---

## Not Scope

- Laya does not replace any existing MCP tool. It annotates output of `blast_radius` and `diff_impact` only.
- Laya does not run in the indexing/write path. Graph construction stays deterministic.
- Laya does not filter VALIDATING mode. Independence required.
- Laya is not required for Graphite to function. Optional accelerator with graceful degradation.
- No cloud, no telemetry, no network calls from Laya. Local inference only.
- No Jev API integration. Laya is the chosen engine (faster, open, fine-tunable).

## Alternatives Considered

### TypeSafe Jev (API)

Rejected: 18x slower (236ms vs 13ms), proprietary, $0.042/M tokens, cannot fine-tune on domain data, requires network. Violates Graphite's privacy principle.

### Jev Open-Source Clones (OpenJev/SemIf, LitJev, NanoJev, Kev-0.5B)

Qwen-based autoregressive models repurposed for classification. Higher parameter counts (0.5B-9B) for similar or worse accuracy. Not purpose-built for typed decisions. Laya's encoder architecture is fundamentally better suited (single forward pass, no text generation).

### No ML (Deterministic Only)

Viable and recommended as Phase 2 baseline. Expected to capture 70-80% of value. Laya is the optional Phase 5 upgrade for the remaining 20-30%, specifically on INFERRED edge validation where deterministic signals are weakest.

### Depth-Limited Blast Radius

Simpler alternative: `blast_radius --depth 3`. Captures most relevant items but discards structural information about deeper dependencies. Not a ranking — a hard cutoff. Works for simple cases, fails when a critical dependency is at depth 4+.

## Sources

- [Laya GitHub](https://github.com/NandhaKishorM/laya) — model code, training pipeline, benchmarks
- [Laya Documentation](https://laya.convaiinnovations.com/) — API reference, checkpoints, fine-tuning guide
- [Laya-MLX GitHub](https://github.com/mizorewww/laya-mlx) — Apple Silicon native port
- [Laya-MLX PyPI](https://pypi.org/project/laya-mlx/) — installation
- [Laya-MLX HuggingFace](https://huggingface.co/aac6fef/laya-mlx) — pre-converted checkpoints
- [JevBench v1.3](https://benchmarkheaven.com/jev-models) — comparative benchmark (52 systems)
- [TypeSafe Jev Docs](https://docs.typesafe.ai/introduction) — System One model reference
- [Jev Alternatives Survey](https://www.latent.space/p/ainews-here-are-6-clones-of-jev-in) — 6+ open-source clones
- [Jev + Claude Code Routes](https://apimaster.ai/blog/jev-claude-code-codex) — four integration patterns
- [fast-jev-compaction](https://github.com/tamaratran/fast-jev-compaction) — context compaction plugin
- [Laya arXiv Paper](https://arxiv.org/abs/2503.23303) — original research (March 2025)
- Opus 5.5 consultation (2026-09-23) — risk analysis, deterministic baseline, implementation sequence
- Fable 5.1 consultation (2026-09-23) — template taxonomy, labeling spec, sidecar architecture, scope boundary
