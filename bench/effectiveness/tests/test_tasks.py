"""Task statements are real issues: no path or symbol the reference touches."""

import sys
from pathlib import Path

import pytest

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))

from common import TRUTH, load_tasks  # noqa: E402
from leaks import find_leaks, touched  # noqa: E402

TASKS = load_tasks()
CODE = [t for t in TASKS.values() if not t.is_question]

DIFF = """\
diff --git a/tools/orch/regent/regent/core/delivery.py b/tools/orch/regent/regent/core/delivery.py
--- a/tools/orch/regent/regent/core/delivery.py
+++ b/tools/orch/regent/regent/core/delivery.py
@@ -10,3 +10,4 @@ def score(task):
-def _is_ancestor(a, b):
+def _is_ancestor(a, b, *, fetch=True):
+class TaskTarget:
+def run(x):
"""


def test_touched_collects_paths_and_defs_from_changed_lines_and_hunk_headers():
    paths, symbols = touched(DIFF)
    assert paths == {"tools/orch/regent/regent/core/delivery.py"}
    assert symbols == {"score", "_is_ancestor", "TaskTarget", "run"}


@pytest.mark.parametrize("statement,leak", [
    ("Bug in regent/core/delivery.py", "file delivery.py"),
    ("`_is_ancestor` returns returncode == 0", "symbol _is_ancestor"),
    ("make is_ancestor tri-state", "symbol _is_ancestor"),
    ("replace the tuples with a TaskTarget", "symbol TaskTarget"),
    ("then call `run` again", "symbol run"),
])
def test_leaks_are_found(statement, leak):
    assert leak in find_leaks(statement, DIFF)


@pytest.mark.parametrize("statement", [
    "The delivery scoreboard reports merged tasks as delivered nothing.",
    "Re-run the task; an ancestry check must answer yes / no / could-not-tell.",
    "`regent run close` must not score a merge against the tip.",
])
def test_symptom_language_is_not_a_leak(statement):
    assert find_leaks(statement, DIFF) == []


def test_allowed_names_are_not_leaks():
    assert find_leaks("edit delivery.py", DIFF, allow=["delivery.py"]) == []


@pytest.mark.parametrize("task", CODE, ids=lambda t: t.id)
def test_issue_statement_names_no_location(task):
    diff = (TRUTH / f"{task.id}.diff").read_text()
    assert find_leaks(task.prompt, diff, task.leak_allow) == []


@pytest.mark.parametrize("task", CODE, ids=lambda t: t.id)
def test_every_code_task_keeps_its_hinted_statement(task):
    assert task.statement_hinted and task.statement_hinted != task.prompt
    assert task.statement("hinted") == task.statement_hinted
    assert task.statement("issue") == task.prompt


def test_the_detector_sees_what_the_old_statements_gave_away():
    """Most hinted statements named a file or function; the check must catch them."""
    leaky = [t.id for t in CODE
             if find_leaks(t.statement_hinted, (TRUTH / f"{t.id}.diff").read_text(), t.leak_allow)]
    assert len(leaky) >= len(CODE) * 2 // 3


def test_questions_are_not_localization_tasks():
    questions = [t for t in TASKS.values() if t.is_question]
    assert questions and not any(t.is_localization for t in questions)
    assert all(t.is_localization for t in CODE)
