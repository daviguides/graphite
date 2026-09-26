//! `diff_impact`: diff hunks → changed symbols → combined blast radius + covering tests, in one call.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::process::Command;

use graphite_model::{EdgeKind, Symbol, SymbolId};
use serde::{Deserialize, Serialize};

use crate::blast::{build_dependents, trim_call_sites, Dependents, SymbolCache};
use crate::compress::{finish, Tiered};
use crate::envelope::{
    role, Causes, Disclosure, Envelope, Risk, RiskLevel, Role, SymbolView, Tier,
};
use crate::sites::{
    direct_sites, indirect, overrides, CallerSites, DirectSummary, Indirect, OverrideRef, Tests,
};
use crate::source::{SourceBlock, SourceReader, SourceState, SOURCE_CAP};
use crate::traverse::name_gaps;
use crate::{Options, QueryContext, QueryError, Result};

/// Changed line range on the new side of a diff (1-based). `line_count == 0` is a pure deletion after `start_line`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    pub path: String,
    pub start_line: u32,
    pub line_count: u32,
}

impl Hunk {
    fn lines(&self) -> (u32, u32) {
        let start = self.start_line.max(1);
        (start, start + self.line_count.max(1) - 1)
    }
}

/// Parse `git diff` unified output (any context size) into new-side hunks.
pub fn parse_unified_diff(text: &str) -> Vec<Hunk> {
    let mut out = Vec::new();
    let mut path: Option<String> = None;
    for line in text.lines() {
        if let Some(p) = line.strip_prefix("+++ ") {
            path = match p.trim() {
                "/dev/null" => None,
                p => Some(p.strip_prefix("b/").unwrap_or(p).to_string()),
            };
        } else if let Some(rest) = line.strip_prefix("@@ ") {
            let Some(path) = &path else { continue };
            let Some(new) = rest.split_whitespace().find(|t| t.starts_with('+')) else {
                continue;
            };
            let mut parts = new[1..].splitn(2, ',');
            let start = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            let count = parts
                .next()
                .map_or(Some(1), |s| s.parse().ok())
                .unwrap_or(1);
            out.push(Hunk {
                path: path.clone(),
                start_line: start,
                line_count: count,
            });
        }
    }
    out
}

/// Hunks of the working tree against `base` (default: HEAD, i.e. uncommitted changes).
pub fn git_diff_hunks(root: &Path, base: Option<&str>) -> Result<Vec<Hunk>> {
    let mut cmd = Command::new("git");
    cmd.current_dir(root)
        .args(["diff", "-U0", "--no-color", "--no-ext-diff"])
        .arg(base.unwrap_or("HEAD"))
        .arg("--");
    let out = cmd.output().map_err(|e| QueryError::Git(e.to_string()))?;
    if !out.status.success() {
        return Err(QueryError::Git(
            String::from_utf8_lossy(&out.stderr).trim().to_string(),
        ));
    }
    Ok(parse_unified_diff(&String::from_utf8_lossy(&out.stdout)))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChangedSymbol {
    pub symbol: SymbolView,
    /// Changed line ranges inside this symbol, as `[start, end]`.
    pub hunks: Vec<[u32; 2]>,
    pub direct_prod_callers: u32,
    pub direct_test_callers: u32,
    /// Ambiguous + unresolved references carrying this symbol's name.
    pub reference_gaps: u32,
    pub risk: RiskLevel,
    pub source: Option<SourceBlock>,
    /// Counts over every direct reference to this symbol.
    pub direct: DirectSummary,
    /// Direct callers with the exact lines where they reference this symbol.
    pub call_sites: Vec<CallerSites>,
    pub overrides: Vec<OverrideRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UnmappedHunk {
    pub path: String,
    pub start_line: u32,
    pub line_count: u32,
    pub reason: &'static str,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiffImpact {
    pub changed: Vec<ChangedSymbol>,
    pub unmapped: Vec<UnmappedHunk>,
    pub risk: Risk,
    pub dependents: Dependents,
    /// Test symbols to run: changed tests plus tests among the dependents, closest first.
    pub covering_tests: Vec<SymbolView>,
    /// How `covering_tests` was derived.
    pub tests_basis: &'static str,
    /// Dependents at depth ≥ 2 grouped by directory; survives every tier.
    pub indirect: Indirect,
    /// Runnable tests reaching the changed symbols, with pytest node ids.
    pub tests: Tests,
}

pub const MAX_COVERING_TESTS: usize = 50;
/// Callers listed with call-site lines per changed symbol.
pub const MAX_CHANGED_CALLERS: usize = 30;

impl Tiered for DiffImpact {
    fn degrade(&mut self, tier: Tier) -> Vec<Disclosure> {
        let mut out = self.dependents.degrade(tier);
        if tier == Tier::ByDirectory {
            for c in &mut self.changed {
                out.extend(trim_call_sites(&mut c.call_sites, "changed_call_sites"));
            }
            let n = self
                .changed
                .iter_mut()
                .filter_map(|c| c.source.take())
                .count();
            if n > 0 {
                out.push(Disclosure::new(
                    "changed_sources",
                    0,
                    n,
                    "tier by_directory: changed-symbol source omitted",
                ));
            }
        }
        out
    }

    fn shrink(&mut self) -> Option<String> {
        self.dependents
            .shrink()
            .then(|| "directory groups".to_string())
    }
}

/// Innermost symbols overlapping `[lo, hi]`: a container is dropped when a child overlaps too.
fn innermost(symbols: &[Symbol], lo: u32, hi: u32) -> Vec<&Symbol> {
    let hits: Vec<&Symbol> = symbols
        .iter()
        .filter(|s| s.start_line <= hi && s.end_line >= lo)
        .collect();
    hits.iter()
        .copied()
        .filter(|s| {
            !hits.iter().any(|t| {
                t.id != s.id
                    && t.start_line >= s.start_line
                    && t.end_line <= s.end_line
                    && (t.end_line - t.start_line) < (s.end_line - s.start_line)
            })
        })
        .collect()
}

/// Blast radius of everything the hunks touch, plus the tests to run.
pub fn diff_impact(
    ctx: &QueryContext,
    hunks: &[Hunk],
    opts: &Options,
) -> Result<Envelope<DiffImpact>> {
    let mut by_path: HashMap<String, Vec<Symbol>> = HashMap::new();
    let mut changed: BTreeMap<SymbolId, (Symbol, Vec<[u32; 2]>)> = BTreeMap::new();
    let mut unmapped = Vec::new();
    for h in hunks {
        if !by_path.contains_key(&h.path) {
            let syms = ctx.store.symbols_in_file(&h.path)?;
            by_path.insert(h.path.clone(), syms);
        }
        let syms = &by_path[&h.path];
        let (lo, hi) = h.lines();
        let hit = innermost(syms, lo, hi);
        if hit.is_empty() {
            unmapped.push(UnmappedHunk {
                path: h.path.clone(),
                start_line: h.start_line,
                line_count: h.line_count,
                reason: if syms.is_empty() {
                    "file not indexed"
                } else {
                    "outside any indexed symbol"
                },
            });
            continue;
        }
        for s in hit {
            let entry = changed
                .entry(s.id)
                .or_insert_with(|| (s.clone(), Vec::new()));
            entry.1.push([lo.max(s.start_line), hi.min(s.end_line)]);
        }
    }

    let mut causes = Causes {
        parse_failures: ctx.parse_failures,
        ref_scope: "changed_symbols".to_string(),
        ..Default::default()
    };
    let mut disclosures = Vec::new();
    if !unmapped.is_empty() {
        disclosures.push(Disclosure::new(
            "hunks",
            hunks.len() - unmapped.len(),
            unmapped.len(),
            "hunks in unindexed files or outside symbols are listed in `unmapped`, not analyzed",
        ));
    }

    let mut reader = SourceReader::new(ctx.root);
    let mut cache = SymbolCache::default();
    cache.prefetch(
        ctx,
        changed
            .keys()
            .flat_map(|id| ctx.adj.callers_of(*id))
            .map(|(src, _, _)| src),
    )?;
    let mut changed_views = Vec::new();
    let mut gaps_total = 0;
    for (sym, ranges) in changed.values() {
        let (amb, unres) = name_gaps(ctx.store, sym, &opts.kinds)?;
        causes.ambiguous_refs += amb;
        causes.unresolved_refs += unres;
        gaps_total += amb + unres;
        let callers = ctx.adj.callers_of(sym.id);
        let (mut prod, mut test) = (0, 0);
        for (src, kind, _) in &callers {
            if !opts.kinds.contains(kind) || *kind == EdgeKind::Imports {
                continue;
            }
            match cache.get(ctx, *src)?.map(role) {
                Some(Role::Prod) => prod += 1,
                Some(Role::Test) => test += 1,
                None => {}
            }
        }
        let source = if opts.compact {
            None
        } else {
            let block = reader.read(sym, SOURCE_CAP);
            if block.state != SourceState::Fresh {
                causes.source_changed += 1;
            }
            Some(block)
        };
        let (call_sites, direct, cut) = direct_sites(
            ctx,
            &[sym.id],
            &opts.kinds,
            &mut cache,
            opts.max_callers.min(MAX_CHANGED_CALLERS),
        )?;
        disclosures.extend(cut);
        let overrides = overrides(ctx, sym, &mut cache)?;
        changed_views.push(ChangedSymbol {
            symbol: SymbolView::from(sym),
            hunks: ranges.clone(),
            direct_prod_callers: prod,
            direct_test_callers: test,
            reference_gaps: amb + unres,
            risk: Risk::assess(prod, prod, test, amb + unres).level,
            source,
            direct,
            call_sites,
            overrides,
        });
    }
    changed_views.sort_by(|a, b| {
        (&a.symbol.path, a.symbol.start_line).cmp(&(&b.symbol.path, b.symbol.start_line))
    });

    let roots: Vec<SymbolId> = changed.keys().copied().collect();
    let built = build_dependents(ctx, &roots, opts, &mut reader, &mut cache)?;
    causes.source_changed += built.source_changed;
    disclosures.extend(built.disclosures);

    let mut tests: Vec<(u32, SymbolView)> = changed
        .values()
        .filter(|(s, _)| role(s) == Role::Test && runnable(s.kind.as_str()))
        .map(|(s, _)| (0, SymbolView::from(s)))
        .collect();
    tests.extend(
        built
            .dependents
            .items
            .iter()
            .filter(|i| i.symbol.role == Role::Test && runnable(i.symbol.kind))
            .map(|i| (i.depth, i.symbol.clone())),
    );
    tests.sort_by(|a, b| (a.0, &a.1.qualified).cmp(&(b.0, &b.1.qualified)));
    if tests.len() > MAX_COVERING_TESTS {
        disclosures.push(Disclosure::new(
            "covering_tests",
            MAX_COVERING_TESTS,
            tests.len() - MAX_COVERING_TESTS,
            "covering tests capped; closest first",
        ));
        tests.truncate(MAX_COVERING_TESTS);
    }

    let indirect = indirect(&built.dependents.items);
    let test_refs = crate::sites::tests(ctx, &roots, opts.depth.max(3), &mut cache)?;
    let risk = Risk::assess(
        built.prod_direct,
        built.prod_total,
        built.test_total,
        gaps_total,
    );
    let result = DiffImpact {
        changed: changed_views,
        unmapped,
        risk,
        dependents: built.dependents,
        covering_tests: tests.into_iter().map(|(_, v)| v).collect(),
        tests_basis: "test symbols among changed symbols and their dependents (no dedicated tests relation yet)",
        indirect,
        tests: test_refs,
    };
    Ok(finish(
        ctx,
        "diff_impact",
        result,
        causes,
        disclosures,
        opts,
    ))
}

/// Test modules are containers, not runnable tests.
fn runnable(kind: &str) -> bool {
    matches!(kind, "function" | "method" | "class")
}
