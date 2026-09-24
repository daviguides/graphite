//! CozoDB (mnestic fork, RocksDB backend) implementation of `GraphStore`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
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
        "symbol:cov",
        "::index create symbol:cov {name, lang, kind, qualified, path, id}",
    ),
    (
        "raw_edge",
        ":create raw_edge {path: String, idx: Int => lang: String, src: String, kind: String, site_line: Int, \
         dst_id: String?, name: String?, key_name: String?, qualifier: String?, import_path: String?, prov: String}",
    ),
    ("raw_edge:by_key", "::index create raw_edge:by_key {key_name, path, idx}"),
    ("raw_edge:by_dst", "::index create raw_edge:by_dst {dst_id, path, idx}"),
    ("raw_edge:by_src", "::index create raw_edge:by_src {src, path, idx}"),
    ("raw_edge:by_qual", "::index create raw_edge:by_qual {qualifier, path, idx}"),
    ("raw_edge:by_kind", "::index create raw_edge:by_kind {kind, path, idx}"),
    (
        "imp_head",
        ":create imp_head {path: String, idx: Int => head: String}",
    ),
    ("imp_head:by_head", "::index create imp_head:by_head {head, path, idx}"),
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
builtin_method[lang, n] <- $builtin_methods
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
repo_head[path, idx] := open[path, idx, _, _, _, _, _, imp, lang], internal[path, idx], !is_null(imp),
    head = regex_extract_first(imp, '^[^.:]+'), *symbol:cov{name: head, lang, kind: mk}, mk in ['module']
guessable[path, idx] := open[path, idx, _, _, _, _, _, imp, _], is_null(imp)
guessable[path, idx] := repo_head[path, idx]

c[path, idx, tier, d, dpath, depth, n] := e[path, idx, _, _, _, _, _, _, _, _, _, _],
    tier = 'none', d = null, dpath = null, depth = 0, n = 0

c[path, idx, tier, d, dpath, depth, n] := e[path, idx, _, _, _, d, _, _, _, _, _, _], !is_null(d),
    *symbol{id: d, path: dpath}, tier = 'direct', depth = 0, n = 1

ikey[key, lang, kind, imp] := open[path, idx, _, kind, _, key, _, imp, lang], !is_null(imp),
    internal[path, idx]
icand[key, lang, kind, imp, d, dpath, sk] := ikey[key, lang, kind, imp], sep[lang, s],
    *symbol:cov{name: key, lang, kind: sk, qualified: q, path: dpath, id: d}, compat[kind, sk],
    or(q == imp, ends_with(q, concat(s, imp)))
imp_hit[path, idx, d, dpath, sk, nm] := open[path, idx, _, kind, nm, key, _, imp, lang], !is_null(imp),
    internal[path, idx], icand[key, lang, kind, imp, d, dpath, sk]
c[path, idx, tier, d, dpath, depth, n] := imp_hit[path, idx, d, dpath, sk, nm], modlike[nm],
    sk in ['module'], tier = 'import', depth = 0, n = 1
c[path, idx, tier, d, dpath, depth, n] := imp_hit[path, idx, d, dpath, _, nm], not modlike[nm],
    tier = 'import', depth = 0, n = 1

need[s] := open[_, _, s, _, _, _, qual, imp, _], is_null(imp), selfish[qual]
need[p] := need[x], *symbol{id: x, parent: p}, !is_null(p)
par[x, p] := need[x], *symbol{id: x, parent: p}, !is_null(p)
parent_of[k] := par[_, k]
klass[k] := parent_of[k], *symbol{id: k, kind: kk}, kk in ['class']
cls_of[x, k] := par[x, k], klass[k]
cls_of[x, k] := par[x, p], not klass[p], cls_of[p, k]
root[k] := cls_of[_, k]
walk[x] := root[_], x = 1
inh[k, path, idx] := walk[1], *raw_edge:by_kind{kind: 'inherits', path, idx}, *raw_edge{path, idx, src: k}
base_e[k, path, idx, b] := inh[k, path, idx], *raw_edge{path, idx, dst_id: b}, !is_null(b), *symbol{id: b}
base_e[k, path, idx, b] := inh[k, path, idx], *raw_edge{path, idx, dst_id, key_name: key, import_path: imp, lang},
    is_null(dst_id), !is_null(imp), !starts_with(imp, '<external>:'), sep[lang, s],
    *symbol:cov{name: key, lang, kind: bk, qualified: q, id: b}, bk in ['class'],
    or(q == imp, ends_with(q, concat(s, imp)))
base[k, b] := base_e[k, _, _, b]
base_hit[path, idx] := base_e[_, path, idx, _]
ext_base[k] := inh[k, path, idx], *raw_edge{path, idx, dst_id}, is_null(dst_id), not base_hit[path, idx]
anc[k, a, min(d)] := root[k], base[k, a], d = 1
anc[k, a, min(d)] := anc[k, m, d0], d0 < 32, base[m, a], d = d0 + 1
ext_chain[k] := root[k], ext_base[k]
ext_chain[k] := anc[k, a, _], ext_base[a]
ext_self[path, idx] := open[path, idx, src, _, _, _, qual, imp, _], is_null(imp), selfish[qual],
    cls_of[src, k], ext_chain[k]
c[path, idx, tier, d, dpath, depth, n] := open[path, idx, src, kind, name, _, qual, imp, _],
    is_null(imp), selfish[qual], cls_of[src, k], anc[k, a, depth],
    *symbol:by_parent{parent: a, id: d}, *symbol{id: d, name: dn, kind: sk, path: dpath}, dn == name,
    compat[kind, sk],
    tier = 'inherit', n = 1

qkey[name, qual, lang, kind] := open[_, _, _, kind, name, _, qual, imp, lang], is_null(imp),
    !is_null(qual), not selfish[qual], not special[name]
qcand[name, qual, lang, kind, d, dpath] := qkey[name, qual, lang, kind], sep[lang, s],
    suffix = concat(s, qual, s, name), *symbol:cov{name, lang, kind: sk, qualified: q, path: dpath, id: d},
    compat[kind, sk], ends_with(q, suffix)
c[path, idx, tier, d, dpath, depth, n] := open[path, idx, _, kind, name, _, qual, imp, lang],
    is_null(imp), !is_null(qual), qcand[name, qual, lang, kind, d, dpath],
    tier = 'qual', depth = 0, n = 1

nkey[name, lang, kind] := named[_, _, name, lang, kind]
ncand[name, lang, kind, d, dpath] := nkey[name, lang, kind],
    *symbol:cov{name, lang, kind: sk, path: dpath, id: d}, compat[kind, sk]
ncount[name, lang, kind, count(d)] := ncand[name, lang, kind, d, _]
named[path, idx, name, lang, kind] := open[path, idx, _, kind, name, _, _, _, lang], guessable[path, idx],
    not special[name], not ext_self[path, idx]
risky[path, idx] := open[path, idx, _, _, name, _, qual, imp, lang], is_null(imp), !is_null(qual),
    not selfish[qual], builtin_method[lang, name]
c[path, idx, tier, d, dpath, depth, n] := named[path, idx, name, lang, kind], not risky[path, idx],
    ncount[name, lang, kind, 1], ncand[name, lang, kind, d, dpath], tier = 'name', depth = 0, n = 1
c[path, idx, tier, d, dpath, depth, n] := named[path, idx, name, lang, kind], not risky[path, idx],
    ncount[name, lang, kind, n], n > 1, tier = 'name', d = null, dpath = null, depth = 0
c[path, idx, tier, d, dpath, depth, n] := named[path, idx, name, lang, kind], risky[path, idx],
    ncount[name, lang, kind, m], n = m + 1, tier = 'name', d = null, dpath = null, depth = 0

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

/// Methods of builtin/stdlib types. An untyped `obj.m()` with one of these names is far more often the
/// builtin than the lone repo method of that name, so it is kept ambiguous instead of name-guessed.
const PY_BUILTIN_METHODS: &[&str] = &[
    "get",
    "items",
    "keys",
    "values",
    "update",
    "pop",
    "popitem",
    "setdefault",
    "copy",
    "clear",
    "append",
    "extend",
    "insert",
    "remove",
    "sort",
    "reverse",
    "index",
    "count",
    "split",
    "rsplit",
    "join",
    "strip",
    "lstrip",
    "rstrip",
    "replace",
    "format",
    "startswith",
    "endswith",
    "lower",
    "upper",
    "title",
    "capitalize",
    "encode",
    "decode",
    "find",
    "rfind",
    "splitlines",
    "partition",
    "rpartition",
    "zfill",
    "isdigit",
    "isalpha",
    "add",
    "discard",
    "union",
    "intersection",
    "difference",
    "issubset",
    "read",
    "write",
    "close",
    "readline",
    "readlines",
    "seek",
    "tell",
    "flush",
    "search",
    "match",
    "fullmatch",
    "sub",
    "findall",
    "finditer",
    "group",
    "groups",
    "post",
    "put",
    "patch",
    "delete",
    "head",
    "json",
    "exists",
    "mkdir",
    "glob",
    "rglob",
    "iterdir",
    "is_file",
    "is_dir",
    "open",
    "resolve",
    "unlink",
    "rename",
    "read_text",
    "write_text",
    "read_bytes",
    "write_bytes",
    "cancel",
    "result",
    "done",
    "wait",
    "set",
    "is_set",
    "acquire",
    "release",
    "put_nowait",
    "get_nowait",
    "total_seconds",
    "isoformat",
    "strftime",
    "timestamp",
    "astimezone",
    "hexdigest",
    "digest",
];

fn builtin_method_rows() -> DataValue {
    list(
        PY_BUILTIN_METHODS
            .iter()
            .map(|m| list(vec![s(Lang::Python.as_str()), s(m)]))
            .collect(),
    )
}

fn params(pairs: Vec<(&str, DataValue)>) -> BTreeMap<String, DataValue> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

/// Resolution-relevant identity of a symbol: if none of these change, no other file's edge can re-resolve.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SymSig {
    id: String,
    name: String,
    qualified: String,
    kind: String,
    lang: String,
    parent: Option<String>,
}

impl SymSig {
    fn of(sym: &Symbol) -> Self {
        SymSig {
            id: sym.id.to_hex(),
            name: sym.name.clone(),
            qualified: sym.qualified.clone(),
            kind: sym.kind.as_str().to_string(),
            lang: sym.lang.as_str().to_string(),
            parent: sym.parent.map(|p| p.to_hex()),
        }
    }
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
        p.insert("builtin_methods".into(), builtin_method_rows());
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

        // Only symbols whose resolution-relevant fields changed can change other files' edges.
        let old = self.run(
            "?[id, name, qualified, kind, lang, parent] := *symbol:by_path{path: $p, id}, \
             *symbol{id, name, qualified, kind, lang, parent}",
            params(vec![("p", s(path))]),
            false,
        )?;
        let old_edges = self.run(
            "?[idx, kind, src, dst_id, key_name, qualifier, import_path] := \
             *raw_edge{path: $p, idx, kind, src, dst_id, key_name, qualifier, import_path}",
            params(vec![("p", s(path))]),
            false,
        )?;

        let mut old_syms: BTreeSet<SymSig> = BTreeSet::new();
        for r in &old.rows {
            old_syms.insert(SymSig {
                id: get_str(&r[0], "id")?.to_string(),
                name: get_str(&r[1], "name")?.to_string(),
                qualified: get_str(&r[2], "qualified")?.to_string(),
                kind: get_str(&r[3], "kind")?.to_string(),
                lang: get_str(&r[4], "lang")?.to_string(),
                parent: rows::get_opt_str(&r[5], "parent")?.map(str::to_string),
            });
        }
        let mut old_inherits: BTreeSet<Vec<Option<String>>> = BTreeSet::new();
        for r in &old_edges.rows {
            if get_str(&r[1], "kind")? == EdgeKind::Inherits.as_str() {
                old_inherits.insert(
                    r[2..]
                        .iter()
                        .map(|v| rows::get_opt_str(v, "inherits").map(|o| o.map(str::to_string)))
                        .collect::<Result<_>>()?,
                );
            }
        }
        let (new_syms, new_inherits) = match facts {
            Some(f) => (
                f.symbols.iter().map(SymSig::of).collect(),
                f.edges
                    .iter()
                    .filter(|e| e.kind == EdgeKind::Inherits)
                    .map(|e| rows::inherits_sig(f, e))
                    .collect(),
            ),
            None => (BTreeSet::new(), BTreeSet::new()),
        };

        let changed: Vec<&SymSig> = old_syms.symmetric_difference(&new_syms).collect();
        let names: BTreeSet<String> = changed.iter().map(|c| c.name.clone()).collect();
        let classes = changed.iter().any(|c| c.kind == SymbolKind::Class.as_str())
            || old_inherits != new_inherits;
        let mut ids: BTreeSet<SymbolId> = BTreeSet::new();
        for sig in old_syms.iter().chain(new_syms.iter()) {
            ids.insert(
                SymbolId::from_hex(&sig.id)
                    .ok_or_else(|| StoreError::Corrupt(format!("id {}", sig.id)))?,
            );
        }

        let rev = self.rev.load(Ordering::SeqCst) + 1;
        let mut script = String::from(
            "{?[id] := *symbol:by_path{path: $p, id} :rm symbol {id}}\n\
             {?[path, idx] := path = $p, *raw_edge{path, idx} :rm raw_edge {path, idx}}\n\
             {?[path, idx] := path = $p, *imp_head{path, idx} :rm imp_head {path, idx}}\n\
             {?[path] <- [[$p]] :rm file {path}}\n\
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
            let heads: Vec<DataValue> = f
                .edges
                .iter()
                .enumerate()
                .filter_map(|(n, e)| {
                    import_head(&e.dst, f.lang).map(|h| list(vec![s(&f.path), i(n as i64), s(&h)]))
                })
                .collect();
            if !heads.is_empty() {
                script
                    .push_str("{?[path, idx, head] <- $heads :put imp_head {path, idx => head}}\n");
                p.push(("heads", list(heads)));
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

    fn symbols(&self, ids: &[SymbolId]) -> Result<Vec<Symbol>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let script = format!(
            "?[{c}] := id in $ids, *symbol{{{c}}}",
            c = rows::SYMBOL_COLS
        );
        self.query_symbols(
            &script,
            vec![("ids", list(ids.iter().map(|d| s(&d.to_hex())).collect()))],
        )
    }

    fn file_hashes(&self) -> Result<HashMap<String, [u8; 32]>> {
        let rows = self.run(
            "?[path, content_hash] := *file{path, content_hash}",
            BTreeMap::new(),
            false,
        )?;
        rows.rows
            .iter()
            .map(|r| {
                let hex = get_str(&r[1], "content_hash")?;
                let mut hash = [0u8; 32];
                if hex.len() != 64 {
                    return Err(StoreError::Corrupt(format!("content_hash {hex}")));
                }
                for (n, byte) in hash.iter_mut().enumerate() {
                    *byte = u8::from_str_radix(&hex[2 * n..2 * n + 2], 16)
                        .map_err(|_| StoreError::Corrupt(format!("content_hash {hex}")))?;
                }
                Ok((get_str(&r[0], "path")?.to_string(), hash))
            })
            .collect()
    }

    fn name_gaps(&self, name: &str) -> Result<Vec<Resolution>> {
        let gaps = self.resolve_with(
            "todo[path, idx] := *raw_edge:by_key{key_name: $n, path, idx}, \
             *raw_edge{path, idx, import_path: imp}, is_null(imp)\n\
             todo[path, idx] := *raw_edge:by_key{key_name: $n, path, idx}, \
             *raw_edge{path, idx, import_path: imp}, !is_null(imp), !starts_with(imp, '<external>:')",
            vec![("n", s(name))],
        )?;
        Ok(gaps
            .into_iter()
            .filter(|r| !matches!(r.outcome, crate::Outcome::Resolved { .. }))
            .collect())
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
            "todo[path, idx] := path = $p, *raw_edge{path, idx}\n\
             todo[path, idx] := n in $names, *raw_edge:by_key{key_name: n, path, idx}\n\
             todo[path, idx] := d in $ids, *raw_edge:by_dst{dst_id: d, path, idx}\n\
             todo[path, idx] := h in $names, *imp_head:by_head{head: h, path, idx}\n",
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

/// First segment of an in-repo import path. Name-guess fallback for import edges depends on a repo
/// module with that name existing, so writes touching such a module re-resolve these edges.
fn import_head(dst: &Target, lang: Lang) -> Option<String> {
    use graphite_model::target::EXTERNAL_PREFIX;
    let Target::Unresolved {
        import_path: Some(imp),
        ..
    } = dst
    else {
        return None;
    };
    if imp.starts_with(EXTERNAL_PREFIX) {
        return None;
    }
    let sep = if lang == Lang::Rust { "::" } else { "." };
    imp.split(sep).next().map(str::to_string)
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
