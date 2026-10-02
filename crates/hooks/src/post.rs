//! PostToolUse: append graph context after native Read/Grep, nudge the index after edits.

use std::path::PathBuf;

use graphite_daemon::{judge, Op, RepoPaths, SearchSpec};
use serde_json::{json, Map, Value};

use crate::{ask, log, DECIDE_TIMEOUT};

const EDIT_TOOLS: &[&str] = &["Write", "Edit", "MultiEdit", "NotebookEdit"];

pub fn handle(input: &Value) -> Option<Value> {
    let tool = input.get("tool_name")?.as_str()?;
    let ti = input.get("tool_input")?;
    let cwd = input
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    let paths = RepoPaths::discover(&cwd);
    let cwd_s = cwd.to_string_lossy().into_owned();
    let t = std::time::Instant::now();
    let mut ev = Map::new();
    ev.insert("tool".into(), tool.into());
    if let Some(sid) = input.get("session_id").and_then(Value::as_str) {
        ev.insert("session_id".into(), sid.into());
    }
    ev.insert("call_id".into(), log::call_id().into());
    if let Some(t) = input.get("tool_use_id").and_then(Value::as_str) {
        ev.insert("turn_id".into(), t.into());
    }
    let result = match tool {
        "Read" => {
            let f = ti.get("file_path")?.as_str()?;
            ev.insert("original_command".into(), f.into());
            if !f.ends_with(".py") {
                return None;
            }
            // Sized to what the Read tool returned, like a routed `cat`.
            let printed = input
                .pointer("/tool_response/file/content")
                .and_then(Value::as_str)
                .map_or(0, str::len);
            let op = Op::FileInfo {
                paths: vec![f.into()],
                cwd: cwd_s,
                budget: Some(crate::exec::read_header_budget(printed)),
            };
            Some(ask(&paths, op, DECIDE_TIMEOUT))
        }
        "Grep" => {
            let pat = ti.get("pattern")?.as_str()?;
            ev.insert("original_command".into(), pat.into());
            let spec = SearchSpec {
                patterns: vec![pat.into()],
                ..Default::default()
            };
            let name = judge::identifier_of(&spec)?;
            Some(ask(
                &paths,
                Op::NameInfo { name, cwd: cwd_s },
                DECIDE_TIMEOUT,
            ))
        }
        e if EDIT_TOOLS.contains(&e) => {
            let f = ti
                .get("file_path")
                .or_else(|| ti.get("notebook_path"))?
                .as_str()?;
            ev.insert("original_command".into(), f.into());
            let r = ask(
                &paths,
                Op::Nudge {
                    paths: vec![f.into()],
                },
                DECIDE_TIMEOUT,
            );
            ev.insert(
                "action".into(),
                if r.is_ok() { "nudge" } else { "passthrough" }.into(),
            );
            if let Err(e) = r {
                ev.insert("error".into(), e.into());
            }
            ev.insert("latency_ms".into(), (t.elapsed().as_millis() as u64).into());
            log::event(&paths.dir, "post", ev);
            return None;
        }
        _ => None,
    }?;
    let out = match result {
        Ok(r) => {
            let text = r.data.get("text").and_then(Value::as_str).unwrap_or("");
            if text.is_empty() {
                ev.insert("action".into(), "passthrough".into());
                ev.insert("reason".into(), "graph has nothing to add".into());
                None
            } else {
                ev.insert("action".into(), "enrich".into());
                ev.insert("answer".into(), text.into());
                ev.insert("answer_bytes".into(), (text.len() as u64).into());
                ev.insert("keys".into(), log::answer_keys(text, None).into());
                Some(json!({
                    "hookSpecificOutput": {
                        "hookEventName": "PostToolUse",
                        "additionalContext": text,
                    }
                }))
            }
        }
        Err(e) => {
            ev.insert("action".into(), "passthrough".into());
            ev.insert("error".into(), e.into());
            None
        }
    };
    ev.insert("latency_ms".into(), (t.elapsed().as_millis() as u64).into());
    log::event(&paths.dir, "post", ev);
    out
}
