//! Install/uninstall the hooks in a repo's `.claude/settings.json`.
//!
//! Edits are textual splices on the original bytes, never a parse-and-reserialize, so the user's
//! key order and formatting are untouched and install followed by uninstall restores the file
//! byte for byte. What install created (the file, the `.claude/` dir, new keys) is recorded in a
//! sidecar so uninstall removes exactly that and nothing else. Foreign hooks (e.g. RTK) are kept.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

const MARK: &str = "graphite-hook";
const PRE_MATCHER: &str = "Bash";
const POST_MATCHER: &str = "Read|Grep|Write|Edit|MultiEdit|NotebookEdit";

pub fn settings_path(root: &Path) -> PathBuf {
    root.join(".claude").join("settings.json")
}

fn sidecar_path(root: &Path) -> PathBuf {
    root.join(".claude").join("settings.json.graphite-install")
}

fn backup_path(root: &Path) -> PathBuf {
    root.join(".claude").join("settings.json.graphite-bak")
}

/// What install created, so uninstall can remove exactly that.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Created {
    #[serde(default)]
    dir: bool,
    #[serde(default)]
    file: bool,
    /// Dotted keys install added (`hooks`, `hooks.PreToolUse`, …), deepest removed first.
    #[serde(default)]
    keys: Vec<String>,
}

// ---- minimal JSON span scanner -------------------------------------------------------------

struct Scan<'a> {
    b: &'a [u8],
    i: usize,
}

impl Scan<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && self.b[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn string(&mut self) -> Result<String, String> {
        let start = self.i;
        if self.b.get(self.i) != Some(&b'"') {
            return Err(format!("expected string at byte {}", self.i));
        }
        self.i += 1;
        while self.i < self.b.len() {
            match self.b[self.i] {
                b'\\' => self.i += 2,
                b'"' => {
                    self.i += 1;
                    let raw =
                        std::str::from_utf8(&self.b[start..self.i]).map_err(|e| e.to_string())?;
                    return serde_json::from_str(raw).map_err(|e| e.to_string());
                }
                _ => self.i += 1,
            }
        }
        Err("unterminated string".into())
    }

    /// Skip one value; returns its (start, end) byte span.
    fn value(&mut self) -> Result<(usize, usize), String> {
        self.ws();
        let start = self.i;
        match self.b.get(self.i) {
            Some(b'"') => {
                self.string()?;
            }
            Some(b'{') | Some(b'[') => {
                let mut depth = 0usize;
                while self.i < self.b.len() {
                    match self.b[self.i] {
                        b'"' => {
                            self.string()?;
                            continue;
                        }
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => {
                            depth -= 1;
                            if depth == 0 {
                                self.i += 1;
                                return Ok((start, self.i));
                            }
                        }
                        _ => {}
                    }
                    self.i += 1;
                }
                return Err("unterminated container".into());
            }
            Some(_) => {
                while self.i < self.b.len()
                    && !matches!(self.b[self.i], b',' | b'}' | b']')
                    && !self.b[self.i].is_ascii_whitespace()
                {
                    self.i += 1;
                }
            }
            None => return Err("unexpected end".into()),
        }
        Ok((start, self.i))
    }
}

#[derive(Debug, Clone)]
struct Member {
    key: String,
    start: usize,
    val: (usize, usize),
}

/// Members of the object whose `{` is at `open`; returns (members, index of `}`).
fn members(text: &str, open: usize) -> Result<(Vec<Member>, usize), String> {
    let mut s = Scan {
        b: text.as_bytes(),
        i: open + 1,
    };
    let mut out = Vec::new();
    loop {
        s.ws();
        match s.b.get(s.i) {
            Some(b'}') => return Ok((out, s.i)),
            Some(b',') => {
                s.i += 1;
                continue;
            }
            Some(b'"') => {
                let start = s.i;
                let key = s.string()?;
                s.ws();
                if s.b.get(s.i) != Some(&b':') {
                    return Err("expected ':'".into());
                }
                s.i += 1;
                let val = s.value()?;
                out.push(Member { key, start, val });
            }
            _ => return Err(format!("unexpected byte {} in object", s.i)),
        }
    }
}

/// Elements of the array whose `[` is at `open`; returns (spans, index of `]`).
fn elements(text: &str, open: usize) -> Result<(Vec<(usize, usize)>, usize), String> {
    let mut s = Scan {
        b: text.as_bytes(),
        i: open + 1,
    };
    let mut out = Vec::new();
    loop {
        s.ws();
        match s.b.get(s.i) {
            Some(b']') => return Ok((out, s.i)),
            Some(b',') => {
                s.i += 1;
            }
            Some(_) => out.push(s.value()?),
            None => return Err("unterminated array".into()),
        }
    }
}

fn top_open(text: &str) -> Result<usize, String> {
    let i = text
        .bytes()
        .position(|c| !c.is_ascii_whitespace())
        .ok_or("empty settings")?;
    if text.as_bytes()[i] != b'{' {
        return Err("settings.json is not a JSON object".into());
    }
    Ok(i)
}

fn find<'a>(ms: &'a [Member], key: &str) -> Option<&'a Member> {
    ms.iter().find(|m| m.key == key)
}

/// Indent unit used by the file (spaces before the first member), default 2.
fn indent_unit(text: &str) -> String {
    for l in text.lines().skip(1) {
        let n = l.len() - l.trim_start().len();
        if n > 0 && l.trim_start().starts_with('"') {
            return l[..n].to_string();
        }
    }
    "  ".into()
}

// ---- rendering our entries -----------------------------------------------------------------

fn js(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

fn entry_text(matcher: &str, command: &str, u: &str, level: usize) -> String {
    let p = |n: usize| u.repeat(n);
    format!(
        "{{\n{i1}\"matcher\": {m},\n{i1}\"hooks\": [\n{i2}{{\n{i3}\"type\": \"command\",\n{i3}\"command\": {c}\n{i2}}}\n{i1}]\n{i0}}}",
        m = js(matcher),
        c = js(command),
        i0 = p(level),
        i1 = p(level + 1),
        i2 = p(level + 2),
        i3 = p(level + 3),
    )
}

/// Insert `member_text` (`"key": value`) into the object at `open`.
fn add_member(
    text: &str,
    open: usize,
    member_text: &str,
    u: &str,
    level: usize,
) -> Result<String, String> {
    let (ms, close) = members(text, open)?;
    Ok(match ms.last() {
        Some(last) => format!(
            "{},\n{}{member_text}{}",
            &text[..last.val.1],
            u.repeat(level),
            &text[last.val.1..]
        ),
        None => format!(
            "{}\n{}{member_text}\n{}{}",
            &text[..open + 1],
            u.repeat(level),
            u.repeat(level.saturating_sub(1)),
            &text[close..]
        ),
    })
}

/// Append `elem` to the array at `open`.
fn add_element(
    text: &str,
    open: usize,
    elem: &str,
    u: &str,
    level: usize,
) -> Result<String, String> {
    let (es, close) = elements(text, open)?;
    Ok(match es.last() {
        Some(&(_, end)) => {
            // Reuse the separator style between existing elements (or the one after `[`).
            let sep = if es.len() > 1 {
                let between = &text[es[es.len() - 2].1..es[es.len() - 1].0];
                between.trim_start_matches(',').to_string()
            } else {
                text[open + 1..es[0].0].to_string()
            };
            let sep = if sep.is_empty() {
                format!("\n{}", u.repeat(level))
            } else {
                sep
            };
            format!("{},{sep}{elem}{}", &text[..end], &text[end..])
        }
        None => format!(
            "{}\n{}{elem}\n{}{}",
            &text[..open + 1],
            u.repeat(level),
            u.repeat(level.saturating_sub(1)),
            &text[close..]
        ),
    })
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

/// Remove the span `[a, b)` of an element/member plus exactly one separator.
fn cut(text: &str, spans: &[(usize, usize)], idx: usize, open: usize, close: usize) -> String {
    let (a, b) = spans[idx];
    if spans.len() == 1 {
        // Sole element: restore an empty container as `[]` / `{}`.
        return format!("{}{}", &text[..open + 1], &text[close..]);
    }
    if idx > 0 {
        let prev_end = spans[idx - 1].1;
        format!("{}{}", &text[..prev_end], &text[b..])
    } else {
        let next_start = spans[1].0;
        format!("{}{}", &text[..a], &text[next_start..])
    }
}

/// Remove every graphite entry from `hooks.*` arrays; returns (text, removed).
fn strip_text(mut text: String) -> Result<(String, usize), String> {
    let mut removed = 0;
    loop {
        let top = top_open(&text)?;
        let (ms, _) = members(&text, top)?;
        let Some(h) = find(&ms, "hooks") else {
            return Ok((text, removed));
        };
        if text.as_bytes()[h.val.0] != b'{' {
            return Ok((text, removed));
        }
        let (events, _) = members(&text, h.val.0)?;
        let mut hit = None;
        'outer: for ev in &events {
            if text.as_bytes()[ev.val.0] != b'[' {
                continue;
            }
            let (es, close) = elements(&text, ev.val.0)?;
            for (i, &(a, b)) in es.iter().enumerate() {
                let v: Value = serde_json::from_str(&text[a..b]).unwrap_or(Value::Null);
                if is_ours(&v) {
                    hit = Some((es.clone(), i, ev.val.0, close));
                    break 'outer;
                }
            }
        }
        let Some((es, i, open, close)) = hit else {
            return Ok((text, removed));
        };
        text = cut(&text, &es, i, open, close);
        removed += 1;
    }
}

/// Remove member `path` (dotted) if its value is an empty object/array.
fn drop_if_empty(text: &str, path: &str) -> Result<String, String> {
    let keys: Vec<&str> = path.split('.').collect();
    let mut open = top_open(text)?;
    for (depth, k) in keys.iter().enumerate() {
        let (ms, close) = members(text, open)?;
        let Some(pos) = ms.iter().position(|m| m.key == *k) else {
            return Ok(text.to_string());
        };
        let m = &ms[pos];
        if depth + 1 == keys.len() {
            let v: Value = serde_json::from_str(&text[m.val.0..m.val.1]).unwrap_or(Value::Null);
            let empty = matches!(&v, Value::Object(o) if o.is_empty())
                || matches!(&v, Value::Array(a) if a.is_empty());
            if !empty {
                return Ok(text.to_string());
            }
            let spans: Vec<(usize, usize)> = ms.iter().map(|m| (m.start, m.val.1)).collect();
            return Ok(cut(text, &spans, pos, open, close));
        }
        if text.as_bytes()[m.val.0] != b'{' {
            return Ok(text.to_string());
        }
        open = m.val.0;
    }
    Ok(text.to_string())
}

fn load_value(text: &str, path: &Path) -> Result<Map<String, Value>, String> {
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Err(format!("{} is not a JSON object", path.display())),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

pub fn install(root: &Path, exe: &Path) -> Result<String, String> {
    let path = settings_path(root);
    let exe_s = crate::shell::quote(&exe.to_string_lossy());
    let wanted = [
        ("PreToolUse", PRE_MATCHER, format!("{exe_s} pre")),
        ("PostToolUse", POST_MATCHER, format!("{exe_s} post")),
    ];
    let mut created: Created = std::fs::read_to_string(sidecar_path(root))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let existed = path.exists();
    let mut text = if existed {
        std::fs::read_to_string(&path).map_err(|e| e.to_string())?
    } else {
        created.dir |= !path.parent().unwrap().exists();
        created.file = true;
        String::new()
    };
    if text.trim().is_empty() {
        // New file (or blank): write the whole thing in the default style.
        let mut hooks = Map::new();
        for (event, matcher, cmd) in &wanted {
            hooks.insert(
                (*event).into(),
                json!([{"matcher": matcher, "hooks": [{"type": "command", "command": cmd}]}]),
            );
        }
        text = serde_json::to_string_pretty(&json!({ "hooks": hooks }))
            .map_err(|e| e.to_string())?
            + "\n";
        if existed {
            created.keys = vec!["hooks".into()];
        }
    } else {
        load_value(&text, &path)?; // refuse to touch invalid JSON
        let backup = backup_path(root);
        if !backup.exists() {
            std::fs::copy(&path, &backup).map_err(|e| e.to_string())?;
        }
        text = strip_text(text)?.0;
        let u = indent_unit(&text);
        for (event, matcher, cmd) in &wanted {
            let top = top_open(&text)?;
            let (ms, _) = members(&text, top)?;
            let hooks_open = match find(&ms, "hooks") {
                Some(h) if text.as_bytes()[h.val.0] == b'{' => h.val.0,
                Some(_) => return Err("`hooks` in settings.json is not an object".into()),
                None => {
                    text = add_member(&text, top, "\"hooks\": {}", &u, 1)?;
                    if !created.keys.iter().any(|k| k == "hooks") {
                        created.keys.push("hooks".into());
                    }
                    let (ms, _) = members(&text, top_open(&text)?)?;
                    find(&ms, "hooks").ok_or("hooks insert failed")?.val.0
                }
            };
            let (evs, _) = members(&text, hooks_open)?;
            let elem = entry_text(matcher, cmd, &u, 3);
            match find(&evs, event) {
                Some(e) if text.as_bytes()[e.val.0] == b'[' => {
                    text = add_element(&text, e.val.0, &elem, &u, 3)?;
                }
                Some(_) => return Err(format!("`hooks.{event}` is not an array")),
                None => {
                    let key = format!("hooks.{event}");
                    if !created.keys.contains(&key) {
                        created.keys.push(key);
                    }
                    let member =
                        format!("{}: [\n{}{elem}\n{}]", js(event), u.repeat(3), u.repeat(2));
                    text = add_member(&text, hooks_open, &member, &u, 2)?;
                }
            }
        }
    }
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&path, &text).map_err(|e| e.to_string())?;
    std::fs::write(
        sidecar_path(root),
        serde_json::to_string(&created).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    Ok(format!(
        "installed graphite hooks in {} (PreToolUse Bash → rewrite; PostToolUse Read/Grep → context, edits → nudge)",
        path.display()
    ))
}

pub fn uninstall(root: &Path) -> Result<String, String> {
    let path = settings_path(root);
    let created: Created = std::fs::read_to_string(sidecar_path(root))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    if !path.exists() {
        let _ = std::fs::remove_file(sidecar_path(root));
        return Ok("no .claude/settings.json; nothing to remove".into());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    load_value(&text, &path)?;
    let (mut text, n) = strip_text(text)?;
    let mut keys = created.keys.clone();
    keys.sort_by_key(|k| std::cmp::Reverse(k.matches('.').count()));
    for k in &keys {
        text = drop_if_empty(&text, k)?;
    }
    let _ = std::fs::remove_file(sidecar_path(root));
    let leftover = load_value(&text, &path)?;
    if created.file && leftover.values().all(|v| {
        matches!(v, Value::Object(o) if o.values().all(|x| matches!(x, Value::Array(a) if a.is_empty())))
    }) {
        // We created the file and nothing else lives in it: remove it (and our dir).
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        if created.dir {
            let dir = path.parent().unwrap();
            if std::fs::read_dir(dir).map(|mut d| d.next().is_none()).unwrap_or(false) {
                std::fs::remove_dir(dir).map_err(|e| e.to_string())?;
            }
        }
        return Ok(format!("removed {n} graphite hook entries; removed {} (install created it)", path.display()));
    }
    std::fs::write(&path, &text).map_err(|e| e.to_string())?;
    let backup = backup_path(root);
    if std::fs::read_to_string(&backup).is_ok_and(|b| b == text) {
        let _ = std::fs::remove_file(&backup);
    }
    Ok(format!(
        "removed {n} graphite hook entries from {}",
        path.display()
    ))
}

/// Installed entries and a tally of `.graphite/hooks.jsonl` actions.
pub fn status(root: &Path) -> Result<Value, String> {
    let path = settings_path(root);
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let m = load_value(&text, &path)?;
    let count = |event: &str| -> usize {
        m.get("hooks")
            .and_then(|h| h.get(event))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter(|e| is_ours(e)).count())
            .unwrap_or(0)
    };
    let mut tally: Map<String, Value> = Map::new();
    for v in crate::log::read_all(&root.join(".graphite")) {
        let key = format!(
            "{}:{}",
            v.get("event").and_then(Value::as_str).unwrap_or("?"),
            v.get("action").and_then(Value::as_str).unwrap_or("?")
        );
        let n = tally.get(&key).and_then(Value::as_u64).unwrap_or(0) + 1;
        tally.insert(key, n.into());
    }
    Ok(json!({
        "settings": path,
        "pre_installed": count("PreToolUse"),
        "post_installed": count("PostToolUse"),
        "events": tally,
    }))
}
