"""Print what's needed to write a bench task from a Continuum commit:
message, file stat, changed signatures, and test functions the commit adds."""

import re
import subprocess
import sys
from pathlib import Path

REPO = Path.home() / "work/sources/continuum"
DEF_RE = re.compile(r"^([+-])\s*(?:async\s+)?def\s+(\w+)\s*\((.*)")


def git(*args: str) -> str:
    return subprocess.run(
        ["git", "-C", str(REPO), *args], capture_output=True, text=True, check=True
    ).stdout


for sha in sys.argv[1:]:
    print("=" * 100)
    print(git("show", "-s", "--format=%h %s%n%n%b", sha)[:1800])
    print(git("show", "--format=", "--stat=120", sha))
    diff = git("show", "--format=", "-U0", sha, "--", "*.py")
    current = None
    for line in diff.splitlines():
        if line.startswith("+++ b/"):
            current = line[6:]
            continue
        m = DEF_RE.match(line)
        if m and ("/tests/" not in (current or "")):
            print(f"  {m.group(1)} {current}: def {m.group(2)}({m.group(3)[:110]}")
        elif m and m.group(1) == "+" and m.group(2).startswith("test_"):
            print(f"  + TEST {current}: {m.group(2)}")
