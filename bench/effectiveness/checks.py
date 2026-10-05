"""Success checks.

Code tasks — success = no regression AND judge verdict "solved".
  regression: the agent tree's suite failures must be a subset of the start
              tree's failures (the agent may update tests; the judge sees it).
  hidden:     the reference commit's test files, copied over the agent tree,
              reported as a pass rate. Secondary signal only: those tests are
              often coupled to the reference's private helpers and mocks.
  coverage:   recall of the reference's non-test source files among the files
              the agent edited.
  judge:      an LLM compares the agent diff with the reference diff against
              the task statement.
Question tasks — success = recall >= 0.9 and precision >= 0.8 of the files
listed after `AFFECTED:` vs the frozen caller list.
"""

import json
import re
import shutil
import subprocess
import time
from pathlib import Path

from common import TRUTH, WORK, add_worktree, remove_worktree, run_suite

JUDGE_PROMPT = """\
You are grading a coding agent's change against a reference change made by
the repository's maintainer. The agent received only the TASK statement.
Judge whether the agent's change accomplishes what the task asks, with the
same observable behaviour as the reference where the task specifies it.
Different internal structure or naming is fine when the task did not require
it. Missing pieces the task explicitly asked for, broken callers, or behaviour
that contradicts the task are not fine.

TASK:
{task}

REFERENCE DIFF (truncated):
{ref}

AGENT DIFF (truncated):
{agent}

TEST SIGNALS:
{signals}

Reply with ONLY a JSON object:
{{"verdict": "solved" | "partial" | "failed",
  "missing": ["short item", ...],
  "reason": "one or two sentences"}}
"""


def _truncate(text: str, limit: int = 40000) -> str:
    return text if len(text) <= limit else text[:limit] + f"\n... [{len(text) - limit} chars truncated]"


VERDICTS = {"solved", "partial", "failed"}
JUDGE_ATTEMPTS = 3
JUDGE_BACKOFF_S = (5, 20, 60)


def _judge_once(prompt: str, model: str) -> dict:
    scratch = WORK / "judge"
    scratch.mkdir(parents=True, exist_ok=True)
    try:
        proc = subprocess.run(
            ["claude", "-p", prompt, "--model", model, "--output-format", "json",
             "--setting-sources", "project", "--strict-mcp-config", "--tools", "",
             "--no-session-persistence"],
            cwd=scratch, capture_output=True, text=True, timeout=600,
            start_new_session=True)
    except subprocess.TimeoutExpired:
        return {"verdict": "error", "reason": "judge timeout"}
    try:
        payload = json.loads(proc.stdout)
        result = [x for x in payload if x.get("type") == "result"][0] if isinstance(payload, list) else payload
    except (json.JSONDecodeError, IndexError):
        return {"verdict": "error", "reason": f"exit {proc.returncode}: {(proc.stdout + proc.stderr)[-400:]}"}
    text = result.get("result", "") or ""
    cost = result.get("total_cost_usd")
    match = re.search(r"\{.*\}", text, re.S)
    try:
        verdict = json.loads(match.group(0)) if match else None
    except json.JSONDecodeError:
        verdict = None
    if not verdict or verdict.get("verdict") not in VERDICTS:
        return {"verdict": "error", "judge_cost_usd": cost,
                "reason": f"unparseable judge reply (is_error={result.get('is_error')}, "
                          f"subtype={result.get('subtype')}): {text[:300]}"}
    verdict["judge_cost_usd"] = cost
    return verdict


def judge(task, truth: dict, agent_diff: str, signals: dict, model: str,
          sleep=time.sleep) -> dict:
    """Bounded retries; a judge that never answers is a harness error, not a
    verdict on the agent."""
    ref = (TRUTH / f"{task.id}.diff").read_text()
    prompt = JUDGE_PROMPT.format(task=task.prompt, ref=_truncate(ref),
                                 agent=_truncate(agent_diff or "(no changes)"),
                                 signals=json.dumps(signals, indent=1))
    errors = []
    for attempt in range(JUDGE_ATTEMPTS):
        v = _judge_once(prompt, model)
        if v["verdict"] != "error":
            v["judge_attempts"] = attempt + 1
            return v
        errors.append(v["reason"])
        if attempt + 1 < JUDGE_ATTEMPTS:
            sleep(JUDGE_BACKOFF_S[attempt])
    return {"verdict": "error", "judge_attempts": JUDGE_ATTEMPTS, "reason": " | ".join(errors)[-800:]}


def _hidden_tests(truth: dict, tree: Path) -> dict:
    """Overlay the reference's test files on a copy of the agent tree and run them."""
    tests = truth.get("ref_tests", [])
    if not tests or not truth.get("suites"):
        return {}
    ref_tree = WORK / "hidden-ref"
    add_worktree(truth["ref"], ref_tree)
    backups: dict[str, bytes] = {}
    copied: list[str] = []
    try:
        for t in tests:
            src, dst = ref_tree / t, tree / t
            if not src.exists():
                continue
            if dst.exists():
                backups[t] = dst.read_bytes()
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(src, dst)
            copied.append(t)
        out = {}
        for suite in truth["suites"]:
            r = run_suite(tree, suite, only=tests)
            out[suite] = {"passed": len(r["passed"]), "failed": len(r["failed"])}
        return out
    finally:
        for t in copied:
            if t in backups:
                (tree / t).write_bytes(backups[t])
            else:
                (tree / t).unlink(missing_ok=True)
        remove_worktree(ref_tree)


def confirm_new_failures(tree: Path, suite: str, new: set[str], baseline: set[str],
                         runner=run_suite) -> tuple[list[str], list[str]]:
    """A new failure counts only if the suite fails it again on a rerun.
    Pilot issue-C2 (runner-pr-base-guard C-0) scored a regression on a
    test_sync test that passes 4/4 on the same agent diff: a flaky test under
    load must not turn a solved run into an agent failure."""
    if not new:
        return [], []
    again = runner(tree, suite)
    if again.get("error"):
        return sorted(new), []
    still = new & (set(again["failed"]) - baseline)
    return sorted(still), sorted(new - still)


def check_code(task, truth: dict, tree: Path, diff: str, edited: list[str],
               out_dir: Path, use_judge: bool = True, judge_model: str = "opus") -> dict:
    res: dict = {}
    regression = {}
    ok = True
    harness_errors = []
    for suite in truth.get("suites", []):
        r = run_suite(tree, suite)
        if r.get("error"):
            harness_errors.append(f"suite {suite}: {r['error']}: {r.get('tail', '')[:300]}")
            regression[suite] = {"error": r["error"], "tail": r.get("tail", "")[:600]}
            continue
        baseline = set(truth.get("start_failures", {}).get(suite, []))
        new, flaky = confirm_new_failures(tree, suite, set(r["failed"]) - baseline, baseline)
        regression[suite] = {"failed": len(r["failed"]), "new_failures": new[:20]}
        if flaky:
            regression[suite]["flaky"] = flaky[:20]
        if new:
            ok = False
    if harness_errors:
        res.update({"outcome": "harness_error", "success": None,
                    "harness_errors": harness_errors, "regression": regression})
        (out_dir / "checks.json").write_text(json.dumps(res, indent=1))
        return res
    res["regression_ok"] = ok
    res["regression"] = regression
    hidden = _hidden_tests(truth, tree) if truth.get("suites") else {}
    passed = sum(v["passed"] for v in hidden.values())
    total = passed + sum(v["failed"] for v in hidden.values())
    res["hidden_pass_rate"] = round(passed / total, 3) if total else None
    expected = set(truth.get("expected_src", []))
    res["coverage"] = round(len(expected & set(edited)) / len(expected), 3) if expected else None
    signals = {"regression_ok": ok, "new_failures": {s: v["new_failures"] for s, v in regression.items()},
               "hidden_pass_rate": res["hidden_pass_rate"], "coverage_of_reference_files": res["coverage"]}
    if use_judge:
        v = judge(task, truth, diff, signals, judge_model)
        res["judge"] = v
        if v.get("verdict") == "error":
            res.update({"outcome": "harness_error", "success": None,
                        "harness_errors": [f"judge: {v.get('reason', '')[:300]}"]})
        else:
            res["success"] = ok and v.get("verdict") == "solved"
            res["outcome"] = "success" if res["success"] else "fail"
    else:
        res["success"] = None
        res["outcome"] = "unjudged"
    (out_dir / "checks.json").write_text(json.dumps(res, indent=1))
    return res



def check_question(task, truth: dict, final_text: str) -> dict:
    listed: list[str] = []
    if "AFFECTED:" in final_text:
        block = final_text.rsplit("AFFECTED:", 1)[1]
        for line in block.splitlines():
            line = line.strip().strip("-*` ").split(" ")[0].split(":")[0]
            if line.endswith(".py"):
                listed.append(line.lstrip("./"))
    listed = sorted(set(listed))
    truth_set = set(truth["callers"])
    acceptable = (truth_set | set(truth.get("defined_in", [])) | set(truth.get("test_callers", []))
                  | set(truth.get("mentioned_only", [])))
    hit = truth_set & set(listed)
    recall = len(hit) / len(truth_set) if truth_set else 1.0
    precision = len([f for f in listed if f in acceptable]) / len(listed) if listed else 0.0
    success = recall >= 0.9 and precision >= 0.8
    return {"answer_files": listed, "missed": sorted(truth_set - set(listed)),
            "spurious": sorted(set(listed) - acceptable),
            "recall": round(recall, 3), "precision": round(precision, 3),
            "success": success, "outcome": "success" if success else "fail"}
