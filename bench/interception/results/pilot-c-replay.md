Replay of 59 Bash decisions (10 runs).

Recorded in the pilot: 12 rewritten, 47 passed through.

| build | rewritten | passed through (reason: n) | hook bytes | plain bytes | ratio | exit-code mismatches | search answers | sites lost | header missing |
|---|---|---|---|---|---|---|---|---|---|
| old | 12 | unsupported shell construct: 24, contains a command graphite does not handle: 16, nothing graphite can answer: 7 | 67034 | 51651 | 1.30 | 0 | 13 | 0 of 0 | 0 |
| new | 18 | unsupported shell construct: 17, contains a command graphite does not handle: 16, nothing graphite can answer: 8 | 150898 | 138284 | 1.09 | 0 | 20 | 0 of 21 | 0 |

Commands rewritten by both builds: 12 — plain 51651 B, old 67034 B (1.30×), new 58080 B (1.12×).

Newly rewritten by new: 6
- `cd tools/orch; grep -rn "_pin_model\|_MODEL_PINS" refiner intake runner/runner --include=*.py | grep -v "^refiner/refiner/core/sdk_wrapper.py:\(9\|10\|11\)"; ca`
- `cd {tree}/tools/orch; grep -rn "_pin_model\|_MODEL_PINS\|import json\|json\.\|Path" refiner intake runner/runner --include='*.py' | grep -E "_pin_model|_MODEL_P`
- `cd tools/orch; sed -n 60,130p continuum-core/continuum_core/sdk.py; sed -n 80,130p refiner/refiner/core/sdk_wrapper.py; sed -n 1,90p intake/intake/core/digest.p`
- `cd {tree}/tools/orch; sed -n 1,25p refiner/pyproject.toml; sed -n 55,75p intake/pyproject.toml; grep -n "continuum" runner/pyproject.toml; grep -n "^import\|^fr`
- `f=$(find . -path '*regent/core/delivery.py' -not -path '*/node_modules/*' | head -1); echo $f; wc -l $f; cat $f`
- `f=$(find . -path ./node_modules -prune -o -name delivery.py -path '*regent*' -print | head -1); echo $f; wc -l $f; cat -n $f`

Still passed through by new:
- unsupported shell construct: `cd {tree}/tools/orch; python3 - <<'E'
import re
p='continuum-core/continuum_core/sdk.py'
s=open(p).read()
a=s.index('def _pin_model'); b=s.i`
- unsupported shell construct: `cd {tree}/tools/orch; 
python3 - <<'E'
for p in ['refiner/pyproject.toml','intake/pyproject.toml']:
    s=open(p).read()
    s=s.replace('  `
- unsupported shell construct: `cd {tree}/tools; (uv sync -q 2>&1 | tail -3); cd orch/intake && uv run pytest -q 2>&1 | tail -3; cd ../refiner && uv run pytest -q 2>&1 | ta`
- contains a command graphite does not handle: `cd {tree}/tools/orch/intake; git stash list >/dev/null; uv run pytest -q tests/test_triage.py 2>&1 | grep -E "Error|assert" | head -5; git s`
- unsupported shell construct: `cd {tree}/tools/orch; python3 - <<'E'
import re
p='continuum-core/continuum_core/sdk.py'
s=open(p).read()
s=s.replace('''def _pin_model(mode`
- contains a command graphite does not handle: `cd {tree}/tools/orch; sed -i '13{/^import json$/d}' refiner/refiner/core/sdk_wrapper.py; grep -n "json" intake/intake/core/digest.py | head `
- unsupported shell construct: `cd {tree}/tools/orch; python3 - <<'E'
p='intake/pyproject.toml'
s=open(p).read()
s=s.replace('    "dao-cli",\n','    "continuum-core",\n    `
- contains a command graphite does not handle: `cd {tree}/tools/orch; sed -i '' '13{/^import json$/d;}' refiner/refiner/core/sdk_wrapper.py; grep -n "json" refiner/refiner/core/sdk_wrapper`
- nothing graphite can answer: `graphite lookup resolve_owner; graphite blast resolve_owner`
- nothing graphite can answer: `graphite blast resolve_owner --all | grep -v tests/ | head -30`
- nothing graphite can answer: `graphite lookup resolve_owner; graphite blast resolve_owner`
- contains a command graphite does not handle: `graphite blast resolve_owner --all | grep -E "^\s+tools/.*:[0-9]" | grep -v tests/ | awk '{print $1}' | cut -d: -f1 | sort -u`
- unsupported shell construct: `cd tools/orch/regent/regent/core && python3 - <<'EOF'
p='delivery.py'
s=open(p).read()

s=s.replace('''    """Run git, returning code and st`
- contains a command graphite does not handle: `cd tools/orch/regent 2>/dev/null; git stash list >/dev/null; uv run pytest -q tests/test_delivery.py 2>&1 | grep -E "Error|assert" | head -1`
- unsupported shell construct: `cd tools/orch/regent && python3 - <<'EOF'
p='tests/test_delivery.py'
s=open(p).read()
s=s.replace('''        if sub == "show" and "--numstat`
- nothing graphite can answer: `pwd; git status --short`
- unsupported shell construct: `python3 - <<'EOF'
p='tests/test_delivery.py'
s=open(p).read()
s=s.replace('''        if sub == "show" and "--numstat" in cmd:
            re`
- unsupported shell construct: `python3 - <<'EOF'
p='tests/test_delivery.py'
s=open(p).read()
a=s.index('        first_fetch = next(')
b=s.index('    def test_the_diff_is_a`
- unsupported shell construct: `cd tools/orch/regent/regent/core; python3 - <<'E'
s=open('delivery.py').read()
def rep(a,b):
    global s
    assert s.count(a)==1,a
    s=s`
- contains a command graphite does not handle: `uv run --quiet pytest -q -k delivery 2>&1 | tail -20`
- unsupported shell construct: `python3 - <<'E'
p='tests/test_delivery.py'
s=open(p).read()
s=s.replace('''            return _completed(0 if self.answers.get("ancestor", T`
- nothing graphite can answer: `graphite lookup post_hook_validate_pr_target; graphite blast post_hook_validate_pr_target --depth 2; graphite lookup execute_finalizing`
- unsupported shell construct: `cd tools/orch/runner && python3 - <<'EOF'
p='runner/core/hooks.py'
s=open(p).read()
a=s.index("def post_hook_validate_pr_target(")
b=s.index`
- contains a command graphite does not handle: `cd tools/orch/runner && python -m pytest tests/test_engine.py tests/test_validating_reject_route_back.py -x -q 2>&1 | grep -E "Error|error" `
- contains a command graphite does not handle: `python -m pytest tests/test_engine.py tests/test_validating_reject_route_back.py -q 2>&1 | grep -E "^E " | head -6`
- contains a command graphite does not handle: `python -m pytest tests/test_engine.py tests/test_validating_reject_route_back.py -q 2>&1 | tail -8; git stash push -q -m tmpchk && python -m`
- contains a command graphite does not handle: `uv run pytest tests -q 2>&1 | tail -4; uv run pytest tests/test_engine.py -q 2>&1 | grep -E "^E " | head -4`
- unsupported shell construct: `cat >> tests/test_pr_target_guard.py <<'EOF'
"""PR base guard: validator result and the pre-merge re-read."""

import subprocess
from pathli`
- nothing graphite can answer: `graphite lookup post_hook_validate_pr_target; graphite blast post_hook_validate_pr_target --depth 1; graphite lookup execute_finalizing`
- unsupported shell construct: `cd tools/orch/runner && python - <<'EOF'
import re
p='runner/core/hooks.py'
s=open(p).read()
a=s.index("def post_hook_validate_pr_target(")
`
- unsupported shell construct: `cd tools/orch/runner 2>/dev/null || true; (uv run pytest tests -q -x 2>&1 || true) | tail -15; grep -rn "def emit" -A6 ../continuum-core/ --`
- contains a command graphite does not handle: `cd tools/orch/runner; uv run pytest tests -q 2>&1 | tail -15; git stash list >/dev/null; git diff --stat`
- nothing graphite can answer: `graphite lookup _resolve_externalized_worktrees_root; graphite blast find_worktrees_root --depth 1`
- unsupported shell construct: `python3 - <<'EOF'
p='tools/flow/sourcerer/sourcerer/wt/integrations/filesystem.py'
s=open(p).read()
s=s.replace('''def find_worktrees_root(p`
- contains a command graphite does not handle: `uv run --all-extras pytest tests/wt -q 2>&1 | tail -8; grep -n "cwd" sourcerer/wt/services/completer.py | head`
- contains a command graphite does not handle: `git stash list >/dev/null; git diff --quiet HEAD -- tests; uv run --all-extras pytest tests/wt/test_filesystem.py -q -k test_returns_real_di`
- nothing graphite can answer: `graphite lookup _resolve_externalized_worktrees_root; graphite blast _resolve_externalized_worktrees_root --depth 3; graphite lookup find_wo`
- unsupported shell construct: `python3 - <<'EOF'
p='tools/flow/sourcerer/sourcerer/wt/integrations/filesystem.py'
s=open(p).read()
s=s.replace('''def find_worktrees_root(p`
- contains a command graphite does not handle: `uv sync --all-extras -q 2>&1 | tail -3; uv run pytest tests/wt -q 2>&1 | tail -8`
- contains a command graphite does not handle: `git stash push -q -m tmpcheck-x && uv run pytest tests/wt/test_filesystem.py -q 2>&1 | grep -E "FAILED|passed|failed"; git stash list --form`
- contains a command graphite does not handle: `git stash apply 6a463c4b264202cc3d983e0dfd3931db84324dc6 -q && git stash drop -q stash@{0} ; git status --short`
