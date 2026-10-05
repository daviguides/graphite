//! `graphite hooks log` / `graphite hooks show N`: what the agent was given, for debugging.
//!
//! Events are numbered newest first (1 = most recent). `show` prints the original command, the
//! rewrite, the exact model-format answer the agent received and, when the canonical record was
//! logged, the same answer re-rendered as `--human` and `--explain`.

use std::fmt::Write as _;
use std::io::IsTerminal;
use std::path::Path;

use graphite_daemon::answer::{render_explain, render_human, Answer};
use serde_json::Value;

use crate::log::read_all;

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn clock(ts: u64) -> String {
    let secs = (ts / 1000) % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        secs / 3600,
        (secs / 60) % 60,
        secs % 60
    )
}

fn one_line(t: &str, max: usize) -> String {
    let t = t.replace('\n', " ");
    if t.chars().count() <= max {
        t
    } else {
        format!("{}…", t.chars().take(max).collect::<String>())
    }
}

/// Newest-first table of the last `n` events.
pub fn list(state_dir: &Path, n: usize) -> String {
    let events = read_all(state_dir);
    let mut out = String::new();
    if events.is_empty() {
        out.push_str("no hook events in .graphite/hooks.jsonl\n");
        return out;
    }
    for (i, e) in events.iter().rev().take(n).enumerate() {
        let what = if e.get("segment").is_some() {
            s(e, "segment")
        } else {
            s(e, "original_command")
        };
        let mut extra = String::new();
        if let Some(b) = e.get("answer_bytes").and_then(Value::as_u64) {
            let _ = write!(extra, " {b}B");
        }
        let v = s(e, "graph_verdict");
        if !v.is_empty() {
            let _ = write!(extra, " {v}");
        }
        let r = s(e, "reason");
        if !r.is_empty() {
            let _ = write!(extra, " ({r})");
        }
        let _ = writeln!(
            out,
            "#{:<3} {} {:<5} {:<12} {:<6}{extra}  {}",
            i + 1,
            clock(e.get("ts").and_then(Value::as_u64).unwrap_or(0)),
            s(e, "event"),
            s(e, "action"),
            s(e, "kind"),
            one_line(what, 90)
        );
    }
    out
}

/// Full view of event `n` (1 = newest).
pub fn show(state_dir: &Path, n: usize) -> Option<String> {
    let events = read_all(state_dir);
    let e = events.iter().rev().nth(n.checked_sub(1)?)?;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "event #{n}: {} / {}  session={} call={}",
        s(e, "event"),
        s(e, "action"),
        s(e, "session_id"),
        s(e, "call_id")
    );
    let _ = writeln!(out, "agent ran:   {}", s(e, "original_command"));
    if !s(e, "segment").is_empty() {
        let _ = writeln!(out, "segment:     {}", s(e, "segment"));
    }
    if !s(e, "rewritten_command").is_empty() {
        let _ = writeln!(out, "rewritten:   {}", s(e, "rewritten_command"));
    }
    if !s(e, "reason").is_empty() {
        let _ = writeln!(out, "reason:      {}", s(e, "reason"));
    }
    if !s(e, "error").is_empty() {
        let _ = writeln!(out, "error:       {}", s(e, "error"));
    }
    if let Some(ms) = e.get("latency_ms").and_then(Value::as_u64) {
        let _ = writeln!(out, "latency:     {ms} ms");
    }
    let answer = s(e, "answer");
    if !answer.is_empty() {
        let _ = writeln!(
            out,
            "\n--- model answer (exactly what the agent received) ---"
        );
        out.push_str(answer);
        if !answer.ends_with('\n') {
            out.push('\n');
        }
    }
    match e
        .get("record")
        .cloned()
        .map(serde_json::from_value::<Answer>)
    {
        Some(Ok(rec)) => {
            let _ = writeln!(out, "\n--- human view (same record) ---");
            out.push_str(&render_human(
                &rec,
                std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none(),
            ));
            let _ = writeln!(out, "\n--- explain (why each line got its class) ---");
            out.push_str(&render_explain(&rec));
        }
        Some(Err(err)) => {
            let _ = writeln!(out, "\n(record present but unreadable: {err})");
        }
        None if e.get("record_omitted").is_some() => {
            let _ = writeln!(out, "\n(record omitted from the log: above 200 KB)");
        }
        None => {}
    }
    Some(out)
}
