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


def test_report_compares_arm_c_and_has_no_money(monkeypatch):
    import analyze
    base = {"kind": "bugfix", "difficulty": "medium", "model": "sonnet", "outcome": "success",
            "success": True, "cost_usd": 0.5, "tool_calls": 5}
    rows = []
    for t in ("t1", "t2"):
        rows.append({**base, "run_id": f"{t}-A", "task": t, "arm": "A", "num_turns": 10, "wall_s": 100})
        rows.append({**base, "run_id": f"{t}-C", "task": t, "arm": "C", "num_turns": 6, "wall_s": 60,
                     "graphite_calls": 0, "graphite_hook_answers": 3, "attribution": {"class": "graphite_used_success"},
                     "hooks": {"answers": 3, "enrich": 0, "fallbacks": 0, "reasks_after_complete": 0,
                               "answer_bytes": 900, "raw_bytes": 4000, "latency_ms": 30,
                               "intra_overlap_keys": 0, "intra_overlap_bytes": 0, "intra_overlap_commands": 0,
                               "cross_overlap_keys": 1, "cross_overlap_bytes": 40, "answer_keys": 9,
                               "overlap_window_ms": 10000}})
    monkeypatch.setattr(analyze, "TREAT", "C")
    report = analyze.render(rows)
    assert "C/A" in report and "$" not in report and "cost" not in report.lower()
    assert "overlap across calls" in report
    c_rows = [l for l in report.splitlines() if l.startswith("| t1 | bugfix | medium | C |")]
    assert c_rows and "| 0 / 3 |" in c_rows[0]  # graphite calls / hook answers column for arm C


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


HOOK_EVENTS = [  # real schema: crates/hooks/src/log.rs (feat/interception)
    {"ts": 1, "event": "pre", "tool": "Bash", "original_command": "grep -rn resolve_owner tools",
     "action": "rewrite", "rewritten_command": "/x/graphite-hook run --cwd /w -- 'grep -rn resolve_owner tools'",
     "latency_ms": 3},
    {"ts": 2, "event": "exec", "tool": "Bash", "segment": "grep -rn resolve_owner tools", "kind": "search",
     "action": "answer", "answer": "resolve_owner  a/core.py:9  [function]\nCOMPLETE · 2 callers\n  a/b.py:3  x()",
     "answer_bytes": 80, "raw_bytes": 900, "matches": 3, "graph_verdict": "complete",
     "residue": {"definition": 1, "reference": 2}, "search_ms": 4, "total_ms": 6},
    {"ts": 3, "event": "pre", "tool": "Bash", "original_command": "rg -n resolve_owner --type-add x",
     "action": "passthrough", "reason": "contains a command graphite does not handle", "latency_ms": 1},
    {"ts": 4, "event": "exec", "tool": "Bash", "segment": "grep -rn owner tools", "kind": "search",
     "action": "answer", "answer": "LOWER-BOUND (1 ambiguous)", "answer_bytes": 25, "raw_bytes": 400,
     "graph_verdict": "lower_bound", "total_ms": 5},
    {"ts": 5, "event": "exec", "tool": "Bash", "segment": "grep -rn foo .", "kind": "search",
     "action": "fallback", "reason": "daemon timeout"},
    {"ts": 6, "event": "pre", "tool": "Grep", "original_command": "resolve_owner", "action": "passthrough"},
    {"ts": 7, "event": "post", "tool": "Read", "original_command": "a/core.py", "action": "enrich",
     "answer": "callers: a/b.py:3", "answer_bytes": 17, "graph_verdict": "complete"},
    {"ts": 8, "event": "post", "tool": "Edit", "action": "nudge"},
]


def test_hooks_summary_real_schema(tmp_path):
    from stream import hooks_summary
    p = tmp_path / "hooks.jsonl"
    p.write_text("\n".join(json.dumps(e) for e in HOOK_EVENTS))
    h = hooks_summary(p)
    assert h["rewrites"] == 1 and h["answers"] == 2 and h["enrich"] == 1 and h["fallbacks"] == 1
    assert h["complete"] and h["verdicts"] == {"complete": 2, "lower_bound": 1}
    assert h["answer_bytes"] == 80 + 25 + 17 and h["raw_bytes"] == 900 + 400
    assert set(h["paths"]) == {"a/core.py", "a/b.py"}
    # after the first complete answer: rg passthrough, grep fallback, native Grep = 3 unanswered
    # searches; the second grep the graph answered is a re-ask, not a residual search
    assert h["searches_after_complete"] == 3 and h["reasks_after_complete"] == 1
    assert h["passthrough_reasons"]["daemon timeout"] == 1


LINE_B3 = len("  a/b.py:3  x()".encode()) + 1


def test_cross_call_overlap_from_text_within_window():
    from stream import answer_overlap
    a = {"ts": 0, "event": "exec", "original_command": "grep -rn x a", "answer": "COMPLETE\n  a/b.py:3  x()\n  a/c.py:9  y()"}
    b = {"ts": 4_000, "event": "exec", "original_command": "grep -rn z a", "answer": "COMPLETE\n  a/b.py:3  x()\n  a/d.py:1  z()"}
    late = {"ts": 30_000, "event": "exec", "original_command": "grep -rn w a", "answer": "  a/b.py:3  x()"}
    o = answer_overlap([a, b, late])
    assert o["cross_overlap_answers"] == 1 and o["cross_overlap_keys"] == 1 and o["cross_overlap_bytes"] == LINE_B3
    assert o["intra_overlap_keys"] == 0 and o["answer_keys"] == 5


def test_intra_command_overlap_across_segments():
    from stream import answer_overlap
    cmd = "grep -rn x a; grep -rn x b && grep -rn xy a"
    s1 = {"ts": 0, "event": "exec", "original_command": cmd, "segment": "grep -rn x a", "answer": "  a/b.py:3  x()\n  a/c.py:9  y()"}
    s2 = {"ts": 5, "event": "exec", "original_command": cmd, "segment": "grep -rn x b", "answer": "  a/e.py:2  x()"}
    s3 = {"ts": 9, "event": "exec", "original_command": cmd, "segment": "grep -rn xy a", "answer": "  a/b.py:3  x()"}
    other = {"ts": 2_000, "event": "exec", "original_command": "grep -rn y a", "answer": "  a/c.py:9  y()"}
    o = answer_overlap([s1, s2, s3, other])
    assert o["intra_overlap_commands"] == 1 and o["intra_overlap_keys"] == 1 and o["intra_overlap_bytes"] == LINE_B3
    assert o["cross_overlap_keys"] == 1  # a/c.py:9 repeated by a different command


def test_overlap_uses_logged_keys_ids_and_sessions():
    from stream import answer_overlap
    a = {"ts": 0, "session_id": "s1", "call_id": "c1", "keys": ["a.py:1", "b.py:2"], "answer_bytes": 100}
    other_session = {"ts": 1_000, "session_id": "s2", "call_id": "c2", "keys": ["a.py:1"], "answer_bytes": 40}
    same = {"ts": 2_000, "session_id": "s1", "call_id": "c3", "keys": [{"path": "a.py", "line": 1}, "c.py:3"],
            "answer_bytes": 60}
    same_call = {"ts": 2_001, "session_id": "s1", "call_id": "c3", "keys": ["c.py:3"], "answer_bytes": 10}
    o = answer_overlap([a, other_session, same, same_call])
    assert o["cross_overlap_answers"] == 1 and o["cross_overlap_keys"] == 1 and o["cross_overlap_bytes"] == 30
    assert o["intra_overlap_keys"] == 1 and o["intra_overlap_bytes"] == 10


def test_cross_call_overlap_same_turn_ignores_window():
    from stream import answer_overlap
    a = {"ts": 0, "session_id": "s", "call_id": "c1", "turn_id": "t1", "keys": ["a.py:1"], "answer_bytes": 10}
    b = {"ts": 60_000, "session_id": "s", "call_id": "c2", "turn_id": "t1", "keys": ["a.py:1"], "answer_bytes": 10}
    c = {"ts": 120_000, "session_id": "s", "call_id": "c3", "turn_id": "t2", "keys": ["a.py:1"], "answer_bytes": 10}
    o = answer_overlap([a, b, c])
    assert o["cross_overlap_answers"] == 1  # b repeats a in the same turn; c is a later turn, > 10 s


def test_hooks_summary_reports_overlap(tmp_path):
    from stream import hooks_summary
    p = tmp_path / "hooks.jsonl"
    p.write_text("\n".join(json.dumps(e) for e in HOOK_EVENTS))
    h = hooks_summary(p)
    # the Read enrich (ts 7) repeats a/b.py:3 from the first grep answer (ts 2): a different call
    assert h["cross_overlap_keys"] == 1 and h["cross_overlap_bytes"] > 0 and h["intra_overlap_keys"] == 0


def test_hooks_summary_no_log(tmp_path):
    from stream import hooks_summary
    h = hooks_summary(tmp_path / "missing.jsonl")
    assert h["events"] == 0 and not h["complete"] and h["searches_after_complete"] is None


def test_hook_served_answer_counts_as_graphite():
    rec = {"arm": "C", "outcome": "fail", "success": False, "graphite_calls": 0,
           "graphite_hook_answers": 2, "files_edited": [], "graphite_paths": ["a/b.py"]}
    assert run.attribute(rec, {"expected_src": ["a/b.py"]}, False)["class"] == "failure_despite_graphite"
    rec["graphite_hook_answers"] = 0
    assert run.attribute(rec, {"expected_src": ["a/b.py"]}, False)["class"] == "graphite_not_used"


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
    assert any("{graphite_hook} install" in c for c in arms["C"]["setup"])
    assert any("{graphite_hook} uninstall" in c for c in arms["C"]["teardown"])
    assert arms["C"]["collect"] == [".graphite/hooks.jsonl"]
    assert "graphite-hook" in arms["C"]["requires"]


INTERCEPTION = HERE.parent / "work/target-feat-interception/release"


@pytest.mark.skipif(not (INTERCEPTION / "graphite-hook").exists(),
                    reason="run ./build_interception.sh first")
def test_arm_c_dry_smoke(monkeypatch, tmp_path):
    """Arm C without the model: setup installs hooks in the task worktree,
    a routed grep and a pre-hook decision go through graphite-hook and land in
    hooks.jsonl, collection + summary work, teardown restores settings.json
    and leaves no agent diff."""
    import tomllib
    from stream import hooks_summary
    monkeypatch.setenv("GRAPHITE_BIN", str(INTERCEPTION / "graphite"))
    monkeypatch.setenv("GRAPHITE_HOOK_BIN", str(INTERCEPTION / "graphite-hook"))
    hook = str(INTERCEPTION / "graphite-hook")
    arm = tomllib.loads((HERE.parent / "arms.toml").read_text())["arms"]["C"]
    common.ensure_mirror()
    truth = json.loads((HERE.parent / "truth/q-resolve-owner.json").read_text())
    tree = common.WORK / "smoke-armC"
    common.add_worktree(truth["sha"], tree)
    try:
        settings = tree / ".claude/settings.json"
        before = settings.read_text() if settings.exists() else None
        snap = run.snapshot(tree, arm["restore"])
        setup = run.run_hooks(arm["setup"], tree)
        assert all(h["code"] == 0 for h in setup), setup
        assert hook in settings.read_text()

        # what Claude Code would do: pre decision, then the rewritten command
        cmd = "grep -rn resolve_owner tools/orch/dao-cli"
        payload = json.dumps({"tool_name": "Bash", "tool_input": {"command": cmd}, "cwd": str(tree)})
        pre = __import__("subprocess").run([hook, "pre"], input=payload, cwd=tree,
                                           capture_output=True, text=True)
        assert pre.returncode == 0, pre.stderr
        decision = json.loads(pre.stdout)["hookSpecificOutput"]["updatedInput"]["command"]
        assert "graphite-hook" in decision and " run " in decision
        out = __import__("subprocess").run(decision, shell=True, cwd=tree, capture_output=True, text=True)
        assert out.returncode == 0 and out.stdout

        out_dir = tmp_path / "run"
        out_dir.mkdir()
        for rel in arm["collect"]:
            import shutil as _sh
            _sh.copyfile(tree / rel, out_dir / Path(rel).name)
        h = hooks_summary(out_dir / "hooks.jsonl")
        assert h["rewrites"] >= 1 and (h["answers"] + h["fallbacks"]) >= 1, h
        assert h["errors"] == 0, h

        teardown = run.run_hooks(arm["teardown"], tree)
        assert all(t["code"] == 0 for t in teardown), teardown
        run.restore(tree, snap)
        after = settings.read_text() if settings.exists() else None
        assert after == before
        diff, names = run.agent_changes(tree)
        assert names == [] and diff == ""
    finally:
        common.run([str(INTERCEPTION / "graphite"), "daemon", "stop", "--repo", str(tree)], check=False)
        common.remove_worktree(tree)


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


def test_flaky_new_failure_is_not_a_regression(tmp_path):
    rerun = lambda tree, suite: {"failed": ["base::t", "x::regressed"], "passed": ["x::flaky"]}
    new, flaky = checks.confirm_new_failures(tmp_path, "s", {"x::flaky", "x::regressed"}, {"base::t"}, runner=rerun)
    assert new == ["x::regressed"]
    assert flaky == ["x::flaky"]


def test_no_new_failures_skips_the_rerun(tmp_path):
    def boom(tree, suite):
        raise AssertionError("must not rerun")
    assert checks.confirm_new_failures(tmp_path, "s", set(), set(), runner=boom) == ([], [])
