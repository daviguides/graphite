//! PreToolUse on Bash: decide whether to route the command through `graphite-hook run`.

use std::path::{Path, PathBuf};

use graphite_daemon::RepoPaths;
use serde_json::{json, Map, Value};

use crate::parse::{answers, plan};
use crate::shell::{quote, split};
use crate::{connect, log, DECIDE_TIMEOUT};

/// Why a command is left alone; None means rewrite.
pub fn decide(command: &str, cwd: &Path, paths: &RepoPaths) -> Result<(), &'static str> {
    let segs = split(command).ok_or("unsupported shell construct")?;
    let actions =
        plan(&segs, cwd, &paths.root).ok_or("contains a command graphite does not handle")?;
    if !answers(&actions) {
        return Err("nothing graphite can answer");
    }
    Ok(())
}

pub fn rewritten(exe: &Path, cwd: &Path, command: &str) -> String {
    format!(
        "{} run --cwd {} -- {}",
        quote(&exe.to_string_lossy()),
        quote(&cwd.to_string_lossy()),
        quote(command)
    )
}

/// Hook entry point: stdin payload → stdout JSON (empty output = no change).
pub fn handle(input: &Value, exe: &Path) -> Option<Value> {
    if input.get("tool_name")?.as_str()? != "Bash" {
        return None;
    }
    let tool_input = input.get("tool_input")?.as_object()?;
    let command = tool_input.get("command")?.as_str()?;
    let cwd = input
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())?;
    let paths = RepoPaths::discover(&cwd);
    let t = std::time::Instant::now();
    let mut ev = Map::new();
    ev.insert("tool".into(), "Bash".into());
    ev.insert("original_command".into(), command.into());
    let decision = if command.contains("graphite-hook") {
        Err("already routed")
    } else if connect(&paths, DECIDE_TIMEOUT).is_none() {
        Err("daemon not running")
    } else {
        decide(command, &cwd, &paths)
    };
    let out = match decision {
        Ok(()) => {
            let new = rewritten(exe, &cwd, command);
            ev.insert("action".into(), "rewrite".into());
            ev.insert("rewritten_command".into(), new.clone().into());
            let mut updated = tool_input.clone();
            updated.insert("command".into(), new.into());
            Some(json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecisionReason": "graphite: answered from the code graph",
                    "updatedInput": Value::Object(updated),
                }
            }))
        }
        Err(reason) => {
            ev.insert("action".into(), "passthrough".into());
            ev.insert("reason".into(), reason.into());
            None
        }
    };
    ev.insert("latency_ms".into(), (t.elapsed().as_millis() as u64).into());
    log::event(&paths.dir, "pre", ev);
    out
}
