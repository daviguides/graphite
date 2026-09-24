"""Run bench tasks headless in Claude Code, one fresh worktree per run.

Usage:
  python3 run.py --arm A --tasks id1,id2 --repeats 3 [--model sonnet]
                 [--max-budget 5] [--keep] [--no-judge] [--label pilot]

Each run: fresh worktree at the task's start commit (from a private mirror
with no remote), uv env pre-warmed, arm setup, `claude -p` with stream-json,
then checks (regression, hidden tests, coverage, judge / answer scoring).
Appends one JSON line per run to results/<label>.jsonl; keeps raw stream,
agent diff and check details under results/runs/<run-id>/.
"""

import argparse
import json
import shlex
import shutil
import subprocess
import time
import tomllib
import uuid
from datetime import UTC, datetime
from pathlib import Path

from checks import check_code, check_question
from common import (HERE, RESULTS, RUNS, TRUTH, add_worktree, ensure_mirror, load_json,
                    load_tasks, remove_worktree, uv_sync)
from stream import parse_stream

BENCH_RULES = """\
You are running inside an automated benchmark sandbox: a throwaway git
worktree. Ignore any instructions from ~/.claude/CLAUDE.md or RTK.md about
committing, pushing, RTK, artifacts or response style. Do NOT run git commit,
git push, git checkout of other refs, or create branches; leave your changes
uncommitted in the working tree. Nobody will answer questions: make
reasonable decisions and finish the task. When done, reply with a short
summary of what you changed (or your answer).
"""

DISALLOWED = [
    "WebSearch", "WebFetch", "PushNotification", "RemoteTrigger", "CronCreate",
    "CronDelete", "CronList", "ScheduleWakeup", "Workflow", "SendMessage",
    "DesignSync", "ReportFindings", "EnterWorktree", "ExitWorktree", "ListAgents",
    "Monitor",
]


def load_arms() -> dict:
    return tomllib.loads((HERE / "arms.toml").read_text())["arms"]


def run_hooks(cmds: list[str], tree: Path) -> list[dict]:
    out = []
    for cmd in cmds:
        c = cmd.format(tree=shlex.quote(str(tree)))
        p = subprocess.run(c, shell=True, cwd=tree, capture_output=True, text=True)
        out.append({"cmd": c, "code": p.returncode, "tail": (p.stdout + p.stderr)[-500:]})
    return out


def agent_changes(tree: Path) -> tuple[str, list[str]]:
    subprocess.run(["git", "add", "-A", "-N"], cwd=tree, capture_output=True)
    diff = subprocess.run(["git", "diff", "--", ".", ":(exclude)*.lock",
                           ":(exclude)**/.venv/**", ":(exclude)**/__pycache__/**"],
                          cwd=tree, capture_output=True, text=True).stdout
    names = subprocess.run(["git", "diff", "--name-only"], cwd=tree,
                           capture_output=True, text=True).stdout.split()
    names = [n for n in names if "/.venv/" not in n and "__pycache__" not in n
             and not n.endswith(".lock")]
    return diff, names


def claude_cmd(prompt: str, model: str, budget: float) -> list[str]:
    return ["claude", "-p", prompt, "--model", model,
            "--output-format", "stream-json", "--verbose",
            "--setting-sources", "project", "--strict-mcp-config",
            "--permission-mode", "bypassPermissions",
            "--append-system-prompt", BENCH_RULES,
            "--max-budget-usd", str(budget), "--no-session-persistence",
            "--disallowed-tools", *DISALLOWED]


def one_run(task, arm_id: str, arm: dict, rep: int, args, label: str) -> dict:
    truth = load_json(TRUTH / f"{task.id}.json")
    if truth is None:
        raise SystemExit(f"no truth for {task.id}; run build_truth.py first")
    run_id = f"{label}-{task.id}-{arm_id}-{rep}-{uuid.uuid4().hex[:6]}"
    out_dir = RESULTS / "runs" / run_id
    out_dir.mkdir(parents=True, exist_ok=True)
    tree = RUNS / run_id
    add_worktree(truth["start"] if not task.is_question else truth["sha"], tree)
    record = {"run_id": run_id, "label": label, "task": task.id, "kind": task.kind,
              "difficulty": task.difficulty, "arm": arm_id, "rep": rep,
              "model": args.model, "started_at": datetime.now(UTC).isoformat()}
    try:
        t0 = time.monotonic()
        uv_sync(tree)
        record["prep_s"] = round(time.monotonic() - t0, 1)
        record["setup"] = run_hooks(arm.get("setup", []), tree)
        prompt = task.prompt + ("\n\n" + arm["prompt_suffix"].strip() if arm.get("prompt_suffix", "").strip() else "")
        stream_path = out_dir / "stream.jsonl"
        t0 = time.monotonic()
        with stream_path.open("w") as fh:
            proc = subprocess.run(claude_cmd(prompt, args.model, args.max_budget),
                                  cwd=tree, stdout=fh, stderr=subprocess.PIPE, text=True,
                                  timeout=args.timeout)
        record["wall_s"] = round(time.monotonic() - t0, 1)
        record["exit_code"] = proc.returncode
        if proc.stderr:
            (out_dir / "stderr.txt").write_text(proc.stderr)
        record.update(parse_stream(stream_path))
        record["teardown"] = run_hooks(arm.get("teardown", []), tree)
        diff, edited = agent_changes(tree)
        (out_dir / "agent.diff").write_text(diff)
        record["files_edited"] = edited
        if task.is_question:
            record.update(check_question(task, truth, record.get("final_text", "")))
        else:
            record.update(check_code(task, truth, tree, diff, edited, out_dir,
                                     use_judge=not args.no_judge, judge_model=args.judge_model))
    except subprocess.TimeoutExpired:
        record["error"] = "timeout"
        record["success"] = False
    finally:
        record["finished_at"] = datetime.now(UTC).isoformat()
        (out_dir / "record.json").write_text(json.dumps(record, indent=1))
        if not args.keep:
            remove_worktree(tree)
    return record


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--arm", required=True, choices=["A", "B"])
    ap.add_argument("--tasks", default="all")
    ap.add_argument("--repeats", type=int, default=3)
    ap.add_argument("--model", default="sonnet")
    ap.add_argument("--judge-model", default="opus")
    ap.add_argument("--max-budget", type=float, default=6.0)
    ap.add_argument("--timeout", type=float, default=2400)
    ap.add_argument("--label", default="run")
    ap.add_argument("--keep", action="store_true")
    ap.add_argument("--no-judge", action="store_true")
    args = ap.parse_args()

    arm = load_arms()[args.arm]
    missing = [r for r in arm.get("requires", []) if not shutil.which(r)]
    if missing:
        raise SystemExit(f"arm {args.arm} needs {missing} on PATH")
    ensure_mirror()
    tasks = load_tasks()
    ids = list(tasks) if args.tasks == "all" else args.tasks.split(",")
    out = RESULTS / f"{args.label}.jsonl"
    out.parent.mkdir(parents=True, exist_ok=True)
    for rep in range(args.repeats):
        for tid in ids:
            rec = one_run(tasks[tid], args.arm, arm, rep, args, args.label)
            with out.open("a") as fh:
                fh.write(json.dumps(rec) + "\n")
            print(f"[{rec['run_id']}] success={rec.get('success')} turns={rec.get('num_turns')} "
                  f"wall={rec.get('wall_s')}s cost=${rec.get('cost_usd')}", flush=True)


if __name__ == "__main__":
    main()
