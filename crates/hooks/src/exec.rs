//! `graphite-hook run`: execute a routed command, answering what Graphite can.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use graphite_daemon::{Op, RepoPaths};
use serde_json::{Map, Value};

use crate::parse::{plan, Action};
use crate::shell::{split, Join};
use crate::{ask, log, ANSWER_TIMEOUT};

fn sh(script: &str, cwd: &Path, stdin: Option<&str>) -> i32 {
    let _ = std::io::stdout().flush();
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(script).current_dir(cwd);
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let Ok(mut child) = cmd.spawn() else {
        return 127;
    };
    if let (Some(text), Some(mut w)) = (stdin, child.stdin.take()) {
        let _ = w.write_all(text.as_bytes());
    }
    child.wait().ok().and_then(|s| s.code()).unwrap_or(1)
}

fn text_of(r: &graphite_daemon::Response) -> String {
    r.data
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Run the whole command; returns the exit status of the last executed segment.
pub fn run(command: &str, cwd: &Path, all: bool) -> i32 {
    let paths = RepoPaths::discover(cwd);
    let segs = split(command);
    let actions = segs.as_ref().and_then(|s| plan(s, cwd, &paths.root));
    let (Some(segs), Some(actions)) = (segs, actions) else {
        // The command changed since the hook looked (or was typed by hand): run it verbatim.
        return sh(command, cwd, None);
    };
    let mut dir: PathBuf = cwd.to_path_buf();
    let mut status = 0;
    for (seg, action) in segs.iter().zip(actions) {
        let skip = match seg.join {
            Join::And => status != 0,
            Join::Or => status == 0,
            _ => false,
        };
        if skip {
            continue;
        }
        let t = std::time::Instant::now();
        let mut ev = Map::new();
        ev.insert("tool".into(), "Bash".into());
        ev.insert("original_command".into(), command.into());
        ev.insert("segment".into(), seg.raw.clone().into());
        status = match action {
            Action::Cd(d) => {
                dir = d;
                ev.insert("action".into(), "cd".into());
                0
            }
            Action::Plain => {
                ev.insert("action".into(), "plain".into());
                sh(&seg.raw, &dir, None)
            }
            Action::Search { mut spec, filters } => {
                spec.all = all;
                ev.insert("kind".into(), "search".into());
                match ask(&paths, Op::Search { spec }, ANSWER_TIMEOUT) {
                    Ok(r) => {
                        let text = text_of(&r);
                        let stats = r.data.get("stats").cloned().unwrap_or(Value::Null);
                        let matches = r.data.get("matches").and_then(Value::as_u64).unwrap_or(0);
                        answer_fields(&mut ev, &text, &stats, matches);
                        if filters.is_empty() {
                            print!("{text}");
                            if matches > 0 {
                                0
                            } else {
                                1
                            }
                        } else {
                            sh(&filters.join(" | "), &dir, Some(&text))
                        }
                    }
                    Err(e) => fallback(&mut ev, &e, &seg.raw, &dir),
                }
            }
            Action::Read { files } => {
                ev.insert("kind".into(), "read".into());
                let op = Op::FileInfo {
                    paths: files,
                    cwd: dir.to_string_lossy().into(),
                };
                match ask(&paths, op, ANSWER_TIMEOUT) {
                    Ok(r) => {
                        let text = text_of(&r);
                        answer_fields(&mut ev, &text, &Value::Null, 0);
                        print!("{text}");
                        sh(&seg.raw, &dir, None)
                    }
                    Err(e) => fallback(&mut ev, &e, &seg.raw, &dir),
                }
            }
            Action::List { dirs } => {
                ev.insert("kind".into(), "list".into());
                let code = sh(&seg.raw, &dir, None);
                let op = Op::DirInfo {
                    paths: dirs,
                    cwd: dir.to_string_lossy().into(),
                };
                match ask(&paths, op, ANSWER_TIMEOUT) {
                    Ok(r) => {
                        let text = text_of(&r);
                        answer_fields(&mut ev, &text, &Value::Null, 0);
                        print!("{text}");
                    }
                    Err(e) => {
                        ev.insert("action".into(), "fallback".into());
                        ev.insert("error".into(), e.into());
                    }
                }
                code
            }
        };
        ev.insert("latency_ms".into(), (t.elapsed().as_millis() as u64).into());
        log::event(&paths.dir, "exec", ev);
    }
    let _ = std::io::stdout().flush();
    status
}

fn answer_fields(ev: &mut Map<String, Value>, text: &str, stats: &Value, matches: u64) {
    ev.insert("action".into(), "answer".into());
    ev.insert("answer".into(), text.into());
    ev.insert("answer_bytes".into(), (text.len() as u64).into());
    let verdict = stats
        .get("graph_verdict")
        .cloned()
        .unwrap_or_else(|| "none".into());
    ev.insert("graph_verdict".into(), verdict);
    if let Some(c) = stats.get("classes") {
        ev.insert("residue".into(), c.clone());
        ev.insert("matches".into(), matches.into());
    }
    for k in ["raw_bytes", "search_ms", "total_ms"] {
        if let Some(v) = stats.get(k) {
            ev.insert(k.into(), v.clone());
        }
    }
}

fn fallback(ev: &mut Map<String, Value>, err: &str, raw: &str, dir: &Path) -> i32 {
    ev.insert("action".into(), "fallback".into());
    ev.insert("error".into(), err.into());
    sh(raw, dir, None)
}
