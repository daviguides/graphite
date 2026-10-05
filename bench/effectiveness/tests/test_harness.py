"""Regression tests for the four harness bugs found in the first pilot, each
pinned to the recorded pilot run that exposed it.

Run: cd bench/effectiveness && python3 -m pytest tests -q
"""

import json
import sys
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

import checks  # noqa: E402
import common  # noqa: E402
import run  # noqa: E402
from stream import parse_stream  # noqa: E402

FIX = HERE / "fixtures"
NO_JUNIT = FIX / "pilot-A-sourcerer-monorepo-root-A-1-30c987"
JUDGE_ERROR = FIX / "pilot-A-regent-is-ancestor-tristate-A-1-5a6a29"
KILLED = FIX / "pilot-A-q-resolve-owner-A-1-962ecf"
CLEAN_QUESTION = FIX / "pilot-A-q-resolve-owner-A-0-abd59c"


def record(run_dir: Path) -> dict:
    return json.loads((run_dir / "record.json").read_text())


class FakeTask:
    id = "t"
    prompt = "p"


# ---- 1. a suite that did not run is a harness error, not an agent failure


def test_recorded_no_junit_run_is_what_the_fix_targets():
    reg = record(NO_JUNIT)["regression"]
    assert reg["tools/flow/sourcerer"]["error"] == "no-junit"
    assert record(NO_JUNIT)["success"] is False  # the old, wrong verdict


def test_suite_error_becomes_harness_error(monkeypatch, tmp_path):
    reg_error = record(NO_JUNIT)["regression"]["tools/flow/sourcerer"]["error"]
    monkeypatch.setattr(checks, "run_suite", lambda tree, suite, only=None: {
        "suite": suite, "error": reg_error, "tail": "Failed to spawn: `pytest`", "passed": [], "failed": []})
    monkeypatch.setattr(checks, "judge", lambda *a, **k: pytest.fail("judge must not run"))
    truth = {"suites": ["tools/flow/sourcerer"], "start_failures": {"tools/flow/sourcerer": []},
             "expected_src": [], "ref_tests": []}
    res = checks.check_code(FakeTask(), truth, tmp_path, "", [], tmp_path)
    assert res["outcome"] == "harness_error"
    assert res["success"] is None
    assert "no-junit" in res["harness_errors"][0]


def test_suite_runner_installs_extras_and_groups(monkeypatch, tmp_path):
    """The root cause: sourcerer keeps pytest in its `dev` extra."""
    (tmp_path / "suite").mkdir()
    seen = {}

    def fake_run(args, **kw):
        seen["args"] = args
        return type("P", (), {"stdout": "", "stderr": "", "returncode": 1})()

    monkeypatch.setattr(common, "run", fake_run)
    common.run_suite(tmp_path, "suite")
    for flag in ("--all-extras", "--all-groups", "--continue-on-collection-errors"):
        assert flag in seen["args"]


def test_zero_collected_tests_is_an_error(monkeypatch, tmp_path):
    (tmp_path / "suite").mkdir()

    def fake_run(args, **kw):
        xml = next(a for a in args if a.startswith("--junitxml=")).split("=", 1)[1]
        Path(xml).write_text('<testsuites><testsuite name="pytest" tests="0"/></testsuites>')
        return type("P", (), {"stdout": "no tests ran", "stderr": "", "returncode": 5})()

    monkeypatch.setattr(common, "run", fake_run)
    assert common.run_suite(tmp_path, "suite")["error"] == "no-tests-collected"


# ---- 2. judge failures are retried, then reported as harness errors


def test_recorded_judge_error_run_is_what_the_fix_targets():
    assert record(JUDGE_ERROR)["judge"]["verdict"] == "error"
    assert record(JUDGE_ERROR)["success"] is False  # the old, wrong verdict


def _judge_inputs(monkeypatch, tmp_path):
    (tmp_path / "t.diff").write_text("ref diff")
    monkeypatch.setattr(checks, "TRUTH", tmp_path)


def test_judge_retries_then_succeeds(monkeypatch, tmp_path):
    _judge_inputs(monkeypatch, tmp_path)
    replies = iter([record(JUDGE_ERROR)["judge"], {"verdict": "solved", "reason": "ok"}])
    monkeypatch.setattr(checks, "_judge_once", lambda prompt, model: next(replies))
    sleeps = []
    v = checks.judge(FakeTask(), {}, "diff", {}, "opus", sleep=sleeps.append)
    assert v["verdict"] == "solved" and v["judge_attempts"] == 2 and len(sleeps) == 1


def test_judge_exhausted_is_harness_error(monkeypatch, tmp_path):
    _judge_inputs(monkeypatch, tmp_path)
    monkeypatch.setattr(checks, "_judge_once", lambda prompt, model: record(JUDGE_ERROR)["judge"])
    monkeypatch.setattr(checks.time, "sleep", lambda s: None)
    monkeypatch.setattr(checks, "run_suite", lambda tree, suite, only=None: {
        "suite": suite, "passed": ["a"], "failed": []})
    truth = {"suites": ["s"], "start_failures": {"s": []}, "expected_src": [], "ref_tests": []}
    res = checks.check_code(FakeTask(), truth, tmp_path, "d", [], tmp_path)
    assert res["judge"]["judge_attempts"] == checks.JUDGE_ATTEMPTS
    assert res["outcome"] == "harness_error" and res["success"] is None


# ---- 3. a run killed from outside is a harness error; the agent runs in its own session


def test_recorded_killed_run_is_harness_error():
    rec = record(KILLED)
    assert rec["exit_code"] == 143
    parsed = parse_stream(KILLED / "stream.jsonl")
    assert not parsed["has_result_event"]
    problem = run.run_problem({**parsed, "exit_code": rec["exit_code"]})
    assert problem and "signal" in problem


def test_clean_recorded_run_has_no_problem():
    parsed = parse_stream(CLEAN_QUESTION / "stream.jsonl")
    assert run.run_problem({**parsed, "exit_code": 0}) is None


def test_budget_stop_is_the_agents_result_not_a_harness_error():
    rec = {"exit_code": 1, "has_result_event": True, "stop_reason": "error_max_budget_usd"}
    assert run.run_problem(rec) is None


def test_agent_process_gets_its_own_session():
    src = Path(run.__file__).read_text()
    assert "start_new_session=True" in src and "os.killpg" in src


# ---- 4. files read through Bash are counted, separately from the Read tool


def test_bash_reads_counted_on_recorded_run():
    known = set((FIX / "sourcerer-start-files.txt").read_text().split())
    parsed = parse_stream(NO_JUNIT / "stream.jsonl", known)
    assert parsed["n_files_read_tool"] == 0
    assert set(parsed["files_read_bash"]) == {
        "tools/flow/sourcerer/sourcerer/wt/integrations/filesystem.py",
        "tools/flow/sourcerer/sourcerer/wt/services/completer.py",
        "tools/flow/sourcerer/sourcerer/wt/services/navigator.py",
        "tools/flow/worktree/worktree/services/project.py",
    }
    assert parsed["n_files_read"] == 4
    # the old record missed every one of them
    assert record(NO_JUNIT)["files_read"] == []


def test_bash_reads_track_cd_and_skip_non_readers():
    from stream import bash_reads
    known = {"a/b/x.py", "a/y.py", "z.py"}
    root = "/w"
    got, cwd = bash_reads("cd a && cat y.py; grep -n foo b/x.py | head", root, ".", known)
    assert got == {"a/y.py", "a/b/x.py"} and cwd == "a"
    got, _ = bash_reads("uv run pytest ../z.py", root, "a", known)
    assert got == set()
    got, _ = bash_reads("sed -n 1,40p /w/z.py", root, "a", known)
    assert got == {"z.py"}


# ---- arm B setup/teardown against the real CLI (skipped if not built)

GRAPHITE = Path(__file__).resolve().parents[3] / "target/release/graphite"


@pytest.mark.skipif(not GRAPHITE.exists(), reason="graphite CLI not built")
def test_arm_b_setup_and_teardown_on_a_task_worktree(monkeypatch):
    import tomllib
    monkeypatch.setenv("GRAPHITE_BIN", str(GRAPHITE))
    arm = tomllib.loads((HERE.parent / "arms.toml").read_text())["arms"]["B"]
    common.ensure_mirror()
    truth = json.loads((HERE.parent / "truth/sourcerer-monorepo-root.json").read_text())
    tree = common.WORK / "smoke-armB"
    common.add_worktree(truth["start"], tree)
    try:
        setup = run.run_hooks(arm["setup"], tree)
        assert all(h["code"] == 0 for h in setup), setup
        status = json.loads(common.run([str(GRAPHITE), "status", "--repo", str(tree), "--json"]).stdout)
        assert status, status
        look = common.run([str(GRAPHITE), "lookup", "find_worktrees_root", "--repo", str(tree), "--json"],
                          check=False)
        assert look.returncode == 0, look.stderr
        teardown = run.run_hooks(arm["teardown"], tree)
        assert all(h["code"] == 0 for h in teardown), teardown
    finally:
        common.run([str(GRAPHITE), "daemon", "stop", "--repo", str(tree)], check=False)
        common.remove_worktree(tree)


# ---- attribution and statistics


def test_attribution_classes():
    truth = {"expected_src": ["a.py", "b.py"]}
    base = {"arm": "B", "outcome": "fail", "success": False, "graphite_calls": 2, "graphite_paths": ["c.py"]}
    assert run.attribute({**base, "graphite_calls": 0}, truth, False)["class"] == "graphite_not_used"
    assert run.attribute({**base, "outcome": "success", "success": True}, truth, False)["class"] == "graphite_used_success"
    caused = run.attribute({**base, "files_edited": ["a.py"], "graphite_paths": ["a.py"]}, truth, False)
    assert caused == {"class": "graphite_caused_failure", "omitted": ["b.py"]}
    despite = run.attribute({**base, "files_edited": ["a.py"], "graphite_paths": ["a.py", "b.py"]}, truth, False)
    assert despite["class"] == "failure_despite_graphite"
    assert run.attribute({**base, "arm": "A"}, truth, False) is None
    assert run.attribute({**base, "outcome": "harness_error"}, truth, False) is None


def test_calls_that_returned_no_paths_are_not_a_graphite_miss():
    # pilot-issue-C2 refinement-stays-finished rep 0: one call, `graphite lookup pause;
    # graphite lookup EGEST`, both NOT FOUND, 0 hook answers; the agent then missed
    # regent/core/pipeline.py, which `blast refiner_egest_done` lists first.
    rec = {"arm": "C", "outcome": "fail", "success": False, "graphite_calls": 1,
           "graphite_hook_answers": 0, "graphite_paths": [],
           "files_edited": ["refiner/reconcile.py"]}
    truth = {"expected_src": ["refiner/reconcile.py", "regent/core/pipeline.py"]}
    assert run.attribute(rec, truth, False) == {"class": "graphite_not_used",
                                                "reason": "no paths returned"}


def test_graphite_calls_parsed_from_stream(tmp_path):
    from stream import graphite_calls, paths_in
    out = {"ok": True, "data": {"result": {"candidates": [{"path": "x/y.py"}]}}}
    lines = [
        {"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "t1", "name": "Bash",
                                                       "input": {"command": "graphite lookup foo --json"}}]}},
        {"type": "user", "message": {"content": [{"type": "tool_result", "tool_use_id": "t1",
                                                  "content": json.dumps(out)}]}},
        {"type": "assistant", "message": {"content": [{"type": "tool_use", "id": "t2", "name": "Bash",
                                                       "input": {"command": "grep -rn foo ."}}]}},
    ]
    s = tmp_path / "s.jsonl"
    s.write_text("\n".join(json.dumps(l) for l in lines))
    calls = graphite_calls(s)
    assert len(calls) == 1 and calls[0]["json"] == [out]
    assert paths_in(calls[0]["json"]) == {"x/y.py"}


def test_go_requires_ci_upper_below_one():
    import analyze
    # 5 tasks, B sometimes much worse: point estimate may pass, CI must not
    stats = {}
    ratios = [0.5, 0.6, 0.7, 1.6, 1.8]
    for i, r in enumerate(ratios):
        stats[(f"t{i}", "A")] = {"turns": 10, "wall": 100, "success_rate": 1.0, "stale": 0}
        stats[(f"t{i}", "B")] = {"turns": 10 * r, "wall": 100 * r, "success_rate": 1.0, "stale": 0}
    v, lines = analyze.verdict(stats)
    assert v == "NO-GO"


def test_graphite_index_is_not_agent_work(tmp_path):
    common.run(["git", "init", "-q"], cwd=tmp_path)
    (tmp_path / "a.py").write_text("x = 1\n")
    common.run(["git", "add", "."], cwd=tmp_path)
    common.run(["git", "-c", "user.email=t@t", "-c", "user.name=t", "commit", "-qm", "i"], cwd=tmp_path)
    (tmp_path / ".graphite").mkdir()
    (tmp_path / ".graphite" / "db").write_bytes(b"\x81\xff\x00binary")
    (tmp_path / "a.py").write_text("x = 2\n")
    diff, names = run.agent_changes(tmp_path)
    assert names == ["a.py"] and ".graphite" not in diff


def test_search_after_complete_graphite_answer_counted():
    stream = FIX / "smoke-B-q-resolve-owner-B-0-b9560d/stream.jsonl"
    if not stream.exists():
        pytest.skip("smoke-B run not present")
    r = parse_stream(stream)
    assert r["graphite_complete_answer"] is True
    assert r["search_after_complete_graphite"] == 2


# ---- B2 text format and C hook-served answers


def _write_stream(tmp_path, lines):
    s = tmp_path / "s.jsonl"
    s.write_text("\n".join(json.dumps(l) for l in lines))
    return s


def _use(i, name, inp):
    return {"type": "assistant", "message": {"content": [{"type": "tool_use", "id": i, "name": name, "input": inp}]}}


def _res(i, content, is_error=False):
    return {"type": "user", "message": {"content": [
        {"type": "tool_result", "tool_use_id": i, "content": content, "is_error": is_error}]}}


def test_text_graphite_answer_parsed(tmp_path):
    from stream import graphite_calls
    text = ("resolve_owner(explicit=None)  tools/orch/dao-cli/dao_cli/core/project.py:471  [function]\n"
            "COMPLETE · 7 direct callers\n  tools/orch/runner/runner/core/hooks.py:88  owner = resolve_owner()\n")
    s = _write_stream(tmp_path, [
        _use("t1", "Bash", {"command": "graphite blast resolve_owner"}), _res("t1", text),
        _use("t2", "Bash", {"command": "grep -rn resolve_owner ."}),
    ])
    (c,) = graphite_calls(s)
    assert c["format"] == "text" and c["complete"] and c["bytes"] == len(text.encode())
    assert set(c["paths"]) == {"tools/orch/dao-cli/dao_cli/core/project.py",
                               "tools/orch/runner/runner/core/hooks.py"}
    r = parse_stream(s)
    assert r["graphite_complete_answer"] and r["search_after_complete_graphite"] == 1


def test_lower_bound_is_not_complete():
    from stream import answer_is_complete
    assert not answer_is_complete("LOWER-BOUND (2 ambiguous refs named `x` could be hidden callers)")
    assert not answer_is_complete("INCOMPLETE")


def test_hook_served_answer_counts_as_graphite(tmp_path):
    (tmp_path / "hooks.jsonl").write_text("\n".join([
        json.dumps({"tool": "Grep", "action": "deny", "answer": "COMPLETE · 2 callers\n  a/b.py:3  x()"}),
        json.dumps({"tool": "Read", "action": "allow"}),
    ]))
    hooks = run.hook_answers(tmp_path / "hooks.jsonl")
    assert len(hooks) == 1 and hooks[0]["complete"] and hooks[0]["paths"] == {"a/b.py"}
    rec = {"arm": "C", "outcome": "fail", "success": False, "graphite_calls": 0,
           "graphite_hook_answers": 1, "files_edited": [], "graphite_paths": ["a/b.py"]}
    assert run.attribute(rec, {"expected_src": ["a/b.py"]}, False)["class"] == "failure_despite_graphite"


def test_denied_search_with_graph_answer_sets_complete(tmp_path):
    s = _write_stream(tmp_path, [
        _use("g", "Grep", {"pattern": "x"}),
        _res("g", "graphite answered: COMPLETE · 1 caller  a.py:3", is_error=True),
        _use("h", "Grep", {"pattern": "y"}),
    ])
    r = parse_stream(s)
    assert r["hook_answers_in_stream"] == 1 and r["search_after_complete_graphite"] == 1


def test_arms_b2_and_c_defined():
    import tomllib
    arms = tomllib.loads((HERE.parent / "arms.toml").read_text())["arms"]
    assert "--json" not in " ".join(arms["B2"]["setup"]) + arms["B2"]["prompt_suffix"]
    assert any("hooks install" in c for c in arms["C"]["setup"])
    assert any("hooks uninstall" in c for c in arms["C"]["teardown"])
    assert arms["C"]["collect"] == [".graphite/hooks.jsonl"]


@pytest.mark.skipif(not GRAPHITE.exists(), reason="graphite CLI not built")
def test_arm_b2_setup_teardown_and_text_answer(monkeypatch):
    import tomllib
    from stream import answer_is_complete, text_paths
    monkeypatch.setenv("GRAPHITE_BIN", str(GRAPHITE))
    arm = tomllib.loads((HERE.parent / "arms.toml").read_text())["arms"]["B2"]
    common.ensure_mirror()
    truth = json.loads((HERE.parent / "truth/q-resolve-owner.json").read_text())
    tree = common.WORK / "smoke-armB2"
    common.add_worktree(truth["sha"], tree)
    try:
        assert all(h["code"] == 0 for h in run.run_hooks(arm["setup"], tree))
        out = common.run([str(GRAPHITE), "blast", "resolve_owner", "--repo", str(tree)], check=False).stdout
        assert out and not out.lstrip().startswith("{")
        assert text_paths(out) or "AMBIGUOUS" in out
        assert answer_is_complete(out) or "LOWER-BOUND" in out or "AMBIGUOUS" in out
        assert all(h["code"] == 0 for h in run.run_hooks(arm["teardown"], tree))
    finally:
        common.run([str(GRAPHITE), "daemon", "stop", "--repo", str(tree)], check=False)
        common.remove_worktree(tree)
