//! Daemon main loop: single-instance lock, initial sync, watcher, unix-socket server.

use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fs4::fs_std::FileExt;
use serde_json::{json, Value};

use crate::engine::{Engine, IndexStats};
use crate::paths::RepoPaths;
use crate::protocol::{Op, Request, Response};
use crate::queries::{Knobs, QueryHandler};
use crate::{watcher, DaemonError, Result};

/// How long a query waits for already-observed edits before answering `stale: true`.
pub const FRESHNESS_WAIT: Duration = Duration::from_millis(200);

struct State {
    engine: Arc<Engine>,
    handler: Box<dyn QueryHandler>,
    started: Instant,
    init: Mutex<IndexStats>,
    shutdown: AtomicBool,
}

/// Run the daemon for `paths` until a `shutdown` request; blocks the calling thread.
pub fn run(paths: RepoPaths, handler: Box<dyn QueryHandler>) -> Result<()> {
    std::fs::create_dir_all(&paths.dir)?;
    let lock = File::create(&paths.lock)?;
    if !lock.try_lock_exclusive().unwrap_or(false) {
        return Err(DaemonError::AlreadyRunning(
            paths.root.display().to_string(),
        ));
    }

    let engine = Arc::new(Engine::open(paths.clone())?);
    let _watch = watcher::start(engine.clone())?;
    let stats = engine.index_all()?;
    eprintln!(
        "graphite: indexed {}",
        serde_json::to_string(&stats).unwrap_or_default()
    );

    if let Some(parent) = paths.socket.parent() {
        std::fs::create_dir_all(parent)?;
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
    }
    let _ = std::fs::remove_file(&paths.socket);
    let listener = UnixListener::bind(&paths.socket)?;
    let state = Arc::new(State {
        engine,
        handler,
        started: Instant::now(),
        init: Mutex::new(stats),
        shutdown: AtomicBool::new(false),
    });

    for stream in listener.incoming() {
        if state.shutdown.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = stream else { continue };
        let st = state.clone();
        std::thread::spawn(move || serve(st, stream));
    }
    let _ = std::fs::remove_file(&paths.socket);
    drop(lock);
    Ok(())
}

fn serve(state: Arc<State>, stream: UnixStream) {
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut writer = stream;
    for line in BufReader::new(read_half).lines() {
        let Ok(line) = line else { return };
        if line.trim().is_empty() {
            continue;
        }
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(req) => handle(&state, req.op),
            Err(e) => Response::err(state.engine.graph_rev(), format!("bad request: {e}")),
        };
        let mut out = serde_json::to_string(&response).unwrap_or_else(|_| "{}".into());
        out.push('\n');
        if writer.write_all(out.as_bytes()).is_err() {
            return;
        }
        if state.shutdown.load(Ordering::SeqCst) {
            // Unblock the accept loop so `run` can return.
            let _ = UnixStream::connect(&state.engine.paths.socket);
            return;
        }
    }
}

fn handle(state: &State, op: Op) -> Response {
    let engine = &state.engine;
    let fresh = match op {
        Op::Nudge { .. } | Op::Shutdown => true,
        _ => engine.fresh.wait_current(FRESHNESS_WAIT),
    };
    let stale = !fresh;
    let result: std::result::Result<Value, String> = match &op {
        Op::Status => Ok(status(state)),
        Op::Lookup { symbol } => state.handler.lookup(engine, stale, symbol),
        Op::Blast {
            symbol,
            depth,
            budget,
            compact,
        } => {
            let knobs = Knobs {
                depth: *depth,
                budget: *budget,
                compact: *compact,
            };
            state.handler.blast(engine, stale, symbol, knobs)
        }
        Op::DiffImpact {
            base,
            depth,
            budget,
            compact,
        } => {
            let knobs = Knobs {
                depth: *depth,
                budget: *budget,
                compact: *compact,
            };
            state
                .handler
                .diff_impact(engine, stale, base.as_deref(), knobs)
        }
        Op::Nudge { paths } => nudge(engine, paths),
        Op::Shutdown => {
            state.shutdown.store(true, Ordering::SeqCst);
            Ok(json!({"stopping": true}))
        }
    };
    let rev = engine.graph_rev();
    match result {
        Ok(data) => Response::ok(rev, !fresh, data),
        Err(e) => Response::err(rev, e),
    }
}

fn status(state: &State) -> Value {
    let e = &state.engine;
    json!({
        "root": e.paths.root,
        "pid": std::process::id(),
        "uptime_s": state.started.elapsed().as_secs(),
        "files": e.file_count(),
        "adjacency_edges": e.adjacency().edge_count(),
        "pending_events": e.fresh.pending(),
        "init": *state.init.lock().unwrap(),
    })
}

fn nudge(engine: &Engine, paths: &[String]) -> std::result::Result<Value, String> {
    let mut changes = serde_json::Map::new();
    for p in paths {
        let abs = std::path::Path::new(p);
        let rel = if abs.is_absolute() {
            match engine.paths.relative(abs) {
                Some(r) => r,
                None => {
                    changes.insert(p.clone(), json!("outside_repo"));
                    continue;
                }
            }
        } else {
            p.clone()
        };
        let res = if engine.paths.root.join(&rel).is_dir() {
            engine
                .apply_dir(&rel)
                .map(|n| json!({"dir_files_changed": n}))
        } else {
            engine.apply_path(&rel).map(|c| json!(c))
        };
        changes.insert(rel, res.map_err(|e| e.to_string())?);
    }
    Ok(Value::Object(changes))
}
