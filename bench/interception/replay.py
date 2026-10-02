#!/usr/bin/env python3
"""Replay the Bash commands of a bench pilot through two builds of the interception hooks.

Measures, per build, on the agent's real commands:
- coverage: which commands the PreToolUse hook rewrites, and why the others pass through;
- size: bytes the agent sees through `graphite-hook run` vs. bytes the plain command prints;
- integrity: every definition/direct call site of a search answer is in the text the agent got
  (full line or folded per file), and the answer starts with its header/verdict line.

Only commands a build rewrote are executed (read-only by the hook's own rules), in a worktree of
the task's start commit. Nothing is written outside the scratch directory.

  python3 replay.py extract --logs ../effectiveness/results/runs --pilot pilot-C -o pilot-c-commands.jsonl
  python3 replay.py run --fixture pilot-c-commands.jsonl --tasks ../effectiveness/tasks.toml \
      --mirror ../effectiveness/work/mirror --scratch /tmp/replay \
      --build old=/path/to/target-old/release --build new=/path/to/target-new/release -o out/
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import tomllib
from collections import Counter
from pathlib import Path

RUN_ROOT = re.compile(r"/[^\s'\"]*?/bench/effectiveness/work/runs/[^/\s'\"]+")
CMD_TIMEOUT = 120


def task_of(run: str, pilot: str) -> str:
    m = re.match(rf"{re.escape(pilot)}-(.+)-[A-Z]\d*-\d+-[0-9a-f]+$", run)
    if not m:
        raise ValueError(f"unexpected run name {run}")
    return m.group(1)


def normalize(text: str) -> str:
    """Absolute run-tree paths → `{tree}` so commands replay in any checkout."""
    return RUN_ROOT.sub("{tree}", text)


def split_top(cmd: str) -> list[str]:
    """Top-level `;` / `&&` / `||` / newline segments, quote-aware (for following `cd`s)."""
    segs, cur, sq, dq, i = [], "", False, False, 0
    while i < len(cmd):
        c = cmd[i]
        if c == "'" and not dq:
            sq = not sq
        elif c == '"' and not sq:
            dq = not dq
        elif not sq and not dq and (c in ";\n" or cmd[i : i + 2] in ("&&", "||")):
            segs.append(cur)
            cur = ""
            i += 1 if c in ";\n" else 2
            continue
        cur += c
        i += 1
    segs.append(cur)
    return [s.strip() for s in segs if s.strip()]


def extract(args: argparse.Namespace) -> None:
    """Pilot hook logs → one fixture line per PreToolUse decision on Bash, in session order."""
    out = []
    for log in sorted(Path(args.logs).glob(f"{args.pilot}-*/hooks.jsonl")):
        run = log.parent.name
        for line in log.read_text().splitlines():
            e = json.loads(line)
            if e.get("event") != "pre" or e.get("tool") != "Bash":
                continue
            m = re.search(r" --cwd (\S+) -- ", e.get("rewritten_command", ""))
            out.append(
                {
                    "run": run,
                    "task": task_of(run, args.pilot),
                    # Known only when the hook rewrote the command (it records --cwd).
                    "cwd": normalize(m.group(1).strip("'")) if m else None,
                    "command": normalize(e["original_command"]),
                    "pilot_action": e["action"],
                    "pilot_reason": e.get("reason", ""),
                }
            )
    Path(args.out).write_text("".join(json.dumps(r) + "\n" for r in out))
    print(f"{len(out)} decisions from {len({r['run'] for r in out})} runs → {args.out}")


def cwds(rows: list[dict], tree: Path) -> list[Path]:
    """The shell's cwd before each command of one run. Claude Code keeps it between calls, so
    top-level `cd`s carry over; a recorded --cwd wins; a `cd` that cannot resolve from the
    current dir is tried from the tree root (the agent's own belief), else ignored."""
    cur, out = tree, []
    for r in rows:
        if r["cwd"]:
            cur = Path(r["cwd"].replace("{tree}", str(tree)))
        out.append(cur if cur.is_dir() else tree)
        for seg in split_top(r["command"].replace("{tree}", str(tree))):
            parts = seg.split()
            if len(parts) < 2 or parts[0] != "cd":
                continue
            target = Path(parts[1])
            for base in (cur, tree):
                cand = (base / target).resolve()
                if cand.is_dir():
                    cur = cand
                    break
    return out


def sh(args: list[str] | str, cwd: Path, env: dict, stdin: str | None = None) -> tuple[bytes, int]:
    try:
        p = subprocess.run(
            args,
            cwd=cwd,
            env=env,
            input=stdin.encode() if stdin is not None else None,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            shell=isinstance(args, str),
            timeout=CMD_TIMEOUT,
        )
        return p.stdout, p.returncode
    except subprocess.TimeoutExpired:
        return b"", -1


def tree_for(task: dict, mirror: Path, scratch: Path) -> Path:
    src = scratch / "src"
    if not src.exists():
        subprocess.run(["git", "clone", "-q", "--shared", "--no-checkout", str(mirror), str(src)], check=True)
    tree = scratch / "trees" / task["id"]
    if not tree.exists():
        start = task["ref"] if task.get("start") == "ref" else f"{task['ref']}~1"
        subprocess.run(["git", "-C", str(src), "worktree", "add", "-q", "--detach", str(tree), start], check=True)
    return tree.resolve()


def log_lines(tree: Path) -> list[dict]:
    p = tree / ".graphite" / "hooks.jsonl"
    if not p.exists():
        return []
    return [json.loads(l) for l in p.read_text().splitlines() if l.strip()]


def sites_kept(answer: str, record: dict) -> tuple[int, list[str]]:
    """Definitions and direct call sites of the record that are missing from the answer text."""
    lines = answer.splitlines()
    one_file = record.get("no_filename", False)
    lost, total = [], 0
    for it in record.get("items", []):
        if it.get("class") not in ("definition", "call", "graph_only") or it.get("line", 0) == 0:
            continue
        total += 1
        path, line = it["path"], str(it["line"])
        prefix = "" if one_file else f"{path}:"  # grep prints one file's lines as `N:text`
        full = any(l.startswith(f"{prefix}{line}:") for l in lines)
        folded = any(
            l.startswith(prefix) and line in l[len(prefix) :].split()[0].split(",") for l in lines if l
        )
        if not (full or folded):
            lost.append(f"{path}:{line}")
    return total, lost


def replay(args: argparse.Namespace) -> None:
    fixture = [json.loads(l) for l in Path(args.fixture).read_text().splitlines() if l.strip()]
    tasks = {t["id"]: t for t in tomllib.loads(Path(args.tasks).read_text())["task"]}
    builds = dict(b.split("=", 1) for b in args.build)
    scratch = Path(args.scratch).resolve()
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    trees = {t: tree_for(tasks[t], Path(args.mirror).resolve(), scratch) for t in sorted({r["task"] for r in fixture})}
    plain_cache: dict[int, tuple[int, int]] = {}
    results: dict[str, list[dict]] = {}
    for label, bindir in builds.items():
        bindir = Path(bindir).resolve()
        env = {**os.environ, "PATH": f"{bindir}:{os.environ['PATH']}"}
        graphite, hook = str(bindir / "graphite"), str(bindir / "graphite-hook")
        rows = []
        for task, tree in trees.items():
            subprocess.run([graphite, "init", "--repo", str(tree), "--json"], env=env, check=True, stdout=subprocess.DEVNULL)
            where: dict[int, Path] = {}
            for run in sorted({r["run"] for r in fixture if r["task"] == task}):
                idxs = [i for i, r in enumerate(fixture) if r["run"] == run]
                where.update(zip(idxs, cwds([fixture[i] for i in idxs], tree)))
            try:
                for idx, r in enumerate(fixture):
                    if r["task"] != task:
                        continue
                    cmd = r["command"].replace("{tree}", str(tree))
                    cwd = where[idx]
                    payload = {
                        "hook_event_name": "PreToolUse",
                        "session_id": f"replay-{label}",
                        "tool_use_id": f"replay-{idx}",
                        "tool_name": "Bash",
                        "tool_input": {"command": cmd},
                        "cwd": str(cwd),
                    }
                    n0 = len(log_lines(tree))
                    o, _ = sh([hook, "pre"], cwd, env, json.dumps(payload))
                    pre = next((e for e in log_lines(tree)[n0:] if e.get("event") == "pre"), {})
                    row = {
                        "idx": idx,
                        "run": r["run"],
                        "task": task,
                        "command": r["command"],
                        "rewrite": bool(o.strip()),
                        "reason": pre.get("reason", ""),
                    }
                    if row["rewrite"]:
                        n1 = len(log_lines(tree))
                        got, code = sh([hook, "run", "--cwd", str(cwd), "--", cmd], cwd, env)
                        if idx not in plain_cache:
                            p_out, p_code = sh(cmd, cwd, env)
                            plain_cache[idx] = (len(p_out), p_code)
                        row.update(hook_bytes=len(got), hook_code=code)
                        row.update(plain_bytes=plain_cache[idx][0], plain_code=plain_cache[idx][1])
                        searches = []
                        for e in log_lines(tree)[n1:]:
                            if e.get("event") != "exec" or e.get("kind") != "search" or e.get("action") != "answer":
                                continue
                            total, lost = sites_kept(e.get("answer", ""), e.get("record") or {})
                            searches.append(
                                {
                                    "segment": e.get("segment"),
                                    "answer_bytes": e.get("answer_bytes"),
                                    "grep_bytes": e.get("raw_bytes"),
                                    "budget_bytes": e.get("budget_bytes"),
                                    "verdict": e.get("graph_verdict"),
                                    "header_ok": e.get("answer", "").startswith("# graphite:"),
                                    "sites": total,
                                    "lost": lost,
                                }
                            )
                        row["searches"] = searches
                    rows.append(row)
            finally:
                subprocess.run([graphite, "daemon", "stop", "--repo", str(tree)], env=env, stdout=subprocess.DEVNULL)
        results[label] = rows
        (out / f"replay-{label}.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))
    report = render(fixture, results)
    (out / "replay.md").write_text(report)
    print(report)


def render(fixture: list[dict], results: dict[str, list[dict]]) -> str:
    lines = [f"Replay of {len(fixture)} Bash decisions ({len({r['run'] for r in fixture})} runs).", ""]
    pilot = Counter(r["pilot_action"] for r in fixture)
    lines.append(f"Recorded in the pilot: {pilot['rewrite']} rewritten, {pilot['passthrough']} passed through.")
    lines.append("")
    lines.append("| build | rewritten | passed through (reason: n) | hook bytes | plain bytes | ratio | exit-code mismatches | search answers | sites lost | header missing |")
    lines.append("|---|---|---|---|---|---|---|---|---|---|")
    for label, rows in results.items():
        rw = [r for r in rows if r["rewrite"]]
        reasons = Counter(r["reason"] for r in rows if not r["rewrite"])
        hb = sum(r["hook_bytes"] for r in rw)
        pb = sum(r["plain_bytes"] for r in rw)
        mism = sum(1 for r in rw if r["hook_code"] != r["plain_code"])
        searches = [s for r in rw for s in r.get("searches", [])]
        lost = sum(len(s["lost"]) for s in searches)
        sites = sum(s["sites"] for s in searches)
        headless = sum(1 for s in searches if not s["header_ok"])
        reason_text = ", ".join(f"{k}: {v}" for k, v in reasons.most_common())
        lines.append(
            f"| {label} | {len(rw)} | {reason_text} | {hb} | {pb} | {hb / pb if pb else 0:.2f} | {mism} | {len(searches)} | {lost} of {sites} | {headless} |"
        )
    labels = list(results)
    if len(labels) == 2:
        a, b = (results[labels[0]], results[labels[1]])
        common = [i for i, (x, y) in enumerate(zip(a, b)) if x["rewrite"] and y["rewrite"]]
        if common:
            ha = sum(a[i]["hook_bytes"] for i in common)
            hb = sum(b[i]["hook_bytes"] for i in common)
            pb = sum(a[i]["plain_bytes"] for i in common)
            lines += [
                "",
                f"Commands rewritten by both builds: {len(common)} — plain {pb} B, {labels[0]} {ha} B ({ha / pb:.2f}×), {labels[1]} {hb} B ({hb / pb:.2f}×).",
            ]
        newly = [y for x, y in zip(a, b) if y["rewrite"] and not x["rewrite"]]
        dropped = [x for x, y in zip(a, b) if x["rewrite"] and not y["rewrite"]]
        lines += ["", f"Newly rewritten by {labels[1]}: {len(newly)}"]
        lines += [f"- `{r['command'][:160]}`" for r in newly]
        if dropped:
            lines += ["", f"Rewritten by {labels[0]} only: {len(dropped)}"]
            lines += [f"- `{r['command'][:160]}` ({b[r['idx']]['reason'] if r['idx'] < len(b) else ''})" for r in dropped]
        lines += ["", f"Still passed through by {labels[1]}:"]
        for r in b:
            if not r["rewrite"]:
                lines.append(f"- {r['reason']}: `{r['command'][:140]}`")
    return "\n".join(lines) + "\n"


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    e = sub.add_parser("extract")
    e.add_argument("--logs", required=True)
    e.add_argument("--pilot", default="pilot-C")
    e.add_argument("-o", "--out", required=True)
    r = sub.add_parser("run")
    r.add_argument("--fixture", required=True)
    r.add_argument("--tasks", required=True)
    r.add_argument("--mirror", required=True)
    r.add_argument("--scratch", required=True)
    r.add_argument("--build", action="append", required=True, help="label=dir with graphite + graphite-hook")
    r.add_argument("-o", "--out", required=True)
    a = ap.parse_args()
    extract(a) if a.cmd == "extract" else replay(a)


if __name__ == "__main__":
    sys.exit(main())
