//! CozoDB (mnestic fork, RocksDB backend) implementation of `GraphStore`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use cozo::{DataValue, DbInstance, NamedRows, ScriptMutability};
use graphite_model::{EdgeKind, FileFacts, GraphRev, Lang, Symbol, SymbolId, SymbolKind};

use crate::rows::{self, get_id, get_str, get_u32, i, list, s};
use crate::{EdgeKey, GraphStore, Resolution, Result, StoreError, WriteDelta};

const SCHEMA: &[(&str, &str)] = &[
    ("file", ":create file {path: String => lang: String, content_hash: String, parse_ok: Bool}"),
    (
        "symbol",
        ":create symbol {id: String => path: String, lang: String, name: String, qualified: String, kind: String, \
         start_line: Int, end_line: Int, start_byte: Int, end_byte: Int, exported: Bool, signature: String, \
         parent: String?, is_test: Bool}",
    ),
    ("symbol:by_path", "::index create symbol:by_path {path, id}"),
    ("symbol:by_name", "::index create symbol:by_name {name, id}"),
    ("symbol:by_qualified", "::index create symbol:by_qualified {qualified, id}"),
    (
        "raw_edge",
        ":create raw_edge {path: String, idx: Int => lang: String, src: String, kind: String, site_line: Int, \
         dst_id: String?, name: String?, qualifier: String?, import_path: String?, prov: String}",
    ),
    ("raw_edge:by_name", "::index create raw_edge:by_name {name, path, idx}"),
    ("raw_edge:by_dst", "::index create raw_edge:by_dst {dst_id, path, idx}"),
    ("raw_edge:by_src", "::index create raw_edge:by_src {src, path, idx}"),
    ("meta", ":create meta {k: String => v: Int}"),
];

/// Cross-file resolution, evaluated at read time so incremental writes equal a full rebuild.
/// Tiers: extractor-bound symbol > import path > qualifier suffix > unique name. More than one
/// candidate at the winning tier is ambiguous, never guessed. Expects a `todo[path, idx]` rule.
const RESOLVE: &str = r#"
sep[lang, s] <- $sep
compat[ek, sk] <- $compat
e[path, idx, src, kind, line, dst_id, name, qual, imp, prov, lang] :=
    todo[path, idx],
    *raw_edge{path, idx, src, kind, site_line: line, dst_id, name, qualifier: qual, import_path: imp, prov, lang}
direct[path, idx, d, prov] := e[path, idx, _, _, _, d, _, _, _, prov, _], !is_null(d), *symbol{id: d}
cand_imp[path, idx, d] := e[path, idx, _, kind, _, dst_id, name, qual, imp, _, lang], is_null(dst_id),
    !is_null(imp), is_null(qual), sep[lang, s], full = concat(imp, s, name),
    *symbol:by_qualified{qualified: full, id: d}, *symbol{id: d, lang, kind: sk}, compat[kind, sk]
cand_imp[path, idx, d] := e[path, idx, _, kind, _, dst_id, name, qual, imp, _, lang], is_null(dst_id),
    !is_null(imp), !is_null(qual), sep[lang, s], full = concat(imp, s, qual, s, name),
    *symbol:by_qualified{qualified: full, id: d}, *symbol{id: d, lang, kind: sk}, compat[kind, sk]
cand_qual[path, idx, d] := e[path, idx, _, kind, _, dst_id, name, qual, _, _, lang], is_null(dst_id),
    !is_null(qual), sep[lang, s], suffix = concat(s, qual, s, name),
    *symbol:by_name{name, id: d}, *symbol{id: d, lang, kind: sk, qualified: q}, compat[kind, sk],
    ends_with(q, suffix)
cand_name[path, idx, d] := e[path, idx, _, kind, _, dst_id, name, _, _, _, lang], is_null(dst_id),
    *symbol:by_name{name, id: d}, *symbol{id: d, lang, kind: sk}, compat[kind, sk]
n_imp[path, idx, count(d)] := cand_imp[path, idx, d]
n_qual[path, idx, count(d)] := cand_qual[path, idx, d]
n_name[path, idx, count(d)] := cand_name[path, idx, d]
has_imp[path, idx] := n_imp[path, idx, _]
has_qual[path, idx] := n_qual[path, idx, _]
res[path, idx, d, prov] := direct[path, idx, d, prov]
res[path, idx, d, prov] := cand_imp[path, idx, d], n_imp[path, idx, 1], prov = 'resolved'
res[path, idx, d, prov] := cand_qual[path, idx, d], n_qual[path, idx, 1], not has_imp[path, idx],
    prov = 'inferred'
res[path, idx, d, prov] := cand_name[path, idx, d], n_name[path, idx, 1], not has_imp[path, idx],
    not has_qual[path, idx], prov = 'name_guess'
amb[path, idx, n] := n_imp[path, idx, n], n > 1
amb[path, idx, n] := n_qual[path, idx, n], n > 1, not has_imp[path, idx]
amb[path, idx, n] := n_name[path, idx, n], n > 1, not has_imp[path, idx], not has_qual[path, idx]
done[path, idx] := res[path, idx, _, _]
done[path, idx] := amb[path, idx, _]
?[path, idx, src, kind, line, d, prov, n] := res[path, idx, d, prov],
    e[path, idx, src, kind, line, _, _, _, _, _, _], n = 1
?[path, idx, src, kind, line, d, prov, n] := amb[path, idx, n],
    e[path, idx, src, kind, line, _, _, _, _, _, _], d = null, prov = null
?[path, idx, src, kind, line, d, prov, n] := e[path, idx, src, kind, line, _, _, _, _, _, _],
    not done[path, idx], d = null, prov = null, n = 0
"#;

/// Which symbol kinds an unresolved reference of a given edge kind may bind to.
fn compat_rows() -> DataValue {
    use SymbolKind::*;
    let mut rows = Vec::new();
    for ek in EdgeKind::ALL {
        let allowed: &[SymbolKind] = match ek {
            EdgeKind::Calls => &[Function, Method, Class, Struct],
            EdgeKind::Inherits => &[Class, Struct, Trait, Interface],
            EdgeKind::Tests => &[Function, Method, Class, Struct],
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

fn params(pairs: Vec<(&str, DataValue)>) -> BTreeMap<String, DataValue> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
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
        let script = format!("{todo}\n{RESOLVE}");
        let rows = self.run(&script, p, false)?;
        rows.rows
            .iter()
            .map(|r| rows::parse_resolution(r))
            .collect()
    }

    fn query_symbols(&self, script: &str, p: Vec<(&str, DataValue)>) -> Result<Vec<Symbol>> {
        let rows = self.run(script, params(p), false)?;
        rows.rows.iter().map(|r| rows::parse_symbol(r)).collect()
    }

    /// Replace (or with `facts == None`, delete) one file's facts in a single atomic script.
    fn write_file(&self, path: &str, facts: Option<&FileFacts>) -> Result<WriteDelta> {
        let _guard = self.write_lock.lock().unwrap_or_else(|p| p.into_inner());

        let old = self.run(
            "?[id, name] := *symbol:by_path{path: $p, id}, *symbol{id, name}",
            params(vec![("p", s(path))]),
            false,
        )?;
        let old_keys = self.run(
            "?[idx] := *raw_edge{path: $p, idx}",
            params(vec![("p", s(path))]),
            false,
        )?;

        let mut names: BTreeSet<String> = BTreeSet::new();
        let mut ids: BTreeSet<SymbolId> = BTreeSet::new();
        for r in &old.rows {
            ids.insert(get_id(&r[0], "id")?);
            names.insert(get_str(&r[1], "name")?.to_string());
        }
        if let Some(f) = facts {
            for sym in &f.symbols {
                ids.insert(sym.id);
                names.insert(sym.name.clone());
            }
        }

        let rev = self.rev.load(Ordering::SeqCst) + 1;
        let mut script = String::from(
            "{?[id] := *symbol:by_path{path: $p, id} :rm symbol {id}}\n\
             {?[path, idx] := *raw_edge{path: $p, idx}, path = $p :rm raw_edge {path, idx}}\n\
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
                script.push_str(
                    "{?[path, idx, lang, src, kind, site_line, dst_id, name, qualifier, import_path, prov] <- $edges \
                     :put raw_edge {path, idx => lang, src, kind, site_line, dst_id, name, qualifier, import_path, prov}}\n",
                );
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

        let removed_keys = old_keys
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

    fn resolve_all(&self) -> Result<Vec<Resolution>> {
        self.resolve_with("todo[path, idx] := *raw_edge{path, idx}", vec![])
    }

    fn resolve_affected(&self, delta: &WriteDelta) -> Result<Vec<Resolution>> {
        self.resolve_with(
            "todo[path, idx] := *raw_edge{path, idx}, path = $p\n\
             todo[path, idx] := n in $names, *raw_edge:by_name{name: n, path, idx}\n\
             todo[path, idx] := d in $ids, *raw_edge:by_dst{dst_id: d, path, idx}",
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
            ],
        )
    }

    fn callers(&self, id: SymbolId) -> Result<Vec<Resolution>> {
        let Some(sym) = self.symbol(id)? else {
            return Ok(vec![]);
        };
        let all = self.resolve_with(
            "todo[path, idx] := *raw_edge:by_name{name: $n, path, idx}\n\
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
