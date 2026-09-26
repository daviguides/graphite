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
import os
import signal
import shlex
import shutil
import subprocess
import time
import tomllib
import uuid
from datetime import UTC, datetime
from pathlib import Path

from checks import check_code, check_question
from common import (HERE, RESULTS, RUNS, TRUTH, add_worktree, ensure_mirror, git, load_json,
                    load_tasks, remove_worktree, uv_sync)
from stream import graphite_calls, parse_stream, paths_in

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


def graphite_bin() -> str | None:
    return os.environ.get("GRAPHITE_BIN") or shutil.which("graphite")


def run_hooks(cmds: list[str], tree: Path) -> list[dict]:
    out = []
    for cmd in cmds:
        c = cmd.format(tree=shlex.quote(str(tree)), graphite=shlex.quote(graphite_bin() or "graphite"))
        p = subprocess.run(c, shell=True, cwd=tree, capture_output=True, text=True, errors="replace")
        out.append({"cmd": c, "code": p.returncode, "tail": (p.stdout + p.stderr)[-500:]})
    return out


# Tool state, not agent work: Graphite's per-repo index, venvs, caches, locks.
EXCLUDE = [":(exclude).graphite", ":(exclude)**/.graphite/**", ":(exclude)*.lock",
           ":(exclude)**/.venv/**", ":(exclude)**/__pycache__/**"]


def agent_changes(tree: Path) -> tuple[str, list[str]]:
    subprocess.run(["git", "add", "-A", "-N", "--", ".", *EXCLUDE], cwd=tree, capture_output=True)
    diff = subprocess.run(["git", "diff", "--", ".", *EXCLUDE], cwd=tree, capture_output=True,
                          text=True, errors="replace").stdout
    names = subprocess.run(["git", "diff", "--name-only", "--", ".", *EXCLUDE], cwd=tree,
                           capture_output=True, text=True, errors="replace").stdout.split()
    return diff, names


def claude_cmd(prompt: str, model: str, budget: float) -> list[str]:
    return ["claude", "-p", prompt, "--model", model,
            "--output-format", "stream-json", "--verbose",
            "--setting-sources", "project", "--strict-mcp-config",
            "--permission-mode", "bypassPermissions",
            "--append-system-prompt", BENCH_RULES,
            "--max-budget-usd", str(budget), "--no-session-persistence",
            "--disallowed-tools", *DISALLOWED]


AGENT_STOP_SUBTYPES = {"success", "error_max_turns", "error_max_budget_usd"}


def attribute(record: dict, truth: dict, is_question: bool) -> dict | None:
    """Arm B only: did Graphite cause the failure?

    graphite_caused_failure = the agent used Graphite, failed, and some
    ground-truth file it did not touch/list was absent from every Graphite
    answer it got — a candidate Graphite correctness miss (needs review:
    the query may simply have been about another symbol).
    """
    if record.get("arm") != "B" or record.get("outcome") in ("harness_error", "unjudged", None):
        return None
    if not record.get("graphite_calls"):
        return {"class": "graphite_not_used"}
    if record.get("success"):
        return {"class": "graphite_used_success"}
    truth_files = set(truth["callers"] if is_question else truth.get("expected_src", []))
    covered_by_agent = set(record.get("answer_files" if is_question else "files_edited") or [])
    missed = truth_files - covered_by_agent
    omitted = sorted(missed - set(record.get("graphite_paths") or []))
    if omitted:
        return {"class": "graphite_caused_failure", "omitted": omitted}
    return {"class": "failure_despite_graphite", "missed": sorted(missed)}


def run_problem(record: dict) -> str | None:
    """Why this run can't be scored, or None. Only the agent's own finish,
    turn limit or budget limit count as the agent's result; anything else —
    a signal, a crash, an API failure, a missing result event — is the harness."""
    code = record.get("exit_code")
    if code is not None and (code < 0 or code >= 128):
        return f"agent process killed by signal (exit {code})"
    if not record.get("has_result_event"):
        return f"no result event in stream (exit {code})"
    if record.get("is_error") and record.get("terminal_reason") in {"api_error", "auth_error", "network_error"}:
        return f"API failure: {record.get('terminal_reason')}"
    if record.get("stop_reason") not in AGENT_STOP_SUBTYPES:
        return f"agent run ended with {record.get('stop_reason')} (exit {code})"
    return None


def _harness_error(record: dict, reason: str) -> dict:
    record.update({"outcome": "harness_error", "success": None})
    record.setdefault("harness_errors", []).append(reason)
    return record


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
        failed_setup = [h for h in record["setup"] if h["code"] != 0]
        if failed_setup:
            return _harness_error(record, f"arm setup failed: {failed_setup[0]['cmd']} → {failed_setup[0]['tail'][-200:]}")
        known = set(git("ls-files", cwd=tree).split())
        prompt = task.prompt + ("\n\n" + arm["prompt_suffix"].strip() if arm.get("prompt_suffix", "").strip() else "")
        stream_path = out_dir / "stream.jsonl"
        t0 = time.monotonic()
        timed_out = False
        with stream_path.open("w") as fh, (out_dir / "stderr.txt").open("w") as err:
            # Own session: signals aimed at the launching shell, a waiter or a
            # monitor's process group must never reach the agent.
            env = dict(os.environ)
            if graphite_bin():
                env["PATH"] = str(Path(graphite_bin()).parent) + os.pathsep + env.get("PATH", "")
            proc = subprocess.Popen(claude_cmd(prompt, args.model, args.max_budget),
                                    cwd=tree, stdout=fh, stderr=err, text=True, env=env,
                                    stdin=subprocess.DEVNULL, start_new_session=True)
            try:
                proc.wait(timeout=args.timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
                os.killpg(proc.pid, signal.SIGTERM)
                proc.wait()
        record["wall_s"] = round(time.monotonic() - t0, 1)
        record["exit_code"] = proc.returncode
        record.update(parse_stream(stream_path, known))
        if timed_out:
            record.update({"outcome": "fail", "success": False, "error": "agent_timeout"})
            return record
        problem = run_problem(record)
        if problem:
            return _harness_error(record, problem)
        record["teardown"] = run_hooks(arm.get("teardown", []), tree)
        diff, edited = agent_changes(tree)
        (out_dir / "agent.diff").write_text(diff)
        record["files_edited"] = edited
        calls = graphite_calls(stream_path)
        if calls:
            with (out_dir / "graphite.jsonl").open("w") as g:
                for c in calls:
                    g.write(json.dumps(c) + "\n")
        record["graphite_calls"] = len(calls)
        record["graphite_paths"] = sorted(set().union(*(paths_in(c.get("json")) for c in calls)) if calls else set())
        if task.is_question:
            record.update(check_question(task, truth, record.get("final_text", "")))
        else:
            record.update(check_code(task, truth, tree, diff, edited, out_dir,
                                     use_judge=not args.no_judge, judge_model=args.judge_model))
        record["attribution"] = attribute(record, truth, task.is_question)
    except Exception as exc:  # any harness crash is a harness error, never an agent verdict
        _harness_error(record, f"harness exception: {type(exc).__name__}: {exc}")
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
    missing = [r for r in arm.get("requires", [])
               if not (graphite_bin() if r == "graphite" else shutil.which(r))]
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
