//! Resolves a user-supplied symbol reference (hex id, qualified name, suffix or bare name) with disambiguation.

use graphite_model::{Symbol, SymbolId};
use serde::Serialize;

use crate::envelope::{Causes, Disclosure, Envelope, SymbolView, Tier, SCHEMA_VERSION};
use crate::{QueryContext, Result};

pub const MAX_CANDIDATES: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupStatus {
    Found,
    /// Several symbols match; pick one by `id` or full `qualified` name. Never guessed.
    Ambiguous,
    NotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Lookup {
    pub query: String,
    pub status: LookupStatus,
    pub symbol: Option<SymbolView>,
    pub candidates: Vec<SymbolView>,
}

pub(crate) struct Resolved {
    pub status: LookupStatus,
    pub matches: Vec<Symbol>,
}

impl Resolved {
    pub fn found(&self) -> Option<&Symbol> {
        match self.status {
            LookupStatus::Found => self.matches.first(),
            _ => None,
        }
    }

    pub fn view(&self, query: &str) -> (Lookup, Vec<Disclosure>) {
        let mut disclosures = Vec::new();
        let (symbol, candidates) = match self.status {
            LookupStatus::Found => (self.matches.first().map(SymbolView::from), Vec::new()),
            _ => {
                let shown = self.matches.len().min(MAX_CANDIDATES);
                if self.matches.len() > shown {
                    disclosures.push(Disclosure::new(
                        "candidates",
                        shown,
                        self.matches.len() - shown,
                        format!("candidate list capped at {MAX_CANDIDATES}; narrow with a qualified name"),
                    ));
                }
                (
                    None,
                    self.matches[..shown].iter().map(SymbolView::from).collect(),
                )
            }
        };
        (
            Lookup {
                query: query.to_string(),
                status: self.status,
                symbol,
                candidates,
            },
            disclosures,
        )
    }
}

fn last_segment(q: &str) -> &str {
    let after_path = q.rsplit("::").next().unwrap_or(q);
    after_path.rsplit('.').next().unwrap_or(after_path)
}

fn classify(mut matches: Vec<Symbol>) -> Resolved {
    matches.sort_by(|a, b| (&a.qualified, &a.path, a.id).cmp(&(&b.qualified, &b.path, b.id)));
    let status = match matches.len() {
        0 => LookupStatus::NotFound,
        1 => LookupStatus::Found,
        _ => LookupStatus::Ambiguous,
    };
    Resolved { status, matches }
}

pub(crate) fn resolve_ref(ctx: &QueryContext, q: &str) -> Result<Resolved> {
    let q = q.trim();
    if let Some(id) = SymbolId::from_hex(q) {
        if let Some(sym) = ctx.store.symbol(id)? {
            return Ok(classify(vec![sym]));
        }
    }
    let by_name = ctx.store.symbols_by_name(last_segment(q))?;
    let qualified_query = q.contains('.') || q.contains("::");
    if !qualified_query {
        return Ok(classify(by_name));
    }
    let exact: Vec<Symbol> = by_name
        .iter()
        .filter(|s| s.qualified == q)
        .cloned()
        .collect();
    if !exact.is_empty() {
        return Ok(classify(exact));
    }
    let suffix: Vec<Symbol> = by_name
        .into_iter()
        .filter(|s| {
            s.qualified.ends_with(&format!(".{q}")) || s.qualified.ends_with(&format!("::{q}"))
        })
        .collect();
    Ok(classify(suffix))
}

/// Find a symbol by id, qualified name, qualified suffix or bare name.
pub fn lookup(ctx: &QueryContext, query: &str) -> Result<Envelope<Lookup>> {
    let resolved = resolve_ref(ctx, query)?;
    let (result, disclosures) = resolved.view(query);
    let causes = Causes {
        parse_failures: ctx.parse_failures,
        ref_scope: "none".to_string(),
        ..Default::default()
    };
    Ok(Envelope {
        schema: SCHEMA_VERSION,
        query: "lookup",
        graph_rev: ctx.adj.rev(),
        stale: ctx.stale,
        tier: Tier::Full,
        completeness: causes.completeness(),
        disclosures,
        result,
    })
}
