"""Parse a `claude -p --output-format stream-json --verbose` transcript into
per-run metrics: turns, tool calls by category, files read, cost, tokens."""

import json
import posixpath
import re
import shlex
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


READERS = READ | SEARCH | {"rg", "less", "more", "bat", "diff", "file", "stat"}
SEGMENT_SPLIT = re.compile(r"&&|\|\||;|\||\n")


def _rel(path: str, root: str, cwd: str) -> str | None:
    if path.startswith(root + "/"):
        return posixpath.normpath(path[len(root) + 1:])
    if path.startswith("/"):
        return None
    return posixpath.normpath(posixpath.join(cwd, path))


def bash_reads(command: str, root: str, cwd: str, known: set[str]) -> tuple[set[str], str]:
    """Repo files a shell command reads (cat/head/sed -n/grep FILE/rg FILE/...),
    and the working directory after it (cd persists across Bash calls)."""
    body = command.split("<<", 1)[0]
    found: set[str] = set()
    for seg in SEGMENT_SPLIT.split(body):
        seg = seg.strip().lstrip("( ").rstrip(") ")
        if not seg:
            continue
        try:
            toks = shlex.split(seg)
        except ValueError:
            toks = seg.split()
        if not toks:
            continue
        head = toks[0].rsplit("/", 1)[-1]
        if head == "cd" and len(toks) > 1:
            target = _rel(toks[1], root, cwd) if toks[1] != root else "."
            if target is not None:
                cwd = target
            continue
        if head == "git" and len(toks) > 1 and toks[1] == "grep":
            head = "grep"
        if head not in READERS:
            continue
        for tok in toks[1:]:
            if tok.startswith("-") or any(c in tok for c in "*?[]{}$"):
                continue
            for cand in (_rel(tok, root, cwd), _rel(tok, root, ".")):
                if cand and cand in known:
                    found.add(cand)
                    break
    return found, cwd


def _result_text(content) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "".join(c.get("text", "") for c in content if isinstance(c, dict))
    return json.dumps(content)


def graphite_calls(path: Path) -> list[dict]:
    """Every `graphite` command the agent ran, with its output (JSON when parseable)."""
    pending: dict[str, dict] = {}
    calls: list[dict] = []
    for line in path.read_text(errors="replace").splitlines():
        try:
            ev = json.loads(line)
        except json.JSONDecodeError:
            continue
        for block in (ev.get("message") or {}).get("content", []) or []:
            if not isinstance(block, dict):
                continue
            if block.get("type") == "tool_use" and block.get("name") == "Bash":
                cmd = (block.get("input") or {}).get("command", "")
                if re.search(r"(^|[\s;&|/(])graphite\s", cmd):
                    pending[block["id"]] = {"command": cmd}
            elif block.get("type") == "tool_result" and block.get("tool_use_id") in pending:
                call = pending.pop(block["tool_use_id"])
                text = _result_text(block.get("content"))
                call["is_error"] = bool(block.get("is_error"))
                parsed = []
                for ln in text.splitlines():
                    ln = ln.strip()
                    if ln.startswith("{"):
                        try:
                            parsed.append(json.loads(ln))
                        except json.JSONDecodeError:
                            pass
                call["json"] = parsed
                call["raw"] = text if not parsed else ""
                call["bytes"] = len(text.encode())
                call["format"] = "json" if parsed else "text"
                call["complete"] = answer_is_complete(text)
                call["paths"] = sorted(paths_in(parsed) | text_paths(text))
                calls.append(call)
    calls.extend(pending.values())
    return calls


TEXT_PATH_RE = re.compile(r"(?<![\w/.-])(?:\./)?((?:[\w.-]+/)*[\w-][\w.-]*\.(?:py|rs|ts|tsx|js|jsx)):\d+")
COMPLETE_TEXT_RE = re.compile(r"(?<![A-Za-z_-])COMPLETE\b")
COMPLETE_JSON_RE = re.compile(r'"completeness":\{[^{}]*(\{[^{}]*\}[^{}]*)?"status":"complete"')


def answer_is_complete(text: str) -> bool:
    """Complete Graphite answer in either format: JSON envelope or text verdict."""
    return bool(COMPLETE_JSON_RE.search(text) or COMPLETE_TEXT_RE.search(text))


def text_paths(text: str) -> set[str]:
    return set(TEXT_PATH_RE.findall(text or ""))


def paths_in(obj) -> set[str]:
    out: set[str] = set()
    if isinstance(obj, dict):
        for k, v in obj.items():
            if k in ("path", "file") and isinstance(v, str):
                out.add(v)
            else:
                out |= paths_in(v)
    elif isinstance(obj, list):
        for v in obj:
            out |= paths_in(v)
    return out


GRAPH_ANSWER_ACTIONS = {"answer", "enrich"}   # exec: Graphite produced the output; post: context added
UNANSWERED_EXEC = {"fallback", "plain"}        # exec: the original command ran
OVERLAP_WINDOW_MS = 10_000
KEY_LINE_RE = re.compile(r"(?<![\w/.-])(?:\./)?((?:[\w.-]+/)*[\w-][\w.-]*\.(?:py|rs|ts|tsx|js|jsx)):(\d+)")
KEY_FIELDS = ("keys", "path_lines", "locations", "answer_keys")


def _answer_keys(e: dict) -> dict[str, int]:
    """`path:line` keys an answer showed, with the bytes each accounts for.
    Uses the event's own key list when the hook logs one (strings "path:line"
    or {path, line} objects); otherwise parses the answer text line by line."""
    for field in KEY_FIELDS:
        raw = e.get(field)
        if isinstance(raw, list) and raw:
            keys = []
            for k in raw:
                if isinstance(k, str):
                    keys.append(k)
                elif isinstance(k, dict) and k.get("path") is not None:
                    keys.append(f"{k['path']}:{k.get('line', k.get('start_line', ''))}")
            if keys:
                per = (e.get("answer_bytes") or len((e.get("answer") or "").encode())) / len(keys)
                return {k: int(per) for k in keys}
    out: dict[str, int] = {}
    for line in (e.get("answer") or "").splitlines():
        m = KEY_LINE_RE.search(line)
        if m:
            key = f"{m.group(1)}:{m.group(2)}"
            out[key] = out.get(key, 0) + len(line.encode()) + 1
    return out


def _command_id(e: dict, i: int) -> str:
    """Which agent command an answer belongs to: the hook's call id when logged,
    else the command as the agent wrote it (all exec segments of one compound
    command share it); a post-hook enrich is its own call."""
    for f in ("call_id", "id", "tool_use_id"):
        if e.get(f):
            return f"{f}:{e[f]}"
    if e.get("event") == "exec" and e.get("original_command"):
        return f"cmd:{e.get('session_id')}:{e['original_command']}"
    return f"event:{i}"


def answer_overlap(answers: list[dict], window_ms: int = OVERLAP_WINDOW_MS) -> dict:
    """path:line keys a graph answer repeats — context the agent paid for twice.

    intra: repeats across segments of ONE compound command (`grep a; grep b`,
           `&&`, `||`: same call, several exec answers), no time window.
    cross: repeats from answers of OTHER calls in the same session (session_id
           when logged) within `window_ms`.
    """
    intra_keys = intra_bytes = intra_cmds = 0
    cross_keys = cross_bytes = cross_answers = 0
    seen: list[tuple[int, str | None, str, dict[str, int]]] = []
    by_cmd: dict[str, set[str]] = {}
    intra_hit: set[str] = set()
    total_keys = 0
    for i, e in enumerate(answers):
        ts, sess, cid, keys = e.get("ts", 0), e.get("session_id"), _command_id(e, i), _answer_keys(e)
        total_keys += len(keys)
        same_cmd = by_cmd.setdefault(cid, set())
        rep_intra = [k for k in keys if k in same_cmd]
        if rep_intra:
            intra_hit.add(cid)
            intra_keys += len(rep_intra)
            intra_bytes += sum(keys[k] for k in rep_intra)
        others = [k for t, s, c, k in seen
                  if c != cid and ts - t <= window_ms and (sess is None or s is None or s == sess)]
        prior = set().union(*others) if others else set()
        rep_cross = [k for k in keys if k in prior and k not in rep_intra]
        if rep_cross:
            cross_answers += 1
            cross_keys += len(rep_cross)
            cross_bytes += sum(keys[k] for k in rep_cross)
        same_cmd |= set(keys)
        seen.append((ts, sess, cid, keys))
    intra_cmds = len(intra_hit)
    return {"answer_keys": total_keys, "overlap_window_ms": window_ms,
            "intra_overlap_commands": intra_cmds, "intra_overlap_keys": intra_keys,
            "intra_overlap_bytes": intra_bytes,
            "cross_overlap_answers": cross_answers, "cross_overlap_keys": cross_keys,
            "cross_overlap_bytes": cross_bytes}


def hooks_summary(path: Path) -> dict:
    """Summarize `.graphite/hooks.jsonl` (schema: crates/hooks/src/log.rs on
    feat/interception). Events: pre (rewrite | passthrough), exec per segment
    (answer | fallback | plain | cd), post (enrich | nudge | passthrough)."""
    events = []
    if path.exists():
        for line in path.read_text(errors="replace").splitlines():
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    events.sort(key=lambda e: e.get("ts", 0))
    actions = Counter(f"{e.get('event')}:{e.get('action')}" for e in events)
    verdicts = Counter(e.get("graph_verdict") for e in events
                       if e.get("action") in GRAPH_ANSWER_ACTIONS and e.get("graph_verdict"))
    reasons = Counter(e.get("reason") for e in events if e.get("reason"))
    paths: set[str] = set()
    first_complete = None
    searches_after = reasks_after = 0
    for e in events:
        act, ev = e.get("action"), e.get("event")
        if act in GRAPH_ANSWER_ACTIONS:
            paths |= text_paths(e.get("answer") or "")
            if first_complete is None and e.get("graph_verdict") == "complete":
                first_complete = e.get("ts", 0)
                continue
        if first_complete is None:
            continue
        if ev == "exec" and e.get("kind") == "search":
            if act == "answer":
                reasks_after += 1
            elif act in UNANSWERED_EXEC:
                searches_after += 1
        elif ev == "pre" and e.get("tool") in ("Grep", "Glob"):
            searches_after += 1
        elif (ev == "pre" and act == "passthrough" and e.get("tool") == "Bash"
              and bash_category(e.get("original_command", "")) in ("search", "list")):
            searches_after += 1
    answers = sum(1 for e in events if e.get("event") == "exec" and e.get("action") == "answer")
    overlap = answer_overlap([e for e in events if e.get("action") in GRAPH_ANSWER_ACTIONS])
    return {
        **overlap,
        "sessions": sorted({e["session_id"] for e in events if e.get("session_id")}),
        "events": len(events),
        "actions": dict(actions),
        "rewrites": actions.get("pre:rewrite", 0),
        "answers": answers,
        "enrich": actions.get("post:enrich", 0),
        "fallbacks": actions.get("exec:fallback", 0),
        "errors": sum(1 for e in events if e.get("error")),
        "verdicts": dict(verdicts),
        "complete": first_complete is not None,
        "answer_bytes": sum(e.get("answer_bytes") or 0 for e in events if e.get("action") in GRAPH_ANSWER_ACTIONS),
        "raw_bytes": sum(e.get("raw_bytes") or 0 for e in events if e.get("action") == "answer"),
        "latency_ms": sum(e.get("latency_ms") or 0 for e in events),
        "graph_ms": sum(e.get("total_ms") or 0 for e in events),
        "passthrough_reasons": dict(reasons),
        "paths": sorted(paths),
        "searches_after_complete": searches_after if first_complete is not None else None,
        "reasks_after_complete": reasks_after if first_complete is not None else None,
    }


def parse_stream(path: Path, known_files: set[str] | None = None) -> dict:
    """known_files: repo-relative paths that exist in the worktree; enables
    counting reads done through Bash."""
    tools = Counter()
    bash = Counter()
    files_read: set[str] = set()
    files_read_bash: set[str] = set()
    root = ""
    shell_cwd = "."
    pending_graphite: set[str] = set()
    complete_seen = False
    search_after_complete = 0
    hook_answers = 0
    subagent_calls = 0
    result: dict = {}
    first_edit_index = None
    call_index = 0
    graphite_stale = 0
    for line in path.read_text(errors="replace").splitlines():
        try:
            ev = json.loads(line)
        except json.JSONDecodeError:
            continue
        typ = ev.get("type")
        if typ == "system" and ev.get("subtype") == "init":
            root = (ev.get("cwd") or "").rstrip("/")
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
                    if re.search(r"(^|[\s;&|/(])graphite\s", inp.get("command", "")):
                        pending_graphite.add(block.get("id"))
                    if complete_seen and not nested and (cat == "search" or cat == "list"):
                        search_after_complete += 1
                    if known_files is not None and root and not nested:
                        got, shell_cwd = bash_reads(inp.get("command", ""), root, shell_cwd, known_files)
                        files_read_bash |= got
                    if cat == "edit" and first_edit_index is None:
                        first_edit_index = call_index
                elif name == "Read" and inp.get("file_path"):
                    fp = inp["file_path"]
                    files_read.add(fp[len(root) + 1:] if root and fp.startswith(root + "/") else fp)
                if name in {"Grep", "Glob"} and complete_seen and not nested:
                    search_after_complete += 1
                if name in {"Edit", "Write", "NotebookEdit", "MultiEdit"} and first_edit_index is None:
                    first_edit_index = call_index
        elif typ == "user":
            for block in ev.get("message", {}).get("content", []) or []:
                if isinstance(block, dict) and block.get("type") == "tool_result":
                    body = _result_text(block.get("content"))
                    if block.get("tool_use_id") in pending_graphite and answer_is_complete(body):
                        complete_seen = True
                    elif block.get("is_error") and answer_is_complete(body):
                        # A PreToolUse hook that denies a search returns the graph's answer as the
                        # (error) tool result.
                        hook_answers += 1
                        complete_seen = True
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
        "terminal_reason": result.get("terminal_reason"),
        "final_text": result.get("result", "") or "",
        "input_tokens": usage.get("input_tokens"),
        "output_tokens": usage.get("output_tokens"),
        "cache_read_tokens": usage.get("cache_read_input_tokens"),
        "cache_creation_tokens": usage.get("cache_creation_input_tokens"),
        "tool_calls": sum(tools.values()),
        "tools": dict(tools),
        "bash": dict(bash),
        "subagent_tool_calls": subagent_calls,
        "files_read_tool": sorted(files_read),
        "files_read_bash": sorted(files_read_bash),
        "files_read": sorted(files_read | files_read_bash),
        "n_files_read_tool": len(files_read),
        "n_files_read_bash": len(files_read_bash),
        "n_files_read": len(files_read | files_read_bash),
        "has_result_event": bool(result),
        "calls_before_first_edit": (first_edit_index - 1) if first_edit_index else None,
        "graphite_stale_results": graphite_stale,
        "graphite_complete_answer": complete_seen,
        "hook_answers_in_stream": hook_answers,
        "search_after_complete_graphite": search_after_complete if complete_seen else None,
    }
