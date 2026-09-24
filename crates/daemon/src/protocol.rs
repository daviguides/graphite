//! Wire protocol: one JSON request per line, one JSON response per line.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Operation requested by a thin client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Status,
    /// Resolve a symbol reference (qualified name, bare name or hex id).
    Lookup {
        symbol: String,
    },
    Blast {
        symbol: String,
        depth: Option<u32>,
    },
    DiffImpact {
        base: Option<String>,
        depth: Option<u32>,
    },
    /// Hooks call this after an edit so the next query sees it.
    Nudge {
        paths: Vec<String>,
    },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    #[serde(flatten)]
    pub op: Op,
}

/// Every answer carries the graph watermark and whether it may be missing recent edits.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    pub graph_rev: u64,
    pub stale: bool,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(graph_rev: u64, stale: bool, data: Value) -> Self {
        Response {
            ok: true,
            graph_rev,
            stale,
            data,
            error: None,
        }
    }

    pub fn err(graph_rev: u64, error: impl Into<String>) -> Self {
        Response {
            ok: false,
            graph_rev,
            stale: false,
            data: Value::Null,
            error: Some(error.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_wire_format() {
        let r: Request =
            serde_json::from_str(r#"{"op":"blast","symbol":"a.f","depth":3}"#).unwrap();
        assert_eq!(
            r.op,
            Op::Blast {
                symbol: "a.f".into(),
                depth: Some(3)
            }
        );
        let s = serde_json::to_string(&Request { op: Op::Status }).unwrap();
        assert_eq!(s, r#"{"op":"status"}"#);
    }
}
