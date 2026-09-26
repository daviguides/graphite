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

use std::io::Write;
use std::path::Path;

use serde_json::{Map, Value};

const ANSWER_CAP: usize = 20_000;

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
