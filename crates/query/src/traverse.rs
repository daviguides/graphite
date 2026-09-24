//! Dependents traversal with depth labels and the weakest-edge confidence of the chosen path.

use std::collections::{BTreeMap, HashMap, HashSet};

use graphite_model::{EdgeKind, Symbol, SymbolId};
use graphite_store::{confidence, Adjacency, Confidence, GraphStore, Outcome, WriteDelta};

use crate::Result;

/// One dependent reached from the roots.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Reached {
    pub id: SymbolId,
    pub depth: u32,
    pub via: SymbolId,
    pub kind: EdgeKind,
    pub edge_conf: Confidence,
    pub path_conf: Confidence,
}

/// Level-by-level BFS over callers. Each node keeps its shallowest depth; among equal-depth paths the
/// strongest (by weakest edge) wins, ties broken by ids for determinism.
pub(crate) fn dependents(
    adj: &Adjacency,
    roots: &[SymbolId],
    max_depth: u32,
    kinds: &[EdgeKind],
) -> (Vec<Reached>, u32) {
    let mut seen: HashSet<SymbolId> = roots.iter().copied().collect();
    let mut path_conf: HashMap<SymbolId, Confidence> =
        roots.iter().map(|r| (*r, Confidence::Extracted)).collect();
    let mut frontier: Vec<SymbolId> = {
        let mut f = roots.to_vec();
        f.sort();
        f.dedup();
        f
    };
    let mut out = Vec::new();
    for depth in 1..=max_depth {
        let mut best: BTreeMap<SymbolId, (Confidence, Confidence, SymbolId, EdgeKind)> =
            BTreeMap::new();
        for n in &frontier {
            let base = path_conf[n];
            for (src, kind, prov) in adj.callers_of(*n) {
                if !kinds.contains(&kind) || seen.contains(&src) {
                    continue;
                }
                let edge = confidence(prov);
                let cand = (base.max(edge), edge, *n, kind);
                best.entry(src)
                    .and_modify(|cur| {
                        if cand < *cur {
                            *cur = cand;
                        }
                    })
                    .or_insert(cand);
            }
        }
        if best.is_empty() {
            return (out, 0);
        }
        frontier.clear();
        for (id, (pc, edge, via, kind)) in best {
            seen.insert(id);
            path_conf.insert(id, pc);
            frontier.push(id);
            out.push(Reached {
                id,
                depth,
                via,
                kind,
                edge_conf: edge,
                path_conf: pc,
            });
        }
    }
    let beyond = frontier
        .iter()
        .filter(|n| {
            adj.callers_of(**n)
                .iter()
                .any(|(src, kind, _)| kinds.contains(kind) && !seen.contains(src))
        })
        .count() as u32;
    (out, beyond)
}

/// References carrying the target's name that did not resolve to a single symbol: potential hidden dependents.
pub(crate) fn name_gaps(
    store: &dyn GraphStore,
    sym: &Symbol,
    kinds: &[EdgeKind],
) -> Result<(u32, u32)> {
    let probe = WriteDelta {
        touched_names: vec![sym.name.clone()],
        ..Default::default()
    };
    let (mut ambiguous, mut unresolved) = (0, 0);
    for r in store.resolve_affected(&probe)? {
        if r.src == sym.id || !kinds.contains(&r.kind) {
            continue;
        }
        match r.outcome {
            Outcome::Ambiguous { .. } => ambiguous += 1,
            Outcome::Unresolved => unresolved += 1,
            Outcome::Resolved { .. } => {}
        }
    }
    Ok((ambiguous, unresolved))
}
