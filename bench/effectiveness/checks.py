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


def judge(task, truth: dict, agent_diff: str, signals: dict, model: str) -> dict:
    ref = (TRUTH / f"{task.id}.diff").read_text()
    prompt = JUDGE_PROMPT.format(task=task.prompt, ref=_truncate(ref),
                                 agent=_truncate(agent_diff or "(no changes)"),
                                 signals=json.dumps(signals, indent=1))
    scratch = WORK / "judge"
    scratch.mkdir(parents=True, exist_ok=True)
    proc = subprocess.run(
        ["claude", "-p", prompt, "--model", model, "--output-format", "json",
         "--setting-sources", "project", "--strict-mcp-config", "--tools", "",
         "--no-session-persistence"],
        cwd=scratch, capture_output=True, text=True, timeout=600)
    try:
        payload = json.loads(proc.stdout)
        result = [x for x in payload if x.get("type") == "result"][0] if isinstance(payload, list) else payload
        text = result.get("result", "")
        cost = result.get("total_cost_usd")
    except (json.JSONDecodeError, IndexError):
        return {"verdict": "error", "reason": (proc.stdout + proc.stderr)[-400:]}
    match = re.search(r"\{.*\}", text, re.S)
    try:
        verdict = json.loads(match.group(0)) if match else {"verdict": "error", "reason": text[:400]}
    except json.JSONDecodeError:
        verdict = {"verdict": "error", "reason": text[:400]}
    verdict["judge_cost_usd"] = cost
    return verdict


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


def check_code(task, truth: dict, tree: Path, diff: str, edited: list[str],
               out_dir: Path, use_judge: bool = True, judge_model: str = "opus") -> dict:
    res: dict = {}
    regression = {}
    ok = True
    for suite in truth.get("suites", []):
        r = run_suite(tree, suite)
        baseline = set(truth.get("start_failures", {}).get(suite, []))
        new = sorted(set(r["failed"]) - baseline)
        regression[suite] = {"failed": len(r["failed"]), "new_failures": new[:20],
                             "error": r.get("error")}
        if new or r.get("error"):
            ok = False
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
        res["success"] = ok and v.get("verdict") == "solved"
    else:
        res["success"] = None
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
    acceptable = truth_set | set(truth.get("defined_in", [])) | set(truth.get("test_callers", []))
    hit = truth_set & set(listed)
    recall = len(hit) / len(truth_set) if truth_set else 1.0
    precision = len([f for f in listed if f in acceptable]) / len(listed) if listed else 0.0
    return {"answer_files": listed, "missed": sorted(truth_set - set(listed)),
            "spurious": sorted(set(listed) - acceptable),
            "recall": round(recall, 3), "precision": round(precision, 3),
            "success": recall >= 0.9 and precision >= 0.8}
