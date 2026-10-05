//! `.graphite/hooks.jsonl`: one JSON object per hook decision, appended.
//!
//! Stable schema (fields absent when not applicable):
//! - `ts` (u64, unix ms), `event` ("pre" | "exec" | "post")
//! - `tool` (Claude Code tool name: "Bash", "Read", "Grep", "Edit", ...)
//! - `original_command` (Bash command as the agent wrote it; for Read/Grep the path/pattern)
//! - `action`:
//!   - pre: "rewrite" (command routed to `graphite-hook run`) | "passthrough"
//!   - exec (one per segment): "answer" (Graphite produced the output) | "fallback" (original ran:
//!     daemon error/timeout) | "plain" (read-only segment run as-is) | "cd"
//!   - post: "enrich" (context appended after a native tool) | "nudge" (edit reported) | "passthrough"
//! - `rewritten_command` (pre/rewrite), `segment` (exec), `kind` (exec: "search" | "read" | "list")
//! - `answer` (text shown to the agent for Graphite-produced output, capped at 20 KB), `answer_bytes`
//! - `raw_bytes` (search: size a plain grep would have printed), `matches`
//! - `search_ms` / `total_ms` (daemon-side search and search+judgment time)
//! - `graph_verdict`: "complete" | "lower_bound" | "none"
//! - `residue` (search: match counts by class — definition, reference, import, graph_gap, code_untracked,
//!   string_or_comment, other_language, not_indexed, docs_config, other_identifier)
//! - `latency_ms`, `reason` (why passthrough/fallback), `error`
//! - `session_id` (Claude Code session, from the hook payload; carried from `pre` to `exec` through
//!   the rewritten command), `call_id` (unique per intercepted call; the `pre` event and its `exec`
//!   events share it)
//! - `turn_id` (the Claude Code `tool_use_id` of the Bash/Read/Grep call, when the payload has it)
//! - `keys` (exec answer / post enrich: the `path:line` keys the answer showed — collapsed
//!   call-site lines expand to one key per line; file reads use `path:0`), for measuring overlap
//!   between answers in one session. Measurement only.
//! - `format` (exec: "model" | "human" | "json" | "explain"), `record` (exec search: the canonical
//!   answer record the views render from; omitted above 200 KB, `record_omitted` says so), `pipeline`
//!   (exec search: notices about how head/tail/grep stages were honored)
//!
//! `graphite hooks log` lists recent events; `graphite hooks show N` re-renders one from `record`.

use std::io::Write;
use std::path::Path;

use serde_json::{Map, Value};

const ANSWER_CAP: usize = 20_000;
const RECORD_CAP: usize = 200_000;

/// Unique id for one intercepted call.
pub fn call_id() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    format!(
        "{:x}-{:x}-{}",
        now_ms(),
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    )
}

/// `path:line` keys an answer showed (grep-shaped lines and collapsed `path:1,2,3` lines).
/// `path:line` keys of the match lines of an answer; `file` names the lines of a single-file
/// answer (printed `N:text`, as grep does).
pub fn answer_keys(text: &str, file: Option<&str>) -> Vec<String> {
    let mut keys = Vec::new();
    for l in text.lines() {
        if l.starts_with('#') || l.is_empty() {
            continue;
        }
        let head = l.split("    ").next().unwrap_or(l);
        let mut parts = head.splitn(3, ':');
        let (Some(path), Some(lines)) = (parts.next(), parts.next()) else {
            continue;
        };
        let (path, lines) = match file {
            Some(f) => (f, path),
            None => (path, lines),
        };
        for n in lines.split(',') {
            if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
                keys.push(format!("{path}:{n}"));
            }
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Append one event if the repo has a `.graphite/` dir; logging never fails the hook.
pub fn event(state_dir: &Path, event: &str, mut fields: Map<String, Value>) {
    if !state_dir.is_dir() {
        return;
    }
    if let Some(Value::String(a)) = fields.get_mut("answer") {
        if a.len() > ANSWER_CAP {
            let mut cut = ANSWER_CAP;
            while !a.is_char_boundary(cut) {
                cut -= 1;
            }
            a.truncate(cut);
        }
    }
    if fields
        .get("record")
        .is_some_and(|r| r.to_string().len() > RECORD_CAP)
    {
        fields.remove("record");
        fields.insert("record_omitted".into(), true.into());
    }
    fields.insert("ts".into(), now_ms().into());
    fields.insert("event".into(), event.into());
    let Ok(mut line) = serde_json::to_string(&Value::Object(fields)) else {
        return;
    };
    line.push('\n');
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state_dir.join("hooks.jsonl"))
    {
        let _ = f.write_all(line.as_bytes());
    }
}

/// Every event in the log, oldest first.
pub fn read_all(state_dir: &Path) -> Vec<Value> {
    std::fs::read_to_string(state_dir.join("hooks.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect()
}
