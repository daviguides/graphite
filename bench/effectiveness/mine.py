"""Mine Continuum git history for commits that change a Python function
signature and update its callers across files — ideal bench tasks because
the historical diff is the ground truth."""

import json
import re
import subprocess
import sys
from pathlib import Path

REPO = Path.home() / "work/sources/continuum"
DEF_RE = re.compile(r"^([+-])\s*(?:async\s+)?def\s+(\w+)\s*\((.*)")


def git(*args: str) -> str:
    return subprocess.run(
        ["git", "-C", str(REPO), *args],
        capture_output=True, text=True, check=True,
    ).stdout


def commits() -> list[tuple[str, str]]:
    out = git("log", "--no-merges", "--format=%H|%s", "--", "*.py")
    return [tuple(line.split("|", 1)) for line in out.splitlines() if line]


def analyze(sha: str) -> dict | None:
    diff = git("show", "--format=", "-U0", sha, "--", "*.py")
    files = git("show", "--format=", "--name-only", sha).split()
    py_files = [f for f in files if f.endswith(".py")]
    src_files = [f for f in py_files if "/tests/" not in f and not Path(f).name.startswith("test_")]
    test_files = [f for f in py_files if f not in src_files]
    removed: dict[str, str] = {}
    added: dict[str, str] = {}
    for line in diff.splitlines():
        m = DEF_RE.match(line)
        if not m:
            continue
        sign, name, rest = m.groups()
        (removed if sign == "-" else added)[name] = rest.strip()
    changed = [
        n for n in removed
        if n in added and removed[n] != added[n] and not n.startswith("__")
    ]
    if not changed:
        return None
    callers_touched = []
    for name in changed:
        call_re = re.compile(rf"^\+.*\b{re.escape(name)}\(")
        files_with_calls = set()
        current = None
        for line in diff.splitlines():
            if line.startswith("+++ b/"):
                current = line[6:]
            elif call_re.match(line) and not DEF_RE.match(line):
                files_with_calls.add(current)
        callers_touched.append((name, sorted(files_with_calls)))
    return {
        "sha": sha,
        "changed_defs": changed,
        "callers": callers_touched,
        "src_files": src_files,
        "test_files": test_files,
        "n_files": len(py_files),
    }


def main() -> None:
    results = []
    for sha, subject in commits():
        info = analyze(sha)
        if not info:
            continue
        caller_files = {f for _, fs in info["callers"] for f in fs}
        if len(caller_files) < 2 or info["n_files"] > 15:
            continue
        info["subject"] = subject
        info["n_caller_files"] = len(caller_files)
        results.append(info)
    json.dump(results, sys.stdout, indent=1)


if __name__ == "__main__":
    main()
