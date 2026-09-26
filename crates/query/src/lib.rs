//! Query functions and the agent-facing response contract over the store and its derived adjacency.

mod blast;
mod compress;
mod diff;
mod envelope;
mod lookup;
mod sites;
mod source;
mod text;
mod traverse;

use std::path::Path;

use graphite_model::EdgeKind;
use graphite_store::{Adjacency, GraphStore, StoreError, DEPENDENCY_KINDS};

pub use blast::{
    blast_radius, BlastRadius, DependentItem, Dependents, DirGroup, FileGroup, Summary,
};
pub use compress::estimate_tokens;
pub use diff::{
    diff_impact, git_diff_hunks, parse_unified_diff, ChangedSymbol, DiffImpact, Hunk, UnmappedHunk,
    MAX_COVERING_TESTS,
};
pub use envelope::{
    Causes, Completeness, CompletenessStatus, ConfidenceView, Disclosure, Envelope, Risk,
    RiskLevel, Role, SymbolView, Tier, SCHEMA_VERSION,
};
pub use lookup::{lookup, Lookup, LookupStatus, MAX_CANDIDATES};
pub use sites::{
    local_name, node_id, CallerSites, DirectSummary, Indirect, ModuleImpact, OverrideRef, TestRef,
    Tests, MAX_CALLERS,
};
pub use source::{SourceBlock, SourceReader, SourceState, SOURCE_CAP};
pub use text::{render_text, TextOptions};

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("git: {0}")]
    Git(String),
}

pub type Result<T> = std::result::Result<T, QueryError>;

/// What a query reads from. The daemon builds one per request from its live state.
pub struct QueryContext<'a> {
    pub store: &'a dyn GraphStore,
    pub adj: &'a Adjacency,
    /// Working-tree root; symbol paths are relative to it.
    pub root: &'a Path,
    /// Index is known to lag the working tree (freshness barrier timed out).
    pub stale: bool,
    /// Indexed files that failed to parse.
    pub parse_failures: u32,
}

/// Knobs shared by every query.
#[derive(Debug, Clone)]
pub struct Options {
    pub depth: u32,
    pub token_budget: usize,
    /// Skip inline source for dependents from the start.
    pub compact: bool,
    /// How many top-ranked dependents get inline source at tier full.
    pub source_items: usize,
    /// Direct callers listed with their call-site lines; counts always cover all.
    pub max_callers: usize,
    pub kinds: Vec<EdgeKind>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            depth: 3,
            token_budget: 8_000,
            compact: false,
            source_items: 12,
            max_callers: MAX_CALLERS,
            kinds: DEPENDENCY_KINDS.to_vec(),
        }
    }
}
