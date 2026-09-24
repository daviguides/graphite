//! Shared fact contract between extractors and the store.

use serde::{Deserialize, Serialize};

/// Language of the analyzed source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Lang {
    Python,
    Rust,
    TypeScript,
}

impl Lang {
    pub fn as_str(self) -> &'static str {
        match self {
            Lang::Python => "python",
            Lang::Rust => "rust",
            Lang::TypeScript => "typescript",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "python" => Some(Lang::Python),
            "rust" => Some(Lang::Rust),
            "typescript" => Some(Lang::TypeScript),
            _ => None,
        }
    }
}

/// Kind of a defined symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SymbolKind {
    Module,
    Class,
    Function,
    Method,
    Struct,
    Enum,
    Trait,
    Interface,
    TypeAlias,
    Const,
    Variable,
}

impl SymbolKind {
    pub const ALL: [SymbolKind; 11] = [
        SymbolKind::Module,
        SymbolKind::Class,
        SymbolKind::Function,
        SymbolKind::Method,
        SymbolKind::Struct,
        SymbolKind::Enum,
        SymbolKind::Trait,
        SymbolKind::Interface,
        SymbolKind::TypeAlias,
        SymbolKind::Const,
        SymbolKind::Variable,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            SymbolKind::Module => "module",
            SymbolKind::Class => "class",
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Trait => "trait",
            SymbolKind::Interface => "interface",
            SymbolKind::TypeAlias => "type_alias",
            SymbolKind::Const => "const",
            SymbolKind::Variable => "variable",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// Deterministic symbol identity: blake3 of (lang, path, qualified name, kind, overload index), never line-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SymbolId(pub [u8; 16]);

impl SymbolId {
    pub fn new(
        lang: Lang,
        path: &str,
        qualified: &str,
        kind: SymbolKind,
        overload_index: u32,
    ) -> Self {
        let mut h = blake3::Hasher::new();
        for part in [lang.as_str(), path, qualified, kind.as_str()] {
            h.update(part.as_bytes());
            h.update(&[0]);
        }
        h.update(&overload_index.to_le_bytes());
        let mut out = [0u8; 16];
        out.copy_from_slice(&h.finalize().as_bytes()[..16]);
        SymbolId(out)
    }

    pub fn to_hex(self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        if s.len() != 32 {
            return None;
        }
        let mut out = [0u8; 16];
        for (i, byte) in out.iter_mut().enumerate() {
            *byte = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
        }
        Some(SymbolId(out))
    }
}

impl std::fmt::Display for SymbolId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// A symbol defined in one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    pub id: SymbolId,
    pub lang: Lang,
    pub path: String,
    pub name: String,
    pub qualified: String,
    pub kind: SymbolKind,
    pub start_line: u32,
    pub end_line: u32,
    pub start_byte: u32,
    pub end_byte: u32,
    pub exported: bool,
    pub signature: String,
    pub parent: Option<SymbolId>,
    pub is_test: bool,
}

/// Relationship kind between two symbols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EdgeKind {
    Calls,
    Imports,
    Inherits,
    Contains,
    References,
    Tests,
}

impl EdgeKind {
    pub const ALL: [EdgeKind; 6] = [
        EdgeKind::Calls,
        EdgeKind::Imports,
        EdgeKind::Inherits,
        EdgeKind::Contains,
        EdgeKind::References,
        EdgeKind::Tests,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Calls => "calls",
            EdgeKind::Imports => "imports",
            EdgeKind::Inherits => "inherits",
            EdgeKind::Contains => "contains",
            EdgeKind::References => "references",
            EdgeKind::Tests => "tests",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// How an edge's target was determined; confidence is derived from this by rule in the store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Provenance {
    Extracted,
    Resolved,
    Inferred,
    NameGuess,
}

impl Provenance {
    pub const ALL: [Provenance; 4] = [
        Provenance::Extracted,
        Provenance::Resolved,
        Provenance::Inferred,
        Provenance::NameGuess,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Provenance::Extracted => "extracted",
            Provenance::Resolved => "resolved",
            Provenance::Inferred => "inferred",
            Provenance::NameGuess => "name_guess",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.as_str() == s)
    }
}

/// Edge target: a symbol known in the same file, or a reference resolved cross-file by the store.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Target {
    Symbol(SymbolId),
    Unresolved {
        name: String,
        qualifier: Option<String>,
        import_path: Option<String>,
    },
}

/// One edge as seen from a single file.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RawEdge {
    pub src: SymbolId,
    pub dst: Target,
    pub kind: EdgeKind,
    pub site_line: u32,
    pub provenance: Provenance,
}

/// Everything an extractor emits for one file; the unit of incremental update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileFacts {
    pub path: String,
    pub lang: Lang,
    pub content_hash: [u8; 32],
    pub symbols: Vec<Symbol>,
    pub edges: Vec<RawEdge>,
    pub parse_ok: bool,
}

/// Monotonic graph revision watermark, bumped on every committed write.
pub type GraphRev = u64;

/// Conventions for `Target::Unresolved`, shared by extractors and the store.
pub mod target {
    /// `import_path` prefix for stdlib/builtins: never in the repo, never a resolution candidate.
    pub const EXTERNAL_PREFIX: &str = "<external>:";
    /// `name` for a reference to a whole module (`import a.b`); `import_path` is the module path.
    pub const MODULE_TARGET: &str = "<module>";
    /// `name` for a wildcard import (`from x import *`); `import_path` is the module path.
    pub const WILDCARD_TARGET: &str = "*";
    /// `name` when the callee is not a name (`f()()`); `qualifier` holds its text.
    pub const DYNAMIC_TARGET: &str = "<dynamic>";
    /// Receivers that mean "the enclosing class or its bases".
    pub const SELF_RECEIVERS: &[&str] = &["self", "cls", "super()"];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_id_is_deterministic_and_line_independent() {
        let a = SymbolId::new(Lang::Python, "a/b.py", "b.f", SymbolKind::Function, 0);
        let b = SymbolId::new(Lang::Python, "a/b.py", "b.f", SymbolKind::Function, 0);
        let c = SymbolId::new(Lang::Python, "a/b.py", "b.f", SymbolKind::Function, 1);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(SymbolId::from_hex(&a.to_hex()), Some(a));
    }

    #[test]
    fn enums_roundtrip_strings() {
        for k in SymbolKind::ALL {
            assert_eq!(SymbolKind::parse(k.as_str()), Some(k));
        }
        for k in EdgeKind::ALL {
            assert_eq!(EdgeKind::parse(k.as_str()), Some(k));
        }
        for p in Provenance::ALL {
            assert_eq!(Provenance::parse(p.as_str()), Some(p));
        }
    }
}
