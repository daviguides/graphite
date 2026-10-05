"""Freeze ground truth per task into truth/<id>.json (+ truth/<id>.diff).

Code tasks: start/ref shas, files the reference touched, the reference diff,
suite failures at the start tree (regression baseline), and how the
reference's own test files behave at start vs ref (discrimination check).
Question tasks: the non-test files that call the symbol at the pinned commit.

Usage: python3 build_truth.py [task-id ...] [--no-suites]
"""

import io
import re
import sys
import tokenize

from common import (MIRROR, TRUTH, WORK, add_worktree, changed_files, dump_json,
                    ensure_mirror, git, load_tasks, remove_worktree, resolve_start,
                    run_suite, split_src_tests, uv_sync)


def _suite_or_die(tree, suite: str, label: str) -> dict:
    """A baseline built from a suite that failed to run is silently wrong
    (every later failure looks new, or none do) — refuse to freeze it."""
    r = run_suite(tree, suite)
    if r.get("error"):
        raise SystemExit(f"suite {suite} at {label} did not run ({r['error']}): {r.get('tail', '')[:400]}")
    return r


def calls_in_code(source: str, symbol: str) -> bool:
    """True if `symbol(` occurs as code: a NAME token followed by `(`, not
    inside a comment, docstring or string. Unparsable files fall back to
    the textual match."""
    try:
        toks = [t for t in tokenize.generate_tokens(io.StringIO(source).readline)
                if t.type not in (tokenize.NL, tokenize.NEWLINE, tokenize.INDENT,
                                  tokenize.DEDENT, tokenize.COMMENT)]
    except (tokenize.TokenError, SyntaxError, IndentationError):
        return f"{symbol}(" in source
    return any(a.type == tokenize.NAME and a.string == symbol and b.string == "("
               for a, b in zip(toks, toks[1:]))


def question_truth(task, sha: str) -> dict:
    """Callers = files that call the symbol in code. Files that only name it
    in a comment, docstring or string are kept apart as `mentioned_only`:
    not required for recall, not spurious for precision."""
    pattern = f"{task.symbol}("
    out = git("grep", "-n", "-F", pattern, sha, "--", "*.py", check=False)
    candidates: set[str] = set()
    defs: set[str] = set()
    for line in out.splitlines():
        _, path, _, text = line.split(":", 3)
        if re.match(rf"\s*(async\s+)?def\s+{re.escape(task.symbol)}\(", text):
            defs.add(path)
            continue
        candidates.add(path)
    callers = {p for p in candidates if calls_in_code(git("show", f"{sha}:{p}"), task.symbol)}
    src, tests = split_src_tests(sorted(callers))
    return {"id": task.id, "kind": "question", "sha": sha, "symbol": task.symbol,
            "defined_in": sorted(defs), "callers": src, "test_callers": tests,
            "mentioned_only": sorted(candidates - callers)}


def code_truth(task, start: str, ref: str, with_suites: bool) -> dict:
    files = changed_files(ref)
    src, tests = split_src_tests(files)
    diff = git("show", "--format=", ref, "--", ".", ":(exclude)*.lock", ":(exclude)*uv.lock")
    (TRUTH / f"{task.id}.diff").write_text(diff)
    truth = {"id": task.id, "kind": task.kind, "start": start, "ref": ref,
             "expected_src": src, "ref_tests": tests, "suites": task.suites,
             "ref_diff_lines": diff.count("\n")}
    if not with_suites or not task.suites:
        return truth
    start_tree, ref_tree = WORK / "truth-start", WORK / "truth-ref"
    add_worktree(start, start_tree)
    add_worktree(ref, ref_tree)
    try:
        uv_sync(start_tree)
        uv_sync(ref_tree)
        truth["start_failures"] = {s: _suite_or_die(start_tree, s, "start")["failed"] for s in task.suites}
        truth["ref_failures"] = {s: _suite_or_die(ref_tree, s, "ref")["failed"] for s in task.suites}
        hidden_ref = {s: run_suite(ref_tree, s, only=tests) for s in task.suites}
        for s in task.suites:
            for t in tests:
                src_file, dst = ref_tree / t, start_tree / t
                if src_file.exists() and src_file.is_relative_to(ref_tree / s):
                    dst.parent.mkdir(parents=True, exist_ok=True)
                    dst.write_bytes(src_file.read_bytes())
        hidden_start = {s: run_suite(start_tree, s, only=tests) for s in task.suites}
        truth["hidden_at_ref"] = {s: {"passed": len(r["passed"]), "failed": len(r["failed"])}
                                  for s, r in hidden_ref.items()}
        truth["hidden_at_start"] = {s: {"passed": len(r["passed"]), "failed": len(r["failed"])}
                                    for s, r in hidden_start.items()}
    finally:
        remove_worktree(start_tree)
        remove_worktree(ref_tree)
    return truth


def main() -> None:
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    with_suites = "--no-suites" not in sys.argv
    ensure_mirror()
    TRUTH.mkdir(exist_ok=True)
    tasks = load_tasks()
    for tid in args or list(tasks):
        task = tasks[tid]
        start = resolve_start(task)
        ref = git("rev-parse", task.ref).strip()
        truth = question_truth(task, start) if task.is_question else code_truth(task, start, ref, with_suites)
        dump_json(TRUTH / f"{tid}.json", truth)
        summary = (f"callers={len(truth['callers'])}" if task.is_question else
                   f"src={len(truth['expected_src'])} tests={len(truth['ref_tests'])} "
                   f"hidden@start={truth.get('hidden_at_start')} hidden@ref={truth.get('hidden_at_ref')}")
        print(f"{tid}: {summary}", flush=True)


if __name__ == "__main__":
    main()
