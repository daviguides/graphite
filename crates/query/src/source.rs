//! Reads symbol source spans from the working tree and detects files that changed since indexing.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use graphite_model::{Symbol, SymbolKind};
use serde::Serialize;

/// Default per-symbol source cap in bytes.
pub const SOURCE_CAP: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    Fresh,
    /// File no longer matches the indexed span; text withheld rather than shown wrong.
    Changed,
    Missing,
}

/// Verbatim, line-numbered source of one symbol.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceBlock {
    pub state: SourceState,
    pub start_line: u32,
    pub text: String,
    pub truncated: bool,
}

/// Per-query file cache over the working tree.
pub struct SourceReader {
    root: PathBuf,
    files: HashMap<String, Option<Vec<u8>>>,
}

impl SourceReader {
    pub fn new(root: &Path) -> Self {
        SourceReader {
            root: root.to_path_buf(),
            files: HashMap::new(),
        }
    }

    /// Source of `sym`, capped at `cap` bytes on a line boundary.
    pub fn read(&mut self, sym: &Symbol, cap: usize) -> SourceBlock {
        let root = &self.root;
        let bytes = self
            .files
            .entry(sym.path.clone())
            .or_insert_with(|| std::fs::read(root.join(&sym.path)).ok());
        let empty = |state| SourceBlock {
            state,
            start_line: sym.start_line,
            text: String::new(),
            truncated: false,
        };
        let Some(bytes) = bytes.as_deref() else {
            return empty(SourceState::Missing);
        };
        let (start, end) = (sym.start_byte as usize, sym.end_byte as usize);
        if !span_matches(bytes, sym, start, end) {
            return empty(SourceState::Changed);
        }
        let span = &bytes[start..end];
        let (span, truncated) = if span.len() > cap {
            let cut = span[..cap]
                .iter()
                .rposition(|b| *b == b'\n')
                .map_or(cap, |i| i + 1);
            (&span[..cut], true)
        } else {
            (span, false)
        };
        let text = String::from_utf8_lossy(span);
        let mut out =
            String::with_capacity(text.len() + 8 * (sym.end_line - sym.start_line + 1) as usize);
        for (i, line) in text.lines().enumerate() {
            out.push_str(&format!("{}| {}\n", sym.start_line as usize + i, line));
        }
        SourceBlock {
            state: SourceState::Fresh,
            start_line: sym.start_line,
            text: out,
            truncated,
        }
    }
}

/// The indexed byte span still starts on the indexed line and still names the symbol.
fn span_matches(bytes: &[u8], sym: &Symbol, start: usize, end: usize) -> bool {
    if start > end || end > bytes.len() {
        return false;
    }
    let line = bytes[..start].iter().filter(|b| **b == b'\n').count() as u32 + 1;
    if line != sym.start_line {
        return false;
    }
    if sym.kind == SymbolKind::Module {
        return end == bytes.len();
    }
    let span = String::from_utf8_lossy(&bytes[start..end]);
    if sym.signature.is_empty() {
        return span.contains(&sym.name);
    }
    let body: String = span
        .lines()
        .skip_while(|l| l.trim_start().starts_with('@'))
        .collect::<Vec<_>>()
        .join(" ");
    let want: String = collapse(&sym.signature)
        .chars()
        .take(SIGNATURE_PREFIX)
        .collect();
    collapse(&body).starts_with(&want)
}

/// Enough of the signature to tell the definition moved or changed.
const SIGNATURE_PREFIX: usize = 32;

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}
