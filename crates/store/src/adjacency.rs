//! In-memory adjacency derived only from the store; serves hot traversals.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

use graphite_model::{EdgeKind, GraphRev, Provenance, SymbolId};

use crate::{EdgeKey, GraphStore, Outcome, Resolution, Result, WriteDelta};

/// Edge kinds that make the source depend on the target (containment is structure, not dependency).
pub const DEPENDENCY_KINDS: &[EdgeKind] = &[
    EdgeKind::Calls,
    EdgeKind::Imports,
    EdgeKind::Inherits,
    EdgeKind::References,
    EdgeKind::Tests,
];

#[derive(Debug, Clone, Copy)]
struct Link {
    src: SymbolId,
    dst: SymbolId,
    kind: EdgeKind,
    provenance: Provenance,
}

/// Resolved edges only, keyed by raw edge so a write can replace exactly what it touched.
#[derive(Debug, Default)]
pub struct Adjacency {
    rev: GraphRev,
    links: HashMap<EdgeKey, Link>,
    callers: HashMap<SymbolId, HashSet<EdgeKey>>,
    callees: HashMap<SymbolId, HashSet<EdgeKey>>,
    tests: HashSet<SymbolId>,
}

/// Comparable view of the adjacency, for incremental-equals-rebuild checks.
pub type EdgeSnapshot = BTreeSet<(EdgeKey, SymbolId, SymbolId, EdgeKind, Provenance)>;

impl Adjacency {
    /// Build from scratch from the store's current facts.
    pub fn rebuild(store: &dyn GraphStore) -> Result<Self> {
        let rev = store.graph_rev();
        let mut adj = Adjacency {
            rev,
            ..Default::default()
        };
        for r in store.resolve_all()? {
            adj.upsert(r);
        }
        adj.tests = store.test_symbols()?.into_iter().collect();
        Ok(adj)
    }

    /// Apply one committed write: drop the file's old edges, then re-resolve everything it may have changed.
    pub fn apply(&mut self, store: &dyn GraphStore, delta: &WriteDelta) -> Result<()> {
        for key in &delta.removed_keys {
            self.unlink(key);
        }
        for r in store.resolve_affected(delta)? {
            self.upsert(r);
        }
        for id in &delta.touched_ids {
            self.tests.remove(id);
        }
        for sym in store.symbols_in_file(&delta.path)? {
            if sym.is_test {
                self.tests.insert(sym.id);
            }
        }
        self.rev = self.rev.max(delta.rev);
        Ok(())
    }

    pub fn rev(&self) -> GraphRev {
        self.rev
    }

    fn unlink(&mut self, key: &EdgeKey) {
        if let Some(link) = self.links.remove(key) {
            if let Some(set) = self.callers.get_mut(&link.dst) {
                set.remove(key);
            }
            if let Some(set) = self.callees.get_mut(&link.src) {
                set.remove(key);
            }
        }
    }

    fn upsert(&mut self, r: Resolution) {
        self.unlink(&r.key);
        if let Outcome::Resolved { dst, provenance } = r.outcome {
            let link = Link {
                src: r.src,
                dst,
                kind: r.kind,
                provenance,
            };
            self.callers.entry(dst).or_default().insert(r.key.clone());
            self.callees.entry(r.src).or_default().insert(r.key.clone());
            self.links.insert(r.key, link);
        }
    }

    /// Direct dependents of `id` as (src, kind, provenance).
    pub fn callers_of(&self, id: SymbolId) -> Vec<(SymbolId, EdgeKind, Provenance)> {
        self.neighbors(&self.callers, id, |l| l.src)
    }

    /// Direct dependencies of `id` as (dst, kind, provenance).
    pub fn callees_of(&self, id: SymbolId) -> Vec<(SymbolId, EdgeKind, Provenance)> {
        self.neighbors(&self.callees, id, |l| l.dst)
    }

    fn neighbors(
        &self,
        index: &HashMap<SymbolId, HashSet<EdgeKey>>,
        id: SymbolId,
        pick: impl Fn(&Link) -> SymbolId,
    ) -> Vec<(SymbolId, EdgeKind, Provenance)> {
        let mut out: Vec<_> = index
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|k| self.links.get(k))
            .map(|l| (pick(l), l.kind, l.provenance))
            .collect();
        out.sort();
        out
    }

    /// Transitive dependents of `target` up to `max_depth`, each at its shallowest depth, sorted by (depth, id).
    pub fn blast_radius(
        &self,
        target: SymbolId,
        max_depth: u32,
        kinds: &[EdgeKind],
    ) -> Vec<(SymbolId, u32)> {
        let mut depth: HashMap<SymbolId, u32> = HashMap::from([(target, 0)]);
        let mut queue = VecDeque::from([target]);
        while let Some(n) = queue.pop_front() {
            let d = depth[&n];
            if d >= max_depth {
                continue;
            }
            for key in self.callers.get(&n).into_iter().flatten() {
                let Some(link) = self.links.get(key) else {
                    continue;
                };
                if !kinds.contains(&link.kind) {
                    continue;
                }
                if let std::collections::hash_map::Entry::Vacant(v) = depth.entry(link.src) {
                    v.insert(d + 1);
                    queue.push_back(link.src);
                }
            }
        }
        let mut out: Vec<(SymbolId, u32)> =
            depth.into_iter().filter(|(id, _)| *id != target).collect();
        out.sort_by_key(|(id, d)| (*d, *id));
        out
    }

    /// Tests covering `target`: test symbols reaching it through calls/references, nearest first.
    /// Derived at read time from resolved calls, so it tracks the graph without a stored relation.
    pub fn covering_tests(&self, target: SymbolId, max_depth: u32) -> Vec<(SymbolId, u32)> {
        self.blast_radius(target, max_depth, &[EdgeKind::Calls, EdgeKind::References])
            .into_iter()
            .filter(|(id, _)| self.tests.contains(id))
            .collect()
    }

    pub fn is_test(&self, id: SymbolId) -> bool {
        self.tests.contains(&id)
    }

    pub fn test_set(&self) -> BTreeSet<SymbolId> {
        self.tests.iter().copied().collect()
    }

    pub fn snapshot(&self) -> EdgeSnapshot {
        self.links
            .iter()
            .map(|(k, l)| (k.clone(), l.src, l.dst, l.kind, l.provenance))
            .collect()
    }

    pub fn edge_count(&self) -> usize {
        self.links.len()
    }
}
