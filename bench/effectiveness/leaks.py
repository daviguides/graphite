"""Location leaks in a task statement.

An issue-style statement must not tell the agent where to look: no path the
reference diff touches and no symbol it defines or edits. User-facing names
(CLI commands and flags, error messages, config keys, task.yaml values) are
fine. Questions name their symbol by design and are not checked.

Usage: python3 leaks.py [task-id ...]   (exit 1 when any statement leaks)
"""

import re
import sys
from pathlib import Path

DEF_RE = re.compile(r"^\s*(?:async\s+)?def\s+(\w+)|^\s*class\s+(\w+)")
HUNK_RE = re.compile(r"^@@ [^@]* @@\s*(.*)$")
# Files whose names say nothing about where the change lives.
GENERIC_BASENAMES = {"__init__.py", "pyproject.toml", "uv.lock", "README.md",
                     "CHANGELOG.md", "CLAUDE.md", "conftest.py", "Makefile"}


def touched(diff: str) -> tuple[set[str], set[str]]:
    """Paths and symbols (defs/classes on changed lines or enclosing a hunk)."""
    paths: set[str] = set()
    symbols: set[str] = set()
    current = None
    for line in diff.splitlines():
        if line.startswith("+++ b/"):
            current = line[6:]
            paths.add(current)
            continue
        if line.startswith(("--- ", "+++ ", "diff --git")):
            continue
        if not (current or "").endswith(".py"):
            continue
        if m := HUNK_RE.match(line):
            if d := DEF_RE.match(m.group(1)):
                symbols.add(d.group(1) or d.group(2))
        elif line[:1] in "+-":
            if d := DEF_RE.match(line[1:]):
                symbols.add(d.group(1) or d.group(2))
    return paths, {s for s in symbols if not s.startswith("__") and not s.startswith("test_")
                   and not s.startswith("Test")}


def _distinctive(symbol: str) -> bool:
    return "_" in symbol.strip("_") or symbol.startswith("_") or any(c.isupper() for c in symbol)


def find_leaks(statement: str, diff: str, allow: list[str] = ()) -> list[str]:
    paths, symbols = touched(diff)
    leaks = []
    for p in sorted(paths):
        name = Path(p).name
        if p in statement:
            leaks.append(f"path {p}")
        elif name not in GENERIC_BASENAMES and re.search(rf"(?<![\w.-]){re.escape(name)}\b", statement):
            leaks.append(f"file {name}")
    for s in sorted(symbols):
        forms = {s, s.lstrip("_")}
        for f in forms:
            if not f:
                continue
            if _distinctive(f):
                hit = re.search(rf"(?<![\w-]){re.escape(f)}(?![\w-])", statement)
            else:  # plain lowercase word: a leak only when written as code
                hit = re.search(rf"`{re.escape(f)}`|(?<![\w-]){re.escape(f)}\(", statement)
            if hit:
                leaks.append(f"symbol {s}")
                break
    return [l for l in leaks if l.split(" ", 1)[1] not in set(allow)]


def main() -> int:
    from common import TRUTH, load_tasks
    tasks = load_tasks()
    ids = sys.argv[1:] or list(tasks)
    bad = 0
    for tid in ids:
        t = tasks[tid]
        if t.is_question:
            continue
        leaks = find_leaks(t.prompt, (TRUTH / f"{tid}.diff").read_text(), t.leak_allow)
        if leaks:
            bad += 1
            print(f"{tid}: {', '.join(leaks)}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
