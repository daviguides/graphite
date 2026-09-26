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
    /// Records which on-disk layout `db` was built with.
    pub db_version: PathBuf,
    pub log: PathBuf,
}

/// Name of the per-repo state directory.
pub const STATE_DIR: &str = ".graphite";

const STATE_GITIGNORE: &str = "*\n";

/// Unix socket paths must fit `sockaddr_un.sun_path` (104 bytes on macOS, 108 on Linux).
const MAX_SOCKET_PATH: usize = 100;

/// Repo-local socket when it fits; otherwise a short per-user path keyed by the repo root.
fn socket_path(root: &Path, dir: &Path) -> PathBuf {
    let local = dir.join("daemon.sock");
    if local.as_os_str().len() <= MAX_SOCKET_PATH {
        return local;
    }
    let user = std::env::var("USER").unwrap_or_else(|_| "user".into());
    let key = blake3::hash(root.as_os_str().as_encoded_bytes()).to_hex();
    PathBuf::from(format!("/tmp/graphite-{user}/{}.sock", &key[..16]))
}

impl RepoPaths {
    pub fn new(root: &Path) -> Self {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let dir = root.join(STATE_DIR);
        RepoPaths {
            db: dir.join("db"),
            socket: socket_path(&root, &dir),
            lock: dir.join("daemon.lock"),
            db_version: dir.join("db.version"),
            log: dir.join("daemon.log"),
            dir,
            root,
        }
    }

    /// Create the state dir with a self-ignoring `.gitignore` so git never sees the index.
    pub fn ensure_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let ignore = self.dir.join(".gitignore");
        if std::fs::read_to_string(&ignore).ok().as_deref() != Some(STATE_GITIGNORE) {
            std::fs::write(ignore, STATE_GITIGNORE)?;
        }
        Ok(())
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
