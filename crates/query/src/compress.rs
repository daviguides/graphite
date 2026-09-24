//! Tiered auto-compression under a token budget: full → summary → by_file → by_directory, every cut disclosed.

use serde::Serialize;

use crate::envelope::{Causes, Disclosure, Envelope, Tier, SCHEMA_VERSION};
use crate::{Options, QueryContext};

/// A result that can be degraded to coarser tiers.
pub(crate) trait Tiered {
    fn degrade(&mut self, tier: Tier) -> Vec<Disclosure>;
    /// Drop one lowest-ranked entry at the coarsest tier; names what was dropped, `None` if nothing left.
    fn shrink(&mut self) -> Option<String>;
}

/// Rough token estimate of the serialized envelope (4 bytes per token).
pub fn estimate_tokens<T: Serialize>(value: &T) -> usize {
    serde_json::to_string(value).map_or(usize::MAX, |s| s.len().div_ceil(4))
}

pub(crate) fn finish<T: Serialize + Tiered>(
    ctx: &QueryContext,
    query: &'static str,
    result: T,
    causes: Causes,
    disclosures: Vec<Disclosure>,
    opts: &Options,
) -> Envelope<T> {
    let mut env = Envelope {
        schema: SCHEMA_VERSION,
        query,
        graph_rev: ctx.adj.rev(),
        stale: ctx.stale,
        tier: Tier::Full,
        completeness: causes.completeness(),
        disclosures,
        result,
    };
    if ctx.stale {
        env.disclosures.push(Disclosure::new(
            "freshness",
            0,
            0,
            "index is behind the working tree; results reflect graph_rev",
        ));
    }
    if opts.compact {
        let cut = env.result.degrade(Tier::Summary);
        env.disclosures.extend(cut);
        env.tier = Tier::Summary;
    }
    for tier in [Tier::Summary, Tier::ByFile, Tier::ByDirectory] {
        if tier <= env.tier || estimate_tokens(&env) <= opts.token_budget {
            continue;
        }
        let cut = env.result.degrade(tier);
        env.disclosures.extend(cut);
        env.tier = tier;
    }
    let mut dropped = 0usize;
    let mut what = None;
    while estimate_tokens(&env) > opts.token_budget {
        match env.result.shrink() {
            Some(w) => {
                dropped += 1;
                what = Some(w);
            }
            None => break,
        }
    }
    if let Some(w) = what {
        env.disclosures.push(Disclosure::new(
            &w,
            0,
            dropped,
            format!(
                "token budget {}: lowest-ranked {w} dropped",
                opts.token_budget
            ),
        ));
    }
    env
}
