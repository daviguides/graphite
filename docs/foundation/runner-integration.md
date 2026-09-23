# Graphite — Runner Integration

> How Graphite accelerates the Continuum runner orchestration layer.

Runner drives Claude Agent SDK sessions through workflow modes (BRIEFING → EXPLORING → RESEARCHING → PLANNING → IMPLEMENTING → VALIDATING → FINALIZING). Graphite provides surgical codebase context that reduces total execution time, increases assertiveness, and improves correctness.

## The Problem Runner Has Today

Runner composes prompts by embedding resume files inline (`task_resume_markdown()`). Each SDK session starts with task state + full file contents. The agent then spends turns calling Read/Grep to understand codebase structure before acting.

```
Today:
  compose_prompt() → embed plan.md, todo.md, validation.md
  SDK session starts → agent reads 20-40 files to understand architecture
  Agent hesitates → "let me check a few more files"
  Agent acts → some changes miss blast radius
  VALIDATING → runs full make test (all tests, not just affected)
```

## How Graphite Changes This

Two integration layers — upfront injection AND mid-session access:

```
With Graphite:
  Layer 1 (upfront): compose_prompt() → inject Graphite context map + source code
  Layer 2 (mid-session): SDK has Graphite MCP as tool source → agent queries on-demand

  Result:
  SDK session starts → agent already has dependency graph + verbatim source
  Agent acts immediately → no exploration phase, no hesitation
  Agent queries Graphite mid-work → blast radius of what it just changed
  VALIDATING → targeted tests first (Graphite identifies them)
```

## Two Integration Layers

### Layer 1: Prompt Injection (upfront context)

Runner pre-computes Graphite context and injects into prompt. Agent starts with full picture. Reduces turns at session start.

### Layer 2: MCP Tool Access (mid-session queries)

Runner registers Graphite MCP server as tool source for SDK session. Agent can query blast radius, dependents, communities mid-execution — without runner composing anything.

```python
# In sdk_wrapper.py — register Graphite MCP alongside gradient plugins
sdk = SDKWrapper(
    cwd=cwd,
    plugins=plugins,
    mcp_servers=[{"name": "graphite", "command": "graphite", "args": ["serve", "--mcp"]}],
    # ...
)
```

Layer 1 gives speed (zero turns to get context). Layer 2 gives assertiveness (agent queries exactly what it needs mid-task).

## Integration Points by Mode

### EXPLORING — Architecture Map

**Problem:** Agent reads 20-40 files blindly to understand codebase.
**Solution:** Inject full architecture overview upfront.

```python
def exploring_context(cwd: Path) -> str | None:
    # Top-level module structure with cross-references
    modules = query("module_map")
    # Community boundaries (Leiden clustering)
    communities = query("communities")
    # Most connected symbols (architectural pillars)
    pillars = query("hot_symbols", top_n=20)
    # Coupling between modules
    coupling = query("coupling_matrix")
```

**Impact:** EXPLORING completes in 5-10 turns instead of 15-25. Agent understands subsystem boundaries before reading a single file.

### RESEARCHING — Subsystem Analysis

**Problem:** No integration today. Agent researches by reading files.
**Solution:** Inject community detection + shortest paths.

```python
def researching_context(task_data: dict, cwd: Path) -> str | None:
    # Which communities are touched by this task's scope?
    scope_files = get_task_scope_files(task_data)
    communities = query("affected_communities", scope_files)
    
    # How do affected subsystems connect?
    paths = query("paths_between_communities", communities)
    
    # Edge confidence: where should agent verify vs trust?
    confidence = query("confidence_summary", scope_files)
```

**Impact:** Agent understands which subsystems are involved and how they connect. Confidence tags tell it where to investigate deeper vs where to trust the graph.

### PLANNING — Decomposition Intelligence

**Problem:** No integration today. Agent plans based on file reads.
**Solution:** Inject coupling matrix + module boundaries for task decomposition.

```python
def planning_context(task_data: dict, cwd: Path) -> str | None:
    scope_files = get_task_scope_files(task_data)
    
    # Which modules are coupled? (affects decomposition strategy)
    coupling = query("coupling_matrix", scope_files)
    
    # Module boundary violations in current code
    violations = query("boundary_violations")
    
    # Circular dependencies in scope
    cycles = query("cycles", scope_files)
```

**Impact:** Agent decomposes work along real module boundaries, not arbitrary file groupings. Cycles and violations are visible before planning, not discovered mid-implementation.

### IMPLEMENTING — Blast Radius + Source Context

**Problem:** Agent misses blast radius. Changes break downstream.
**Solution:** Inject blast radius with verbatim source code.

```python
def implementing_context(cwd: Path) -> str | None:
    changed_files = get_changed_files(cwd)
    
    # Combined diff impact: changed files + blast radius + source
    impact = query("diff_impact", changed_files)
    # Returns: affected symbols with line-numbered source,
    # edge confidence, community boundaries crossed
    
    if not impact or impact.affected_count < 5:
        return None
    
    return format_impact_warning(impact)
```

**Key insight from landscape:** return source code inline (CodeGraph's approach). Agent doesn't need Read to see the code — Graphite already has it. Edge confidence tells agent where to be careful vs confident.

Plus Layer 2: agent can query `blast_radius` mid-session after each change to see live impact.

### VALIDATING — Targeted Tests + Correctness Checks

**Problem:** `make test` runs full suite. Slow feedback.
**Solution:** Graphite identifies which test files cover changed symbols.

```python
def targeted_tests(cwd: Path) -> list[str] | None:
    changed_files = get_changed_files(cwd)
    
    # Test files that import/reference symbols from changed files
    relevant_tests = query("test_coverage", changed_files)
    
    # Framework routes affected (Express/Django/Axum handlers)
    affected_routes = query("affected_routes", changed_files)
    
    return relevant_tests
```

**Integration in pre_hook_validation:**

```python
targeted = targeted_tests(cwd)
if targeted:
    # Run targeted tests first (fast feedback, seconds not minutes)
    run_targeted(targeted)
    # Full suite as safety net
    run_full("make test")
```

**Impact:** Fast feedback on relevant tests. Framework route awareness catches integration issues CodeGraph-style.

## Consolidated Query: diff_impact

Instead of multiple queries per mode, one consolidated query:

```bash
graphite diff-impact --json
```

Returns everything at once:
- Changed files (from git diff)
- Changed symbols with verbatim source
- Blast radius (transitive dependents) with depth
- Affected communities
- Affected test files
- Affected routes (framework-aware)
- Edge confidence summary
- Boundary crossings

Runner calls this once per mode, formats per mode's needs.

## Communication Protocol

### Primary: MCP (when server running)

Runner connects to Graphite MCP server directly. No subprocess overhead.

```python
# graphite_bridge.py
import asyncio
from mcp import ClientSession, StdioServerParameters

async def query_mcp(tool: str, args: dict) -> dict:
    """Query Graphite via MCP protocol."""
    server = StdioServerParameters(command="graphite", args=["serve", "--mcp"])
    async with ClientSession(server) as session:
        result = await session.call_tool(tool, args)
        return result
```

### Fallback: CLI subprocess

When MCP server not running (CI, one-off use):

```bash
graphite diff-impact --json
graphite blast src/auth/token.rs --json --depth 3
```

### Future: PyO3 in-process

Eliminate all IPC. Graphite as Python library via PyO3 bindings:

```python
import graphite
db = graphite.open("graphite.db")
impact = db.diff_impact()
```

## Architecture

```
runner/core/
├── graphite_bridge.py     # All Graphite interaction
│   ├── is_available()     # MCP server running? graphite.db exists?
│   ├── query_mcp()        # Primary: MCP protocol
│   ├── query_cli()        # Fallback: subprocess
│   ├── diff_impact()      # Consolidated query for any mode
│   ├── exploring_context()# Architecture map
│   ├── researching_context()  # Communities + paths
│   ├── planning_context()     # Coupling + cycles
│   ├── implementing_context() # Blast radius + source
│   ├── targeted_tests()       # Test files for VALIDATING
│   └── register_mcp()    # Register as SDK tool source (Layer 2)
```

## Expected Impact

| Metric | Without Graphite | With Graphite |
|--------|-----------------|---------------|
| EXPLORING turns | 15-25 (blind reads) | 3-8 (map + communities pre-loaded) |
| RESEARCHING turns | 10-15 | 5-8 (subsystem analysis upfront) |
| PLANNING quality | File-based decomposition | Module-boundary-aware decomposition |
| IMPLEMENTING blind spots | Agent misses blast radius | Blast radius + source inline + mid-session queries |
| VALIDATING feedback | Full suite (~minutes) | Targeted tests (~seconds) + full suite |
| Token spend per task | Baseline | Est. 30-50% reduction |
| Total task wall-clock | Baseline | Est. 40-60% reduction |

## Not Scope (for this integration)

- Runner does NOT depend on Graphite. Optional enhancement only.
- Graphite does NOT know about runner internals. It exposes generic tools.
- No changes to runner's workflow definitions, gate system, or SDK wrapper.
- No changes to dao-cli state management.
- PyO3 bindings are future optimization, not v1.
