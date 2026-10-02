//! `graphite-hook run`: execute a routed command, answering what Graphite can.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use graphite_daemon::answer::OutFormat;
use graphite_daemon::{Op, RepoPaths};
use serde_json::{Map, Value};

use crate::parse::{classify, plan, Action};
use crate::shell::{env_prefix, expand, split, Env, Join};
use crate::{ask, log, ANSWER_TIMEOUT};

/// A file header never costs more than this share of what the read printed…
const READ_HEADER_SHARE: usize = 4;
/// …within these bounds (bytes): enough for the busiest symbols, never a second page.
const READ_HEADER_MIN: usize = 160;
const READ_HEADER_MAX: usize = 600;

/// Header budget for a read that printed `printed` bytes.
pub fn read_header_budget(printed: usize) -> usize {
    (printed / READ_HEADER_SHARE).clamp(READ_HEADER_MIN, READ_HEADER_MAX)
}

fn shell(script: &str, cwd: &Path) -> Command {
    let _ = std::io::stdout().flush();
    let mut cmd = Command::new("/bin/sh");
    cmd.arg("-c").arg(script).current_dir(cwd);
    cmd
}

fn sh(script: &str, cwd: &Path, stdin: Option<&str>) -> i32 {
    let mut cmd = shell(script, cwd);
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    let Ok(mut child) = cmd.spawn() else {
        return 127;
    };
    if let (Some(text), Some(mut w)) = (stdin, child.stdin.take()) {
        // A filter that exits early (`head`) closes its stdin; that is not an error.
        let _ = w.write_all(text.as_bytes());
    }
    child.wait().ok().and_then(|s| s.code()).unwrap_or(1)
}

/// Run `script` capturing stdout (stderr passes through).
fn sh_capture(script: &str, cwd: &Path) -> (String, i32) {
    let mut cmd = shell(script, cwd);
    cmd.stdout(Stdio::piped()).stderr(Stdio::inherit());
    match cmd.output() {
        Ok(o) => (
            String::from_utf8_lossy(&o.stdout).into_owned(),
            o.status.code().unwrap_or(1),
        ),
        Err(_) => (String::new(), 127),
    }
}

fn text_of(r: &graphite_daemon::Response) -> String {
    r.data
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// How `graphite-hook run` was invoked.
#[derive(Debug, Clone, Default)]
pub struct RunOpts {
    pub all: bool,
    pub format: OutFormat,
    pub session: String,
    pub call: String,
    pub turn: String,
}

/// State carried across the segments of one command.
struct Run<'a> {
    paths: &'a RepoPaths,
    opts: &'a RunOpts,
    dir: PathBuf,
    env: Env,
    /// Files whose graph header was already printed by this command.
    headers: HashSet<String>,
}

/// Run the whole command; returns the exit status of the last executed segment.
pub fn run(command: &str, cwd: &Path, opts: &RunOpts) -> i32 {
    let paths = RepoPaths::discover(cwd);
    let segs = split(command);
    let actions = segs.as_ref().and_then(|s| plan(s, cwd, &paths.root));
    let (Some(segs), Some(actions)) = (segs, actions) else {
        // The command changed since the hook looked (or was typed by hand): run it verbatim.
        return sh(command, cwd, None);
    };
    let mut state = Run {
        paths: &paths,
        opts,
        dir: cwd.to_path_buf(),
        env: Env::new(),
        headers: HashSet::new(),
    };
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
        let mut ev = event(opts, command, &seg.raw);
        status = match action {
            Action::Assign { name, inner } => {
                ev.insert("action".into(), "assign".into());
                let script = format!("{}{inner}", env_prefix(&state.env));
                let (out, code) = sh_capture(&script, &state.dir);
                state
                    .env
                    .insert(name, out.trim_end_matches('\n').to_string());
                code
            }
            Action::Deferred { .. } => {
                let expanded = expand(&seg.raw, &state.env);
                ev.insert("expanded".into(), expanded.clone().into());
                match resolve(&expanded, &state) {
                    Some(a) => state.perform(a, &expanded, &mut ev),
                    None => {
                        ev.insert("action".into(), "plain".into());
                        let script = format!("{}{}", env_prefix(&state.env), seg.raw);
                        sh(&script, &state.dir, None)
                    }
                }
            }
            a => state.perform(a, &seg.raw, &mut ev),
        };
        ev.insert("latency_ms".into(), (t.elapsed().as_millis() as u64).into());
        log::event(&paths.dir, "exec", ev);
    }
    let _ = std::io::stdout().flush();
    status
}

/// A deferred segment with its variables expanded, classified for real; None runs it as written.
fn resolve(expanded: &str, state: &Run) -> Option<Action> {
    let segs = split(expanded)?;
    let [one] = segs.as_slice() else {
        return None;
    };
    if one.assign.is_some() {
        return None;
    }
    match classify(one, &state.dir, &state.paths.root)? {
        Action::Assign { .. } | Action::Deferred { .. } | Action::Cd(_) => None,
        a => Some(a),
    }
}

fn event(opts: &RunOpts, command: &str, segment: &str) -> Map<String, Value> {
    let mut ev = Map::new();
    ev.insert("tool".into(), "Bash".into());
    if !opts.session.is_empty() {
        ev.insert("session_id".into(), opts.session.clone().into());
    }
    if !opts.turn.is_empty() {
        ev.insert("turn_id".into(), opts.turn.clone().into());
    }
    let call = if opts.call.is_empty() {
        log::call_id()
    } else {
        opts.call.clone()
    };
    ev.insert("call_id".into(), call.into());
    ev.insert("original_command".into(), command.into());
    ev.insert("segment".into(), segment.into());
    ev
}

impl Run<'_> {
    /// Execute one classified segment; `script` is its text as `/bin/sh` must run it.
    fn perform(&mut self, action: Action, script: &str, ev: &mut Map<String, Value>) -> i32 {
        match action {
            Action::Cd(d) => {
                self.dir = d;
                ev.insert("action".into(), "cd".into());
                0
            }
            Action::Plain | Action::Assign { .. } | Action::Deferred { .. } => {
                ev.insert("action".into(), "plain".into());
                sh(script, &self.dir, None)
            }
            Action::Search { spec, filters } => self.search(*spec, &filters, script, ev),
            Action::Read { files } => self.read(files, script, ev),
            Action::List { dirs } => {
                ev.insert("kind".into(), "list".into());
                let code = sh(script, &self.dir, None);
                let op = Op::DirInfo {
                    paths: dirs,
                    cwd: self.dir.to_string_lossy().into(),
                };
                match ask(self.paths, op, ANSWER_TIMEOUT) {
                    Ok(r) => {
                        let text = text_of(&r);
                        answer_fields(ev, &text, &Value::Null, 0);
                        print!("{text}");
                    }
                    Err(e) => {
                        ev.insert("action".into(), "fallback".into());
                        ev.insert("error".into(), e.into());
                    }
                }
                code
            }
        }
    }

    fn search(
        &self,
        mut spec: graphite_daemon::SearchSpec,
        filters: &[String],
        script: &str,
        ev: &mut Map<String, Value>,
    ) -> i32 {
        let opts = self.opts;
        spec.all = opts.all;
        spec.format = opts.format;
        ev.insert("kind".into(), "search".into());
        ev.insert(
            "format".into(),
            serde_json::to_value(opts.format).unwrap_or(Value::Null),
        );
        let r = match ask(self.paths, Op::Search { spec }, ANSWER_TIMEOUT) {
            Ok(r) => r,
            Err(e) => return fallback(ev, &e, script, &self.dir),
        };
        let mut text = text_of(&r);
        if opts.format == OutFormat::Human {
            // Color only for a terminal; the daemon can't know.
            if let Some(rec) = r
                .data
                .get("record")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
            {
                use std::io::IsTerminal;
                let color =
                    std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
                text = graphite_daemon::answer::render_human(&rec, color);
            }
        }
        let stats = r.data.get("stats").cloned().unwrap_or(Value::Null);
        let matches = r.data.get("matches").and_then(Value::as_u64).unwrap_or(0);
        answer_fields(ev, &text, &stats, matches);
        if let Some(rec) = r.data.get("record") {
            let one_file = rec
                .get("no_filename")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                .then(|| rec.pointer("/items/0/path").and_then(Value::as_str))
                .flatten();
            if one_file.is_some() {
                ev.insert("keys".into(), log::answer_keys(&text, one_file).into());
            }
            if let Some(n) = rec
                .get("notices")
                .filter(|n| n.as_array().is_some_and(|a| !a.is_empty()))
            {
                ev.insert("pipeline".into(), n.clone());
            }
            ev.insert("record".into(), rec.clone());
        }
        if filters.is_empty() {
            print!("{text}");
            i32::from(matches == 0)
        } else {
            sh(&filters.join(" | "), &self.dir, Some(&text))
        }
    }

    /// The real read, preceded by a graph header sized to it (once per file per command).
    fn read(&mut self, files: Vec<String>, script: &str, ev: &mut Map<String, Value>) -> i32 {
        ev.insert("kind".into(), "read".into());
        let (out, code) = sh_capture(script, &self.dir);
        let fresh: Vec<String> = files
            .into_iter()
            .filter(|f| !self.headers.contains(f))
            .collect();
        if fresh.is_empty() {
            ev.insert("action".into(), "plain".into());
            ev.insert("note".into(), "header already shown in this command".into());
            print!("{out}");
            return code;
        }
        let op = Op::FileInfo {
            paths: fresh.clone(),
            cwd: self.dir.to_string_lossy().into(),
            budget: Some(read_header_budget(out.len())),
        };
        match ask(self.paths, op, ANSWER_TIMEOUT) {
            Ok(r) => {
                let text = text_of(&r);
                answer_fields(ev, &text, &Value::Null, 0);
                ev.insert("read_bytes".into(), (out.len() as u64).into());
                self.headers.extend(fresh);
                print!("{text}");
            }
            Err(e) => {
                ev.insert("action".into(), "fallback".into());
                ev.insert("error".into(), e.into());
            }
        }
        print!("{out}");
        code
    }
}

fn answer_fields(ev: &mut Map<String, Value>, text: &str, stats: &Value, matches: u64) {
    ev.insert("action".into(), "answer".into());
    ev.insert("answer".into(), text.into());
    ev.insert("answer_bytes".into(), (text.len() as u64).into());
    ev.insert("keys".into(), crate::log::answer_keys(text, None).into());
    let verdict = stats
        .get("graph_verdict")
        .cloned()
        .unwrap_or_else(|| "none".into());
    ev.insert("graph_verdict".into(), verdict);
    if let Some(c) = stats.get("classes") {
        ev.insert("residue".into(), c.clone());
        ev.insert("matches".into(), matches.into());
    }
    for k in ["raw_bytes", "search_ms", "total_ms", "budget_bytes"] {
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
