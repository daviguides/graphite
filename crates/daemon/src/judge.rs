//! Residue judgment: which search hits the graph explains, rendered as a compact grep-shaped answer.
//! Every hit is either printed or counted in an explicit "+N more" line; nothing is dropped silently.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use graphite_model::{EdgeKind, Symbol, SymbolId, SymbolKind};
use graphite_store::{confidence, Confidence, GraphStore, Outcome, WriteDelta};
use serde_json::{json, Value};

use crate::engine::Engine;
use crate::search::{display_path, Hit, SearchOutcome, SearchSpec};

const CAP_REFS: usize = 60;
const CAP_RESIDUE: usize = 12;
const CAP_GROUPED: usize = 150;
const CAP_FILES: usize = 80;
const MAX_NAME_SYMBOLS: usize = 200;

/// Why a matching line is there.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Class {
    Definition,
    Reference,
    GraphGap,
    CodeUntracked,
    StringOrComment,
    OtherLanguage,
    NotIndexed,
    DocsConfig,
    OtherIdentifier,
}

impl Class {
    fn key(self) -> &'static str {
        match self {
            Class::Definition => "definition",
            Class::Reference => "reference",
            Class::GraphGap => "graph_gap",
            Class::CodeUntracked => "code_untracked",
            Class::StringOrComment => "string_or_comment",
            Class::OtherLanguage => "other_language",
            Class::NotIndexed => "not_indexed",
            Class::DocsConfig => "docs_config",
            Class::OtherIdentifier => "other_identifier",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Class::GraphGap => "graph gap — calls the graph could not resolve to one target",
            Class::CodeUntracked => {
                "code the graph does not track as a reference (value or attribute use)"
            }
            Class::StringOrComment => "in strings or comments",
            Class::OtherLanguage => "in source the graph does not index yet",
            Class::NotIndexed => "in Python files outside the index",
            Class::DocsConfig => "in docs / config",
            Class::OtherIdentifier => "part of a different identifier",
            Class::Definition | Class::Reference => "",
        }
    }

    const RESIDUE: [Class; 7] = [
        Class::GraphGap,
        Class::CodeUntracked,
        Class::StringOrComment,
        Class::OtherLanguage,
        Class::NotIndexed,
        Class::DocsConfig,
        Class::OtherIdentifier,
    ];
}

const CODE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "go", "java", "kt", "swift", "c", "h", "cc",
    "cpp", "hpp", "rb", "php", "sh", "bash", "zsh", "lua", "scala", "cs", "sql", "pyx", "pyi",
];

/// Name to look up when the pattern is a plain identifier (optionally dotted, optionally `\b`-wrapped).
pub fn identifier_of(spec: &SearchSpec) -> Option<String> {
    if spec.patterns.len() != 1 {
        return None;
    }
    let mut p = spec.patterns[0].as_str();
    if !spec.fixed {
        p = p.strip_prefix("\\b").unwrap_or(p);
        p = p.strip_suffix("\\b").unwrap_or(p);
    }
    let core = if spec.fixed {
        p.to_string()
    } else {
        p.replace("\\.", ".")
    };
    let ok = !core.is_empty()
        && core.split('.').all(|seg| {
            let mut c = seg.chars();
            matches!(c.next(), Some(ch) if ch == '_' || ch.is_ascii_alphabetic())
                && c.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        });
    ok.then(|| core.rsplit('.').next().unwrap_or(&core).to_string())
}

struct RefInfo {
    src: SymbolId,
    kind: EdgeKind,
    conf: Confidence,
}

/// What the graph knows about a name.
pub(crate) struct NameFacts {
    name: String,
    pub(crate) syms: Vec<Symbol>,
    refs: HashMap<(String, u32), RefInfo>,
    gaps: HashMap<(String, u32), String>,
    labels: HashMap<SymbolId, String>,
}

impl NameFacts {
    pub(crate) fn ref_count(&self) -> usize {
        self.refs.len()
    }

    pub(crate) fn gap_count(&self) -> usize {
        self.gaps.len()
    }

    pub(crate) fn ref_paths(&self) -> impl Iterator<Item = &str> {
        self.refs.keys().map(|(p, _)| p.as_str())
    }
}

pub(crate) fn short_label(s: &Symbol) -> String {
    let parts: Vec<&str> = s.qualified.rsplit('.').take(2).collect();
    match (parts.first(), parts.get(1), s.parent.is_some()) {
        (Some(n), Some(p), true) => format!("{p}.{n}"),
        (Some(n), _, _) => n.to_string(),
        _ => s.name.clone(),
    }
}

pub(crate) fn name_facts(engine: &Engine, name: &str, dotted: &str) -> Result<NameFacts, String> {
    let store = &engine.store;
    let mut syms = store.symbols_by_name(name).map_err(|e| e.to_string())?;
    if dotted.contains('.') {
        syms.retain(|s| s.qualified.ends_with(dotted));
    }
    syms.truncate(MAX_NAME_SYMBOLS);
    let ids: HashSet<SymbolId> = syms.iter().map(|s| s.id).collect();
    let delta = WriteDelta {
        touched_names: vec![name.to_string()],
        touched_ids: ids.iter().copied().collect(),
        ..Default::default()
    };
    let mut refs = HashMap::new();
    for r in store.resolve_affected(&delta).map_err(|e| e.to_string())? {
        if let Outcome::Resolved { dst, provenance } = r.outcome {
            // Containment is structure (module → its own definition), not a use.
            if ids.contains(&dst) && r.kind != EdgeKind::Contains {
                refs.insert(
                    (r.key.path.clone(), r.site_line),
                    RefInfo {
                        src: r.src,
                        kind: r.kind,
                        conf: confidence(provenance),
                    },
                );
            }
        }
    }
    let mut gaps = HashMap::new();
    for r in store.name_gaps(name).map_err(|e| e.to_string())? {
        let why = match r.outcome {
            Outcome::Ambiguous { candidates } => format!("ambiguous: {candidates} candidates"),
            _ => "unresolved".to_string(),
        };
        gaps.insert((r.key.path.clone(), r.site_line), why);
    }
    let srcs: Vec<SymbolId> = refs.values().map(|r| r.src).collect();
    let labels = store
        .symbols(&srcs)
        .map_err(|e| e.to_string())?
        .iter()
        .map(|s| (s.id, short_label(s)))
        .collect();
    Ok(NameFacts {
        name: name.to_string(),
        syms,
        refs,
        gaps,
        labels,
    })
}

/// Lazily parsed Python sources, for string/comment checks and alias lines.
#[derive(Default)]
struct Files {
    text: HashMap<PathBuf, Option<Vec<String>>>,
    trees: HashMap<PathBuf, Option<(tree_sitter::Tree, String)>>,
}

impl Files {
    fn lines(&mut self, abs: &Path) -> Option<&Vec<String>> {
        self.text
            .entry(abs.to_path_buf())
            .or_insert_with(|| {
                std::fs::read(abs).ok().map(|b| {
                    String::from_utf8_lossy(&b)
                        .lines()
                        .map(String::from)
                        .collect()
                })
            })
            .as_ref()
    }

    fn line(&mut self, abs: &Path, line: u32) -> String {
        self.lines(abs)
            .and_then(|l| l.get(line.saturating_sub(1) as usize))
            .cloned()
            .unwrap_or_default()
    }

    fn in_string_or_comment(&mut self, abs: &Path, line: u32, col: usize) -> bool {
        let entry = self.trees.entry(abs.to_path_buf()).or_insert_with(|| {
            let src = String::from_utf8_lossy(&std::fs::read(abs).ok()?).into_owned();
            let mut p = tree_sitter::Parser::new();
            p.set_language(&tree_sitter_python::LANGUAGE.into()).ok()?;
            let tree = p.parse(&src, None)?;
            Some((tree, src))
        });
        let Some((tree, _)) = entry else {
            return false;
        };
        let pt = tree_sitter::Point {
            row: line.saturating_sub(1) as usize,
            column: col,
        };
        let mut node = tree.root_node().descendant_for_point_range(pt, pt);
        while let Some(n) = node {
            if matches!(
                n.kind(),
                "string" | "string_content" | "comment" | "concatenated_string"
            ) {
                return true;
            }
            node = n.parent();
        }
        false
    }
}

fn ext_of(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Identifier token around the match column.
fn token_at(text: &str, col: usize) -> String {
    let b = text.as_bytes();
    let is_id = |c: u8| c == b'_' || c.is_ascii_alphanumeric();
    let col = col.min(b.len());
    let mut s = col;
    while s > 0 && is_id(b[s - 1]) {
        s -= 1;
    }
    let mut e = col;
    while e < b.len() && is_id(b[e]) {
        e += 1;
    }
    text[s..e].to_string()
}

fn is_definition_line(text: &str, name: &str) -> bool {
    let t = text.trim_start();
    let t = t.strip_prefix("async ").unwrap_or(t);
    ["def ", "class "].iter().any(|kw| {
        t.strip_prefix(kw).is_some_and(|rest| {
            rest.starts_with(name)
                && !rest[name.len()..].starts_with(|c: char| c == '_' || c.is_ascii_alphanumeric())
        })
    })
}

fn classify(engine: &Engine, facts: Option<&NameFacts>, hit: &Hit, files: &mut Files) -> Class {
    let rel = engine.paths.relative(&hit.abs);
    if let (Some(f), Some(rel)) = (facts, rel.as_deref()) {
        let key = (rel.to_string(), hit.line);
        if f.refs.contains_key(&key) {
            return Class::Reference;
        }
        if f.syms.iter().any(|s| s.path == rel) && is_definition_line(&hit.text, &f.name) {
            return Class::Definition;
        }
        if f.gaps.contains_key(&key) {
            return Class::GraphGap;
        }
        if token_at(&hit.text, hit.col) != f.name {
            return Class::OtherIdentifier;
        }
    }
    let ext = ext_of(&hit.abs);
    if ext != "py" {
        return if CODE_EXTS.contains(&ext.as_str()) {
            Class::OtherLanguage
        } else {
            Class::DocsConfig
        };
    }
    match rel.as_deref() {
        Some(r) if engine.is_indexable(r) => {
            if files.in_string_or_comment(&hit.abs, hit.line, hit.col) {
                Class::StringOrComment
            } else {
                Class::CodeUntracked
            }
        }
        _ => Class::NotIndexed,
    }
}

fn kind_word(k: EdgeKind) -> &'static str {
    match k {
        EdgeKind::Calls => "calls",
        EdgeKind::Imports => "imports",
        EdgeKind::Inherits => "inherits",
        EdgeKind::Contains => "contains",
        EdgeKind::References => "references",
        EdgeKind::Tests => "tests",
    }
}

fn quote_argv(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            if !a.is_empty()
                && a.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "-_./=:,@%+".contains(c))
            {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', "'\\''"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

struct Out<'a> {
    buf: String,
    spec: &'a SearchSpec,
    files: &'a mut Files,
}

impl Out<'_> {
    fn line(&mut self, s: &str) {
        self.buf.push_str(s);
        self.buf.push('\n');
    }

    /// One hit in grep format, with -A/-B context rendered from the file.
    fn hit(&mut self, display: &str, abs: &Path, line: u32, text: &str, note: &str) {
        let (b, a) = (self.spec.before, self.spec.after);
        if b > 0 || a > 0 {
            for n in line.saturating_sub(b).max(1)..line {
                let t = self.files.line(abs, n);
                let _ = writeln!(self.buf, "{display}-{n}-{t}");
            }
        }
        let _ = writeln!(self.buf, "{display}:{line}:{text}{note}");
        if b > 0 || a > 0 {
            for n in line + 1..=line + a {
                let Some(t) = self
                    .files
                    .lines(abs)
                    .and_then(|l| l.get(n as usize - 1))
                    .cloned()
                else {
                    break;
                };
                let _ = writeln!(self.buf, "{display}-{n}-{t}");
            }
            self.buf.push_str("--\n");
        }
    }
}

fn more_hint(spec: &SearchSpec) -> String {
    if spec.argv.is_empty() {
        String::new()
    } else {
        format!(
            " — all: graphite-hook exec --all -- {}",
            quote_argv(&spec.argv)
        )
    }
}

fn excluded_note(out: &mut Out, res: &SearchOutcome, cwd: &Path) {
    if res.excluded_dirs.is_empty() || (res.excluded_matches == 0 && res.excluded_scan_complete) {
        return;
    }
    let mut dirs: Vec<String> = res
        .excluded_dirs
        .iter()
        .map(|d| format!("{}/", display_path(d, cwd)))
        .collect();
    dirs.sort();
    let shown: Vec<String> = dirs.iter().take(6).cloned().collect();
    let extra = dirs.len().saturating_sub(shown.len());
    let count = if res.excluded_scan_complete {
        format!("{} matches there", res.excluded_matches)
    } else {
        format!("≥{} matches there (scan cut short)", res.excluded_matches)
    };
    out.line(&format!(
        "not searched (default excludes): {}{} — {count}; name the dir explicitly to search it.",
        shown.join(" "),
        if extra > 0 {
            format!(" +{extra} more")
        } else {
            String::new()
        }
    ));
}

/// Render the agent-facing answer for a search; returns (text, stats).
pub fn render(
    engine: &Engine,
    spec: &SearchSpec,
    res: &SearchOutcome,
    stale: bool,
) -> Result<(String, Value), String> {
    let cwd = PathBuf::from(&spec.cwd);
    let name = identifier_of(spec);
    let facts = match &name {
        Some(n) => {
            let dotted = spec.patterns[0].replace("\\.", ".");
            let dotted = dotted.trim_start_matches("\\b").trim_end_matches("\\b");
            Some(name_facts(engine, n, dotted)?)
        }
        None => None,
    };
    let mut files = Files::default();
    let classes: Vec<Class> = res
        .hits
        .iter()
        .map(|h| classify(engine, facts.as_ref(), h, &mut files))
        .collect();
    let mut counts: BTreeMap<Class, usize> = BTreeMap::new();
    for c in &classes {
        *counts.entry(*c).or_default() += 1;
    }
    let nfiles: HashSet<&Path> = res.hits.iter().map(|h| h.abs.as_path()).collect();
    let raw_bytes: usize = res
        .hits
        .iter()
        .map(|h| h.display.len() + h.text.len() + 8)
        .sum();

    let mut out = Out {
        buf: String::new(),
        spec,
        files: &mut files,
    };
    let label = if spec.label.is_empty() {
        spec.patterns.join(" | ")
    } else {
        spec.label.clone()
    };
    let explained = counts.get(&Class::Reference).copied().unwrap_or(0)
        + counts.get(&Class::Definition).copied().unwrap_or(0);
    let mut header = format!(
        "[graphite] {label} → {} matches in {} files",
        res.hits.len(),
        nfiles.len()
    );
    if facts.is_some() {
        let _ = write!(header, " · graph explains {explained}");
    }
    out.line(&header);
    if stale {
        out.line(
            "graph may lag your latest edit (indexing in progress); the text matches are current.",
        );
    }
    if res.truncated {
        out.line(&format!(
            "search stopped at {} matches — narrow the pattern or path.",
            crate::search::MAX_MATCHES
        ));
    }

    let mut alias_refs = 0usize;
    match &facts {
        Some(f) => render_identifier(
            &mut out,
            engine,
            spec,
            res,
            &classes,
            f,
            &cwd,
            &mut alias_refs,
        ),
        None => render_grouped(&mut out, engine, spec, res),
    }
    excluded_note(&mut out, res, &cwd);

    let text = out.buf;
    let stats = json!({
        "mode": if facts.is_some() { "identifier" } else { "grouped" },
        "matches": res.hits.len(),
        "files": nfiles.len(),
        "classes": counts.iter().map(|(c, n)| (c.key().to_string(), json!(n))).collect::<serde_json::Map<_,_>>(),
        "alias_refs": alias_refs,
        "excluded_matches": res.excluded_matches,
        "out_bytes": text.len(),
        "raw_bytes": raw_bytes,
    });
    Ok((text, stats))
}

#[allow(clippy::too_many_arguments)]
fn render_identifier(
    out: &mut Out,
    engine: &Engine,
    spec: &SearchSpec,
    res: &SearchOutcome,
    classes: &[Class],
    f: &NameFacts,
    cwd: &Path,
    alias_refs: &mut usize,
) {
    let gaps = f.gaps.len();
    if f.syms.is_empty() {
        out.line(&format!(
            "graph: no symbol named `{}` in the index — matches grouped by kind below.",
            f.name
        ));
    } else if gaps == 0 {
        out.line(&format!(
            "graph: complete — every reference the graph resolved to `{}` is listed; 0 unresolved or ambiguous calls with that name.",
            f.name
        ));
    } else {
        out.line(&format!(
            "graph: lower bound — {gaps} calls named `{}` could not be resolved to one target (listed under graph gap if they matched).",
            f.name
        ));
    }

    if spec.files_only {
        render_files_only(out, res, classes);
        return;
    }

    let idx =
        |c: Class| -> Vec<usize> { (0..classes.len()).filter(|&i| classes[i] == c).collect() };

    let defs = idx(Class::Definition);
    if !defs.is_empty() {
        out.line("definition:");
        for i in defs {
            let h = &res.hits[i];
            out.hit(&h.display, &h.abs, h.line, &h.text, "");
        }
    }

    let refs = idx(Class::Reference);
    let seen: HashSet<(String, u32)> = refs
        .iter()
        .filter_map(|&i| {
            let h = &res.hits[i];
            engine.paths.relative(&h.abs).map(|r| (r, h.line))
        })
        .collect();
    let mut hidden: Vec<(&(String, u32), &RefInfo)> = f
        .refs
        .iter()
        .filter(|(k, _)| !seen.contains(*k) && res.searched.contains(&engine.paths.root.join(&k.0)))
        .collect();
    hidden.sort_by(|a, b| a.0.cmp(b.0));
    *alias_refs = hidden.len();

    if !refs.is_empty() {
        let files: HashSet<&Path> = refs.iter().map(|&i| res.hits[i].abs.as_path()).collect();
        out.line(&format!(
            "references ({} sites in {} files):",
            refs.len(),
            files.len()
        ));
        let cap = if spec.all { usize::MAX } else { CAP_REFS };
        for &i in refs.iter().take(cap) {
            let h = &res.hits[i];
            let rel = engine.paths.relative(&h.abs).unwrap_or_default();
            let note = f
                .refs
                .get(&(rel, h.line))
                .map(|r| ref_note(f, r))
                .unwrap_or_default();
            out.hit(&h.display, &h.abs, h.line, &h.text, &note);
        }
        if refs.len() > cap {
            out.line(&format!(
                "+{} more references{}",
                refs.len() - cap,
                more_hint(spec)
            ));
        }
    }
    if !hidden.is_empty() {
        out.line("references the text search cannot see (aliased import or renamed call):");
        for ((path, line), r) in hidden.iter().take(CAP_REFS) {
            let abs = engine.paths.root.join(path);
            let text = out.files.line(&abs, *line);
            let note = ref_note(f, r);
            out.hit(&display_path(&abs, cwd), &abs, *line, &text, &note);
        }
        if hidden.len() > CAP_REFS {
            out.line(&format!("+{} more", hidden.len() - CAP_REFS));
        }
    }

    let residue: usize = Class::RESIDUE.iter().map(|c| idx(*c).len()).sum();
    if residue > 0 {
        out.line(&format!("other matches ({residue}):"));
    }
    for c in Class::RESIDUE {
        let items = idx(c);
        if items.is_empty() {
            continue;
        }
        out.line(&format!("  {} ({}):", c.label(), items.len()));
        let cap = if spec.all { usize::MAX } else { CAP_RESIDUE };
        if c == Class::OtherIdentifier {
            let mut by_tok: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for &i in &items {
                let h = &res.hits[i];
                by_tok
                    .entry(token_at(&h.text, h.col))
                    .or_default()
                    .push(format!("{}:{}", h.display, h.line));
            }
            for (tok, locs) in by_tok {
                let shown: Vec<&String> = locs.iter().take(cap).collect();
                let more = locs.len() - shown.len();
                out.line(&format!(
                    "    {tok} ×{}: {}{}",
                    locs.len(),
                    shown
                        .iter()
                        .map(|s| s.as_str())
                        .collect::<Vec<_>>()
                        .join(", "),
                    if more > 0 {
                        format!(" +{more} more")
                    } else {
                        String::new()
                    }
                ));
            }
            continue;
        }
        for &i in items.iter().take(cap) {
            let h = &res.hits[i];
            let note = if c == Class::GraphGap {
                let rel = engine.paths.relative(&h.abs).unwrap_or_default();
                f.gaps
                    .get(&(rel, h.line))
                    .map(|w| format!("    [{w}]"))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            out.hit(&h.display, &h.abs, h.line, &h.text, &note);
        }
        if items.len() > cap {
            out.line(&format!("  +{} more{}", items.len() - cap, more_hint(spec)));
        }
    }
}

fn ref_note(f: &NameFacts, r: &RefInfo) -> String {
    let src = f.labels.get(&r.src).cloned().unwrap_or_default();
    let conf = match r.conf {
        Confidence::Extracted => "",
        Confidence::Inferred => ", inferred",
        Confidence::Ambiguous => ", name guess",
    };
    format!("    ← {src} ({}{conf})", kind_word(r.kind))
}

fn render_files_only(out: &mut Out, res: &SearchOutcome, classes: &[Class]) {
    let mut per: Vec<(String, BTreeMap<Class, usize>)> = Vec::new();
    let mut pos: HashMap<String, usize> = HashMap::new();
    for (h, c) in res.hits.iter().zip(classes) {
        let i = *pos.entry(h.display.clone()).or_insert_with(|| {
            per.push((h.display.clone(), BTreeMap::new()));
            per.len() - 1
        });
        *per[i].1.entry(*c).or_default() += 1;
    }
    per.sort_by_key(|(d, m)| {
        let graph = m.get(&Class::Definition).copied().unwrap_or(0)
            + m.get(&Class::Reference).copied().unwrap_or(0);
        (graph == 0, d.clone())
    });
    let cap = if out.spec.all { usize::MAX } else { CAP_FILES };
    for (d, m) in per.iter().take(cap) {
        let parts: Vec<String> = m
            .iter()
            .map(|(c, n)| format!("{n} {}", c.key().replace('_', " ")))
            .collect();
        out.line(&format!("{d}    ({})", parts.join(", ")));
    }
    if per.len() > cap {
        let hint = more_hint(out.spec);
        out.line(&format!("+{} more files{hint}", per.len() - cap));
    }
}

/// Non-identifier patterns: hits in grep format, grouped under their enclosing symbol.
fn render_grouped(out: &mut Out, engine: &Engine, spec: &SearchSpec, res: &SearchOutcome) {
    if spec.files_only {
        let mut seen = HashSet::new();
        let files: Vec<&str> = res
            .hits
            .iter()
            .filter(|h| seen.insert(h.display.as_str()))
            .map(|h| h.display.as_str())
            .collect();
        let cap = if spec.all { usize::MAX } else { CAP_FILES * 2 };
        for d in files.iter().take(cap) {
            out.line(d);
        }
        if files.len() > cap {
            out.line(&format!(
                "+{} more files{}",
                files.len() - cap,
                more_hint(spec)
            ));
        }
        return;
    }
    let cap = if spec.all { usize::MAX } else { CAP_GROUPED };
    let mut syms_cache: HashMap<String, Vec<Symbol>> = HashMap::new();
    let mut last_group: Option<(PathBuf, Option<SymbolId>)> = None;
    for h in res.hits.iter().take(cap) {
        let group = engine
            .paths
            .relative(&h.abs)
            .filter(|r| engine.is_indexable(r))
            .and_then(|r| {
                let syms = syms_cache
                    .entry(r.clone())
                    .or_insert_with(|| engine.store.symbols_in_file(&r).unwrap_or_default());
                syms.iter()
                    .filter(|s| s.start_line <= h.line && h.line <= s.end_line)
                    .min_by_key(|s| (s.end_line - s.start_line, s.kind == SymbolKind::Module))
                    .cloned()
            });
        let key = (h.abs.clone(), group.as_ref().map(|s| s.id));
        if last_group.as_ref() != Some(&key) {
            if let Some(s) = &group {
                out.line(&format!(
                    "# {} ({} L{}-{})",
                    short_label(s),
                    s.kind.as_str(),
                    s.start_line,
                    s.end_line
                ));
            }
            last_group = Some(key);
        }
        out.hit(&h.display, &h.abs, h.line, &h.text, "");
    }
    if res.hits.len() > cap {
        out.line(&format!(
            "+{} more matches{}",
            res.hits.len() - cap,
            more_hint(spec)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(p: &str, fixed: bool) -> SearchSpec {
        SearchSpec {
            patterns: vec![p.into()],
            fixed,
            ..Default::default()
        }
    }

    #[test]
    fn identifier_patterns() {
        assert_eq!(
            identifier_of(&spec("resolve_owner", false)).as_deref(),
            Some("resolve_owner")
        );
        assert_eq!(
            identifier_of(&spec("\\bload_yaml\\b", false)).as_deref(),
            Some("load_yaml")
        );
        assert_eq!(
            identifier_of(&spec("core.project.resolve_owner", false)).as_deref(),
            Some("resolve_owner")
        );
        assert_eq!(identifier_of(&spec("TODO|FIXME", false)), None);
        assert_eq!(identifier_of(&spec("def resolve", false)), None);
        assert_eq!(identifier_of(&spec("a-b", true)), None);
    }

    #[test]
    fn definition_and_token() {
        assert!(is_definition_line("    async def run_task(x):", "run_task"));
        assert!(!is_definition_line("def run_task_later():", "run_task"));
        assert!(is_definition_line("class Foo(Base):", "Foo"));
        assert_eq!(token_at("x = _resolve_owners(a)", 5), "_resolve_owners");
    }
}
