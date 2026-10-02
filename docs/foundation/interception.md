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

**Shell shapes covered (2026-10-02).** Pilot C passed 47 of 59 Bash commands through; 40 for shape, not intent. Read-only shapes now routed:
- `NAME=$(cmd)` (also `NAME="$(cmd)"`) when `cmd` is one supported read-only segment: it runs for real and its output is stored. Segments using `$NAME` are accepted when their command is read-only whatever the value (`cat`, `sed -n`, `grep`, `wc`, `echo`, …) and are classified again at run time with the value expanded; if they still can't be answered they run as written with the same variables (fail-open). Substitution anywhere else (`echo $(…)`, env prefixes, nesting) stays unsupported.
- Reads of non-Python files (`sed -n 1,25p pyproject.toml`, `cat x.toml | sed -n …`) run as written next to answered segments instead of refusing the whole command.
- Read-only `git` (`status`, `diff`, `log`, `show`, `rev-parse`, `ls-files`, `blame`, `grep`, `stash list`, branch listing; never `--output`) and stdout to `/dev/null` (`>/dev/null`, `&>/dev/null`; a discarded search is not answered).

Why: these are how agents read (one compound command per turn); refusing a whole command for one unmodeled read wastes the graph answer on every other segment. Replay of the 59 pilot-C commands (`bench/interception/replay.py`): 12 → 18 rewritten; the 41 left are writes (heredoc edits, `sed -i`, `cat >>`), test runs, `git stash push/apply`, and commands of only `graphite` CLI calls — correctly untouched. 0 exit-code mismatches.

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

**Alternations (2026-10-02).** `grep -n "a\|b"`, `rg "a|b"`, `-e a -e b`: when every top-level alternative is an identifier (dotted or `\b`-wrapped allowed, at most 6), each name gets its own verdict and definition in the header (`2 identifiers: \`a\` graph COMPLETE, def …, N call sites · \`b\` graph LOWER-BOUND (…)`), every match is judged against the name it is about, graph-only references of every name join the list, and the footer warns per untested name. The overall verdict is COMPLETE only if every name's is. Why: agents batch the names they are about to change into one alternation (pilot C: 4 of 13 searches); without per-name verdicts those answers were plain text and the confirmation grep came back. Mixed alternations (`a\|def `) stay grouped by enclosing symbol.

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
- **One file searched → grep's own `N:text` shape** (2026-10-02): when grep would not name files (one file operand without `-r`/`-H`, or `-h`; `rg` on one file), match, context and folded lines drop the path; the header names the file. A path on every line of a single-file `-A70` read cost 12–25 bytes a line for nothing the agent can filter on.

## 4. Size: budget, never byte-cut

**Decision.** A size limiter in the agent's pipe (`| head -c N`, `| head -n N`, `| tail -n N`) is read as a **budget**, not applied as a cut. Graphite composes the answer within it: header + direct sites first, the rest summarized, every cut disclosed with how to get more.

**Why.** Pilot B showed agents cutting 14–25 KB one-line JSON with `head -c 3000/6000`, destroying dependents and verdict, then grepping to recover. The intent ("don't flood my context") is honored; the mechanism (mid-item cut) is not.

**Default budget (2026-10-02).** With no limiter, the answer never costs more than the agent's own command would have printed, plus a bounded overhead:
- base = the plain output in grep's shape (`path:line:text`, context lines, `--` separators, after the agent's own filters), plus the graph-only lines (what grep misses);
- overhead = a quarter of the base, 384–1024 bytes, for annotations and the footer; the header (the agent's command echoed, verdict, pipeline notices) comes on top;
- `| head -N` keeps its N lines and is also capped at those N grep lines' bytes plus the overhead (annotated lines are longer than grep's);
- under any budget every definition, call site (prod or test) and graph-only reference stays: as many full lines as fit, the overflow folded per file (`path:12,40,77 ← fns (3 sites)`); only lower-ranked classes are dropped, and counted;
- a cut under the default budget points back in the agent's vocabulary: `— to see them: <the same command> | head -c N`, N large enough for the whole answer.

It replaces the old count caps (60 priority / 24 other lines), which hid direct sites on large searches.

**Why.** Pilot C: hook answers were 2.3× the greps they replaced (57 KB vs 25 KB) because without a pipe nothing bounded them. Speed is turns; context is what a turn reads. Replay of the pilot-C commands (`bench/interception/results/pilot-c-replay.md`): on the 12 commands both builds route, the bytes the agent sees over the plain commands went from +15.4 KB to +6.4 KB; search answers alone 1.47× → 1.32× the plain grep (small greps sit on the 384-byte floor); 0 of 21 definition/call sites lost; every answer still opens with its verdict line.

**Reads.** A file's graph header is printed once per file per command and sized to the read: at most a quarter of what the read printed, 160–600 bytes; test names go first, then the least-used symbols (counted in `+N more`). Three `sed -n` reads of one file used to repeat a 910-byte header three times.

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
