"""Parse a `claude -p --output-format stream-json --verbose` transcript into
per-run metrics: turns, tool calls by category, files read, cost, tokens."""

import json
import re
from collections import Counter
from pathlib import Path

SEARCH = {"grep", "rg", "ag", "ack", "git-grep"}
LIST = {"find", "ls", "tree", "fd"}
READ = {"cat", "head", "tail", "less", "wc", "sed", "awk", "nl"}
RUN = {"pytest", "uv", "python", "python3", "make", "ruff", "mypy", "pip"}


EDIT_PATTERNS = [
    re.compile(r"\bsed\s+(-\w*\s+)*-i"),
    re.compile(r"\bperl\s+(-\w*\s+)*-i"),
    re.compile(r"(write_text|\.write\(|open\([^)]*['\"][wa]['\"])"),
    re.compile(r"(^|[;&|]\s*)(cat|echo|printf|tee)\b[^|]*>\s*\S+\.(py|toml|md|json|ya?ml)\b"),
]


def is_bash_edit(command: str) -> bool:
    return any(p.search(command) for p in EDIT_PATTERNS)


def bash_category(command: str) -> str:
    cmd = command.strip().lstrip("( ")
    cmd = re.sub(r"^(cd\s+\S+\s*(&&|;)\s*)+", "", cmd).lstrip("( ")
    if is_bash_edit(cmd):
        return "edit"
    first = cmd.split()[0] if cmd.split() else ""
    first = first.rsplit("/", 1)[-1]
    if first == "git":
        sub = cmd.split()[1] if len(cmd.split()) > 1 else ""
        return "search" if sub == "grep" else "git"
    if first == "graphite":
        return "graphite"
    if first in SEARCH:
        return "search"
    if first in LIST:
        return "list"
    if first in READ:
        return "read"
    if first in RUN:
        return "run"
    return "other"


def parse_stream(path: Path) -> dict:
    tools = Counter()
    bash = Counter()
    files_read: set[str] = set()
    subagent_calls = 0
    result: dict = {}
    first_edit_index = None
    call_index = 0
    graphite_stale = 0
    for line in path.read_text().splitlines():
        try:
            ev = json.loads(line)
        except json.JSONDecodeError:
            continue
        typ = ev.get("type")
        if typ == "assistant":
            nested = ev.get("parent_tool_use_id") is not None
            for block in ev.get("message", {}).get("content", []) or []:
                if block.get("type") != "tool_use":
                    continue
                name = block.get("name", "?")
                inp = block.get("input", {}) or {}
                call_index += 1
                if nested:
                    subagent_calls += 1
                tools[name] += 1
                if name == "Bash":
                    cat = bash_category(inp.get("command", ""))
                    bash[cat] += 1
                    if cat == "edit" and first_edit_index is None:
                        first_edit_index = call_index
                elif name == "Read" and inp.get("file_path"):
                    files_read.add(inp["file_path"])
                if name in {"Edit", "Write", "NotebookEdit", "MultiEdit"} and first_edit_index is None:
                    first_edit_index = call_index
        elif typ == "user":
            for block in ev.get("message", {}).get("content", []) or []:
                if isinstance(block, dict) and block.get("type") == "tool_result":
                    text = json.dumps(block.get("content", ""))
                    if '"stale": true' in text or '\\"stale\\": true' in text:
                        graphite_stale += 1
        elif typ == "result":
            result = ev
    usage = result.get("usage", {}) or {}
    return {
        "num_turns": result.get("num_turns"),
        "duration_ms": result.get("duration_ms"),
        "cost_usd": result.get("total_cost_usd"),
        "is_error": result.get("is_error"),
        "stop_reason": result.get("subtype"),
        "final_text": result.get("result", "") or "",
        "input_tokens": usage.get("input_tokens"),
        "output_tokens": usage.get("output_tokens"),
        "cache_read_tokens": usage.get("cache_read_input_tokens"),
        "cache_creation_tokens": usage.get("cache_creation_input_tokens"),
        "tool_calls": sum(tools.values()),
        "tools": dict(tools),
        "bash": dict(bash),
        "subagent_tool_calls": subagent_calls,
        "files_read": sorted(files_read),
        "calls_before_first_edit": (first_edit_index - 1) if first_edit_index else None,
        "graphite_stale_results": graphite_stale,
    }
