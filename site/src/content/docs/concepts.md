---
title: Concepts
order: 2
---

Core concepts behind Graphite's design.

## Optimization targets

Three targets, in priority order. Every design decision is measured against them.

1. **Agent execution speed.** Total wall-clock time the agent takes to finish a task. Not query latency. Every file read eliminated, every turn skipped, every exploration phase removed.

2. **Assertiveness.** The agent acts with confidence because it has the right context upfront. No "let me check a few more files," no hesitation, no backtracking. Edge confidence tells the agent when to trust the graph and when to verify.

3. **Correctness.** The agent makes fewer mistakes because blast radius is visible, dynamic dispatch flows are mapped, framework routes are connected, and nothing is silently missed.

## Edge confidence

Every edge in the graph carries a confidence tier and provenance.

| Tier | Meaning | Example |
|------|---------|---------|
| **EXTRACTED** | Parsed directly from source AST | Static call, explicit import, type annotation |
| **INFERRED** | Derived by rule from context | Duck-typed call resolved to likely target |
| **AMBIGUOUS** | Multiple possible targets | Dynamic dispatch, reflection, generated code |

Responses disclose when a result is a lower bound and why. The agent knows when to trust the graph and when to verify with a file read.

## Blast radius

Everything that transitively depends on a symbol. Change `function_a` and the blast radius tells you every function, class, and test that could break.

- Default depth: 3 (configurable)
- Source code returned inline with line numbers
- Covering tests identified automatically
- Prod/test partition in results

## Symbols and edges

Graphite extracts:

- **Symbols**: functions, classes, structs, traits, interfaces, types, constants, enums
- **Edges**: calls, imports, implements, references, contains

Each symbol gets a deterministic ID (BLAKE3 hash of file + name + kind + line). Each edge links two symbols with a kind and confidence tier.

## Source inline

Every query response returns verbatim source code with line numbers alongside graph context. The agent gets the answer AND the code in one call; no follow-up `Read` needed.

## Transparent interception

PreToolUse hooks rewrite the agent's habitual read-only commands (`grep`, `find`, `cat`, `head`, `tail`) into enriched graph-aware answers. The agent doesn't need to know Graphite exists; it gets structural context transparently.

The enriched response includes:
- Standard `path:line:` format the agent expects
- Graph annotations (callers, confidence, blast radius markers)
- Verdict header summarizing what the graph knows
- Footer with graph revision and staleness indicator
