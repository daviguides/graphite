//! Query dispatch. `QueryHandler` is the seam where graphite-query plugs in; `BasicQueries` is the provisional implementation.

use std::collections::{BTreeMap, HashMap};
use std::process::Command;

use graphite_model::{Symbol, SymbolId, SymbolKind};
use graphite_store::{GraphStore, DEPENDENCY_KINDS};
use serde_json::{json, Value};

use crate::engine::Engine;

/// Default blast radius depth: real graphs reach most of their depth-10 set by depth 3.
pub const DEFAULT_DEPTH: u32 = 3;
/// Items listed per answer before truncation is disclosed.
pub const MAX_ITEMS: usize = 200;

pub type QueryResult = std::result::Result<Value, String>;

/// Answers the agent-facing queries; every answer is success-shaped JSON unless the daemon itself failed.
pub trait QueryHandler: Send + Sync {
    fn lookup(&self, engine: &Engine, symbol: &str) -> QueryResult;
    fn blast(&self, engine: &Engine, symbol: &str, depth: u32) -> QueryResult;
    fn diff_impact(&self, engine: &Engine, base: Option<&str>, depth: u32) -> QueryResult;
}

/// Outcome of turning a user-supplied reference into a symbol.
pub enum Lookup {
    Found(Symbol),
    Ambiguous(Vec<Symbol>),
    NotFound,
}

/// Resolve a hex id, a fully qualified name, a qualified suffix (`Class.method`) or a bare name.
pub fn resolve_symbol(engine: &Engine, reference: &str) -> Result<Lookup, String> {
    let store = &engine.store;
    if let Some(id) = SymbolId::from_hex(reference) {
        if let Some(s) = store.symbol(id).map_err(|e| e.to_string())? {
            return Ok(Lookup::Found(s));
        }
    }
    let name = reference.rsplit(['.', ':']).next().unwrap_or(reference);
    let candidates = store.symbols_by_name(name).map_err(|e| e.to_string())?;
    let pick = |mut v: Vec<Symbol>| match v.len() {
        0 => None,
        1 => Some(Lookup::Found(v.remove(0))),
        _ => {
            v.sort_by(|a, b| (&a.qualified, &a.path).cmp(&(&b.qualified, &b.path)));
            Some(Lookup::Ambiguous(v))
        }
    };
    let exact: Vec<Symbol> = candidates
        .iter()
        .filter(|s| s.qualified == reference)
        .cloned()
        .collect();
    if let Some(l) = pick(exact) {
        return Ok(l);
    }
    let dotted = format!(".{reference}");
    let suffix: Vec<Symbol> = candidates
        .iter()
        .filter(|s| s.qualified.ends_with(&dotted))
        .cloned()
        .collect();
    if let Some(l) = pick(suffix) {
        return Ok(l);
    }
    if reference == name {
        if let Some(l) = pick(candidates) {
            return Ok(l);
        }
    }
    Ok(Lookup::NotFound)
}

pub fn symbol_json(s: &Symbol) -> Value {
    json!({
        "id": s.id.to_hex(),
        "qualified": s.qualified,
        "kind": s.kind.as_str(),
        "path": s.path,
        "line": s.start_line,
        "end_line": s.end_line,
        "signature": s.signature,
        "is_test": s.is_test,
    })
}

fn lookup_json(l: &Lookup, reference: &str) -> Value {
    match l {
        Lookup::Found(s) => json!({"status": "found", "symbol": symbol_json(s)}),
        Lookup::Ambiguous(v) => json!({
            "status": "ambiguous",
            "hint": "several symbols match; retry with a qualified name or an id",
            "candidates": v.iter().map(symbol_json).collect::<Vec<_>>(),
        }),
        Lookup::NotFound => json!({
            "status": "not_found",
            "reference": reference,
            "hint": "no indexed symbol matches; v1.0 indexes Python only",
        }),
    }
}

/// Provisional handlers over the store + adjacency until graphite-query lands.
pub struct BasicQueries;

impl BasicQueries {
    /// Dependents of several roots, each at its shallowest depth, remembering which root reached it.
    fn impacted(
        engine: &Engine,
        roots: &[SymbolId],
        depth: u32,
    ) -> BTreeMap<SymbolId, (u32, SymbolId)> {
        let adj = engine.adjacency();
        let mut best: HashMap<SymbolId, (u32, SymbolId)> = HashMap::new();
        for &root in roots {
            for (id, d) in adj.blast_radius(root, depth, DEPENDENCY_KINDS) {
                let e = best.entry(id).or_insert((d, root));
                if d < e.0 {
                    *e = (d, root);
                }
            }
        }
        for r in roots {
            best.remove(r);
        }
        best.into_iter().collect()
    }

    fn items_json(
        engine: &Engine,
        impacted: &BTreeMap<SymbolId, (u32, SymbolId)>,
        via: Option<&HashMap<SymbolId, String>>,
    ) -> Result<Value, String> {
        let mut rows: Vec<(u32, SymbolId, SymbolId)> =
            impacted.iter().map(|(id, (d, r))| (*d, *id, *r)).collect();
        rows.sort();
        let total = rows.len();
        let mut prod = 0;
        let mut test = 0;
        let mut by_depth: BTreeMap<u32, usize> = BTreeMap::new();
        let mut items = Vec::new();
        for (d, id, root) in &rows {
            let Some(s) = engine.store.symbol(*id).map_err(|e| e.to_string())? else {
                continue;
            };
            *by_depth.entry(*d).or_default() += 1;
            if s.is_test {
                test += 1;
            } else {
                prod += 1;
            }
            if items.len() < MAX_ITEMS {
                let mut v = symbol_json(&s);
                v["depth"] = json!(d);
                if let Some(names) = via {
                    v["via"] = json!(names.get(root));
                }
                items.push(v);
            }
        }
        Ok(json!({
            "total": total,
            "prod": prod,
            "test": test,
            "by_depth": by_depth,
            "listed": items.len(),
            "truncated": total > items.len(),
            "items": items,
        }))
    }
}

impl QueryHandler for BasicQueries {
    fn lookup(&self, engine: &Engine, symbol: &str) -> QueryResult {
        Ok(lookup_json(&resolve_symbol(engine, symbol)?, symbol))
    }

    fn blast(&self, engine: &Engine, symbol: &str, depth: u32) -> QueryResult {
        let target = match resolve_symbol(engine, symbol)? {
            Lookup::Found(s) => s,
            other => return Ok(lookup_json(&other, symbol)),
        };
        let impacted = Self::impacted(engine, &[target.id], depth);
        Ok(json!({
            "status": "found",
            "target": symbol_json(&target),
            "depth": depth,
            "dependents": Self::items_json(engine, &impacted, None)?,
        }))
    }

    fn diff_impact(&self, engine: &Engine, base: Option<&str>, depth: u32) -> QueryResult {
        let base = base.unwrap_or("HEAD");
        let changes = changed_ranges(engine, base)?;
        let mut changed = Vec::new();
        let mut roots = Vec::new();
        let mut names = HashMap::new();
        let mut unindexed = Vec::new();
        for (path, ranges) in &changes {
            if !engine.is_indexable(path) {
                unindexed.push(path.clone());
                continue;
            }
            let syms = engine
                .store
                .symbols_in_file(path)
                .map_err(|e| e.to_string())?;
            for s in touched_symbols(&syms, ranges) {
                names.insert(s.id, s.qualified.clone());
                roots.push(s.id);
                changed.push(symbol_json(s));
            }
        }
        let impacted = Self::impacted(engine, &roots, depth);
        Ok(json!({
            "base": base,
            "depth": depth,
            "changed_files": changes.keys().collect::<Vec<_>>(),
            "unindexed_files": unindexed,
            "changed_symbols": changed,
            "impacted": Self::items_json(engine, &impacted, Some(&names))?,
        }))
    }
}

/// Innermost non-module symbols whose span overlaps a changed line range.
fn touched_symbols<'a>(syms: &'a [Symbol], ranges: &[(u32, u32)]) -> Vec<&'a Symbol> {
    let hit: Vec<&Symbol> = syms
        .iter()
        .filter(|s| s.kind != SymbolKind::Module)
        .filter(|s| {
            ranges
                .iter()
                .any(|(a, b)| s.start_line <= *b && *a <= s.end_line)
        })
        .collect();
    hit.iter()
        .filter(|s| !hit.iter().any(|o| o.parent == Some(s.id)))
        .copied()
        .collect()
}

fn git(engine: &Engine, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(&engine.paths.root)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Changed new-side line ranges per file vs `base`, working tree included; untracked files count as fully changed.
fn changed_ranges(
    engine: &Engine,
    base: &str,
) -> Result<BTreeMap<String, Vec<(u32, u32)>>, String> {
    let mut out: BTreeMap<String, Vec<(u32, u32)>> = BTreeMap::new();
    let diff = git(engine, &["diff", "-U0", "--no-renames", "--no-color", base])?;
    let mut current: Option<String> = None;
    for line in diff.lines() {
        if let Some(p) = line.strip_prefix("+++ ") {
            current = p.strip_prefix("b/").map(str::to_string);
            if let Some(c) = &current {
                out.entry(c.clone()).or_default();
            }
        } else if let Some(h) = line.strip_prefix("@@ ") {
            let (Some(path), Some(new)) = (&current, h.split(' ').nth(1)) else {
                continue;
            };
            let new = new.trim_start_matches('+');
            let (start, len) = match new.split_once(',') {
                Some((s, l)) => (s.parse().unwrap_or(0), l.parse().unwrap_or(0)),
                None => (new.parse().unwrap_or(0), 1u32),
            };
            let end = start + len.max(1) - 1;
            out.entry(path.clone())
                .or_default()
                .push((start.max(1), end.max(1)));
        } else if let Some(p) = line.strip_prefix("--- a/") {
            // Deleted files report `+++ /dev/null`; keep them listed.
            out.entry(p.to_string()).or_default();
        }
    }
    for p in git(engine, &["ls-files", "--others", "--exclude-standard"])?.lines() {
        out.insert(p.to_string(), vec![(1, u32::MAX)]);
    }
    Ok(out)
}
