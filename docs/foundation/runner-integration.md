# Graphite — Runner Integration

> How Graphite accelerates the Continuum runner orchestration layer.

Runner drives Claude Agent SDK sessions through workflow modes (BRIEFING → EXPLORING → RESEARCHING → PLANNING → IMPLEMENTING → VALIDATING → FINALIZING). Graphite provides surgical codebase context that reduces token spend, turns, and blind exploration.

## The Problem Runner Has Today

Runner composes prompts by embedding resume files inline (`task_resume_markdown()`). Each SDK session starts with task state + full file contents. The agent then spends turns calling Read/Grep to understand codebase structure before acting.

```
Today:
  compose_prompt() → embed plan.md, todo.md, validation.md
  SDK session starts → agent reads 20-40 files to understand architecture
  Agent acts → some changes miss blast radius
  VALIDATING → runs full make test (all tests, not just affected)
```

## How Graphite Changes This

```
With Graphite:
  compose_prompt() → embed resume files + Graphite context map
  SDK session starts → agent already has dependency graph
  Agent acts → blast radius visible, fewer blind spots
  VALIDATING → runs relevant tests first (Graphite identifies them)
```

## Integration Points

### 1. Prompt Injection (compose_prompt)

**Where:** `runner/core/prompt.py:compose_prompt()`
**When:** Every mode, before SDK execution

Query Graphite for context relevant to the task's scope:

```python
# runner/core/graphite_bridge.py

def inject_context(task_data: dict, cwd: Path) -> str | None:
    """Query Graphite for task-relevant context.
    
    Returns markdown section or None if Graphite unavailable.
    """
    if not is_available(cwd):
        return None
    
    changed_files = get_changed_files(cwd)
    if not changed_files:
        return None
    
    sections = []
    
    # Blast radius of changed files
    blast = query("blast_radius", changed_files)
    if blast:
        sections.append(f"### Blast Radius\n\n{blast}")
    
    # Overview of changed files (symbols, imports, exports)
    overview = query("file_overview", changed_files)
    if overview:
        sections.append(f"### File Overview\n\n{overview}")
    
    # Hot symbols (most-depended-on in affected area)
    hot = query("hot_symbols", changed_files, top_n=10)
    if hot:
        sections.append(f"### High-Impact Symbols\n\n{hot}")
    
    if not sections:
        return None
    
    header = (
        "## Code Map (via Graphite)\n\n"
        "The following dependency context was pre-computed. "
        "Use it to understand impact before reading files.\n\n"
    )
    return header + "\n\n".join(sections)
```

**Integration in compose_prompt:**

```python
# After task_resume_markdown(), before mode boundary section
graphite_context = inject_context(task_data, worktree_path)
if graphite_context:
    prompt += f"\n\n{graphite_context}"
```

### 2. EXPLORING Mode — Architecture Map

**Where:** `runner/core/prompt.py:compose_prompt()`, mode-conditional
**When:** EXPLORING mode only

EXPLORING is where agents read "blindly". Inject a pre-computed architecture overview:

```python
def exploring_context(cwd: Path) -> str | None:
    """Full architecture overview for EXPLORING mode."""
    if not is_available(cwd):
        return None
    
    # Top-level module structure with cross-references
    modules = query("module_map")
    
    # Most connected symbols (architectural pillars)
    pillars = query("hot_symbols", top_n=20)
    
    # Coupling between modules
    coupling = query("coupling_matrix")
    
    # ...format as markdown
```

**Effect:** Agent skips 10-15 Read/Grep calls. EXPLORING completes in fewer turns.

### 3. VALIDATING Mode — Targeted Tests

**Where:** `runner/core/hooks.py:pre_hook_validation()`
**When:** Before `make test`

Query Graphite for test files that import symbols from changed files:

```python
def targeted_tests(cwd: Path) -> list[str] | None:
    """Find test files covering changed symbols."""
    if not is_available(cwd):
        return None
    
    changed_files = get_changed_files(cwd)
    
    # Which test files import/reference symbols from changed files?
    relevant_tests = query("test_coverage", changed_files)
    
    return relevant_tests  # e.g. ["tests/test_auth.py", "tests/test_token.py"]
```

**Integration in pre_hook_validation:**

```python
# Before running make test
targeted = targeted_tests(cwd)
if targeted:
    # Run targeted tests first (fast feedback)
    run_targeted(targeted)  # pytest specific files
    # Then run full suite as safety net
    run_full("make test")
```

**Effect:** Fast feedback on relevant tests. Full suite still runs but failures in unrelated tests don't block early signal.

### 4. IMPLEMENTING Mode — Blast Radius Guardrails

**Where:** Prompt injection, IMPLEMENTING-specific
**When:** IMPLEMENTING mode only

```python
def implementing_guardrails(cwd: Path) -> str | None:
    """Blast radius warning for IMPLEMENTING mode."""
    changed_files = get_changed_files(cwd)
    blast = query("blast_radius", changed_files)
    
    if not blast or blast.affected_count < 5:
        return None
    
    return (
        "## Blast Radius Warning\n\n"
        f"Your changes affect {blast.affected_count} files "
        f"across {blast.module_count} modules.\n\n"
        f"High-impact symbols:\n{blast.hot_list}\n\n"
        "Check these modules for regressions before finishing."
    )
```

## Architecture

```
runner/core/
├── graphite_bridge.py     # All Graphite interaction
│   ├── is_available()     # graphite.db exists in cwd?
│   ├── query()            # CLI call: graphite <tool> --json
│   ├── inject_context()   # Generic context for any mode
│   ├── exploring_context()# Architecture map for EXPLORING
│   ├── targeted_tests()   # Test files for VALIDATING
│   └── implementing_guardrails()  # Blast radius for IMPLEMENTING
```

### Communication Protocol

Runner calls Graphite via CLI with JSON output:

```bash
graphite blast src/auth/token.rs --json --depth 3
graphite overview src/auth/ --json
graphite hot-symbols --top 10 --json
graphite test-coverage src/auth/token.rs --json
```

Future: PyO3 bindings for in-process calls (eliminates subprocess overhead).

### Graceful Degradation

```python
def is_available(cwd: Path) -> bool:
    """Check if Graphite is installed and repo is indexed."""
    try:
        result = subprocess.run(
            ["graphite", "stats", "--json"],
            cwd=cwd, capture_output=True, timeout=2,
        )
        return result.returncode == 0
    except (FileNotFoundError, subprocess.TimeoutExpired):
        return False
```

If Graphite is not installed or repo has no `graphite.db`:
- All `graphite_bridge` functions return `None`
- Runner operates exactly as today
- Zero breaking changes, zero new dependencies

## Expected Impact

| Metric | Without Graphite | With Graphite |
|--------|-----------------|---------------|
| EXPLORING turns | 15-25 (blind reads) | 5-10 (map pre-loaded) |
| IMPLEMENTING blind spots | Agent misses blast radius | Blast radius in prompt |
| VALIDATING feedback | Full test suite (~minutes) | Targeted tests (~seconds) + full suite |
| Token spend per mode | Baseline | Est. 20-40% reduction (fewer Read/Grep) |

## Not Scope (for this integration)

- Runner does NOT depend on Graphite. Optional enhancement only.
- Graphite does NOT know about runner internals. It exposes generic tools.
- No changes to runner's workflow definitions, gate system, or SDK wrapper.
- No changes to dao-cli state management.
- PyO3 bindings are future optimization, not v1.
