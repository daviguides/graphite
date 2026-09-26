# Graphite — Interception & Output Format

> Decisions of 2026-09-27. The agent keeps its habits; Graphite answers them.

Every rule here is derived from the three targets (speed > assertiveness > correctness) and carries its reasoning. Revise any of them when evidence shows it hurts a target.

Prototype code lives on branch `feat/interception` (not `main`) until it is adjusted to these decisions and the user approves it.

## 1. Steering by transparent interception

**Decision.** The agent keeps its trained search/read habits — `grep`, `rg`, `ack`, `find`, `ls`, `cat`, `sed -n`, `head`, `tail`. A Claude Code **PreToolUse** hook rewrites read-only commands through `updatedInput` (the RTK mechanism) so Graphite executes them and answers. The agent gets the answer as the output of its own command.

**Why.**
- Instruction-only steering measured ~0% uptake (code-graph-mcp). Our own pilot B confirmed it: one task never called Graphite (0/2), and in all 8 runs that did, the agent still grepped after a complete answer (1–3 extra searches).
- These commands are trained behavior (pre-training + agentic RL), not just prompt compliance; one prompt line cannot outweigh them. Meeting the habit is cheaper than fighting it.

**Guardrails (correctness).**
- Only commands whose every segment is read-only are rewritten; anything that writes, redirects to files, uses `$(…)`/subshells/heredocs, or targets paths outside the repo runs untouched.
- No permission bypass: the hook sets no `permissionDecision`; normal checks apply.
- Fail-open: any daemon error or timeout runs the original command. Hooks never start a daemon and never break a tool call. grep exit codes are preserved.
- Hooks live in the repo's `.claude/settings.json` (plugin `hooks.json` only honors SessionStart), installed by `graphite hooks install`, preserving foreign hooks (e.g. RTK) and the file's key order. PostToolUse on writes nudges the daemon.

## 2. The answer: an enriched grep

**Decision.** For an intercepted search, Graphite returns an **integrated** list — not a ripgrep block plus a graph block:
- every real ripgrep match appears **exactly once**, annotated by the graph: `[definition]`, call site `← enclosing fn ← its callers`, import, `[mock in test]`, `[docs]`, `[string/comment]`, `[unresolved call]`, `[other language]`;
- matches the graph knows but text search cannot see (e.g. aliased calls) go **into the same list**, tagged `[graph-only]`;
- lines are dropped only when the graph **provably** explains them (e.g. imports counted, not listed), and every drop is disclosed;
- one short **header** line: target, **completeness verdict** (COMPLETE / LOWER-BOUND + causes), counts, risk;
- one short **footer** line: indirect impact summary, covering tests (or "none ⚠"), overrides, omissions.

**Why.**
- The agent asked a textual question; the answer keeps its shape and adds what each line *is* — the edit/ignore/update decision happens per line, where the annotation sits.
- One occurrence per location avoids duplicate lines and cross-referencing two lists (tokens + confusion).
- The completeness verdict is what grep can never give; it is what removes the post-answer "confirmation grep" seen in pilot B.

## 3. Output format — decided by experiment

**Decision.** Self-contained `path:line:` lines (the `grep -rn` shape) + one header + one footer.

**Evidence.** Phase-1 experiment ([output-format-eval-phase1.md](../../references/studies/output-format-eval-phase1.md)): 6 real Continuum cases (graph + real ripgrep residue), 9 formats, identical content, Sonnet × 3 repeats, deterministic scoring, US$3.13.

| Format | Accuracy | Input tokens | Median latency |
|---|---|---|---|
| **Lines (`grep -rn` shape)** | **100%**, zero variance | **5,086** | **5.7 s**, fewest output tokens |
| XML sections | 100% | 5,816 | 6.1 s |
| JSON | 98.9% | 6,082 | 6.3 s |
| YAML | 98.1% | 6,293 | 6.1 s |
| Edge list | 97.2% | 6,454 | 7.0 s |
| Tree | 95.8% | 4,738 | 5.7 s |
| Mermaid | 95.0% (only significantly worse) | 5,942 | 5.9 s |
| Prose | 94.4% | 4,707 | 6.4 s |
| Grouped by file (`rg --heading`) | 93.1% (least stable) | 4,226 | 5.7 s |

- Near-ceiling for all formats: once the graph organizes and annotates the data, comprehension barely depends on format. No errors came from reading graph relations (zero direction flips, zero class confusions).
- Lines win on accuracy, stability, tokens among 100% formats, latency, familiarity, and **filter robustness by construction** (each line carries its own path).
- **Phase 2 skipped:** it would test survival under `| grep -v test`; lines survive by construction and no outcome would change the decision.
- The dominant speed factor in an agent is turns, not reading time: one avoided confirmation grep (5–10 s) outweighs every latency difference in the table. That is measured only in the agent bench (arm C).
- Found by the experiment: for multi-definition names the answer must tag each reference with the **`path:line` of the definition it resolves to**, not just the short name.

## 4. Size: budget, never byte-cut

**Decision.** A size limiter in the agent's pipe (`| head -c N`, `| head -n N`, `| tail -n N`) is read as a **budget**, not applied as a cut. Graphite composes the answer within it: header + direct sites first, the rest summarized, every cut disclosed with how to get more.

**Why.** Pilot B showed agents cutting 14–25 KB one-line JSON with `head -c 3000/6000`, destroying dependents and verdict, then grepping to recover. The intent ("don't flood my context") is honored; the mechanism (mid-item cut) is not.

## 5. Filters by intent

| Pipe after the search | Intent | Graphite does |
|---|---|---|
| `head`, `tail`, `head -c` | Protect context | Budget (§4) |
| `grep -v test`, `grep -v /tests/` | Production code only | Semantic prod-only filter (the graph knows test vs prod) + disclosure: "N test call sites / M mocks omitted" — mocks break on signature changes, so they are never silently gone |
| Other content filters (`grep foo`, `grep -v bar`) | Unknown to the graph | Applied for real on the self-contained lines (safe because every line carries its path) |
| Transformations (`wc`, `sort`, `uniq`, `cut`, `awk`, `xargs`) | Compute something | Pass the raw command through, no enrichment |

Always one notice line when Graphite changes what was asked (e.g. "graphite: `| head -c 3000` applied as answer budget").

## 6. Embedded ripgrep

**Decision.** Search runs **inside the daemon** with ripgrep's crates (`ignore`, `grep-searcher`, `grep-regex`) — no subprocess. It respects `.gitignore`, skips hidden directories and the indexer's default excludes (`.git`, `.graphite`, `.venv`, `__pycache__`, `node_modules`, `target`, `dist`, `build`, caches…), searches them anyway when the command explicitly targets them, and discloses omitted counts.

**Why.**
- Speed: parallel, gitignore-aware; the user measured `rg` instant where `grep -r` was slow; prototype: ~180 ms end to end vs 480 ms for plain grep on a 391 MB copy.
- Correctness of the answer: `grep -r` returns `__pycache__` binary matches and agent worktree copies (205 of 270 matches in one sample were `.claude/worktrees/` copies).
- Plain `rg` in a pipe prints **no line numbers**; intercepted answers always include them.

## 7. Surfaces and formats

| Surface | Question type | Format |
|---|---|---|
| Intercepted `grep`/`rg`/`cat`/`find`/`ls` | Textual ("where does X appear?") | Enriched grep lines (§2–3); reads get a one-line graph header before the real content, listings a short module summary after |
| Explicit `graphite blast` / `diff-impact` | Structural ("what breaks?") | Graph-centric text: verdict, call sites with lines, indirect by module, tests, overrides |
| Programs (runner, frontend) | Full data | `--json` |

## 8. Human vs model rendering

**Decision.**
- **One canonical record, several renderers**, with a parity test that all renderers carry the same content — so what a human debugs is exactly what the agent saw.
- **Default output = the model format**, byte for byte what the agent receives. No TTY auto-switch: the default always equals the agent's view.
- `--human`: same content grouped by file (the `rg --heading` shape), colored, with legends. The format that lost for the model is the one that reads best for people.
- `--json`: the full record.
- `--explain`: why each line got its class, which resolution tier bound a reference, why an answer is LOWER-BOUND.
- Real sessions: every interception is logged to `.graphite/hooks.jsonl`; `graphite hooks log` lists them and `graphite hooks show <n>` shows side by side the original command, the model answer, the `--human` render and `--explain`.

## 9. Roadmap impact

Interception and the enriched grep move **into v1.0**: v1.0's own "rethink if" rule fired — without steering the agent did not use Graphite reliably. The thesis is now tested by the **arm C** pilot (new format + interception) against arms A (no Graphite) and B (CLI + prompt line, old format). See [features.md](features.md).
