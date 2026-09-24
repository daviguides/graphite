//! Agent-facing response contract: every answer carries its revision, freshness, completeness and every cut made.

use graphite_model::Symbol;
use graphite_store::Confidence;
use serde::Serialize;

pub const SCHEMA_VERSION: u32 = 1;

/// Uniform wrapper for every query answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Envelope<T> {
    pub schema: u32,
    pub query: &'static str,
    pub graph_rev: u64,
    /// Index may lag the working tree; answers reflect `graph_rev`.
    pub stale: bool,
    pub tier: Tier,
    pub completeness: Completeness,
    /// Every truncation, filter or grouping applied, with counts. Empty means nothing was cut.
    pub disclosures: Vec<Disclosure>,
    pub result: T,
}

/// Level of detail, degraded in order to fit the token budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Full,
    Summary,
    ByFile,
    ByDirectory,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Completeness {
    pub status: CompletenessStatus,
    pub causes: Causes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletenessStatus {
    Complete,
    /// Results are a floor: something may be missing for the reasons in `causes`.
    LowerBound,
}

/// Why a result may be incomplete.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Causes {
    /// References with the target's name that matched several symbols; any could be a missed dependent.
    pub ambiguous_refs: u32,
    /// References with the target's name that matched nothing (untyped receivers, dynamic calls).
    pub unresolved_refs: u32,
    /// Files in the index that failed to parse.
    pub parse_failures: u32,
    /// Symbols whose file changed since indexing; their source is withheld.
    pub source_changed: u32,
    /// Which symbols the reference counts cover.
    pub ref_scope: String,
}

impl Causes {
    pub(crate) fn completeness(self) -> Completeness {
        let partial =
            self.ambiguous_refs + self.unresolved_refs + self.parse_failures + self.source_changed
                > 0;
        Completeness {
            status: if partial {
                CompletenessStatus::LowerBound
            } else {
                CompletenessStatus::Complete
            },
            causes: self,
        }
    }
}

/// One cut made to the answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Disclosure {
    pub what: String,
    pub shown: u32,
    pub omitted: u32,
    pub reason: String,
}

impl Disclosure {
    pub(crate) fn new(what: &str, shown: usize, omitted: usize, reason: impl Into<String>) -> Self {
        Disclosure {
            what: what.to_string(),
            shown: shown as u32,
            omitted: omitted as u32,
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Prod,
    Test,
}

/// Confidence of an edge or of the weakest edge on a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceView {
    Extracted,
    Inferred,
    Ambiguous,
}

impl From<Confidence> for ConfidenceView {
    fn from(c: Confidence) -> Self {
        match c {
            Confidence::Extracted => ConfidenceView::Extracted,
            Confidence::Inferred => ConfidenceView::Inferred,
            Confidence::Ambiguous => ConfidenceView::Ambiguous,
        }
    }
}

/// Symbol as shown to the agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SymbolView {
    pub id: String,
    pub qualified: String,
    pub kind: &'static str,
    pub path: String,
    pub start_line: u32,
    pub end_line: u32,
    pub signature: String,
    pub role: Role,
}

impl From<&Symbol> for SymbolView {
    fn from(s: &Symbol) -> Self {
        SymbolView {
            id: s.id.to_hex(),
            qualified: s.qualified.clone(),
            kind: s.kind.as_str(),
            path: s.path.clone(),
            start_line: s.start_line,
            end_line: s.end_line,
            signature: s.signature.clone(),
            role: role(s),
        }
    }
}

pub(crate) fn role(s: &Symbol) -> Role {
    if s.is_test {
        Role::Test
    } else {
        Role::Prod
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    /// No production dependents found, but unresolved or ambiguous references could hide some.
    Unknown,
}

/// Change-risk verdict from production and test dependents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Risk {
    pub level: RiskLevel,
    pub prod_direct: u32,
    pub prod_total: u32,
    pub test_total: u32,
    pub reason: String,
}

impl Risk {
    pub(crate) fn assess(prod_direct: u32, prod_total: u32, test_total: u32, gaps: u32) -> Risk {
        let (level, reason) = if prod_total == 0 && gaps > 0 {
            (
                RiskLevel::Unknown,
                format!("no production dependents found, but {gaps} unresolved/ambiguous references with this name could hide some"),
            )
        } else if prod_total == 0 {
            (
                RiskLevel::Low,
                "no production dependents and no unresolved references with this name".to_string(),
            )
        } else if prod_direct >= 10 || prod_total >= 50 {
            (
                RiskLevel::High,
                format!("{prod_direct} direct / {prod_total} total production dependents"),
            )
        } else if prod_direct >= 3 || prod_total >= 10 {
            (
                RiskLevel::Medium,
                format!("{prod_direct} direct / {prod_total} total production dependents"),
            )
        } else {
            (
                RiskLevel::Low,
                format!("{prod_direct} direct / {prod_total} total production dependents"),
            )
        };
        let reason = if gaps > 0 && level != RiskLevel::Unknown {
            format!("{reason}; lower bound: {gaps} unresolved/ambiguous references")
        } else {
            reason
        };
        Risk {
            level,
            prod_direct,
            prod_total,
            test_total,
            reason,
        }
    }
}
