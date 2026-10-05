//! Indexing engine: extracts files, writes facts to the store, keeps the derived adjacency in step.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, RwLock, RwLockReadGuard};
use std::time::Instant;

use graphite_model::FileFacts;
use graphite_store::{Adjacency, CozoStore, GraphStore};
use rayon::prelude::*;
use serde::Serialize;

use crate::freshness::Freshness;
use crate::paths::RepoPaths;
use crate::Result;

/// Directory names never indexed, on top of `.gitignore` and hidden entries; shared with embedded search.
const SKIP_DIRS: &[&str] = crate::paths::DEFAULT_EXCLUDES;

/// Bump whenever the store schema, extractor output or resolution rules change in a way old DB contents can't satisfy.
pub const DB_VERSION: u32 = 2;

/// Remove the DB if it was built with a different (or unknown) layout; true if wiped.
fn ensure_db_version(paths: &RepoPaths) -> Result<bool> {
    if !paths.db.exists() {
        return Ok(false);
    }
    let found = std::fs::read_to_string(&paths.db_version)
        .ok()
        .and_then(|s| s.trim().parse::<u32>().ok());
    if found == Some(DB_VERSION) {
        return Ok(false);
    }
    std::fs::remove_dir_all(&paths.db)?;
    Ok(true)
}

/// Outcome of the initial (or recovery) full sync.
#[derive(Debug, Clone, Default, Serialize)]
pub struct IndexStats {
    pub files_seen: usize,
    pub files_written: usize,
    pub files_removed: usize,
    pub symbols: usize,
    pub edges: usize,
    pub resolved_edges: usize,
    pub extract_ms: u128,
    pub write_ms: u128,
    pub adjacency_ms: u128,
    pub total_ms: u128,
}

/// What a single-path update did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Change {
    Unchanged,
    Replaced,
    Removed,
    Ignored,
}

pub struct Engine {
    pub paths: RepoPaths,
    pub store: CozoStore,
    pub fresh: Freshness,
    adj: RwLock<Adjacency>,
    hashes: Mutex<HashMap<String, [u8; 32]>>,
    failures: Mutex<HashSet<String>>,
    write: Mutex<()>,
    ignore: ignore::gitignore::Gitignore,
}

impl Engine {
    /// Open the repo's store; call `index_all` before serving. A DB built with another layout is wiped first.
    pub fn open(paths: RepoPaths) -> Result<Self> {
        paths.ensure_dir()?;
        let wiped = ensure_db_version(&paths)?;
        if wiped {
            eprintln!("graphite: index layout changed, rebuilding from scratch");
        }
        let store = CozoStore::open(&paths.db)?;
        std::fs::write(&paths.db_version, DB_VERSION.to_string())?;
        // Seed from what the store already holds so a restart rewrites only changed files.
        let hashes = store.file_hashes()?;
        let (ignore, _) = ignore::gitignore::Gitignore::new(paths.root.join(".gitignore"));
        Ok(Engine {
            paths,
            store,
            fresh: Freshness::default(),
            adj: RwLock::new(Adjacency::default()),
            hashes: Mutex::new(hashes),
            failures: Mutex::new(HashSet::new()),
            write: Mutex::new(()),
            ignore,
        })
    }

    pub fn adjacency(&self) -> RwLockReadGuard<'_, Adjacency> {
        self.adj.read().unwrap()
    }

    /// True if `rel` is a file this engine indexes (language + ignore rules).
    pub fn is_indexable(&self, rel: &str) -> bool {
        if !rel.ends_with(".py") {
            return false;
        }
        let parts: Vec<&str> = rel.split('/').collect();
        let (dirs, _) = parts.split_at(parts.len() - 1);
        if dirs
            .iter()
            .any(|d| d.starts_with('.') || SKIP_DIRS.contains(d))
        {
            return false;
        }
        !self
            .ignore
            .matched_path_or_any_parents(rel, false)
            .is_ignore()
    }

    fn walk(&self) -> Vec<String> {
        let skip = |e: &ignore::DirEntry| {
            let name = e.file_name().to_string_lossy();
            !(e.file_type().is_some_and(|t| t.is_dir()) && SKIP_DIRS.contains(&name.as_ref()))
        };
        let mut out: Vec<String> = ignore::WalkBuilder::new(&self.paths.root)
            .hidden(true)
            .filter_entry(skip)
            .build()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
            .filter_map(|e| self.paths.relative(e.path()))
            .filter(|rel| self.is_indexable(rel))
            .collect();
        out.sort();
        out
    }

    fn read_and_extract(root: &Path, rel: &str) -> Option<FileFacts> {
        let source = std::fs::read(root.join(rel)).ok()?;
        Some(graphite_extract_python::extract(rel, &source))
    }

    /// Full sync: extract every indexable file in parallel, write changed ones serially, drop vanished ones, rebuild adjacency.
    pub fn index_all(&self) -> Result<IndexStats> {
        let _w = self.write.lock().unwrap();
        let t0 = Instant::now();
        let files = self.walk();
        let root = self.paths.root.clone();
        let facts: Vec<FileFacts> = files
            .par_iter()
            .filter_map(|rel| Self::read_and_extract(&root, rel))
            .collect();
        let extract_ms = t0.elapsed().as_millis();

        let t1 = Instant::now();
        let mut stats = IndexStats {
            files_seen: files.len(),
            extract_ms,
            ..Default::default()
        };
        let mut hashes = self.hashes.lock().unwrap();
        for f in &facts {
            stats.symbols += f.symbols.len();
            stats.edges += f.edges.len();
            self.note_parse(&f.path, f.parse_ok);
            if hashes.get(&f.path) == Some(&f.content_hash) {
                continue;
            }
            self.store.replace_file(f)?;
            hashes.insert(f.path.clone(), f.content_hash);
            stats.files_written += 1;
        }
        let live: HashSet<&str> = facts.iter().map(|f| f.path.as_str()).collect();
        for path in self.store.file_paths()? {
            if !live.contains(path.as_str()) {
                self.store.remove_file(&path)?;
                hashes.remove(&path);
                self.failures.lock().unwrap().remove(&path);
                stats.files_removed += 1;
            }
        }
        stats.write_ms = t1.elapsed().as_millis();

        let t2 = Instant::now();
        let adj = Adjacency::rebuild(&self.store)?;
        stats.resolved_edges = adj.edge_count();
        *self.adj.write().unwrap() = adj;
        stats.adjacency_ms = t2.elapsed().as_millis();
        stats.total_ms = t0.elapsed().as_millis();
        Ok(stats)
    }

    /// Bring one repo-relative path up to date (edit, create or delete).
    pub fn apply_path(&self, rel: &str) -> Result<Change> {
        let _w = self.write.lock().unwrap();
        let abs = self.paths.root.join(rel);
        let known = self.hashes.lock().unwrap().contains_key(rel);
        if !abs.is_file() || !self.is_indexable(rel) {
            if !known {
                return Ok(Change::Ignored);
            }
            let delta = self.store.remove_file(rel)?;
            self.hashes.lock().unwrap().remove(rel);
            self.failures.lock().unwrap().remove(rel);
            self.adj.write().unwrap().apply(&self.store, &delta)?;
            return Ok(Change::Removed);
        }
        let source = std::fs::read(&abs)?;
        let hash = *blake3::hash(&source).as_bytes();
        if self.hashes.lock().unwrap().get(rel) == Some(&hash) {
            return Ok(Change::Unchanged);
        }
        let facts = graphite_extract_python::extract(rel, &source);
        let delta = self.store.replace_file(&facts)?;
        self.note_parse(rel, facts.parse_ok);
        self.hashes
            .lock()
            .unwrap()
            .insert(rel.to_string(), facts.content_hash);
        self.adj.write().unwrap().apply(&self.store, &delta)?;
        Ok(Change::Replaced)
    }

    /// A changed path that isn't a file: a directory was created, moved or deleted; reconcile everything under it.
    pub fn apply_dir(&self, rel: &str) -> Result<usize> {
        let prefix = format!("{}/", rel.trim_end_matches('/'));
        let mut targets: HashSet<String> = self
            .hashes
            .lock()
            .unwrap()
            .keys()
            .filter(|p| p.starts_with(&prefix))
            .cloned()
            .collect();
        let abs = self.paths.root.join(rel);
        if abs.is_dir() {
            for e in ignore::WalkBuilder::new(&abs)
                .hidden(true)
                .build()
                .flatten()
            {
                if let Some(r) = self.paths.relative(e.path()) {
                    if self.is_indexable(&r) {
                        targets.insert(r);
                    }
                }
            }
        }
        let mut n = 0;
        for t in targets {
            if !matches!(self.apply_path(&t)?, Change::Unchanged | Change::Ignored) {
                n += 1;
            }
        }
        Ok(n)
    }

    fn note_parse(&self, path: &str, ok: bool) {
        let mut f = self.failures.lock().unwrap();
        if ok {
            f.remove(path);
        } else {
            f.insert(path.to_string());
        }
    }

    /// Indexed files whose parse failed; reported in every answer's completeness causes.
    pub fn parse_failures(&self) -> u32 {
        self.failures.lock().unwrap().len() as u32
    }

    pub fn graph_rev(&self) -> u64 {
        self.store.graph_rev()
    }

    pub fn file_count(&self) -> usize {
        self.hashes.lock().unwrap().len()
    }

    /// Repo-relative paths of every indexed file.
    pub fn indexed_paths(&self) -> Vec<String> {
        self.hashes.lock().unwrap().keys().cloned().collect()
    }
}
