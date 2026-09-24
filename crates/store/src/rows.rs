//! Conversions between model types and CozoDB rows.

use std::collections::{BTreeMap, BTreeSet};

use cozo::DataValue;
use graphite_model::{
    EdgeKind, FileFacts, Lang, Provenance, RawEdge, Symbol, SymbolId, SymbolKind, Target,
};

use crate::{EdgeKey, Outcome, Resolution, Result, StoreError};

pub fn s(v: &str) -> DataValue {
    DataValue::Str(v.into())
}

pub fn opt_s(v: Option<&str>) -> DataValue {
    v.map(s).unwrap_or(DataValue::Null)
}

pub fn i(v: impl Into<i64>) -> DataValue {
    DataValue::from(v.into())
}

pub fn list(v: Vec<DataValue>) -> DataValue {
    DataValue::List(v)
}

fn corrupt(what: &str, v: &DataValue) -> StoreError {
    StoreError::Corrupt(format!("{what}: {v:?}"))
}

pub fn get_str<'a>(v: &'a DataValue, what: &str) -> Result<&'a str> {
    v.get_str().ok_or_else(|| corrupt(what, v))
}

pub fn get_opt_str<'a>(v: &'a DataValue, what: &str) -> Result<Option<&'a str>> {
    match v {
        DataValue::Null => Ok(None),
        other => get_str(other, what).map(Some),
    }
}

pub fn get_u32(v: &DataValue, what: &str) -> Result<u32> {
    v.get_int()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| corrupt(what, v))
}

pub fn get_bool(v: &DataValue, what: &str) -> Result<bool> {
    v.get_bool().ok_or_else(|| corrupt(what, v))
}

pub fn get_id(v: &DataValue, what: &str) -> Result<SymbolId> {
    SymbolId::from_hex(get_str(v, what)?).ok_or_else(|| corrupt(what, v))
}

/// Column order: id, path, lang, name, qualified, kind, start_line, end_line, start_byte, end_byte, exported, signature, parent, is_test.
pub const SYMBOL_COLS: &str =
    "id, path, lang, name, qualified, kind, start_line, end_line, start_byte, end_byte, exported, signature, parent, is_test";

pub fn symbol_row(sym: &Symbol) -> DataValue {
    list(vec![
        s(&sym.id.to_hex()),
        s(&sym.path),
        s(sym.lang.as_str()),
        s(&sym.name),
        s(&sym.qualified),
        s(sym.kind.as_str()),
        i(sym.start_line),
        i(sym.end_line),
        i(sym.start_byte),
        i(sym.end_byte),
        DataValue::Bool(sym.exported),
        s(&sym.signature),
        opt_s(sym.parent.map(|p| p.to_hex()).as_deref()),
        DataValue::Bool(sym.is_test),
    ])
}

pub fn parse_symbol(r: &[DataValue]) -> Result<Symbol> {
    let lang = get_str(&r[2], "lang")?;
    let kind = get_str(&r[5], "kind")?;
    Ok(Symbol {
        id: get_id(&r[0], "id")?,
        path: get_str(&r[1], "path")?.to_string(),
        lang: Lang::parse(lang).ok_or_else(|| corrupt("lang", &r[2]))?,
        name: get_str(&r[3], "name")?.to_string(),
        qualified: get_str(&r[4], "qualified")?.to_string(),
        kind: SymbolKind::parse(kind).ok_or_else(|| corrupt("kind", &r[5]))?,
        start_line: get_u32(&r[6], "start_line")?,
        end_line: get_u32(&r[7], "end_line")?,
        start_byte: get_u32(&r[8], "start_byte")?,
        end_byte: get_u32(&r[9], "end_byte")?,
        exported: get_bool(&r[10], "exported")?,
        signature: get_str(&r[11], "signature")?.to_string(),
        parent: match get_opt_str(&r[12], "parent")? {
            Some(h) => Some(SymbolId::from_hex(h).ok_or_else(|| corrupt("parent", &r[12]))?),
            None => None,
        },
        is_test: get_bool(&r[13], "is_test")?,
    })
}

/// Column order of `raw_edge` rows written by `edge_row`.
pub const EDGE_COLS: &str =
    "path, idx, lang, src, kind, site_line, dst_id, name, key_name, qualifier, import_path, prov";

pub fn edge_row(facts: &FileFacts, idx: usize, e: &RawEdge) -> DataValue {
    let (dst_id, name, qualifier, import_path) = match &e.dst {
        Target::Symbol(id) => (Some(id.to_hex()), None, None, None),
        Target::Unresolved {
            name,
            qualifier,
            import_path,
        } => (
            None,
            Some(name.as_str()),
            qualifier.as_deref(),
            import_path.as_deref(),
        ),
    };
    let key = crate::cozo_store::key_name(&e.dst, facts.lang);
    list(vec![
        s(&facts.path),
        i(idx as i64),
        s(facts.lang.as_str()),
        s(&e.src.to_hex()),
        s(e.kind.as_str()),
        i(e.site_line),
        opt_s(dst_id.as_deref()),
        opt_s(name),
        opt_s(key.as_deref()),
        opt_s(qualifier),
        opt_s(import_path),
        s(e.provenance.as_str()),
    ])
}

#[derive(Default)]
struct Pending {
    src: Option<SymbolId>,
    kind: Option<EdgeKind>,
    line: u32,
    prov: Option<Provenance>,
    direct: BTreeSet<SymbolId>,
    import: BTreeSet<(SymbolId, String)>,
    inherit: BTreeSet<(u32, SymbolId)>,
    qual: BTreeSet<SymbolId>,
    name: Option<SymbolId>,
    name_ambiguous: u32,
}

/// Pick one outcome per edge from candidate rows `path, idx, src, kind, line, prov, tier, d, dpath, depth, n`.
/// First tier with candidates wins; ties are ambiguous, except import ties broken by the candidate
/// sharing the longest directory prefix with the referencing file, and inherit ties by nearest base.
pub fn select(rows: &[Vec<DataValue>]) -> Result<Vec<Resolution>> {
    let mut edges: BTreeMap<EdgeKey, Pending> = BTreeMap::new();
    for r in rows {
        let key = EdgeKey {
            path: get_str(&r[0], "path")?.to_string(),
            idx: get_u32(&r[1], "idx")?,
        };
        let p = edges.entry(key).or_default();
        p.src = Some(get_id(&r[2], "src")?);
        let kind = get_str(&r[3], "kind")?;
        p.kind = Some(EdgeKind::parse(kind).ok_or_else(|| corrupt("kind", &r[3]))?);
        p.line = get_u32(&r[4], "site_line")?;
        let prov = get_str(&r[5], "prov")?;
        p.prov = Some(Provenance::parse(prov).ok_or_else(|| corrupt("prov", &r[5]))?);
        let cand = match &r[7] {
            DataValue::Null => None,
            v => Some(get_id(v, "dst")?),
        };
        match (get_str(&r[6], "tier")?, cand) {
            ("none", _) => {}
            ("direct", Some(d)) => {
                p.direct.insert(d);
            }
            ("import", Some(d)) => {
                p.import.insert((d, get_str(&r[8], "dpath")?.to_string()));
            }
            ("inherit", Some(d)) => {
                p.inherit.insert((get_u32(&r[9], "depth")?, d));
            }
            ("qual", Some(d)) => {
                p.qual.insert(d);
            }
            ("name", Some(d)) => p.name = Some(d),
            ("name", None) => p.name_ambiguous = get_u32(&r[10], "n")?,
            (tier, _) => return Err(StoreError::Corrupt(format!("tier {tier}"))),
        }
    }
    edges
        .into_iter()
        .map(|(key, p)| {
            let outcome = decide(&key.path, &p);
            Ok(Resolution {
                src: p.src.ok_or_else(|| StoreError::Corrupt("src".into()))?,
                kind: p.kind.ok_or_else(|| StoreError::Corrupt("kind".into()))?,
                site_line: p.line,
                key,
                outcome,
            })
        })
        .collect()
}

fn resolved(dst: SymbolId, provenance: Provenance) -> Outcome {
    Outcome::Resolved { dst, provenance }
}

fn one_or_ambiguous(c: &BTreeSet<SymbolId>, provenance: Provenance) -> Outcome {
    match c.len() {
        1 => resolved(*c.iter().next().expect("len 1"), provenance),
        n => Outcome::Ambiguous {
            candidates: n as u32,
        },
    }
}

fn shared_dirs(a: &str, b: &str) -> usize {
    a.split('/')
        .zip(b.split('/'))
        .take_while(|(x, y)| x == y)
        .count()
}

fn decide(path: &str, p: &Pending) -> Outcome {
    if let Some(d) = p.direct.iter().next() {
        return resolved(*d, p.prov.unwrap_or(Provenance::Extracted));
    }
    if !p.import.is_empty() {
        let ids: BTreeSet<SymbolId> = p.import.iter().map(|(d, _)| *d).collect();
        if ids.len() == 1 {
            return one_or_ambiguous(&ids, Provenance::Resolved);
        }
        let best = p
            .import
            .iter()
            .map(|(_, dp)| shared_dirs(path, dp))
            .max()
            .unwrap_or(0);
        let top: BTreeSet<SymbolId> = p
            .import
            .iter()
            .filter(|(_, dp)| shared_dirs(path, dp) == best)
            .map(|(d, _)| *d)
            .collect();
        return match top.len() {
            1 => one_or_ambiguous(&top, Provenance::Resolved),
            _ => Outcome::Ambiguous {
                candidates: ids.len() as u32,
            },
        };
    }
    if let Some((nearest, _)) = p.inherit.iter().next() {
        let top: BTreeSet<SymbolId> = p
            .inherit
            .iter()
            .filter(|(d, _)| d == nearest)
            .map(|(_, id)| *id)
            .collect();
        return one_or_ambiguous(&top, Provenance::Inferred);
    }
    if !p.qual.is_empty() {
        return one_or_ambiguous(&p.qual, Provenance::Inferred);
    }
    if let Some(d) = p.name {
        return resolved(d, Provenance::NameGuess);
    }
    if p.name_ambiguous > 1 {
        return Outcome::Ambiguous {
            candidates: p.name_ambiguous,
        };
    }
    Outcome::Unresolved
}
