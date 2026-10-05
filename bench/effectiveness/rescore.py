"""Re-run the success checks of recorded runs after a harness fix.

Usage: python3 rescore.py results/<label>.jsonl RUN_ID [RUN_ID ...] [--judge-model opus]
       python3 rescore.py results/<label>.jsonl --questions

Question runs are re-scored from their recorded answer against the current
truth (no tree, no judge); `--questions` re-scores every question run.

Rebuilds each run's tree at the task's start commit, applies its recorded
agent.diff, runs check_code again (suites, hidden tests, judge) and rewrites
the run's line in the jsonl and its record.json. The previous verdict is kept
under `rescored_from` so the change stays auditable.
"""

import json
import sys
from datetime import UTC, datetime

from checks import check_code, check_question
from common import RESULTS, TRUTH, WORK, add_worktree, git, load_json, load_tasks, remove_worktree, uv_sync
from run import attribute

CHECK_KEYS = ("outcome", "success", "regression_ok", "regression", "hidden_pass_rate",
              "coverage", "judge", "harness_errors")


QUESTION_KEYS = ("answer_files", "missed", "spurious", "recall", "precision", "success", "outcome")


def rescore_question(record: dict, task, truth: dict) -> dict:
    before = {k: record.get(k) for k in QUESTION_KEYS if k in record}
    record.update(check_question(task, truth, record.get("final_text", "")))
    if record.get("arm") != "A":
        record["attribution"] = attribute(record, truth, True)
    record["rescored_from"] = before
    record["rescored_at"] = datetime.now(UTC).isoformat()
    return record


def rescore(record: dict, judge_model: str) -> dict:
    task = load_tasks()[record["task"]]
    truth = load_json(TRUTH / f"{task.id}.json")
    out_dir = RESULTS / "runs" / record["run_id"]
    if task.is_question:
        record = rescore_question(record, task, truth)
        (out_dir / "record.json").write_text(json.dumps(record, indent=1))
        return record
    diff = (out_dir / "agent.diff").read_text()
    tree = WORK / "rescore" / record["run_id"]
    add_worktree(truth["start"], tree)
    try:
        if diff.strip():
            git("apply", "--whitespace=nowarn", str(out_dir / "agent.diff"), cwd=tree)
        uv_sync(tree)
        before = {k: record.get(k) for k in CHECK_KEYS if k in record}
        for k in CHECK_KEYS:
            record.pop(k, None)
        record.update(check_code(task, truth, tree, diff, record.get("files_edited", []), out_dir,
                                 judge_model=judge_model))
        record["attribution"] = attribute(record, truth, False)
        record["rescored_from"] = before
        record["rescored_at"] = datetime.now(UTC).isoformat()
    finally:
        remove_worktree(tree)
    (out_dir / "record.json").write_text(json.dumps(record, indent=1))
    return record


def main() -> None:
    args = sys.argv[1:]
    judge_model = "opus"
    if "--judge-model" in args:
        i = args.index("--judge-model")
        judge_model = args[i + 1]
        args = args[:i] + args[i + 2:]
    path, ids = args[0], set(args[1:])
    questions = "--questions" in ids
    ids.discard("--questions")
    question_ids = {t for t, task in load_tasks().items() if task.is_question}
    lines = []
    with open(path) as fh:
        for line in fh:
            if not line.strip():
                continue
            r = json.loads(line)
            if r["run_id"] in ids or (questions and r["task"] in question_ids):
                r = rescore(r, judge_model)
                print(f"[{r['run_id']}] outcome={r['outcome']} was={r['rescored_from'].get('outcome')}")
            lines.append(json.dumps(r))
    with open(path, "w") as fh:
        fh.write("\n".join(lines) + "\n")


if __name__ == "__main__":
    main()
