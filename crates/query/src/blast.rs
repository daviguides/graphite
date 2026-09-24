//! `blast_radius`: transitive dependents of one symbol, ranked, with risk and disclosure.

use std::collections::{BTreeMap, HashMap};

use graphite_model::{Symbol, SymbolId};
use serde::Serialize;

use crate::compress::{finish, Tiered};
use crate::envelope::{Causes, ConfidenceView, Disclosure, Envelope, Risk, Role, SymbolView, Tier};
use crate::lookup::{resolve_ref, Lookup};
use crate::source::{SourceBlock, SourceReader, SourceState, SOURCE_CAP};
use crate::traverse::{dependents, name_gaps};
use crate::{Options, QueryContext, Result};

/// A symbol that depends (transitively) on a root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DependentItem {
    pub symbol: SymbolView,
    pub depth: u32,
    /// Qualified name of the symbol this one reaches the root through.
    pub via: String,
    pub edge: &'static str,
    pub edge_confidence: ConfidenceView,
    /// Weakest edge on the path back to the root.
    pub confidence: ConfidenceView,
    pub fan_in: u32,
    pub source: Option<SourceBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileGroup {
    pub path: String,
    pub count: u32,
    pub prod: u32,
    pub test: u32,
    pub min_depth: u32,
    pub symbols: Vec<String>,
    pub omitted_symbols: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DirGroup {
    pub dir: String,
    pub files: u32,
    pub count: u32,
    pub prod: u32,
    pub test: u32,
    pub min_depth: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub total: u32,
    pub prod: u32,
    pub test: u32,
    /// Dependents per depth, index 0 = depth 1.
    pub by_depth: Vec<u32>,
    pub depth_limit: u32,
    /// Dependents at the depth limit that have further, unexplored dependents.
    pub more_beyond_depth: u32,
}

/// Dependents of a set of roots, in every tier's shape; the tier decides which are serialized.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Dependents {
    pub summary: Summary,
    pub items: Vec<DependentItem>,
    pub by_file: Vec<FileGroup>,
    pub by_directory: Vec<DirGroup>,
    #[serde(skip)]
    pending_by_file: Vec<FileGroup>,
    #[serde(skip)]
    pending_by_directory: Vec<DirGroup>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BlastRadius {
    pub target: Lookup,
    pub source: Option<SourceBlock>,
    pub risk: Option<Risk>,
    pub dependents: Dependents,
}

const FILE_GROUP_SYMBOLS: usize = 10;

pub(crate) struct Built {
    pub dependents: Dependents,
    pub prod_direct: u32,
    pub disclosures: Vec<Disclosure>,
    pub source_changed: u32,
}

/// Per-request symbol rows, filled in batches so a large result costs one store query, not one per symbol.
#[derive(Default)]
pub(crate) struct SymbolCache {
    rows: HashMap<SymbolId, Option<Symbol>>,
}

impl SymbolCache {
    /// Load every id not yet cached in one store call; ids the store doesn't know are remembered as absent.
    pub(crate) fn prefetch(
        &mut self,
        ctx: &QueryContext,
        ids: impl IntoIterator<Item = SymbolId>,
    ) -> Result<()> {
        let mut want: Vec<SymbolId> = ids
            .into_iter()
            .filter(|id| !self.rows.contains_key(id))
            .collect();
        if want.is_empty() {
            return Ok(());
        }
        want.sort();
        want.dedup();
        for id in &want {
            self.rows.insert(*id, None);
        }
        for sym in ctx.store.symbols(&want)? {
            self.rows.insert(sym.id, Some(sym));
        }
        Ok(())
    }

    pub(crate) fn get(&mut self, ctx: &QueryContext, id: SymbolId) -> Result<Option<&Symbol>> {
        self.prefetch(ctx, [id])?;
        Ok(self.rows[&id].as_ref())
    }
}

/// Dependents of `roots` (roots themselves excluded), ranked and grouped.
pub(crate) fn build_dependents(
    ctx: &QueryContext,
    roots: &[SymbolId],
    opts: &Options,
    reader: &mut SourceReader,
    cache: &mut SymbolCache,
) -> Result<Built> {
    let (reached, beyond) = dependents(ctx.adj, roots, opts.depth, &opts.kinds);
    cache.prefetch(ctx, reached.iter().flat_map(|r| [r.id, r.via]))?;
    let mut items = Vec::with_capacity(reached.len());
    let mut missing = 0usize;
    for r in reached {
        let Some(via) = cache.get(ctx, r.via)?.map(|v| v.qualified.clone()) else {
            missing += 1;
            continue;
        };
        let Some(sym) = cache.get(ctx, r.id)? else {
            missing += 1;
            continue;
        };
        items.push(DependentItem {
            symbol: SymbolView::from(sym),
            depth: r.depth,
            via,
            edge: r.kind.as_str(),
            edge_confidence: r.edge_conf.into(),
            confidence: r.path_conf.into(),
            fan_in: ctx.adj.callers_of(r.id).len() as u32,
            source: None,
        });
    }
    items.sort_by(|a, b| {
        (
            a.depth,
            a.symbol.role,
            a.confidence,
            std::cmp::Reverse(a.fan_in),
            &a.symbol.qualified,
            &a.symbol.id,
        )
            .cmp(&(
                b.depth,
                b.symbol.role,
                b.confidence,
                std::cmp::Reverse(b.fan_in),
                &b.symbol.qualified,
                &b.symbol.id,
            ))
    });

    let mut disclosures = Vec::new();
    if missing > 0 {
        disclosures.push(Disclosure::new(
            "dependents",
            items.len(),
            missing,
            "symbol rows missing from the store (removed mid-query); omitted",
        ));
    }
    if beyond > 0 {
        disclosures.push(Disclosure::new(
            "depth",
            items.len(),
            beyond as usize,
            format!(
                "{beyond} dependents at depth {} have further dependents; raise depth to see them",
                opts.depth
            ),
        ));
    }

    let mut source_changed = 0;
    let shown = if opts.compact {
        0
    } else {
        opts.source_items.min(items.len())
    };
    for item in items.iter_mut().take(shown) {
        let id = SymbolId::from_hex(&item.symbol.id).expect("own hex id");
        if let Some(sym) = cache.get(ctx, id)? {
            let block = reader.read(sym, SOURCE_CAP);
            if block.state != SourceState::Fresh {
                source_changed += 1;
            }
            item.source = Some(block);
        }
    }
    if items.len() > shown && !opts.compact {
        disclosures.push(Disclosure::new(
            "dependent_sources",
            shown,
            items.len() - shown,
            format!("source inlined for the top {shown} ranked dependents only"),
        ));
    }

    let mut summary = Summary {
        total: items.len() as u32,
        depth_limit: opts.depth,
        more_beyond_depth: beyond,
        by_depth: vec![0; opts.depth as usize],
        ..Default::default()
    };
    let mut prod_direct = 0;
    for it in &items {
        match it.symbol.role {
            Role::Prod => summary.prod += 1,
            Role::Test => summary.test += 1,
        }
        if it.depth == 1 && it.symbol.role == Role::Prod {
            prod_direct += 1;
        }
        summary.by_depth[(it.depth - 1) as usize] += 1;
    }
    let (by_file, by_dir) = group(&items);
    Ok(Built {
        dependents: Dependents {
            summary,
            items,
            by_file: Vec::new(),
            by_directory: Vec::new(),
            pending_by_file: by_file,
            pending_by_directory: by_dir,
        },
        prod_direct,
        disclosures,
        source_changed,
    })
}

fn group(items: &[DependentItem]) -> (Vec<FileGroup>, Vec<DirGroup>) {
    let mut files: BTreeMap<&str, FileGroup> = BTreeMap::new();
    for it in items {
        let g = files.entry(&it.symbol.path).or_insert_with(|| FileGroup {
            path: it.symbol.path.clone(),
            count: 0,
            prod: 0,
            test: 0,
            min_depth: u32::MAX,
            symbols: Vec::new(),
            omitted_symbols: 0,
        });
        g.count += 1;
        match it.symbol.role {
            Role::Prod => g.prod += 1,
            Role::Test => g.test += 1,
        }
        g.min_depth = g.min_depth.min(it.depth);
        if g.symbols.len() < FILE_GROUP_SYMBOLS {
            g.symbols.push(it.symbol.qualified.clone());
        } else {
            g.omitted_symbols += 1;
        }
    }
    let mut dirs: BTreeMap<String, (DirGroup, u32)> = BTreeMap::new();
    for g in files.values() {
        let dir = match g.path.rfind('/') {
            Some(i) => g.path[..i].to_string(),
            None => ".".to_string(),
        };
        let (d, _) = dirs.entry(dir.clone()).or_insert_with(|| {
            (
                DirGroup {
                    dir,
                    files: 0,
                    count: 0,
                    prod: 0,
                    test: 0,
                    min_depth: u32::MAX,
                },
                0,
            )
        });
        d.files += 1;
        d.count += g.count;
        d.prod += g.prod;
        d.test += g.test;
        d.min_depth = d.min_depth.min(g.min_depth);
    }
    let mut files: Vec<FileGroup> = files.into_values().collect();
    files.sort_by(|a, b| {
        (a.min_depth, std::cmp::Reverse(a.count), &a.path).cmp(&(
            b.min_depth,
            std::cmp::Reverse(b.count),
            &b.path,
        ))
    });
    let mut dirs: Vec<DirGroup> = dirs.into_values().map(|(d, _)| d).collect();
    dirs.sort_by(|a, b| {
        (a.min_depth, std::cmp::Reverse(a.count), &a.dir).cmp(&(
            b.min_depth,
            std::cmp::Reverse(b.count),
            &b.dir,
        ))
    });
    (files, dirs)
}

impl Dependents {
    /// Move to a coarser tier; returns what was cut.
    pub(crate) fn degrade(&mut self, tier: Tier) -> Vec<Disclosure> {
        let mut out = Vec::new();
        match tier {
            Tier::Full => {}
            Tier::Summary => {
                let stripped = self
                    .items
                    .iter_mut()
                    .filter_map(|i| i.source.take())
                    .count();
                if stripped > 0 {
                    out.push(Disclosure::new(
                        "dependent_sources",
                        0,
                        stripped,
                        "tier summary: dependent source omitted; signatures kept",
                    ));
                }
            }
            Tier::ByFile => {
                let n = self.items.len();
                self.items.clear();
                self.by_file = std::mem::take(&mut self.pending_by_file);
                if n > 0 {
                    out.push(Disclosure::new(
                        "dependents",
                        0,
                        n,
                        format!(
                            "tier by_file: {n} dependents grouped into {} files",
                            self.by_file.len()
                        ),
                    ));
                }
            }
            Tier::ByDirectory => {
                let n = self.items.len();
                self.items.clear();
                let files = self.by_file.len().max(self.pending_by_file.len());
                self.by_file.clear();
                self.pending_by_file.clear();
                self.by_directory = std::mem::take(&mut self.pending_by_directory);
                if n + files > 0 {
                    out.push(Disclosure::new(
                        "dependents",
                        0,
                        self.summary.total as usize,
                        format!("tier by_directory: {} dependents in {files} files grouped into {} directories", self.summary.total, self.by_directory.len()),
                    ));
                }
            }
        }
        out
    }

    /// Drop the lowest-ranked directory group; `false` when nothing is left to drop.
    pub(crate) fn shrink(&mut self) -> bool {
        self.by_directory.pop().is_some()
    }
}

impl Tiered for BlastRadius {
    fn degrade(&mut self, tier: Tier) -> Vec<Disclosure> {
        let mut out = self.dependents.degrade(tier);
        if tier == Tier::ByDirectory && self.source.take().is_some() {
            out.push(Disclosure::new(
                "target_source",
                0,
                1,
                "tier by_directory: target source omitted",
            ));
        }
        out
    }

    fn shrink(&mut self) -> Option<String> {
        self.dependents
            .shrink()
            .then(|| "directory groups".to_string())
    }
}

/// Transitive dependents of `symbol` (id, qualified name, suffix or bare name) up to `opts.depth`.
pub fn blast_radius(
    ctx: &QueryContext,
    symbol: &str,
    opts: &Options,
) -> Result<Envelope<BlastRadius>> {
    let resolved = resolve_ref(ctx, symbol)?;
    let (target, mut disclosures) = resolved.view(symbol);
    let mut causes = Causes {
        parse_failures: ctx.parse_failures,
        ref_scope: "target".to_string(),
        ..Default::default()
    };
    let Some(sym) = resolved.found().cloned() else {
        causes.ref_scope = "none".to_string();
        let result = BlastRadius {
            target,
            source: None,
            risk: None,
            dependents: Dependents::default(),
        };
        return Ok(finish(
            ctx,
            "blast_radius",
            result,
            causes,
            disclosures,
            opts,
        ));
    };

    let mut reader = SourceReader::new(ctx.root);
    let mut cache = SymbolCache::default();
    let source = reader.read(&sym, SOURCE_CAP);
    if source.state != SourceState::Fresh {
        causes.source_changed += 1;
    }
    let built = build_dependents(ctx, &[sym.id], opts, &mut reader, &mut cache)?;
    causes.source_changed += built.source_changed;
    disclosures.extend(built.disclosures);
    let (ambiguous, unresolved) = name_gaps(ctx.store, &sym, &opts.kinds)?;
    causes.ambiguous_refs = ambiguous;
    causes.unresolved_refs = unresolved;
    let s = &built.dependents.summary;
    let risk = Risk::assess(built.prod_direct, s.prod, s.test, ambiguous + unresolved);
    let result = BlastRadius {
        target,
        source: Some(source),
        risk: Some(risk),
        dependents: built.dependents,
    };
    Ok(finish(
        ctx,
        "blast_radius",
        result,
        causes,
        disclosures,
        opts,
    ))
}
