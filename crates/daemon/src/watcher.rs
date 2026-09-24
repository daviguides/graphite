//! Filesystem watcher: records each change for the freshness barrier, then indexes eagerly in debounced batches.

use std::collections::BTreeSet;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};

use crate::engine::Engine;
use crate::Result;

/// Quiet period that closes a batch.
const QUIET: Duration = Duration::from_millis(15);
/// Upper bound on how long a batch may keep growing.
const MAX_BATCH: Duration = Duration::from_millis(100);

/// Keeps the OS watcher alive; dropping it stops watching.
pub struct WatchHandle {
    _watcher: notify::RecommendedWatcher,
}

/// Start watching the repo; indexing happens on a dedicated thread. Events arriving before `index_all` finishes are queued.
pub fn start(engine: Arc<Engine>) -> Result<WatchHandle> {
    let (tx, rx) = channel::<(String, u64)>();
    let root = engine.paths.root.clone();
    let paths = engine.paths.clone();
    let observer = engine.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(event) = res else { return };
        if event.kind.is_access() {
            return;
        }
        for p in event.paths {
            let Some(rel) = paths.relative(&p) else {
                continue;
            };
            if rel.is_empty() || rel.starts_with(".graphite") || rel.starts_with(".git/") {
                continue;
            }
            let seq = observer.fresh.observe();
            let _ = tx.send((rel, seq));
        }
    })?;
    watcher.watch(&root, RecursiveMode::Recursive)?;
    std::thread::Builder::new()
        .name("graphite-indexer".into())
        .spawn(move || run_indexer(engine, rx))?;
    Ok(WatchHandle { _watcher: watcher })
}

fn run_indexer(engine: Arc<Engine>, rx: Receiver<(String, u64)>) {
    while let Ok(first) = rx.recv() {
        let mut batch = BTreeSet::from([first.0]);
        let mut max_seq = first.1;
        let opened = Instant::now();
        loop {
            match rx.recv_timeout(QUIET) {
                Ok((rel, seq)) => {
                    batch.insert(rel);
                    max_seq = max_seq.max(seq);
                    if opened.elapsed() >= MAX_BATCH {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        for rel in batch {
            let is_file_path = rel.ends_with(".py") || engine.paths.root.join(&rel).is_file();
            let res = if is_file_path {
                engine.apply_path(&rel).map(|_| ())
            } else {
                engine.apply_dir(&rel).map(|_| ())
            };
            if let Err(e) = res {
                eprintln!("graphite: index {rel}: {e}");
            }
        }
        engine.fresh.mark_indexed(max_seq);
    }
}
