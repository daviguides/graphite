//! Install/uninstall the hooks in a repo's `.claude/settings.json`, preserving foreign hooks.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

const MARK: &str = "graphite-hook";
const PRE_MATCHER: &str = "Bash";
const POST_MATCHER: &str = "Read|Grep|Write|Edit|MultiEdit|NotebookEdit";

pub fn settings_path(root: &Path) -> PathBuf {
    root.join(".claude").join("settings.json")
}

fn load(path: &Path) -> Result<Map<String, Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(s) if s.trim().is_empty() => Ok(Map::new()),
        Ok(s) => match serde_json::from_str::<Value>(&s) {
            Ok(Value::Object(m)) => Ok(m),
            Ok(_) => Err(format!("{} is not a JSON object", path.display())),
            Err(e) => Err(format!("{}: {e}", path.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(e.to_string()),
    }
}

fn save(path: &Path, m: Map<String, Value>) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    let body = serde_json::to_string_pretty(&Value::Object(m)).map_err(|e| e.to_string())? + "\n";
    std::fs::write(path, body).map_err(|e| e.to_string())
}

fn is_ours(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|hs| {
            hs.iter().any(|h| {
                h.get("command")
                    .and_then(Value::as_str)
                    .is_some_and(|c| c.contains(MARK))
            })
        })
}

/// Drop our entries from every event; remove events and `hooks` left empty.
fn strip(m: &mut Map<String, Value>) -> usize {
    let mut removed = 0;
    if let Some(Value::Object(hooks)) = m.get_mut("hooks") {
        for list in hooks.values_mut() {
            if let Value::Array(a) = list {
                let before = a.len();
                a.retain(|e| !is_ours(e));
                removed += before - a.len();
            }
        }
        hooks.retain(|_, v| !matches!(v, Value::Array(a) if a.is_empty()));
    }
    if matches!(m.get("hooks"), Some(Value::Object(h)) if h.is_empty()) {
        m.remove("hooks");
    }
    removed
}

fn entry(matcher: &str, command: String) -> Value {
    json!({"matcher": matcher, "hooks": [{"type": "command", "command": command}]})
}

pub fn install(root: &Path, exe: &Path) -> Result<String, String> {
    let path = settings_path(root);
    let mut m = load(&path)?;
    let backup = path.with_extension("json.graphite-bak");
    if path.exists() && !backup.exists() {
        std::fs::copy(&path, &backup).map_err(|e| e.to_string())?;
    }
    strip(&mut m);
    let exe_s = crate::shell::quote(&exe.to_string_lossy());
    let hooks = m
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(hooks) = hooks else {
        return Err("`hooks` in settings.json is not an object".into());
    };
    for (event, matcher, sub) in [
        ("PreToolUse", PRE_MATCHER, "pre"),
        ("PostToolUse", POST_MATCHER, "post"),
    ] {
        let list = hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()));
        let Value::Array(list) = list else {
            return Err(format!("`hooks.{event}` is not an array"));
        };
        list.push(entry(matcher, format!("{exe_s} {sub}")));
    }
    save(&path, m)?;
    Ok(format!(
        "installed graphite hooks in {} (PreToolUse Bash → rewrite; PostToolUse Read/Grep → context, edits → nudge)",
        path.display()
    ))
}

pub fn uninstall(root: &Path) -> Result<String, String> {
    let path = settings_path(root);
    if !path.exists() {
        return Ok("no .claude/settings.json; nothing to remove".into());
    }
    let mut m = load(&path)?;
    let n = strip(&mut m);
    save(&path, m)?;
    Ok(format!(
        "removed {n} graphite hook entries from {}",
        path.display()
    ))
}

/// Installed entries and a tally of `.graphite/hooks.jsonl` actions.
pub fn status(root: &Path) -> Result<Value, String> {
    let m = load(&settings_path(root))?;
    let count = |event: &str| -> usize {
        m.get("hooks")
            .and_then(|h| h.get(event))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter(|e| is_ours(e)).count())
            .unwrap_or(0)
    };
    let mut tally: Map<String, Value> = Map::new();
    if let Ok(s) = std::fs::read_to_string(root.join(".graphite").join("hooks.jsonl")) {
        for l in s.lines() {
            let Ok(v) = serde_json::from_str::<Value>(l) else {
                continue;
            };
            let key = format!(
                "{}:{}",
                v.get("event").and_then(Value::as_str).unwrap_or("?"),
                v.get("action").and_then(Value::as_str).unwrap_or("?")
            );
            let n = tally.get(&key).and_then(Value::as_u64).unwrap_or(0) + 1;
            tally.insert(key, n.into());
        }
    }
    Ok(json!({
        "settings": settings_path(root),
        "pre_installed": count("PreToolUse"),
        "post_installed": count("PostToolUse"),
        "events": tally,
    }))
}
