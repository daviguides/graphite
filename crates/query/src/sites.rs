//! Agent-oriented facts about a target: where it is referenced (line by line), what depends on it
//! indirectly (by module), which tests reach it, and which methods override it or are overridden by it.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use graphite_model::{EdgeKind, Symbol, SymbolId, SymbolKind};
use graphite_store::{confidence, Confidence};
use serde::Serialize;

use crate::blast::{DependentItem, SymbolCache};
use crate::envelope::{role, ConfidenceView, Disclosure, Role, SymbolView};
use crate::{QueryContext, Result};

/// One caller of the target and every line where it references it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CallerSites {
    pub caller: SymbolView,
    pub lines: Vec<u32>,
    pub edge: &'static str,
    /// Strongest confidence among this caller's references.
    pub confidence: ConfidenceView,
    /// Local names of the caller's own top callers, most-called first.
    pub called_by: Vec<String>,
    pub called_by_total: u32,
}

/// Counts over every direct reference, independent of how many are listed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DirectSummary {
    pub callers: u32,
    pub sites: u32,
    pub files: u32,
    pub prod: u32,
    pub test: u32,
    /// Files importing the target by name; not listed as call sites (a signature change doesn't break them).
    pub imported_by_files: u32,
}

/// Indirect dependents (depth ≥ 2) grouped by directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModuleImpact {
    pub module: String,
    pub count: u32,
    pub prod: u32,
    pub test: u32,
    pub min_depth: u32,
    /// Local names of the top-ranked dependents in this module.
    pub top: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Indirect {
    pub total: u32,
    pub prod: u32,
    pub test: u32,
    pub modules: u32,
    pub groups: Vec<ModuleImpact>,
}

/// A runnable test that reaches the target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TestRef {
    /// pytest-style node id: `path::Class::test`.
    pub node_id: String,
    pub path: String,
    pub line: u32,
    pub depth: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Tests {
    pub total: u32,
    pub depth_limit: u32,
    pub items: Vec<TestRef>,
}

/// A method related to the target through class inheritance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OverrideRef {
    /// `overridden_by`: a subclass redefines the target; `overrides`: the target redefines a base method.
    pub relation: &'static str,
    pub symbol: SymbolView,
}

pub const MAX_CALLERS: usize = 200;
pub const MAX_CALLED_BY: usize = 3;
pub const MAX_INDIRECT_GROUPS: usize = 12;
pub const MAX_TESTS: usize = 20;
const MAX_INHERIT_DEPTH: u32 = 6;

/// Name relative to the symbol's own module: `Config.get` for `app.core.Config.get` in `app/core.py`.
pub fn local_name<'a>(qualified: &'a str, path: &str) -> &'a str {
    let stem = path.strip_suffix(".py").unwrap_or(path);
    let module = stem.replace('/', ".");
    let package = module.strip_suffix(".__init__").unwrap_or(&module);
    for prefix in [module.as_str(), package] {
        if let Some(rest) = qualified
            .strip_prefix(prefix)
            .and_then(|r| r.strip_prefix('.'))
        {
            return rest;
        }
    }
    if let Some(i) = qualified.rfind('.') {
        let (head, tail) = qualified.split_at(i);
        let class_tail = head.rsplit('.').next().unwrap_or("");
        if class_tail.chars().next().is_some_and(char::is_uppercase) {
            return &qualified[i - class_tail.len()..];
        }
        return &tail[1..];
    }
    qualified
}

/// pytest node id of a test symbol.
pub fn node_id(qualified: &str, path: &str) -> String {
    format!("{path}::{}", local_name(qualified, path).replace('.', "::"))
}

fn local_of(s: &Symbol) -> String {
    local_name(&s.qualified, &s.path).to_string()
}

/// Every direct reference into `targets`, grouped per caller with call-site lines.
pub(crate) fn direct_sites(
    ctx: &QueryContext,
    targets: &[SymbolId],
    kinds: &[EdgeKind],
    cache: &mut SymbolCache,
    cap: usize,
) -> Result<(Vec<CallerSites>, DirectSummary, Vec<Disclosure>)> {
    let excluded: HashSet<SymbolId> = targets.iter().copied().collect();
    let mut per_caller: BTreeMap<SymbolId, (Vec<u32>, EdgeKind, Confidence)> = BTreeMap::new();
    let mut importers: HashSet<SymbolId> = HashSet::new();
    for t in targets {
        for s in ctx.adj.sites_into(*t) {
            if !kinds.contains(&s.kind) || excluded.contains(&s.src) {
                continue;
            }
            if s.kind == EdgeKind::Imports {
                importers.insert(s.src);
                continue;
            }
            let conf = confidence(s.provenance);
            let e = per_caller
                .entry(s.src)
                .or_insert_with(|| (Vec::new(), s.kind, conf));
            e.0.push(s.line);
            if (kind_rank(s.kind), conf) < (kind_rank(e.1), e.2) {
                e.1 = s.kind;
                e.2 = conf;
            }
        }
    }
    let upstream: HashMap<SymbolId, Vec<SymbolId>> = per_caller
        .keys()
        .map(|c| {
            let mut up: Vec<SymbolId> = ctx
                .adj
                .callers_of(*c)
                .into_iter()
                .filter(|(src, kind, _)| {
                    kinds.contains(kind) && *kind != EdgeKind::Imports && src != c
                })
                .map(|(src, _, _)| src)
                .collect();
            up.sort();
            up.dedup();
            (*c, up)
        })
        .collect();
    cache.prefetch(
        ctx,
        per_caller
            .keys()
            .copied()
            .chain(upstream.values().flatten().copied()),
    )?;

    cache.prefetch(ctx, importers.iter().copied())?;
    let mut import_files = HashSet::new();
    for i in &importers {
        if let Some(sym) = cache.get(ctx, *i)? {
            import_files.insert(sym.path.clone());
        }
    }
    let mut out = Vec::with_capacity(per_caller.len());
    let mut files = HashSet::new();
    let mut summary = DirectSummary {
        imported_by_files: import_files.len() as u32,
        ..Default::default()
    };
    for (id, (mut lines, kind, conf)) in per_caller {
        let Some(sym) = cache.get(ctx, id)?.cloned() else {
            continue;
        };
        lines.sort_unstable();
        lines.dedup();
        summary.callers += 1;
        summary.sites += lines.len() as u32;
        match role(&sym) {
            Role::Prod => summary.prod += 1,
            Role::Test => summary.test += 1,
        }
        files.insert(sym.path.clone());
        let up = &upstream[&id];
        let mut ranked: Vec<(usize, String)> = Vec::with_capacity(up.len());
        for u in up {
            if let Some(s) = cache.get(ctx, *u)? {
                ranked.push((ctx.adj.callers_of(*u).len(), local_of(s)));
            }
        }
        ranked.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        out.push(CallerSites {
            caller: SymbolView::from(&sym),
            lines,
            edge: kind.as_str(),
            confidence: conf.into(),
            called_by: ranked
                .into_iter()
                .take(MAX_CALLED_BY)
                .map(|(_, n)| n)
                .collect(),
            called_by_total: up.len() as u32,
        });
    }
    summary.files = files.len() as u32;
    out.sort_by(|a, b| {
        (a.caller.role, a.confidence, &a.caller.path, a.lines.first()).cmp(&(
            b.caller.role,
            b.confidence,
            &b.caller.path,
            b.lines.first(),
        ))
    });
    let mut disclosures = Vec::new();
    if out.len() > cap {
        disclosures.push(Disclosure::new(
            "call_sites",
            cap,
            out.len() - cap,
            format!("call sites listed for the first {cap} callers (prod first); counts cover all"),
        ));
        out.truncate(cap);
    }
    Ok((out, summary, disclosures))
}

/// Calls first: they are what a signature change breaks.
fn kind_rank(k: EdgeKind) -> u8 {
    match k {
        EdgeKind::Calls => 0,
        EdgeKind::Inherits => 1,
        EdgeKind::References => 2,
        EdgeKind::Imports => 3,
        _ => 4,
    }
}

/// Lines where each dependent references the symbol it reaches the root through.
pub(crate) fn fill_lines(ctx: &QueryContext, items: &mut [DependentItem]) {
    let mut by_via: HashMap<SymbolId, HashMap<SymbolId, Vec<u32>>> = HashMap::new();
    for item in items.iter_mut() {
        let sites = by_via.entry(item.via_id).or_insert_with(|| {
            let mut m: HashMap<SymbolId, Vec<u32>> = HashMap::new();
            for s in ctx.adj.sites_into(item.via_id) {
                m.entry(s.src).or_default().push(s.line);
            }
            m
        });
        let Some(id) = SymbolId::from_hex(&item.symbol.id) else {
            continue;
        };
        let mut lines = sites.get(&id).cloned().unwrap_or_default();
        lines.sort_unstable();
        lines.dedup();
        item.lines = lines;
    }
}

/// Dependents at depth ≥ 2 (modules reached only through imports excluded), grouped by directory,
/// most production dependents first.
pub(crate) fn indirect(items: &[DependentItem]) -> Indirect {
    let mut groups: BTreeMap<String, ModuleImpact> = BTreeMap::new();
    let mut out = Indirect::default();
    for it in items
        .iter()
        .filter(|i| i.depth >= 2 && i.symbol.kind != "module")
    {
        out.total += 1;
        let module = match it.symbol.path.rfind('/') {
            Some(i) => it.symbol.path[..i].to_string(),
            None => ".".to_string(),
        };
        let g = groups
            .entry(module.clone())
            .or_insert_with(|| ModuleImpact {
                module,
                count: 0,
                prod: 0,
                test: 0,
                min_depth: u32::MAX,
                top: Vec::new(),
            });
        g.count += 1;
        match it.symbol.role {
            Role::Prod => {
                g.prod += 1;
                out.prod += 1;
            }
            Role::Test => {
                g.test += 1;
                out.test += 1;
            }
        }
        g.min_depth = g.min_depth.min(it.depth);
        if g.top.len() < 3 {
            g.top
                .push(local_name(&it.symbol.qualified, &it.symbol.path).to_string());
        }
    }
    let mut groups: Vec<ModuleImpact> = groups.into_values().collect();
    groups.sort_by(|a, b| {
        (
            std::cmp::Reverse(a.prod),
            std::cmp::Reverse(a.count),
            &a.module,
        )
            .cmp(&(
                std::cmp::Reverse(b.prod),
                std::cmp::Reverse(b.count),
                &b.module,
            ))
    });
    out.modules = groups.len() as u32;
    groups.truncate(MAX_INDIRECT_GROUPS);
    out.groups = groups;
    out
}

/// Runnable tests reaching any of `targets` through calls/references, nearest first.
pub(crate) fn tests(
    ctx: &QueryContext,
    targets: &[SymbolId],
    depth: u32,
    cache: &mut SymbolCache,
) -> Result<Tests> {
    let mut best: HashMap<SymbolId, u32> = HashMap::new();
    for t in targets {
        if ctx.adj.is_test(*t) {
            best.insert(*t, 0);
        }
        for (id, d) in ctx.adj.covering_tests(*t, depth) {
            let e = best.entry(id).or_insert(d);
            *e = (*e).min(d);
        }
    }
    cache.prefetch(ctx, best.keys().copied())?;
    let mut items = Vec::new();
    for (id, d) in best {
        let Some(s) = cache.get(ctx, id)? else {
            continue;
        };
        if !matches!(
            s.kind,
            SymbolKind::Function | SymbolKind::Method | SymbolKind::Class
        ) {
            continue;
        }
        items.push(TestRef {
            node_id: node_id(&s.qualified, &s.path),
            path: s.path.clone(),
            line: s.start_line,
            depth: d,
        });
    }
    items.sort_by(|a, b| (a.depth, &a.node_id).cmp(&(b.depth, &b.node_id)));
    let total = items.len() as u32;
    items.truncate(MAX_TESTS);
    Ok(Tests {
        total,
        depth_limit: depth,
        items,
    })
}

/// Methods with the target's name in subclasses (overridden_by) and base classes (overrides).
pub(crate) fn overrides(
    ctx: &QueryContext,
    sym: &Symbol,
    cache: &mut SymbolCache,
) -> Result<Vec<OverrideRef>> {
    if sym.kind != SymbolKind::Method {
        return Ok(Vec::new());
    }
    let Some(class) = sym.parent else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    let mut files: HashMap<String, Vec<Symbol>> = HashMap::new();
    for (relation, up) in [("overridden_by", false), ("overrides", true)] {
        for c in inheritance(ctx, class, up) {
            let Some(cls) = cache.get(ctx, c)?.cloned() else {
                continue;
            };
            if !files.contains_key(&cls.path) {
                let syms = ctx.store.symbols_in_file(&cls.path)?;
                files.insert(cls.path.clone(), syms);
            }
            for m in &files[&cls.path] {
                if m.parent == Some(c) && m.name == sym.name && m.kind == SymbolKind::Method {
                    out.push(OverrideRef {
                        relation,
                        symbol: SymbolView::from(m),
                    });
                }
            }
        }
    }
    out.sort_by(|a, b| {
        (a.relation, &a.symbol.path, a.symbol.start_line).cmp(&(
            b.relation,
            &b.symbol.path,
            b.symbol.start_line,
        ))
    });
    Ok(out)
}

/// Classes transitively inheriting from `class` (`up == false`) or inherited by it (`up == true`).
fn inheritance(ctx: &QueryContext, class: SymbolId, up: bool) -> Vec<SymbolId> {
    let mut seen = HashSet::from([class]);
    let mut queue = VecDeque::from([(class, 0u32)]);
    let mut out = Vec::new();
    while let Some((c, d)) = queue.pop_front() {
        if d >= MAX_INHERIT_DEPTH {
            continue;
        }
        let next = if up {
            ctx.adj.callees_of(c)
        } else {
            ctx.adj.callers_of(c)
        };
        for (n, kind, _) in next {
            if kind == EdgeKind::Inherits && seen.insert(n) {
                out.push(n);
                queue.push_back((n, d + 1));
            }
        }
    }
    out
}
