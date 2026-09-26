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
"""

import json
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
            "turns": med([r.get("num_turns") for r in rs]),
            "turns_range": spread([r.get("num_turns") for r in rs]),
            "wall": med([r.get("wall_s") for r in rs]),
            "wall_range": spread([r.get("wall_s") for r in rs]),
            "cost": med([r.get("cost_usd") for r in rs]),
            "tool_calls": med([r.get("tool_calls") for r in rs]),
            "search_calls": med([(r.get("bash") or {}).get("search", 0) + (r.get("tools") or {}).get("Grep", 0)
                                 for r in rs]),
            "reads": med([r.get("n_files_read", len(r.get("files_read") or [])) for r in rs]),
            "before_edit": med([r.get("calls_before_first_edit") for r in rs]),
            "success_rate": (sum(1 for s in succ if s) / len(succ)) if succ else None,
            "stale": sum(r.get("graphite_stale_results", 0) or 0 for r in rs),
        }
    return out


def verdict(stats) -> tuple[str, list[str]]:
    tasks = sorted({t for t, _ in stats})
    paired = [t for t in tasks if (t, "A") in stats and (t, "B") in stats]
    if not paired:
        return "NO VERDICT — arm B has not run yet (baseline only).", []
    turn_ratios = [stats[(t, "B")]["turns"] / stats[(t, "A")]["turns"] for t in paired
                   if stats[(t, "A")]["turns"] and stats[(t, "B")]["turns"] is not None]
    wall_ratios = [stats[(t, "B")]["wall"] / stats[(t, "A")]["wall"] for t in paired
                   if stats[(t, "A")]["wall"] and stats[(t, "B")]["wall"] is not None]
    sa = med([stats[(t, "A")]["success_rate"] for t in paired])
    sb = med([stats[(t, "B")]["success_rate"] for t in paired])
    succ_a = st.mean([stats[(t, "A")]["success_rate"] or 0 for t in paired])
    succ_b = st.mean([stats[(t, "B")]["success_rate"] or 0 for t in paired])
    stale = sum(stats[(t, "B")]["stale"] for t in paired)
    tr, wr = med(turn_ratios), med(wall_ratios)
    lines = [
        f"- median per-task turns ratio B/A: {fmt(tr, 2)} (target <= {1 - TARGET_DROP:.2f})",
        f"- median per-task wall-clock ratio B/A: {fmt(wr, 2)} (target <= {1 - TARGET_DROP:.2f})",
        f"- mean success rate: A {succ_a:.2f} → B {succ_b:.2f} (B must be >= A)",
        f"- silent-stale Graphite answers: {stale} (must be 0; counts results flagged stale)",
    ]
    go = (tr is not None and tr <= 1 - TARGET_DROP and wr is not None and wr <= 1 - TARGET_DROP
          and succ_b >= succ_a and stale == 0)
    rethink = []
    if tr is not None and tr > 1 - TARGET_DROP and succ_b >= succ_a:
        rethink.append("Graphite did not cut turns enough — check conversion: was it used?")
    if succ_b < succ_a:
        rethink.append("Correctness regressed — stop and investigate before any other wave.")
    return ("GO" if go else "NO-GO"), lines + [f"- {r}" for r in rethink]


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
                f"- median turns {fmt(med([r.get('num_turns') for r in rs]))}, "
                f"median wall {fmt(med([r.get('wall_s') for r in rs]))} s, "
                f"median cost ${fmt(med([r.get('cost_usd') for r in rs]), 2)}, "
                f"total cost ${sum(r.get('cost_usd') or 0 for r in rs):.2f} "
                f"(+ judge ${sum(((r.get('judge') or {}).get('judge_cost_usd') or 0) for r in rs):.2f})",
                f"- median files read {fmt(med([r.get('n_files_read') for r in rs]))} "
                f"(Read tool {fmt(med([r.get('n_files_read_tool') for r in rs]))}, "
                f"via Bash {fmt(med([r.get('n_files_read_bash') for r in rs]))})",
                f"- median tool calls {fmt(med([r.get('tool_calls') for r in rs]))}, "
                f"median calls before first edit {fmt(med([r.get('calls_before_first_edit') for r in rs]))}",
                ""]
    out += ["## Per task (medians over repeats)", "",
            "| task | kind | diff | arm | n | turns (min–max) | wall s (min–max) | cost $ | tools | search | files read | calls before edit | success |",
            "|---|---|---|---|---|---|---|---|---|---|---|---|---|"]
    for (task, arm), s in sorted(stats.items()):
        out.append(
            f"| {task} | {s['kind']} | {s['difficulty']} | {arm} | {s['n']} | "
            f"{fmt(s['turns'])} ({fmt(s['turns_range'][0])}–{fmt(s['turns_range'][1])}) | "
            f"{fmt(s['wall'])} ({fmt(s['wall_range'][0])}–{fmt(s['wall_range'][1])}) | "
            f"{fmt(s['cost'], 2)} | {fmt(s['tool_calls'])} | {fmt(s['search_calls'])} | {fmt(s['reads'])} | "
            f"{fmt(s['before_edit'])} | {fmt(s['success_rate'], 2)} |")
    paired = sorted({t for t, a in stats if a == "A"} & {t for t, a in stats if a == "B"})
    if paired:
        out += ["", "## Per-task deltas (B vs A)", "", "| task | turns Δ% | wall Δ% | success A→B |", "|---|---|---|---|"]
        for t in paired:
            a, b = stats[(t, "A")], stats[(t, "B")]
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
    args = sys.argv[1:]
    out_path = None
    if "-o" in args:
        i = args.index("-o")
        out_path = args[i + 1]
        args = args[:i] + args[i + 2:]
    report = render(load(args))
    if out_path:
        Path(out_path).write_text(report)
    print(report)


if __name__ == "__main__":
    main()
