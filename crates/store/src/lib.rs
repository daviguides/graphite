//! Graph storage: CozoDB holds per-file facts and resolution rules; `Adjacency` is derived from it for hot traversals.

mod adjacency;
mod cozo_store;
mod rows;

pub use adjacency::{Adjacency, EdgeSnapshot, Site, DEPENDENCY_KINDS};
pub use cozo_store::CozoStore;

use std::collections::HashMap;

use graphite_model::{EdgeKind, FileFacts, GraphRev, Provenance, Symbol, SymbolId};

/// Store error.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("cozo: {0}")]
    Cozo(String),
    #[error("corrupt row: {0}")]
    Corrupt(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Identity of one raw edge: its file and position in that file's edge list.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EdgeKey {
    pub path: String,
    pub idx: u32,
}

/// Result of resolving one raw edge against the current symbol table.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Outcome {
    Resolved {
        dst: SymbolId,
        provenance: Provenance,
    },
    /// Several candidates matched at the best tier; kept, not guessed.
    Ambiguous {
        candidates: u32,
    },
    Unresolved,
}

/// One raw edge after resolution.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Resolution {
    pub key: EdgeKey,
    pub src: SymbolId,
    pub kind: EdgeKind,
    pub site_line: u32,
    pub outcome: Outcome,
}

/// Confidence tier derived from provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Confidence {
    Extracted,
    Inferred,
    Ambiguous,
}

/// Rule: extractor-bound or import-resolved edges are certain; qualifier matches are inferred; bare name guesses are weak.
pub fn confidence(p: Provenance) -> Confidence {
    match p {
        Provenance::Extracted | Provenance::Resolved => Confidence::Extracted,
        Provenance::Inferred => Confidence::Inferred,
        Provenance::NameGuess => Confidence::Ambiguous,
    }
}

/// What a committed write touched; enough for `Adjacency::apply` to update without a rebuild.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WriteDelta {
    pub rev: GraphRev,
    pub path: String,
    pub removed_keys: Vec<EdgeKey>,
    pub touched_names: Vec<String>,
    pub touched_ids: Vec<SymbolId>,
    /// The file had or has classes or inheritance, so self/cls/super() edges anywhere may re-resolve.
    pub touched_classes: bool,
}

/// Storage contract. CozoDB is the only implementation today; kept behind a trait so the engine can be swapped.
pub trait GraphStore: Send + Sync {
    fn replace_file(&self, facts: &FileFacts) -> Result<WriteDelta>;
    fn remove_file(&self, path: &str) -> Result<WriteDelta>;
    fn graph_rev(&self) -> GraphRev;
    fn file_paths(&self) -> Result<Vec<String>>;
    fn symbol(&self, id: SymbolId) -> Result<Option<Symbol>>;
    fn symbols_by_name(&self, name: &str) -> Result<Vec<Symbol>>;
    fn symbols_in_file(&self, path: &str) -> Result<Vec<Symbol>>;
    /// Batch lookup; unknown ids are skipped, order is unspecified.
    fn symbols(&self, ids: &[SymbolId]) -> Result<Vec<Symbol>>;
    /// blake3 content hash of every stored file, so a restart can skip unchanged files.
    fn file_hashes(&self) -> Result<HashMap<String, [u8; 32]>>;
    /// Ambiguous or unresolved references named `name`, excluding stdlib/builtin targets: potential
    /// hidden dependents of any symbol with that name.
    fn name_gaps(&self, name: &str) -> Result<Vec<Resolution>>;
    fn test_symbols(&self) -> Result<Vec<SymbolId>>;
    /// Every raw edge resolved; used to rebuild the adjacency from scratch.
    fn resolve_all(&self) -> Result<Vec<Resolution>>;
    /// Only the raw edges whose resolution a write may have changed.
    fn resolve_affected(&self, delta: &WriteDelta) -> Result<Vec<Resolution>>;
    /// Resolved edges pointing at `id`.
    fn callers(&self, id: SymbolId) -> Result<Vec<Resolution>>;
    /// Resolved edges leaving `id`.
    fn callees(&self, id: SymbolId) -> Result<Vec<Resolution>>;
}
