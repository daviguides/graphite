//! Where a repo's daemon keeps its state.

use std::path::{Path, PathBuf};

/// State layout under `<repo>/.graphite/`.
#[derive(Debug, Clone)]
pub struct RepoPaths {
    pub root: PathBuf,
    pub dir: PathBuf,
    pub db: PathBuf,
    pub socket: PathBuf,
    pub lock: PathBuf,
    pub log: PathBuf,
}

impl RepoPaths {
    pub fn new(root: &Path) -> Self {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let dir = root.join(".graphite");
        RepoPaths {
            db: dir.join("db"),
            socket: dir.join("daemon.sock"),
            lock: dir.join("daemon.lock"),
            log: dir.join("daemon.log"),
            dir,
            root,
        }
    }

    /// Repo root for `start`: nearest ancestor with `.git`, else `start` itself.
    pub fn discover(start: &Path) -> Self {
        let start = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
        let root = start
            .ancestors()
            .find(|p| p.join(".git").exists())
            .unwrap_or(&start)
            .to_path_buf();
        RepoPaths::new(&root)
    }

    /// Repo-relative path with `/` separators, or None if outside the repo.
    pub fn relative(&self, abs: &Path) -> Option<String> {
        let rel = abs.strip_prefix(&self.root).ok()?;
        Some(rel.to_string_lossy().replace('\\', "/"))
    }
}
