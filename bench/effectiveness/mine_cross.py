"""Mine Continuum history for cross-module tasks: commits whose non-test
Python source changes span at least two tools (tools/<group>/<tool>), with
tests in the same commit and a size an agent can do in one session.

Usage: python3 mine_cross.py [--min-tools 2] > work/cross.json
"""

import json
import sys
from collections import Counter

from common import SOURCE_REPO, TEST_PATH_RE, run

EXCLUDE_SUBJECT = ("merge", "revert", "chore", "bump", "release", "format", "lint", "docs")


def git(*args: str) -> str:
    return run(["git", "-C", str(SOURCE_REPO), *args]).stdout


def tool_of(path: str) -> str | None:
    parts = path.split("/")
    if parts[0] != "tools" or len(parts) < 4:
        return None
    if parts[1] == "canvas":  # tools/canvas is itself one tool
        return "tools/canvas"
    return "/".join(parts[:3])


def main() -> None:
    min_tools = int(sys.argv[sys.argv.index("--min-tools") + 1]) if "--min-tools" in sys.argv else 2
    out = []
    for line in git("log", "--no-merges", "--format=%H|%s", "--", "tools/*.py").splitlines():
        sha, subject = line.split("|", 1)
        if subject.lower().startswith(EXCLUDE_SUBJECT):
            continue
        stat = git("show", "--format=", "--numstat", sha)
        files = []
        for row in stat.splitlines():
            added, removed, path = row.split("\t", 2)
            if path.endswith(".py"):
                files.append((path, int(added) if added != "-" else 0, int(removed) if removed != "-" else 0))
        src = [f for f in files if not TEST_PATH_RE.search(f[0])]
        tests = [f for f in files if TEST_PATH_RE.search(f[0])]
        tools = Counter(t for t in (tool_of(f[0]) for f in src) if t)
        if len(tools) < min_tools or not tests:
            continue
        src_lines = sum(a + r for _, a, r in src)
        if not 30 <= src_lines <= 600 or len(src) > 12:
            continue
        out.append({"sha": sha[:10], "subject": subject, "tools": dict(tools),
                    "src_files": [f[0] for f in src], "test_files": [f[0] for f in tests],
                    "src_lines": src_lines})
    json.dump(out, sys.stdout, indent=1)


if __name__ == "__main__":
    main()
