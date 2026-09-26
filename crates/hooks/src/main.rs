//! `graphite-hook`: Claude Code hook processor and routed-command runner.

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use graphite_daemon::answer::OutFormat;
use graphite_daemon::RepoPaths;
use graphite_hooks::{exec, install, logview, post, pre};
use serde_json::Value;

const USAGE: &str = "usage: graphite-hook pre | post | run [--all] [--human|--json|--explain] [--session ID] [--call ID] [--turn ID] [--cwd DIR] -- COMMAND | install|uninstall|status|log [-n N]|show N [--repo DIR]";

fn stdin_json() -> Option<Value> {
    let mut s = String::new();
    std::io::stdin().read_to_string(&mut s).ok()?;
    serde_json::from_str(&s).ok()
}

/// Hooks must never break the tool call: any failure prints nothing and exits 0.
fn hook(f: impl FnOnce(&Value) -> Option<Value>) -> ExitCode {
    if let Some(v) = stdin_json().and_then(|i| f(&i)) {
        println!("{v}");
    }
    ExitCode::SUCCESS
}

fn repo_arg(args: &[String]) -> PathBuf {
    let start = args
        .iter()
        .position(|a| a == "--repo")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    RepoPaths::discover(&start).root
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("graphite-hook"));
    match args.first().map(String::as_str) {
        Some("pre") => hook(|i| pre::handle(i, &exe)),
        Some("post") => hook(post::handle),
        Some("run") => {
            let sep = args.iter().position(|a| a == "--");
            let Some(sep) = sep else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            let opts = &args[1..sep];
            let val = |k: &str| {
                opts.iter()
                    .position(|a| a == k)
                    .and_then(|i| opts.get(i + 1))
                    .cloned()
                    .unwrap_or_default()
            };
            let has = |k: &str| opts.iter().any(|a| a == k);
            let run_opts = exec::RunOpts {
                all: has("--all"),
                format: if has("--human") {
                    OutFormat::Human
                } else if has("--json") {
                    OutFormat::Json
                } else if has("--explain") {
                    OutFormat::Explain
                } else {
                    OutFormat::Model
                },
                session: val("--session"),
                call: val("--call"),
                turn: val("--turn"),
            };
            let cwd = opts
                .iter()
                .position(|a| a == "--cwd")
                .and_then(|i| opts.get(i + 1))
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            let command = args[sep + 1..].join(" ");
            let code = exec::run(&command, &cwd, &run_opts);
            ExitCode::from(code.clamp(0, 255) as u8)
        }
        Some("log") => {
            let root = repo_arg(&args);
            let n = args
                .iter()
                .position(|a| a == "-n")
                .and_then(|i| args.get(i + 1))
                .and_then(|v| v.parse().ok())
                .unwrap_or(20);
            print!("{}", logview::list(&RepoPaths::new(&root).dir, n));
            ExitCode::SUCCESS
        }
        Some("show") => {
            let root = repo_arg(&args);
            let Some(n) = args.get(1).and_then(|v| v.parse::<usize>().ok()) else {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            };
            match logview::show(&RepoPaths::new(&root).dir, n) {
                Some(t) => {
                    print!("{t}");
                    ExitCode::SUCCESS
                }
                None => {
                    eprintln!("graphite-hook: no event #{n} in .graphite/hooks.jsonl");
                    ExitCode::FAILURE
                }
            }
        }
        Some(cmd @ ("install" | "uninstall" | "status")) => {
            let root = repo_arg(&args);
            let r = match cmd {
                "install" => install::install(&root, &exe).map(Value::String),
                "uninstall" => install::uninstall(&root).map(Value::String),
                _ => install::status(&root),
            };
            match r {
                Ok(Value::String(s)) => println!("{s}"),
                Ok(v) => println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default()),
                Err(e) => {
                    eprintln!("graphite-hook: {e}");
                    return ExitCode::FAILURE;
                }
            }
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
