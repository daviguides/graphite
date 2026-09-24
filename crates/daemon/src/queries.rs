//! Query dispatch: builds a `graphite_query::QueryContext` from live daemon state per request.

use graphite_query::{Options, QueryContext};
use serde::Serialize;
use serde_json::Value;

use crate::engine::Engine;

pub type QueryResult = std::result::Result<Value, String>;

/// Per-request knobs a client may override; unset fields keep `Options::default()`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Knobs {
    pub depth: Option<u32>,
    pub budget: Option<usize>,
    pub compact: Option<bool>,
}

impl Knobs {
    fn options(self) -> Options {
        let mut o = Options::default();
        if let Some(d) = self.depth {
            o.depth = d;
        }
        if let Some(b) = self.budget {
            o.token_budget = b;
        }
        if let Some(c) = self.compact {
            o.compact = c;
        }
        o
    }
}

/// Seam between the daemon and the query layer; answers are serialized envelopes.
pub trait QueryHandler: Send + Sync {
    fn lookup(&self, engine: &Engine, stale: bool, symbol: &str) -> QueryResult;
    fn blast(&self, engine: &Engine, stale: bool, symbol: &str, knobs: Knobs) -> QueryResult;
    fn diff_impact(
        &self,
        engine: &Engine,
        stale: bool,
        base: Option<&str>,
        knobs: Knobs,
    ) -> QueryResult;
}

/// Production handler backed by graphite-query.
pub struct GraphQueries;

fn to_value<T: Serialize>(v: graphite_query::Result<T>) -> QueryResult {
    let v = v.map_err(|e| e.to_string())?;
    serde_json::to_value(v).map_err(|e| e.to_string())
}

fn with_ctx<R>(engine: &Engine, stale: bool, f: impl FnOnce(&QueryContext) -> R) -> R {
    let adj = engine.adjacency();
    let ctx = QueryContext {
        store: &engine.store,
        adj: &adj,
        root: &engine.paths.root,
        stale,
        parse_failures: engine.parse_failures(),
    };
    f(&ctx)
}

impl QueryHandler for GraphQueries {
    fn lookup(&self, engine: &Engine, stale: bool, symbol: &str) -> QueryResult {
        with_ctx(engine, stale, |ctx| {
            to_value(graphite_query::lookup(ctx, symbol))
        })
    }

    fn blast(&self, engine: &Engine, stale: bool, symbol: &str, knobs: Knobs) -> QueryResult {
        let opts = knobs.options();
        with_ctx(engine, stale, |ctx| {
            to_value(graphite_query::blast_radius(ctx, symbol, &opts))
        })
    }

    fn diff_impact(
        &self,
        engine: &Engine,
        stale: bool,
        base: Option<&str>,
        knobs: Knobs,
    ) -> QueryResult {
        let hunks =
            graphite_query::git_diff_hunks(&engine.paths.root, base).map_err(|e| e.to_string())?;
        let opts = knobs.options();
        with_ctx(engine, stale, |ctx| {
            to_value(graphite_query::diff_impact(ctx, &hunks, &opts))
        })
    }
}
