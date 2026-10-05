"""Aggregate bench results and render the go/no-go report.

Usage: python3 analyze.py results/<label>.jsonl [more.jsonl ...] [-o report.md]

Go (v1.0 exit criteria, features.md): with Graphite (arm B) vs baseline
(arm A) on the same tasks —
  * median turns per task drops >= 20%
  * median wall-clock per task drops >= 20%
  * success rate is equal or better
  * zero silent-stale Graphite answers
Per-task medians are taken over repeats first, then the median of per-task
ratios B/A is reported (robust to a few long tasks dominating).

Turns are model round trips (distinct assistant messages). Claude Code's
`num_turns` counts tool results + 1, so parallel tool calls in one response
inflate it; it is reported as "tool turns" for reference.
"""

import json
import random
import statistics as st
import sys
from collections import defaultdict
from pathlib import Path

TARGET_DROP = 0.20


def load(paths: list[str]) -> list[dict]:
    rows = []
    for p in paths:
        for line in Path(p).read_text().splitlines():
            if line.strip():
                rows.append(json.loads(line))
    return rows


def outcome(r: dict) -> str:
    if r.get("outcome"):
        return r["outcome"]
    return "success" if r.get("success") else ("fail" if r.get("success") is False else "unjudged")


def split_harness(rows):
    """Harness errors say nothing about the agent: drop them from every metric."""
    return ([r for r in rows if outcome(r) != "harness_error"],
            [r for r in rows if outcome(r) == "harness_error"])


def med(xs):
    xs = [x for x in xs if x is not None]
    return st.median(xs) if xs else None


def spread(xs):
    xs = [x for x in xs if x is not None]
    return (min(xs), max(xs)) if xs else (None, None)


def fmt(x, nd=1):
    return "–" if x is None else (f"{x:.{nd}f}" if isinstance(x, float) else str(x))


def turns(r: dict):
    """Model round trips; records from before they were recorded fall back to num_turns."""
    return r.get("round_trips") or r.get("num_turns")


def per_task(rows):
    g = defaultdict(list)
    for r in rows:
        g[(r["task"], r["arm"])].append(r)
    out = {}
    for (task, arm), rs in g.items():
        succ = [r.get("success") for r in rs if r.get("success") is not None]
        out[(task, arm)] = {
            "n": len(rs),
            "kind": rs[0]["kind"],
            "difficulty": rs[0]["difficulty"],
            "turns": med([turns(r) for r in rs]),
            "turns_range": spread([turns(r) for r in rs]),
            "tool_turns": med([r.get("num_turns") for r in rs]),
            "wall": med([r.get("wall_s") for r in rs]),
            "wall_range": spread([r.get("wall_s") for r in rs]),
            "graphite_calls": med([r.get("graphite_calls") for r in rs]) if rs[0]["arm"] != "A" else None,
            "hook_answers": med([r.get("graphite_hook_answers") for r in rs]) if rs[0].get("hooks") else None,
            "tool_calls": med([r.get("tool_calls") for r in rs]),
            "search_calls": med([(r.get("bash") or {}).get("search", 0) + (r.get("tools") or {}).get("Grep", 0)
                                 for r in rs]),
            "reads": med([r.get("n_files_read", len(r.get("files_read") or [])) for r in rs]),
            "before_edit": med([r.get("calls_before_first_edit") for r in rs]),
            "success_rate": (sum(1 for s in succ if s) / len(succ)) if succ else None,
            "stale": sum(r.get("graphite_stale_results", 0) or 0 for r in rs),
            "search_after_complete": med([r.get("search_after_complete_graphite") for r in rs]),
        }
    return out


TREAT = "B"  # treatment arm compared against baseline A; set from the data / --treatment
BOOT = 5000
SUCCESS_MARGIN = 0.05


def _ratio(stats, t, key):
    a, b = stats[(t, "A")][key], stats[(t, TREAT)][key]
    return b / a if a and b is not None else None


def bootstrap(paired, fn, n=BOOT, seed=7):
    """95% CI of fn(sample) resampling tasks with replacement (paired A/B)."""
    rng = random.Random(seed)
    vals = []
    for _ in range(n):
        sample = [rng.choice(paired) for _ in paired]
        v = fn(sample)
        if v is not None:
            vals.append(v)
    if not vals:
        return (None, None)
    vals.sort()
    return (vals[int(0.025 * len(vals))], vals[int(0.975 * len(vals)) - 1])


def verdict(stats) -> tuple[str, list[str]]:
    tasks = sorted({t for t, _ in stats})
    paired = [t for t in tasks if (t, "A") in stats and (t, TREAT) in stats]
    if not paired:
        return f"NO VERDICT — arm {TREAT} has not run yet (baseline only).", []

    def med_ratio(key):
        return lambda ts: med([_ratio(stats, t, key) for t in ts])

    def succ_diff(ts):
        return (st.mean([stats[(t, TREAT)]["success_rate"] or 0 for t in ts])
                - st.mean([stats[(t, "A")]["success_rate"] or 0 for t in ts]))

    tr, wr, sd = med_ratio("turns")(paired), med_ratio("wall")(paired), succ_diff(paired)
    ttr = med_ratio("tool_turns")(paired) if all("tool_turns" in stats[(t, "A")] for t in paired) else None
    tr_ci, wr_ci, sd_ci = (bootstrap(paired, med_ratio("turns")), bootstrap(paired, med_ratio("wall")),
                           bootstrap(paired, succ_diff))
    stale = sum(stats[(t, TREAT)]["stale"] for t in paired)
    target = 1 - TARGET_DROP
    lines = [
        f"- paired tasks: {len(paired)} · bootstrap {BOOT} resamples of tasks, 95% CI",
        f"- turns ratio {TREAT}/A: {fmt(tr, 2)} [CI {fmt(tr_ci[0], 2)}–{fmt(tr_ci[1], 2)}] "
        f"(need point <= {target:.2f} and CI upper <= 1.00)",
        f"- tool turns (num_turns = tool calls + 1) ratio {TREAT}/A, reference only: {fmt(ttr, 2)}",
        f"- wall-clock ratio {TREAT}/A: {fmt(wr, 2)} [CI {fmt(wr_ci[0], 2)}–{fmt(wr_ci[1], 2)}] "
        f"(need point <= {target:.2f} and CI upper <= 1.00)",
        f"- success rate {TREAT}−A: {fmt(sd * 100, 1)} pts [CI {fmt(sd_ci[0] * 100 if sd_ci[0] is not None else None, 1)}"
        f"–{fmt(sd_ci[1] * 100 if sd_ci[1] is not None else None, 1)}] "
        f"(CI upper must be >= −{SUCCESS_MARGIN * 100:.0f}: {TREAT} not provably worse by >5 pts)",
        f"- silent-stale Graphite answers: {stale} (must be 0)",
    ]
    ok_ratio = lambda p, ci: p is not None and p <= target and ci[1] is not None and ci[1] <= 1.0
    go = (ok_ratio(tr, tr_ci) and ok_ratio(wr, wr_ci)
          and sd_ci[1] is not None and sd_ci[1] >= -SUCCESS_MARGIN and stale == 0)
    rethink = []
    if not ok_ratio(tr, tr_ci) and (sd_ci[1] or 0) >= -SUCCESS_MARGIN:
        rethink.append("Turns not cut enough (or not significantly) — check attribution: was Graphite used?")
    if sd_ci[1] is not None and sd_ci[1] < -SUCCESS_MARGIN:
        rethink.append("Correctness regressed — stop and investigate before any other wave.")
    return ("GO" if go else "NO-GO"), lines + [f"- {r}" for r in rethink]


def attribution_section(rows) -> list[str]:
    b = [r for r in rows if r.get("arm") == TREAT and r.get("attribution")]
    if not b:
        return []
    counts = defaultdict(int)
    for r in b:
        counts[r["attribution"]["class"]] += 1
    complete = [r for r in b if r.get("graphite_complete_answer")]
    kept = [r for r in complete if (r.get("search_after_complete_graphite") or 0) > 0]
    out = [f"## Arm {TREAT} Graphite use and attribution", "",
           f"- runs that got a complete Graphite answer: {len(complete)}; of those still searched "
           f"(grep/find/Grep/Glob) afterwards: {len(kept)} "
           f"(median searches after: {fmt(med([r.get('search_after_complete_graphite') for r in complete]))})"]
    out += [f"- {k}: {counts[k]}" for k in ("graphite_not_used", "graphite_used_success",
                                             "failure_despite_graphite", "graphite_caused_failure")]
    hooked = [r for r in b if r.get("hooks")]
    if hooked:
        H = lambda k: [r["hooks"].get(k) for r in hooked]
        out += [
            f"- hooks ({len(hooked)} runs): median graph answers {fmt(med(H('answers')))}, "
            f"enrich {fmt(med(H('enrich')))}, fallbacks {fmt(med(H('fallbacks')))}, "
            f"re-asks after complete {fmt(med(H('reasks_after_complete')))}",
            f"- hook answer bytes: median {fmt(med(H('answer_bytes')))} vs raw grep "
            f"{fmt(med(H('raw_bytes')))}; hook latency median {fmt(med(H('latency_ms')))} ms",
            f"- overlap inside one compound command (segments of `a; b`, `&&`, `||`): median "
            f"duplicated path:line keys {fmt(med(H('intra_overlap_keys')))}, bytes "
            f"{fmt(med(H('intra_overlap_bytes')))} (total {sum(v or 0 for v in H('intra_overlap_bytes'))} B; "
            f"{sum(v or 0 for v in H('intra_overlap_commands'))} commands)",
            f"- overlap across calls (same session, ≤{hooked[0]['hooks'].get('overlap_window_ms', 10000) // 1000} s): "
            f"median duplicated keys {fmt(med(H('cross_overlap_keys')))} of "
            f"{fmt(med(H('answer_keys')))}, bytes {fmt(med(H('cross_overlap_bytes')))} "
            f"(total {sum(v or 0 for v in H('cross_overlap_bytes'))} B over {len(hooked)} runs)",
        ]
    caused = [r for r in b if r["attribution"]["class"] == "graphite_caused_failure"]
    if caused:
        out += ["", "Graphite correctness misses (candidate — review each):", "",
                "| run | task | omitted file(s) |", "|---|---|---|"]
        out += [f"| {r['run_id']} | {r['task']} | {', '.join(r['attribution']['omitted'])} |" for r in caused]
    return out + [""]


def render(all_rows) -> str:
    rows, harness = split_harness(all_rows)
    stats = per_task(rows)
    out = ["# Effectiveness bench report", ""]
    arms = sorted({r["arm"] for r in rows})
    out.append(f"Runs: {len(rows)} · arms: {', '.join(arms)} · tasks: {len({r['task'] for r in rows})} · "
               f"models: {', '.join(sorted({r.get('model', '?') for r in rows}))}")
    out.append(f"Harness errors (excluded from every metric): {len(harness)}"
               + (" — " + "; ".join(f"{r['run_id']}: {(r.get('harness_errors') or ['?'])[0][:120]}" for r in harness)
                  if harness else ""))
    out.append("")
    v, lines = verdict(stats)
    out += [f"## Verdict: {v}", ""] + lines + [""]
    for arm in arms:
        rs = [r for r in rows if r["arm"] == arm]
        succ = [r.get("success") for r in rs if r.get("success") is not None]
        out += [f"## Arm {arm} totals", "",
                f"- runs {len(rs)}, success {sum(1 for s in succ if s)}/{len(succ)}",
                f"- median turns {fmt(med([turns(r) for r in rs]))} "
                f"(tool turns {fmt(med([r.get('num_turns') for r in rs]))}), "
                f"median wall {fmt(med([r.get('wall_s') for r in rs]))} s",
                f"- median files read {fmt(med([r.get('n_files_read') for r in rs]))} "
                f"(Read tool {fmt(med([r.get('n_files_read_tool') for r in rs]))}, "
                f"via Bash {fmt(med([r.get('n_files_read_bash') for r in rs]))})",
                f"- median tool calls {fmt(med([r.get('tool_calls') for r in rs]))}, "
                f"median calls before first edit {fmt(med([r.get('calls_before_first_edit') for r in rs]))}",
                ""]
    out += ["## Per task (medians over repeats)", "",
            "| task | kind | diff | arm | n | turns (min–max) | wall s (min–max) | tools | search | files read | calls before edit | graphite calls / hook answers | searches after complete | success |",
            "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
    for (task, arm), s in sorted(stats.items()):
        out.append(
            f"| {task} | {s['kind']} | {s['difficulty']} | {arm} | {s['n']} | "
            f"{fmt(s['turns'])} ({fmt(s['turns_range'][0])}–{fmt(s['turns_range'][1])}) | "
            f"{fmt(s['wall'])} ({fmt(s['wall_range'][0])}–{fmt(s['wall_range'][1])}) | "
            f"{fmt(s['tool_calls'])} | {fmt(s['search_calls'])} | {fmt(s['reads'])} | "
            f"{fmt(s['before_edit'])} | {fmt(s['graphite_calls'])} / {fmt(s['hook_answers'])} | "
            f"{fmt(s['search_after_complete'])} | {fmt(s['success_rate'], 2)} |")
    out += [""] + attribution_section(rows)
    paired = sorted({t for t, a in stats if a == "A"} & {t for t, a in stats if a == TREAT})
    if paired:
        out += ["", f"## Per-task deltas ({TREAT} vs A)", "", f"| task | turns Δ% | wall Δ% | success A→{TREAT} |", "|---|---|---|---|"]
        for t in paired:
            a, b = stats[(t, "A")], stats[(t, TREAT)]
            dt = (b["turns"] / a["turns"] - 1) * 100 if a["turns"] else None
            dw = (b["wall"] / a["wall"] - 1) * 100 if a["wall"] else None
            out.append(f"| {t} | {fmt(dt, 0)} | {fmt(dw, 0)} | {fmt(a['success_rate'], 2)}→{fmt(b['success_rate'], 2)} |")
    out += ["", "## Run details", "", "| run | outcome | judge | regression | hidden | coverage | files read (tool/bash) | recall/precision |",
            "|---|---|---|---|---|---|---|---|"]
    for r in all_rows:
        j = (r.get("judge") or {}).get("verdict", "–")
        rp = f"{r['recall']}/{r['precision']}" if "recall" in r else "–"
        out.append(f"| {r['run_id']} | {outcome(r)} | {j} | {r.get('regression_ok', '–')} | "
                   f"{fmt(r.get('hidden_pass_rate'), 2)} | {fmt(r.get('coverage'), 2)} | "
                   f"{fmt(r.get('n_files_read'))} ({fmt(r.get('n_files_read_tool'))}/{fmt(r.get('n_files_read_bash'))}) | {rp} |")
    return "\n".join(out) + "\n"


def main() -> None:
    """analyze.py FILE.jsonl ... [-o report.md] [--treatment C]
    Treatment defaults to the one non-A arm present."""
    global TREAT
    args = sys.argv[1:]
    out_path = None
    treatment = None
    if "-o" in args:
        i = args.index("-o")
        out_path = args[i + 1]
        args = args[:i] + args[i + 2:]
    if "--treatment" in args:
        i = args.index("--treatment")
        treatment = args[i + 1]
        args = args[:i] + args[i + 2:]
    rows = load(args)
    others = sorted({r["arm"] for r in rows} - {"A"})
    if treatment is None and len(others) > 1:
        raise SystemExit(f"several treatment arms {others}: pass --treatment")
    TREAT = treatment or (others[0] if others else "B")
    report = render(rows)
    if out_path:
        Path(out_path).write_text(report)
    print(report)


if __name__ == "__main__":
    main()
