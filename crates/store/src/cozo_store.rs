//! CozoDB (mnestic fork, RocksDB backend) implementation of `GraphStore`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use cozo::{DataValue, DbInstance, NamedRows, ScriptMutability};
use graphite_model::target::SELF_RECEIVERS;
use graphite_model::{EdgeKind, FileFacts, GraphRev, Lang, Symbol, SymbolId, SymbolKind, Target};

use crate::rows::{self, get_id, get_str, get_u32, i, list, s};
use crate::{EdgeKey, GraphStore, Resolution, Result, StoreError, WriteDelta};

const SCHEMA: &[(&str, &str)] = &[
    (
        "file",
        ":create file {path: String => lang: String, content_hash: String, parse_ok: Bool}",
    ),
    (
        "symbol",
        ":create symbol {id: String => path: String, lang: String, name: String, qualified: String, kind: String, \
         start_line: Int, end_line: Int, start_byte: Int, end_byte: Int, exported: Bool, signature: String, \
         parent: String?, is_test: Bool}",
    ),
    ("symbol:by_path", "::index create symbol:by_path {path, id}"),
    ("symbol:by_name", "::index create symbol:by_name {name, id}"),
    ("symbol:by_parent", "::index create symbol:by_parent {parent, id}"),
    (
        "raw_edge",
        ":create raw_edge {path: String, idx: Int => lang: String, src: String, kind: String, site_line: Int, \
         dst_id: String?, name: String?, key_name: String?, qualifier: String?, import_path: String?, prov: String}",
    ),
    ("raw_edge:by_key", "::index create raw_edge:by_key {key_name, path, idx}"),
    ("raw_edge:by_dst", "::index create raw_edge:by_dst {dst_id, path, idx}"),
    ("raw_edge:by_src", "::index create raw_edge:by_src {src, path, idx}"),
    ("raw_edge:by_qual", "::index create raw_edge:by_qual {qualifier, path, idx}"),
    ("meta", ":create meta {k: String => v: Int}"),
];

/// Cross-file candidate generation, evaluated at read time so incremental writes equal a full rebuild.
/// Emits one row per (edge, tier, candidate); `select` in rows.rs picks the winner. Tiers, in order:
/// `direct` (extractor-bound), `import` (suffix match of the import path), `inherit` (self/cls/super()
/// member of a base class), `qual` (qualifier.name suffix), `name` (global name, counted), `none`.
/// Expects a `todo[path, idx]` rule.
const RESOLVE: &str = r#"
sep[lang, s] <- $sep
compat[ek, sk] <- $compat
selfish[q] <- $selfish
special[n] <- [['<module>'], ['*'], ['<dynamic>']]
modlike[n] <- [['<module>'], ['*']]

e[path, idx, src, kind, line, dst_id, name, key, qual, imp, prov, lang] :=
    todo[path, idx],
    *raw_edge{path, idx, src, kind, site_line: line, dst_id, name, key_name: key, qualifier: qual,
              import_path: imp, prov, lang}
open[path, idx, src, kind, name, key, qual, imp, lang] :=
    e[path, idx, src, kind, _, dst_id, name, key, qual, imp, _, lang], is_null(dst_id)
internal[path, idx] := open[path, idx, _, _, _, _, _, imp, _], is_null(imp)
internal[path, idx] := open[path, idx, _, _, _, _, _, imp, _], !is_null(imp),
    !starts_with(imp, '<external>:')

c[path, idx, tier, d, dpath, depth, n] := e[path, idx, _, _, _, _, _, _, _, _, _, _],
    tier = 'none', d = null, dpath = null, depth = 0, n = 0

c[path, idx, tier, d, dpath, depth, n] := e[path, idx, _, _, _, d, _, _, _, _, _, _], !is_null(d),
    *symbol{id: d, path: dpath}, tier = 'direct', depth = 0, n = 1

imp_hit[path, idx, d, dpath, sk, nm] := open[path, idx, _, kind, nm, key, _, imp, lang], !is_null(imp),
    internal[path, idx], sep[lang, s], *symbol:by_name{name: key, id: d},
    *symbol{id: d, lang, kind: sk, qualified: q, path: dpath}, compat[kind, sk],
    or(q == imp, ends_with(q, concat(s, imp)))
c[path, idx, tier, d, dpath, depth, n] := imp_hit[path, idx, d, dpath, sk, nm], modlike[nm],
    sk == 'module', tier = 'import', depth = 0, n = 1
c[path, idx, tier, d, dpath, depth, n] := imp_hit[path, idx, d, dpath, _, nm], not modlike[nm],
    tier = 'import', depth = 0, n = 1

need[s] := open[_, _, s, _, _, _, qual, imp, _], is_null(imp), selfish[qual]
need[p] := need[x], *symbol{id: x, parent: p}, !is_null(p)
cls_of[x, k] := need[x], *symbol{id: x, parent: k}, !is_null(k), *symbol{id: k, kind: 'class'}
cls_of[x, k] := need[x], *symbol{id: x, parent: p}, !is_null(p), *symbol{id: p, kind: pk},
    pk != 'class', cls_of[p, k]
root[k] := cls_of[_, k]
base[k, b] := *raw_edge:by_src{src: k, path, idx}, *raw_edge{path, idx, kind: 'inherits', dst_id: b},
    !is_null(b), *symbol{id: b}
base[k, b] := *raw_edge:by_src{src: k, path, idx},
    *raw_edge{path, idx, kind: 'inherits', dst_id, key_name: key, import_path: imp, lang},
    is_null(dst_id), !is_null(imp), !starts_with(imp, '<external>:'), sep[lang, s],
    *symbol:by_name{name: key, id: b}, *symbol{id: b, kind: 'class', qualified: q},
    or(q == imp, ends_with(q, concat(s, imp)))
anc[k, a, min(d)] := root[k], base[k, a], d = 1
anc[k, a, min(d)] := anc[k, m, d0], d0 < 32, base[m, a], d = d0 + 1
c[path, idx, tier, d, dpath, depth, n] := open[path, idx, src, kind, name, _, qual, imp, _],
    is_null(imp), selfish[qual], cls_of[src, k], anc[k, a, depth],
    *symbol:by_parent{parent: a, id: d}, *symbol{id: d, name, kind: sk, path: dpath}, compat[kind, sk],
    tier = 'inherit', n = 1

c[path, idx, tier, d, dpath, depth, n] := open[path, idx, _, kind, name, _, qual, imp, lang],
    is_null(imp), !is_null(qual), not selfish[qual], not special[name], sep[lang, s],
    suffix = concat(s, qual, s, name), *symbol:by_name{name, id: d},
    *symbol{id: d, lang, kind: sk, qualified: q, path: dpath}, compat[kind, sk], ends_with(q, suffix),
    tier = 'qual', depth = 0, n = 1

name_hit[path, idx, d] := open[path, idx, _, kind, name, _, qual, _, lang], internal[path, idx],
    not special[name], is_null(qual), *symbol:by_name{name, id: d}, *symbol{id: d, lang, kind: sk},
    compat[kind, sk]
name_hit[path, idx, d] := open[path, idx, _, _, name, _, qual, _, lang], internal[path, idx],
    not special[name], !is_null(qual), *symbol:by_name{name, id: d},
    *symbol{id: d, lang, kind: 'method'}
n_name[path, idx, count(d)] := name_hit[path, idx, d]
c[path, idx, tier, d, dpath, depth, n] := name_hit[path, idx, d], n_name[path, idx, 1],
    *symbol{id: d, path: dpath}, tier = 'name', depth = 0, n = 1
c[path, idx, tier, d, dpath, depth, n] := n_name[path, idx, n], n > 1, tier = 'name', d = null,
    dpath = null, depth = 0

?[path, idx, src, kind, line, prov, tier, d, dpath, depth, n] := c[path, idx, tier, d, dpath, depth, n],
    e[path, idx, src, kind, line, _, _, _, _, _, prov, _]
"#;

/// Which symbol kinds an unresolved reference of a given edge kind may bind to.
fn compat_rows() -> DataValue {
    use SymbolKind::*;
    let mut rows = Vec::new();
    for ek in EdgeKind::ALL {
        let allowed: &[SymbolKind] = match ek {
            EdgeKind::Calls | EdgeKind::Tests => &[Function, Method, Class, Struct],
            EdgeKind::Inherits => &[Class, Struct, Trait, Interface],
            EdgeKind::Imports | EdgeKind::References | EdgeKind::Contains => &SymbolKind::ALL,
        };
        for sk in allowed {
            rows.push(list(vec![s(ek.as_str()), s(sk.as_str())]));
        }
    }
    list(rows)
}

fn sep_rows() -> DataValue {
    list(
        [
            (Lang::Python, "."),
            (Lang::Rust, "::"),
            (Lang::TypeScript, "."),
        ]
        .into_iter()
        .map(|(l, sep)| list(vec![s(l.as_str()), s(sep)]))
        .collect(),
    )
}

fn selfish_rows() -> DataValue {
    list(SELF_RECEIVERS.iter().map(|q| list(vec![s(q)])).collect())
}

fn params(pairs: Vec<(&str, DataValue)>) -> BTreeMap<String, DataValue> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn touches_classes(facts: &FileFacts) -> bool {
    facts.symbols.iter().any(|s| s.kind == SymbolKind::Class)
        || facts.edges.iter().any(|e| e.kind == EdgeKind::Inherits)
}

/// CozoDB-backed store. Single writer (serialized by a mutex), concurrent readers.
pub struct CozoStore {
    db: DbInstance,
    rev: AtomicU64,
    write_lock: Mutex<()>,
}

impl CozoStore {
    /// Open or create a store in `dir` (RocksDB backend).
    pub fn open(dir: &Path) -> Result<Self> {
        let db = DbInstance::new("newrocksdb", dir, "")
            .map_err(|e| StoreError::Cozo(format!("{e:?}")))?;
        let store = CozoStore {
            db,
            rev: AtomicU64::new(0),
            write_lock: Mutex::new(()),
        };
        store.ensure_schema()?;
        let rows = store.run("?[v] := *meta{k: 'rev', v}", BTreeMap::new(), false)?;
        let rev = rows
            .rows
            .first()
            .map(|r| get_u32(&r[0], "rev"))
            .transpose()?
            .unwrap_or(0);
        store.rev.store(u64::from(rev), Ordering::SeqCst);
        Ok(store)
    }

    fn run(&self, script: &str, p: BTreeMap<String, DataValue>, write: bool) -> Result<NamedRows> {
        let m = if write {
            ScriptMutability::Mutable
        } else {
            ScriptMutability::Immutable
        };
        self.db
            .run_script(script, p, m)
            .map_err(|e| StoreError::Cozo(format!("{e:?}")))
    }

    fn ensure_schema(&self) -> Result<()> {
        let existing: BTreeSet<String> = self
            .run("::relations", BTreeMap::new(), false)?
            .rows
            .iter()
            .filter_map(|r| r.first().and_then(|v| v.get_str()).map(str::to_string))
            .collect();
        for (name, ddl) in SCHEMA {
            if !existing.contains(*name) {
                self.run(ddl, BTreeMap::new(), true)?;
            }
        }
        Ok(())
    }

    fn resolve_with(&self, todo: &str, p: Vec<(&str, DataValue)>) -> Result<Vec<Resolution>> {
        let mut p = params(p);
        p.insert("sep".into(), sep_rows());
        p.insert("compat".into(), compat_rows());
        p.insert("selfish".into(), selfish_rows());
        let script = format!("{todo}\n{RESOLVE}");
        let rows = self.run(&script, p, false)?;
        rows::select(&rows.rows)
    }

    fn query_symbols(&self, script: &str, p: Vec<(&str, DataValue)>) -> Result<Vec<Symbol>> {
        let rows = self.run(script, params(p), false)?;
        rows.rows.iter().map(|r| rows::parse_symbol(r)).collect()
    }

    /// Replace (or with `facts == None`, delete) one file's facts in a single atomic script.
    fn write_file(&self, path: &str, facts: Option<&FileFacts>) -> Result<WriteDelta> {
        let _guard = self.write_lock.lock().unwrap_or_else(|p| p.into_inner());

        let old = self.run(
            "?[id, name, kind] := *symbol:by_path{path: $p, id}, *symbol{id, name, kind}",
            params(vec![("p", s(path))]),
            false,
        )?;
        let old_edges = self.run(
            "?[idx, kind] := *raw_edge{path: $p, idx, kind}",
            params(vec![("p", s(path))]),
            false,
        )?;

        let mut names: BTreeSet<String> = BTreeSet::new();
        let mut ids: BTreeSet<SymbolId> = BTreeSet::new();
        let mut classes = false;
        for r in &old.rows {
            ids.insert(get_id(&r[0], "id")?);
            names.insert(get_str(&r[1], "name")?.to_string());
            classes |= get_str(&r[2], "kind")? == SymbolKind::Class.as_str();
        }
        for r in &old_edges.rows {
            classes |= get_str(&r[1], "kind")? == EdgeKind::Inherits.as_str();
        }
        if let Some(f) = facts {
            for sym in &f.symbols {
                ids.insert(sym.id);
                names.insert(sym.name.clone());
            }
            classes |= touches_classes(f);
        }

        let rev = self.rev.load(Ordering::SeqCst) + 1;
        let mut script = String::from(
            "{?[id] := *symbol:by_path{path: $p, id} :rm symbol {id}}\n\
             {?[path, idx] := *raw_edge{path, idx}, path = $p :rm raw_edge {path, idx}}\n\
             {?[path] := *file{path}, path = $p :rm file {path}}\n\
             {?[k, v] <- [['rev', $rev]] :put meta {k => v}}\n",
        );
        let mut p = vec![("p", s(path)), ("rev", i(rev as i64))];
        if let Some(f) = facts {
            script.push_str(
                "{?[path, lang, content_hash, parse_ok] <- [[$p, $lang, $hash, $ok]] \
                 :put file {path => lang, content_hash, parse_ok}}\n",
            );
            let hash: String = f.content_hash.iter().map(|b| format!("{b:02x}")).collect();
            p.push(("lang", s(f.lang.as_str())));
            p.push(("hash", s(&hash)));
            p.push(("ok", DataValue::Bool(f.parse_ok)));
            if !f.symbols.is_empty() {
                script.push_str(&format!(
                    "{{?[{c}] <- $syms :put symbol {{id => {rest}}}}}\n",
                    c = rows::SYMBOL_COLS,
                    rest = rows::SYMBOL_COLS.trim_start_matches("id, "),
                ));
                p.push((
                    "syms",
                    list(f.symbols.iter().map(rows::symbol_row).collect()),
                ));
            }
            if !f.edges.is_empty() {
                script.push_str(&format!(
                    "{{?[{c}] <- $edges :put raw_edge {{path, idx => {rest}}}}}\n",
                    c = rows::EDGE_COLS,
                    rest = rows::EDGE_COLS.trim_start_matches("path, idx, "),
                ));
                p.push((
                    "edges",
                    list(
                        f.edges
                            .iter()
                            .enumerate()
                            .map(|(n, e)| rows::edge_row(f, n, e))
                            .collect(),
                    ),
                ));
            }
        }
        self.run(&script, params(p), true)?;
        self.rev.store(rev, Ordering::SeqCst);

        let removed_keys = old_edges
            .rows
            .iter()
            .map(|r| {
                Ok(EdgeKey {
                    path: path.to_string(),
                    idx: get_u32(&r[0], "idx")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(WriteDelta {
            rev,
            path: path.to_string(),
            removed_keys,
            touched_names: names.into_iter().collect(),
            touched_ids: ids.into_iter().collect(),
            touched_classes: classes,
        })
    }
}

impl GraphStore for CozoStore {
    fn replace_file(&self, facts: &FileFacts) -> Result<WriteDelta> {
        self.write_file(&facts.path, Some(facts))
    }

    fn remove_file(&self, path: &str) -> Result<WriteDelta> {
        self.write_file(path, None)
    }

    fn graph_rev(&self) -> GraphRev {
        self.rev.load(Ordering::SeqCst)
    }

    fn file_paths(&self) -> Result<Vec<String>> {
        let rows = self.run("?[path] := *file{path}", BTreeMap::new(), false)?;
        rows.rows
            .iter()
            .map(|r| get_str(&r[0], "path").map(str::to_string))
            .collect()
    }

    fn symbol(&self, id: SymbolId) -> Result<Option<Symbol>> {
        let script = format!("?[{c}] := *symbol{{{c}}}, id = $id", c = rows::SYMBOL_COLS);
        Ok(self
            .query_symbols(&script, vec![("id", s(&id.to_hex()))])?
            .into_iter()
            .next())
    }

    fn symbols_by_name(&self, name: &str) -> Result<Vec<Symbol>> {
        let script = format!(
            "?[{c}] := *symbol:by_name{{name: $n, id}}, *symbol{{{c}}}",
            c = rows::SYMBOL_COLS
        );
        self.query_symbols(&script, vec![("n", s(name))])
    }

    fn symbols_in_file(&self, path: &str) -> Result<Vec<Symbol>> {
        let script = format!(
            "?[{c}] := *symbol:by_path{{path: $p, id}}, *symbol{{{c}}}",
            c = rows::SYMBOL_COLS
        );
        self.query_symbols(&script, vec![("p", s(path))])
    }

    fn test_symbols(&self) -> Result<Vec<SymbolId>> {
        let rows = self.run(
            "?[id] := *symbol{id, is_test: true}",
            BTreeMap::new(),
            false,
        )?;
        rows.rows.iter().map(|r| get_id(&r[0], "id")).collect()
    }

    fn resolve_all(&self) -> Result<Vec<Resolution>> {
        self.resolve_with("todo[path, idx] := *raw_edge{path, idx}", vec![])
    }

    fn resolve_affected(&self, delta: &WriteDelta) -> Result<Vec<Resolution>> {
        let mut todo = String::from(
            "todo[path, idx] := *raw_edge{path, idx}, path = $p\n\
             todo[path, idx] := n in $names, *raw_edge:by_key{key_name: n, path, idx}\n\
             todo[path, idx] := d in $ids, *raw_edge:by_dst{dst_id: d, path, idx}\n",
        );
        if delta.touched_classes {
            todo.push_str(
                "todo[path, idx] := q in $selfish_q, *raw_edge:by_qual{qualifier: q, path, idx}\n",
            );
        }
        self.resolve_with(
            &todo,
            vec![
                ("p", s(&delta.path)),
                (
                    "names",
                    list(delta.touched_names.iter().map(|n| s(n)).collect()),
                ),
                (
                    "ids",
                    list(delta.touched_ids.iter().map(|d| s(&d.to_hex())).collect()),
                ),
                (
                    "selfish_q",
                    list(SELF_RECEIVERS.iter().map(|q| s(q)).collect()),
                ),
            ],
        )
    }

    fn callers(&self, id: SymbolId) -> Result<Vec<Resolution>> {
        let Some(sym) = self.symbol(id)? else {
            return Ok(vec![]);
        };
        let all = self.resolve_with(
            "todo[path, idx] := *raw_edge:by_key{key_name: $n, path, idx}\n\
             todo[path, idx] := *raw_edge:by_dst{dst_id: $d, path, idx}",
            vec![("n", s(&sym.name)), ("d", s(&id.to_hex()))],
        )?;
        Ok(all
            .into_iter()
            .filter(|r| matches!(r.outcome, crate::Outcome::Resolved { dst, .. } if dst == id))
            .collect())
    }

    fn callees(&self, id: SymbolId) -> Result<Vec<Resolution>> {
        self.resolve_with(
            "todo[path, idx] := *raw_edge:by_src{src: $s, path, idx}",
            vec![("s", s(&id.to_hex()))],
        )
    }
}

/// Name an unresolved edge is indexed under: its name, or the module's last segment for module/wildcard targets.
pub(crate) fn key_name(dst: &Target, lang: Lang) -> Option<String> {
    use graphite_model::target::{MODULE_TARGET, WILDCARD_TARGET};
    match dst {
        Target::Symbol(_) => None,
        Target::Unresolved {
            name, import_path, ..
        } => {
            if name == MODULE_TARGET || name == WILDCARD_TARGET {
                let sep = if lang == Lang::Rust { "::" } else { "." };
                import_path
                    .as_deref()
                    .and_then(|p| p.rsplit(sep).next())
                    .map(str::to_string)
            } else {
                Some(name.clone())
            }
        }
    }
}
