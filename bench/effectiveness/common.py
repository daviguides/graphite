"""Shared paths, task loading, git and suite helpers for the effectiveness bench."""

import json
import os
import re
import shutil
import subprocess
import tomllib
import xml.etree.ElementTree as ET
from dataclasses import dataclass, field
from pathlib import Path

HERE = Path(__file__).resolve().parent
WORK = HERE / "work"
MIRROR = WORK / "mirror"
RUNS = WORK / "runs"
TRUTH = HERE / "truth"
RESULTS = HERE / "results"
SOURCE_REPO = Path.home() / "work/sources/continuum"

TEST_PATH_RE = re.compile(r"(^|/)tests?/|(^|/)test_[^/]*\.py$")


@dataclass
class Task:
    id: str
    kind: str
    difficulty: str
    ref: str
    prompt: str
    suites: list[str] = field(default_factory=list)
    start: str = "parent"
    symbol: str | None = None

    @property
    def is_question(self) -> bool:
        return self.kind == "question"


def load_tasks() -> dict[str, Task]:
    data = tomllib.loads((HERE / "tasks.toml").read_text())
    tasks = {}
    for raw in data["task"]:
        raw = dict(raw)
        raw["prompt"] = raw["prompt"].strip()
        tasks[raw["id"]] = Task(**raw)
    return tasks


def run(cmd: list[str], cwd: Path | None = None, check: bool = True,
        env: dict | None = None, timeout: float | None = None) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True,
                          check=check, env=env, timeout=timeout)


def git(*args: str, cwd: Path = MIRROR, check: bool = True) -> str:
    return run(["git", *args], cwd=cwd, check=check).stdout


def ensure_mirror() -> None:
    """Private clone of Continuum; its origin is removed so no agent can push
    back into the source repo."""
    if not MIRROR.exists():
        WORK.mkdir(parents=True, exist_ok=True)
        run(["git", "clone", "-q", "--no-checkout", str(SOURCE_REPO), str(MIRROR)])
    else:
        run(["git", "fetch", "-q", str(SOURCE_REPO), "+refs/heads/*:refs/remotes/src/*"],
            cwd=MIRROR, check=False)
    if "origin" in git("remote", cwd=MIRROR).split():
        git("remote", "remove", "origin", cwd=MIRROR)


def resolve_start(task: Task) -> str:
    ref = git("rev-parse", task.ref).strip()
    return ref if task.start == "ref" else git("rev-parse", f"{ref}~1").strip()


def add_worktree(sha: str, path: Path) -> None:
    if path.exists():
        remove_worktree(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    git("worktree", "add", "-q", "--detach", str(path), sha)


def remove_worktree(path: Path) -> None:
    git("worktree", "remove", "--force", str(path), check=False)
    if path.exists():
        shutil.rmtree(path, ignore_errors=True)
    git("worktree", "prune", check=False)


def changed_files(sha: str) -> list[str]:
    return [f for f in git("show", "--format=", "--name-only", sha).split() if f]


def split_src_tests(files: list[str]) -> tuple[list[str], list[str]]:
    py = [f for f in files if f.endswith(".py")]
    tests = [f for f in py if TEST_PATH_RE.search(f)]
    return [f for f in py if f not in tests], tests


def uv_sync(tree: Path) -> None:
    """Pre-warm the tools/ uv workspace so neither arm pays install time."""
    if (tree / "tools/pyproject.toml").exists():
        run(["uv", "sync", "--quiet", "--all-packages", "--directory", str(tree / "tools")],
            check=False, timeout=900)


def run_suite(tree: Path, suite: str, only: list[str] | None = None,
              timeout: float = 900) -> dict:
    """Run a tool's pytest suite; return passed/failed test ids.
    `only` restricts to specific test files (repo-relative)."""
    tool = tree / suite
    if not tool.exists():
        return {"suite": suite, "error": "missing", "failed": [], "passed": []}
    xml = tool / ".bench-junit.xml"
    args = ["uv", "run", "--quiet", "pytest", "-q", "-p", "no:cacheprovider",
            f"--junitxml={xml}", "-o", "addopts="]
    if only:
        rel = [str((tree / f).relative_to(tool)) for f in only if (tree / f).exists()
               and (tree / f).is_relative_to(tool)]
        if not rel:
            return {"suite": suite, "failed": [], "passed": [], "skipped_only": True}
        args += rel
    env = dict(os.environ)
    env.pop("VIRTUAL_ENV", None)
    try:
        proc = run(args, cwd=tool, check=False, env=env, timeout=timeout)
    except subprocess.TimeoutExpired:
        return {"suite": suite, "error": "timeout", "failed": [], "passed": []}
    passed, failed = [], []
    if xml.exists():
        for case in ET.parse(xml).getroot().iter("testcase"):
            tid = f"{case.get('classname')}::{case.get('name')}"
            if case.find("failure") is not None or case.find("error") is not None:
                failed.append(tid)
            elif case.find("skipped") is None:
                passed.append(tid)
        xml.unlink()
    else:
        return {"suite": suite, "error": "no-junit", "tail": proc.stdout[-1500:] + proc.stderr[-1500:],
                "failed": [], "passed": []}
    return {"suite": suite, "passed": passed, "failed": failed}


def load_json(path: Path, default=None):
    return json.loads(path.read_text()) if path.exists() else default


def dump_json(path: Path, data) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=1))
