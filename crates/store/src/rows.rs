//! Conversions between model types and CozoDB rows.

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

/// Column order: path, idx, lang, src, kind, site_line, dst_id, name, qualifier, import_path, prov.
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
    list(vec![
        s(&facts.path),
        i(idx as i64),
        s(facts.lang.as_str()),
        s(&e.src.to_hex()),
        s(e.kind.as_str()),
        i(e.site_line),
        opt_s(dst_id.as_deref()),
        opt_s(name),
        opt_s(qualifier),
        opt_s(import_path),
        s(e.provenance.as_str()),
    ])
}

/// Row order: path, idx, src, kind, site_line, dst, prov, n.
pub fn parse_resolution(r: &[DataValue]) -> Result<Resolution> {
    let kind = get_str(&r[3], "kind")?;
    let outcome = match (&r[5], get_u32(&r[7], "n")?) {
        (DataValue::Null, 0) => Outcome::Unresolved,
        (DataValue::Null, n) => Outcome::Ambiguous { candidates: n },
        (dst, _) => {
            let prov = get_str(&r[6], "prov")?;
            Outcome::Resolved {
                dst: get_id(dst, "dst")?,
                provenance: Provenance::parse(prov).ok_or_else(|| corrupt("prov", &r[6]))?,
            }
        }
    };
    Ok(Resolution {
        key: EdgeKey {
            path: get_str(&r[0], "path")?.to_string(),
            idx: get_u32(&r[1], "idx")?,
        },
        src: get_id(&r[2], "src")?,
        kind: EdgeKind::parse(kind).ok_or_else(|| corrupt("kind", &r[3]))?,
        site_line: get_u32(&r[4], "site_line")?,
        outcome,
    })
}
