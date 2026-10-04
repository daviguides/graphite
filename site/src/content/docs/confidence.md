---
title: Edge Confidence
order: 5
---

Every edge in Graphite carries a confidence tier and categorical provenance. This is the honesty mechanism: the agent knows when to trust the graph and when to verify.

## Three tiers

### EXTRACTED

The edge was parsed directly from the source AST. A static function call, an explicit import statement, a type annotation, a class inheritance declaration.

```
EXTRACTED  rover/git_ops.py:142  rebase_onto_base()
  calls rebase_main_onto_current (direct call in AST)
```

The agent can trust this completely.

### INFERRED

The edge was derived by rule from context. A duck-typed call resolved to the most likely target based on name, argument count, and module scope. Marked with provenance explaining the reasoning.

```
INFERRED   runner/engine.py:245  execute_task()
  calls pre_hook (resolved via call chain, target confirmed)
```

The agent should generally trust this, but the provenance is available if verification is needed.

### AMBIGUOUS

Multiple possible targets exist. Dynamic dispatch, reflection, plugin registries, generated code. The graph says "I don't know which one" instead of guessing.

```
AMBIGUOUS  dispatch/router.py:89  handle()
  calls ??? (dynamic dispatch, 4 possible targets)
  targets: AuthHandler.handle, UserHandler.handle,
           BillingHandler.handle, AdminHandler.handle
```

The agent should verify with a file read. Ambiguous edges can be resolved with overrides.

## Provenance

Every INFERRED and AMBIGUOUS edge carries provenance: the rule that derived it and the evidence it used. Provenance flows into query responses so the agent can judge for itself.

## In responses

Query responses include an `epistemic` field that summarizes the confidence landscape:

- How many edges were EXTRACTED vs INFERRED vs AMBIGUOUS
- Whether the result is a lower bound (some paths could not be fully resolved)
- What causes the incompleteness

This lets the agent calibrate its trust level per query, not per edge.

## Overrides

Ambiguous edges can be resolved by human curation. The `override` relation (stored in `.graphite/overrides.toml`) lets you tell the graph about edges it can't see:

- Add an edge: "`Router.dispatch` calls every `*Handler.handle`"
- Suppress an edge: "ignore `log::*` as a dependency"
- Tag a boundary: mark modules as `public-api`, `internal`, `deprecated`

Each override makes every future agent session more accurate.
