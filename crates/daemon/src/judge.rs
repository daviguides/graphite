//! Residue judgment: which search hits the graph explains, rendered as a compact grep-shaped answer.
//! Every hit is either printed or counted in an explicit "+N more" line; nothing is dropped silently.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use graphite_model::{EdgeKind, Symbol, SymbolId, SymbolKind};
use graphite_store::{confidence, Confidence, GraphStore, Outcome};
use serde_json::{json, Value};

use crate::answer::{
    grep_output_bytes, grep_output_lines, rank, Answer, Budget, Item, LineFilter, NameVerdict,
    OutFormat, Target,
};
use crate::engine::Engine;
use crate::search::{display_path, Hit, SearchOutcome, SearchSpec};

const MAX_NAME_SYMBOLS: usize = 200;

/// Why a matching line is there.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub enum Class {
    Definition,
    Reference,
    Import,
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
            Class::Import => "import",
            Class::GraphGap => "graph_gap",
            Class::CodeUntracked => "code_untracked",
            Class::StringOrComment => "string_or_comment",
            Class::OtherLanguage => "other_language",
            Class::NotIndexed => "not_indexed",
            Class::DocsConfig => "docs_config",
            Class::OtherIdentifier => "other_identifier",
        }
    }
}

const CODE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "go", "java", "kt", "swift", "c", "h", "cc",
    "cpp", "hpp", "rb", "php", "sh", "bash", "zsh", "lua", "scala", "cs", "sql", "pyx", "pyi",
];

const CONFIG_EXTS: &[&str] = &[
    "toml",
    "ini",
    "cfg",
    "conf",
    "yaml",
    "yml",
    "json",
    "jsonc",
    "properties",
    "env",
];

/// Class of a non-code match by where it lives: CI definition, tool/dot config, or docs.
pub(crate) fn non_code_class(rel: &str) -> &'static str {
    let lower = rel.to_ascii_lowercase();
    let file = lower.rsplit('/').next().unwrap_or(&lower);
    let ci = lower.starts_with(".github/workflows/")
        || lower.contains("/.github/workflows/")
        || lower.starts_with(".circleci/")
        || lower.starts_with(".buildkite/")
        || file == ".gitlab-ci.yml"
        || file == ".travis.yml"
        || file == "azure-pipelines.yml"
        || file == "jenkinsfile";
    if ci {
        return "ci";
    }
    let hidden = lower.split('/').any(|c| c.starts_with('.') && c.len() > 1);
    let ext = file.rsplit_once('.').map(|x| x.1).unwrap_or("");
    if hidden || CONFIG_EXTS.contains(&ext) {
        "config"
    } else {
        "docs"
    }
}

/// Most identifiers judged in one alternation (`a\|b\|…`); more is searched as plain text.
pub const MAX_NAMES: usize = 6;
/// Definitions whose callers/tests are computed per answer, shared across the names.
const TARGET_FACTS_BUDGET: usize = 12;

/// `\b`-trimmed, `\.`-unescaped identifier (optionally dotted): (last segment, dotted form).
fn identifier_alt(p: &str, fixed: bool) -> Option<(String, String)> {
    let mut p = p;
    if !fixed {
        p = p.strip_prefix("\\b").unwrap_or(p);
        p = p.strip_suffix("\\b").unwrap_or(p);
    }
    let core = if fixed {
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
    ok.then(|| {
        let name = core.rsplit('.').next().unwrap_or(&core).to_string();
        (name, core)
    })
}

/// Top-level alternatives of a regex (`a|b`); None when `|` sits inside a group or class.
fn alternatives(p: &str) -> Option<Vec<&str>> {
    let b = p.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'(' | b')' | b'[' | b']' => return None,
            b'|' => {
                out.push(&p[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(&p[start..]);
    Some(out)
}

/// Identifiers a search looks for when every alternative (`a\|b`, `a|b`, `-e a -e b`) is one:
/// `(name, dotted)` pairs, deduplicated, at most `MAX_NAMES`. None for any other pattern.
pub fn identifiers_of(spec: &SearchSpec) -> Option<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = Vec::new();
    for p in &spec.patterns {
        let alts = if spec.fixed {
            vec![p.as_str()]
        } else {
            alternatives(p)?
        };
        for alt in alts {
            let id = identifier_alt(alt, spec.fixed)?;
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    (!out.is_empty() && out.len() <= MAX_NAMES).then_some(out)
}

/// Name to look up when the pattern is a plain identifier (optionally dotted, optionally `\b`-wrapped).
pub fn identifier_of(spec: &SearchSpec) -> Option<String> {
    match identifiers_of(spec)?.as_slice() {
        [(name, _)] => Some(name.clone()),
        _ => None,
    }
}

struct RefInfo {
    src: SymbolId,
    dst: SymbolId,
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
    test_srcs: HashSet<SymbolId>,
}

impl NameFacts {
    pub(crate) fn ref_count(&self) -> usize {
        self.refs.len()
    }

    pub(crate) fn gap_count(&self) -> usize {
        self.gaps.len()
    }

    fn syms_known_test(&self, id: SymbolId) -> bool {
        self.test_srcs.contains(&id)
    }

    pub(crate) fn ref_paths(&self) -> impl Iterator<Item = &str> {
        self.refs.keys().map(|(p, _)| p.as_str())
    }
}

pub(crate) fn short_label(s: &Symbol) -> String {
    let mut parts = s.qualified.rsplit('.');
    let name = parts.next().unwrap_or(&s.name);
    match (s.kind, parts.next()) {
        (SymbolKind::Method, Some(class)) => format!("{class}.{name}"),
        _ => name.to_string(),
    }
}

pub(crate) fn name_facts(engine: &Engine, name: &str, dotted: &str) -> Result<NameFacts, String> {
    let store = &engine.store;
    let mut syms = store.symbols_by_name(name).map_err(|e| e.to_string())?;
    if dotted.contains('.') {
        syms.retain(|s| s.qualified.ends_with(dotted));
    }
    syms.truncate(MAX_NAME_SYMBOLS);
    // Call-site lines come from the in-memory adjacency; a site's file is its source symbol's file.
    let mut sites = Vec::new();
    {
        let adj = engine.adjacency();
        for s in &syms {
            for site in adj.sites_into(s.id) {
                // Containment is structure (module → its own definition), not a use.
                if site.kind != EdgeKind::Contains {
                    sites.push((s.id, site));
                }
            }
        }
    }
    let mut ids: Vec<SymbolId> = sites.iter().map(|(_, site)| site.src).collect();
    ids.extend(syms.iter().map(|s| s.id));
    ids.sort();
    ids.dedup();
    let known: HashMap<SymbolId, Symbol> = store
        .symbols(&ids)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|s| (s.id, s))
        .collect();
    let mut refs = HashMap::new();
    for (dst, site) in sites {
        let Some(src) = known.get(&site.src) else {
            continue;
        };
        refs.insert(
            (src.path.clone(), site.line),
            RefInfo {
                src: site.src,
                dst,
                kind: site.kind,
                conf: confidence(site.provenance),
            },
        );
    }
    let mut gaps = HashMap::new();
    for r in store.name_gaps(name).map_err(|e| e.to_string())? {
        let why = match r.outcome {
            Outcome::Ambiguous { candidates } => format!("ambiguous: {candidates} candidates"),
            _ => "unresolved".to_string(),
        };
        gaps.insert((r.key.path.clone(), r.site_line), why);
    }
    let labels = known.values().map(|s| (s.id, short_label(s))).collect();
    let test_srcs = known.values().filter(|s| s.is_test).map(|s| s.id).collect();
    Ok(NameFacts {
        name: name.to_string(),
        syms,
        refs,
        gaps,
        labels,
        test_srcs,
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

/// A name on a continuation line of a parenthesized `from x import (` whose import edge the graph resolved.
fn in_import_block(f: &NameFacts, rel: &str, abs: &Path, line: u32, files: &mut Files) -> bool {
    let starts: Vec<u32> = f
        .refs
        .iter()
        .filter(|((p, l), r)| p == rel && *l < line && r.kind == EdgeKind::Imports)
        .map(|((_, l), _)| *l)
        .collect();
    let Some(&start) = starts.iter().max() else {
        return false;
    };
    let Some(lines) = files.lines(abs) else {
        return false;
    };
    let block: String = lines
        .iter()
        .skip(start.saturating_sub(1) as usize)
        .take((line - start) as usize)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    block.contains('(') && !block.contains(')')
}

fn classify(engine: &Engine, facts: Option<&NameFacts>, hit: &Hit, files: &mut Files) -> Class {
    let rel = engine.paths.relative(&hit.abs);
    if let (Some(f), Some(rel)) = (facts, rel.as_deref()) {
        let key = (rel.to_string(), hit.line);
        if let Some(r) = f.refs.get(&key) {
            return if r.kind == EdgeKind::Imports {
                Class::Import
            } else {
                Class::Reference
            };
        }
        if in_import_block(f, rel, &hit.abs, hit.line, files) {
            return Class::Import;
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

/// The agent's own search, reduced to a grep it can re-run on a narrower path ("see more" hints
/// stay in the vocabulary the agent believes it used).
fn hint_base(spec: &SearchSpec) -> String {
    let Some(p) = spec.patterns.first() else {
        return String::new();
    };
    let mut flags = String::from("-rn");
    if spec.ignore_case {
        flags.push('i');
    }
    if spec.word {
        flags.push('w');
    }
    if spec.fixed {
        flags.push('F');
    } else if p.contains('|') || p.contains('+') || p.contains('?') {
        flags.push('E');
    }
    let quoted = if p
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '.')
    {
        p.clone()
    } else {
        format!("'{}'", p.replace('\'', "'\\''"))
    };
    format!("grep {flags} {quoted}")
}

fn secrets_note(res: &SearchOutcome) -> Option<String> {
    (res.secrets_skipped > 0)
        .then(|| "secret-looking files skipped unread (.env, keys, credentials)".to_string())
}

/// Per-caller facts from the query layer, keyed by (caller path, site line).
#[derive(Default)]
struct TargetFacts {
    target: Target,
    called_by: HashMap<(String, u32), (Vec<String>, u32)>,
    indirect_total: u32,
    indirect: Vec<crate::answer::IndirectGroup>,
    tests_total: u32,
    tests: Vec<String>,
    overrides: Vec<String>,
}

fn target_facts(engine: &Engine, stale: bool, sym: &Symbol, cwd: &Path) -> TargetFacts {
    let abs = engine.paths.root.join(&sym.path);
    let mut out = TargetFacts {
        target: Target {
            def: format!("{}:{}", display_path(&abs, cwd), sym.start_line),
            signature: sym.signature.clone(),
            ..Default::default()
        },
        ..Default::default()
    };
    let opts = graphite_query::Options {
        compact: true,
        source_items: 0,
        token_budget: 1_000_000,
        ..Default::default()
    };
    let v = {
        let adj = engine.adjacency();
        let ctx = graphite_query::QueryContext {
            store: &engine.store,
            adj: &adj,
            root: &engine.paths.root,
            stale,
            parse_failures: engine.parse_failures(),
        };
        match graphite_query::blast_radius(&ctx, &sym.id.to_hex(), &opts)
            .ok()
            .and_then(|e| serde_json::to_value(e).ok())
        {
            Some(v) => v,
            None => return out,
        }
    };
    let r = &v["result"];
    let u = |x: &Value| x.as_u64().unwrap_or(0) as u32;
    let d = &r["direct"];
    out.target.callers = u(&d["callers"]);
    out.target.sites = u(&d["sites"]);
    out.target.files = u(&d["files"]);
    out.target.prod = u(&d["prod"]);
    out.target.test = u(&d["test"]);
    out.target.risk = r["risk"]["level"].as_str().map(String::from);
    for cs in r["call_sites"].as_array().into_iter().flatten() {
        let path = cs["caller"]["path"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        // Caller chains are production-only; tests live in the covering-tests list.
        let by: Vec<String> = cs["called_by_prod"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
        let total = u(&cs["called_by_prod_total"]);
        for l in cs["lines"].as_array().into_iter().flatten() {
            out.called_by
                .insert((path.clone(), u(l)), (by.clone(), total));
        }
    }
    let ind = &r["indirect"];
    out.indirect_total = u(&ind["total"]);
    for g in ind["groups"].as_array().into_iter().flatten().take(5) {
        out.indirect.push(crate::answer::IndirectGroup {
            module: g["module"].as_str().unwrap_or("?").to_string(),
            count: u(&g["count"]),
            top: g["top"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .take(3)
                .map(String::from)
                .collect(),
        });
    }
    out.tests_total = u(&r["tests"]["total"]);
    out.tests = r["tests"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["node_id"].as_str().map(String::from))
        .take(crate::answer::MAX_TESTS_LISTED)
        .collect();
    for o in r["overrides"].as_array().into_iter().flatten() {
        let s = &o["symbol"];
        let name: Vec<&str> = s["qualified"]
            .as_str()
            .unwrap_or_default()
            .rsplit('.')
            .take(2)
            .collect();
        out.overrides.push(format!(
            "{} {} at {}:{}",
            o["relation"].as_str().unwrap_or("related"),
            name.into_iter().rev().collect::<Vec<_>>().join("."),
            display_path(
                &engine
                    .paths
                    .root
                    .join(s["path"].as_str().unwrap_or_default()),
                cwd
            ),
            u(&s["start_line"])
        ));
    }
    out
}

type Context = Vec<(u32, String)>;

fn context(files: &mut Files, spec: &SearchSpec, abs: &Path, line: u32) -> (Context, Context) {
    let (b, a) = (spec.before, spec.after);
    if b == 0 && a == 0 {
        return (Vec::new(), Vec::new());
    }
    let before = (line.saturating_sub(b).max(1)..line)
        .map(|n| (n, files.line(abs, n)))
        .collect();
    let mut after = Vec::new();
    for n in line + 1..=line + a {
        let Some(t) = files
            .lines(abs)
            .and_then(|l| l.get(n as usize - 1))
            .cloned()
        else {
            break;
        };
        after.push((n, t));
    }
    (before, after)
}

/// A test-code line that patches/mocks `name`: a mock API on the line, or a dotted-path string
/// literal ending in the name (the target of a multi-line `patch(` / `patch.dict`).
fn looks_like_mock(text: &str, name: &str) -> bool {
    let t = text.to_ascii_lowercase();
    ["patch", "mock", "monkeypatch", "setattr"]
        .iter()
        .any(|k| t.contains(k))
        || [format!(".{name}\""), format!(".{name}'")]
            .iter()
            .any(|p| text.contains(p.as_str()))
}

fn conf_word(c: Confidence) -> Option<String> {
    match c {
        Confidence::Extracted => None,
        Confidence::Inferred => Some("inferred".into()),
        Confidence::Ambiguous => Some("name guess".into()),
    }
}

/// Judge a search against the graph: the canonical record plus stats for logging.
pub fn build(
    engine: &Engine,
    spec: &SearchSpec,
    res: &SearchOutcome,
    stale: bool,
) -> Result<(Answer, Value), String> {
    let cwd = PathBuf::from(&spec.cwd);
    let facts: Vec<NameFacts> = identifiers_of(spec)
        .unwrap_or_default()
        .iter()
        .map(|(name, dotted)| name_facts(engine, name, dotted))
        .collect::<Result<_, _>>()?;
    let mut files = Files::default();
    let pick: Vec<usize> = res.hits.iter().map(|h| fact_for(&facts, h)).collect();
    let classes: Vec<Class> = res
        .hits
        .iter()
        .zip(&pick)
        .map(|(h, &k)| classify(engine, facts.get(k), h, &mut files))
        .collect();
    let mut counts: BTreeMap<Class, usize> = BTreeMap::new();
    for c in &classes {
        *counts.entry(*c).or_default() += 1;
    }
    let nfiles: HashSet<&Path> = res.hits.iter().map(|h| h.abs.as_path()).collect();

    let mut a = Answer {
        query: if spec.label.is_empty() {
            spec.patterns.join(" | ")
        } else {
            spec.label.clone()
        },
        matches: res.hits.len(),
        files: nfiles.len(),
        name: match facts.as_slice() {
            [f] => Some(f.name.clone()),
            _ => None,
        },
        all: spec.all,
        hint_base: hint_base(spec),
        budget: spec.budget.clone(),
        ..Default::default()
    };
    if stale {
        a.omitted.push(
            "graph may lag your latest edit (indexing in progress); text matches are current"
                .into(),
        );
    }
    if res.truncated {
        a.omitted.push(format!(
            "search stopped at {} matches — narrow the pattern or path",
            crate::search::MAX_MATCHES
        ));
    }
    if let Some(n) = secrets_note(res) {
        a.omitted.push(n);
    }

    let mut alias_refs = 0usize;
    if spec.files_only {
        a.mode = "files".into();
        a.verdict = "none".into();
        files_items(&mut a, res, &classes);
    } else if !facts.is_empty() {
        a.mode = "identifier".into();
        let judged = Judged {
            classes: &classes,
            facts: &facts,
            pick: &pick,
        };
        identifier_items(
            &mut a,
            engine,
            spec,
            res,
            &judged,
            &cwd,
            stale,
            &mut files,
            &mut alias_refs,
        );
    } else {
        a.mode = "grouped".into();
        a.verdict = "none".into();
        a.verdict_note =
            "non-identifier pattern; each match tagged with its enclosing symbol".into();
        grouped_items(&mut a, engine, spec, res, &mut files);
    }
    a.items.sort_by_key(|i| rank(&i.class, i.test));
    apply_pipeline(&mut a, spec);
    // What the agent's own command would have printed (after its own filters): grep names files
    // only when it searched more than one.
    let single_file = spec.paths.len() == 1 && Path::new(&spec.paths[0]).is_file();
    let raw_bytes = grep_output_bytes(&a, !single_file);
    if !spec.all && spec.format == OutFormat::Model {
        cap_to_grep_output(&mut a, raw_bytes);
    }

    let mut class_counts = serde_json::Map::new();
    for (c, n) in &counts {
        class_counts.insert(c.key().to_string(), json!(n));
    }
    let stats = json!({
        "graph_verdict": if a.verdict.is_empty() { "none" } else { a.verdict.as_str() },
        "mode": a.mode,
        "matches": res.hits.len(),
        "files": nfiles.len(),
        "classes": class_counts,
        "alias_refs": alias_refs,
        "secrets_skipped": res.secrets_skipped,
        "raw_bytes": raw_bytes,
        "budget_bytes": a.budget.as_ref().and_then(|b| b.bytes),
        "names": a.names.iter().map(|n| json!({"name": n.name, "verdict": n.verdict})).collect::<Vec<_>>(),
    });
    Ok((a, stats))
}

/// Never cost more than the agent's own command: the plain output (the first N lines of it under
/// `| head -N`) plus a bounded overhead. Binding cap → the default-budget escape hint.
fn cap_to_grep_output(a: &mut Answer, grep_bytes: usize) {
    let grep_lines = grep_output_lines(a);
    // References only the graph found (aliased calls) are what the answer adds: room for them.
    let graph_only: usize = a
        .items
        .iter()
        .filter(|i| i.class == "graph_only")
        .map(|i| crate::answer::item_line(i).len() + 1)
        .sum();
    let share = graph_only
        + match a.budget.as_ref().and_then(|b| b.lines) {
            Some(n) if grep_lines > 0 => grep_bytes * n.min(grep_lines) / grep_lines,
            _ => grep_bytes,
        };
    let cap = Budget::implicit(share);
    match &mut a.budget {
        None => a.budget = Some(cap),
        Some(b) if b.bytes.is_none_or(|x| x > cap.bytes.unwrap_or(usize::MAX)) => {
            b.bytes = cap.bytes;
            b.implicit = true;
        }
        Some(_) => {}
    }
}

/// Model-format text + stats (the record is `build`'s first value).
pub fn render(
    engine: &Engine,
    spec: &SearchSpec,
    res: &SearchOutcome,
    stale: bool,
) -> Result<(String, Value), String> {
    let (a, mut stats) = build(engine, spec, res, stale)?;
    let text = crate::answer::render(&a, spec.format);
    stats["out_bytes"] = json!(text.len());
    Ok((text, stats))
}

/// Which name of an alternation a hit is about: the identifier at the match, else the
/// alternative the match starts with (a different identifier containing it), else the first.
fn fact_for(facts: &[NameFacts], hit: &Hit) -> usize {
    if facts.len() < 2 {
        return 0;
    }
    let token = token_at(&hit.text, hit.col);
    let at = hit.text.get(hit.col..).unwrap_or_default();
    facts
        .iter()
        .position(|f| f.name == token)
        .or_else(|| facts.iter().position(|f| at.starts_with(f.name.as_str())))
        .unwrap_or(0)
}

/// Per-hit judgment of an identifier search: class and the name it is about.
struct Judged<'a> {
    classes: &'a [Class],
    facts: &'a [NameFacts],
    pick: &'a [usize],
}

/// Verdict of one name: (verdict, note for the single-name header, short note for alternations).
fn name_verdict(f: &NameFacts) -> (&'static str, String, String) {
    let gaps = f.gaps.len();
    if f.syms.is_empty() {
        let note = format!("no symbol named `{}` in the index", f.name);
        ("none", note, "not in the index".into())
    } else if gaps == 0 {
        (
            "complete",
            format!(
                "every reference the graph resolved to `{}` is listed; 0 unresolved or ambiguous calls with that name",
                f.name
            ),
            String::new(),
        )
    } else {
        (
            "lower_bound",
            format!(
                "{gaps} calls named `{}` unresolved/ambiguous — tagged [unresolved call] where they matched",
                f.name
            ),
            format!("{gaps} unresolved"),
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn identifier_items(
    a: &mut Answer,
    engine: &Engine,
    spec: &SearchSpec,
    res: &SearchOutcome,
    judged: &Judged,
    cwd: &Path,
    stale: bool,
    files: &mut Files,
    alias_refs: &mut usize,
) {
    let facts = judged.facts;
    if let [f] = facts {
        let (verdict, note, _) = name_verdict(f);
        a.verdict = verdict.into();
        a.verdict_note = note;
    } else {
        let verdicts: Vec<&str> = facts.iter().map(|f| name_verdict(f).0).collect();
        a.verdict = if verdicts.iter().all(|v| *v == "complete") {
            "complete"
        } else if verdicts.iter().all(|v| *v == "none") {
            "none"
        } else {
            "lower_bound"
        }
        .into();
        a.verdict_note = facts
            .iter()
            .zip(&verdicts)
            .filter(|(_, v)| **v != "complete")
            .map(|(f, _)| format!("`{}` {}", f.name, name_verdict(f).2))
            .collect::<Vec<_>>()
            .join(", ");
    }

    // Query-layer facts per definition (bounded: common names can have many).
    let per_name = (TARGET_FACTS_BUDGET / facts.len()).max(1);
    let mut called_by: HashMap<(String, u32), (Vec<String>, u32)> = HashMap::new();
    let mut def_display: HashMap<SymbolId, String> = HashMap::new();
    for f in facts {
        let mut nv = NameVerdict {
            name: f.name.clone(),
            verdict: name_verdict(f).0.into(),
            note: name_verdict(f).2,
            ..Default::default()
        };
        for sym in f.syms.iter().take(per_name) {
            let tf = target_facts(engine, stale, sym, cwd);
            def_display.insert(sym.id, tf.target.def.clone());
            called_by.extend(tf.called_by);
            a.footer.indirect_total += tf.indirect_total;
            a.footer.indirect.extend(tf.indirect);
            a.footer.tests_total += tf.tests_total;
            nv.tests_total += tf.tests_total;
            for t in tf.tests {
                if a.footer.tests.len() < crate::answer::MAX_TESTS_LISTED {
                    a.footer.tests.push(t);
                }
            }
            a.footer.overrides.extend(tf.overrides);
            nv.targets.push(tf.target.clone());
            a.targets.push(tf.target);
        }
        for sym in f.syms.iter().skip(per_name) {
            let abs = engine.paths.root.join(&sym.path);
            def_display.insert(
                sym.id,
                format!("{}:{}", display_path(&abs, cwd), sym.start_line),
            );
        }
        if facts.len() > 1 {
            a.names.push(nv);
        }
    }

    let enrich = |it: &mut Item, f: &NameFacts, rel: &str, r: &RefInfo| {
        it.in_fn = f.labels.get(&r.src).cloned();
        if let Some((by, total)) = called_by.get(&(rel.to_string(), it.line)) {
            it.called_by = by.clone();
            it.called_by_total = *total;
        }
        it.confidence = conf_word(r.conf);
        if f.syms.len() > 1 {
            it.resolves_to = def_display.get(&r.dst).cloned();
        }
        it.why = format!(
            "{} edge from {} resolved by the graph ({})",
            kind_word(r.kind),
            f.labels.get(&r.src).cloned().unwrap_or_default(),
            match r.conf {
                Confidence::Extracted => "extracted",
                Confidence::Inferred => "inferred",
                Confidence::Ambiguous => "unique-name guess",
            }
        );
    };

    let mut seen: HashSet<(String, u32)> = HashSet::new();
    let mut import_files: HashSet<String> = HashSet::new();
    for (i, h) in res.hits.iter().enumerate() {
        let f = &facts[judged.pick[i]];
        let rel = engine.paths.relative(&h.abs).unwrap_or_default();
        let is_test = graphite_extract_python::is_test_path(&rel);
        let (before, after) = context(files, spec, &h.abs, h.line);
        let mut it = Item {
            path: h.display.clone(),
            line: h.line,
            text: h.text.clone(),
            test: is_test,
            before,
            after,
            ..Default::default()
        };
        let key = (rel.clone(), h.line);
        match judged.classes[i] {
            Class::Definition => {
                it.class = "definition".into();
                it.test = false;
                it.why = "`def`/`class` line of an indexed symbol with this name".into();
            }
            Class::Reference => {
                seen.insert(key.clone());
                it.class = "call".into();
                if let Some(r) = f.refs.get(&key) {
                    it.test = is_test || f.syms_known_test(r.src);
                    enrich(&mut it, f, &rel, r);
                }
            }
            Class::Import => {
                seen.insert(key.clone());
                it.class = "import".into();
                import_files.insert(rel.clone());
                it.why =
                    "import line (edge resolved, or continuation of a resolved `from … import (`)"
                        .into();
            }
            Class::GraphGap => {
                it.class = "unresolved_call".into();
                it.gap = f.gaps.get(&key).cloned();
                it.why = format!(
                    "call named `{}` the graph could not resolve to one target ({})",
                    f.name,
                    it.gap.clone().unwrap_or_default()
                );
            }
            Class::StringOrComment => {
                if is_test && looks_like_mock(&h.text, &f.name) {
                    it.class = "mock_in_test".into();
                    it.why = "string in test code patching/mocking this name".into();
                } else {
                    it.class = "string_comment".into();
                    it.why = "inside a string or comment (tree-sitter)".into();
                }
            }
            Class::CodeUntracked => {
                it.class = if is_test && looks_like_mock(&h.text, &f.name) {
                    "mock_in_test"
                } else {
                    "untracked_code"
                }
                .into();
                it.why =
                    "code use the graph does not track as a reference (value/attribute use)".into();
            }
            Class::OtherLanguage => {
                it.class = "other_language".into();
                it.why = "source language the graph does not index yet".into();
            }
            Class::NotIndexed => {
                it.class = "not_indexed".into();
                it.why = "Python file outside the index (ignored or unindexable path)".into();
            }
            Class::DocsConfig => {
                let c = non_code_class(&rel);
                it.class = c.into();
                it.why = match c {
                    "ci" => "CI definition (non-code file)",
                    "config" => "tool/dot configuration (non-code file)",
                    _ => "documentation or other non-code file",
                }
                .into();
            }
            Class::OtherIdentifier => {
                it.class = "other_identifier".into();
                it.why = format!(
                    "matched inside `{}`, a different identifier",
                    token_at(&h.text, h.col)
                );
            }
        }
        a.items.push(it);
    }
    a.footer.import_lines = a.items.iter().filter(|i| i.class == "import").count();
    a.footer.import_files = import_files.len();

    // References the text search could not see (aliased import, renamed call).
    let mut hidden: Vec<(&NameFacts, &(String, u32), &RefInfo)> = facts
        .iter()
        .flat_map(|f| f.refs.iter().map(move |(k, r)| (f, k, r)))
        .filter(|(_, k, r)| {
            r.kind != EdgeKind::Imports
                && !seen.contains(*k)
                && res.searched.contains(&engine.paths.root.join(&k.0))
        })
        .collect();
    hidden.sort_by(|x, y| x.1.cmp(y.1));
    hidden.dedup_by(|x, y| x.1 == y.1);
    *alias_refs = hidden.len();
    for (f, (path, line), r) in hidden {
        let abs = engine.paths.root.join(path);
        let (before, after) = context(files, spec, &abs, *line);
        let mut it = Item {
            path: display_path(&abs, cwd),
            line: *line,
            text: files.line(&abs, *line),
            class: "graph_only".into(),
            test: graphite_extract_python::is_test_path(path) || f.syms_known_test(r.src),
            before,
            after,
            ..Default::default()
        };
        enrich(&mut it, f, path, r);
        it.why = format!(
            "{} — the text search cannot see it (aliased import or renamed call)",
            it.why
        );
        a.items.push(it);
    }
}

fn files_items(a: &mut Answer, res: &SearchOutcome, classes: &[Class]) {
    let mut per: Vec<(String, BTreeMap<Class, usize>)> = Vec::new();
    let mut pos: HashMap<String, usize> = HashMap::new();
    for (h, c) in res.hits.iter().zip(classes) {
        let i = *pos.entry(h.display.clone()).or_insert_with(|| {
            per.push((h.display.clone(), BTreeMap::new()));
            per.len() - 1
        });
        *per[i].1.entry(*c).or_default() += 1;
    }
    for (d, m) in per {
        let graph = m.get(&Class::Definition).copied().unwrap_or(0)
            + m.get(&Class::Reference).copied().unwrap_or(0);
        let parts: Vec<String> = m
            .iter()
            .map(|(c, n)| format!("{n} {}", c.key().replace('_', " ")))
            .collect();
        a.items.push(Item {
            path: d,
            line: 0,
            text: parts.join(", "),
            class: if graph > 0 { "call" } else { "match" }.into(),
            why: "file with matches (-l)".into(),
            ..Default::default()
        });
    }
}

fn grouped_items(
    a: &mut Answer,
    engine: &Engine,
    spec: &SearchSpec,
    res: &SearchOutcome,
    files: &mut Files,
) {
    let mut syms_cache: HashMap<String, Vec<Symbol>> = HashMap::new();
    for h in &res.hits {
        let rel = engine.paths.relative(&h.abs);
        let group = rel
            .as_ref()
            .filter(|r| engine.is_indexable(r))
            .and_then(|r| {
                let syms = syms_cache
                    .entry(r.clone())
                    .or_insert_with(|| engine.store.symbols_in_file(r).unwrap_or_default());
                syms.iter()
                    .filter(|s| s.start_line <= h.line && h.line <= s.end_line)
                    .min_by_key(|s| (s.end_line - s.start_line, s.kind == SymbolKind::Module))
                    .cloned()
            });
        let (before, after) = context(files, spec, &h.abs, h.line);
        let test = rel
            .as_deref()
            .is_some_and(graphite_extract_python::is_test_path);
        let ext = ext_of(&h.abs);
        let non_code = ext != "py" && !CODE_EXTS.contains(&ext.as_str());
        let class = match (&rel, non_code) {
            (Some(r), true) => non_code_class(r),
            _ => "match",
        };
        a.items.push(Item {
            path: h.display.clone(),
            line: h.line,
            text: h.text.clone(),
            class: class.into(),
            in_fn: group.as_ref().map(short_label),
            test,
            why: match &group {
                Some(s) => format!(
                    "text match inside {} ({} L{}-{})",
                    short_label(s),
                    s.kind.as_str(),
                    s.start_line,
                    s.end_line
                ),
                None => "text match outside any indexed symbol".into(),
            },
            before,
            after,
            ..Default::default()
        });
    }
}

fn filter_regex(f: &LineFilter) -> Option<regex::Regex> {
    let mut p = if f.fixed {
        regex::escape(&f.pattern)
    } else {
        f.pattern.clone()
    };
    if f.word {
        p = format!(r"\b(?:{p})\b");
    }
    regex::RegexBuilder::new(&p)
        .case_insensitive(f.ignore_case)
        .build()
        .ok()
}

/// Honor the agent's pipeline: test filter → semantic, grep filters → match lines only.
fn apply_pipeline(a: &mut Answer, spec: &SearchSpec) {
    if let Some(src) = &spec.drop_tests {
        let before = a.items.len();
        let mocks = a.items.iter().filter(|i| i.class == "mock_in_test").count();
        a.items.retain(|i| !i.test && i.class != "mock_in_test");
        let dropped = before - a.items.len();
        // Prod-only view: test functions out of caller chains too; test totals stay as counts.
        let is_test_name = |n: &str| {
            n.split('.')
                .any(|seg| seg.starts_with("test_") || seg.starts_with("Test"))
        };
        for it in &mut a.items {
            let keep: Vec<String> = it
                .called_by
                .iter()
                .filter(|n| !is_test_name(n))
                .cloned()
                .collect();
            let gone = (it.called_by.len() - keep.len()) as u32;
            it.called_by = keep;
            it.called_by_total = it.called_by_total.saturating_sub(gone);
        }
        a.footer.tests.clear();
        a.notices.push(format!(
            "`{src}` → {dropped} test matches ({mocks} mocks) omitted (you filtered tests)"
        ));
    }
    for f in &spec.line_filters {
        let Some(re) = filter_regex(f) else { continue };
        let before = a.items.len();
        a.items
            .retain(|i| re.is_match(&crate::answer::item_line(i)) != f.invert);
        a.notices.push(format!(
            "`{}` applied to match lines ({} of {before} kept; header/footer kept)",
            f.source,
            a.items.len()
        ));
    }
    if let Some(b) = &spec.budget {
        let what = match (b.bytes, b.lines) {
            (Some(n), _) => format!("{n} bytes"),
            (_, Some(n)) => format!("{n} lines"),
            _ => "budget".into(),
        };
        a.notices.push(format!(
            "`{}` applied as answer budget ({what}); nothing cut mid-line",
            b.source
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
    fn non_code_classes_by_location() {
        assert_eq!(non_code_class(".github/workflows/ci.yml"), "ci");
        assert_eq!(non_code_class("site/.github/workflows/deploy.yml"), "ci");
        assert_eq!(non_code_class(".gitlab-ci.yml"), "ci");
        assert_eq!(non_code_class(".pre-commit-config.yaml"), "config");
        assert_eq!(non_code_class(".agents/notes.md"), "config");
        assert_eq!(non_code_class("pyproject.toml"), "config");
        assert_eq!(non_code_class("docs/owner.md"), "docs");
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

    // kinhin: decision(ref="docs/foundation/interception.md#2-the-answer-an-enriched-grep")
    #[test]
    fn alternations_of_identifiers_are_judged_per_name() {
        let names = |p: &str, fixed: bool| -> Option<Vec<String>> {
            identifiers_of(&spec(p, fixed)).map(|v| v.into_iter().map(|(n, _)| n).collect())
        };
        let two = |a: &str, b: &str| Some(vec![a.to_string(), b.to_string()]);
        assert_eq!(
            names("_pin_model|model_pins", false),
            two("_pin_model", "model_pins")
        );
        assert_eq!(
            names("\\bload_yaml\\b|core\\.dump", false),
            two("load_yaml", "dump")
        );
        assert_eq!(names("a|a|b", false), two("a", "b"));
        let mut multi = spec("a", false);
        multi.patterns = vec!["find_root".into(), "resolve_owner".into()];
        assert_eq!(
            identifiers_of(&multi).map(|v| v.len()),
            Some(2),
            "-e a -e b"
        );
        for p in [
            "resolve_(owner|x)",
            "_is_ancestor|def ",
            "a|[bc]",
            "a|",
            "a|b|c|d|e|f|g",
        ] {
            assert_eq!(names(p, false), None, "{p}");
        }
        assert_eq!(names("a|b", true), None, "fixed: a literal pipe");
        assert_eq!(
            identifier_of(&spec("TODO|FIXME", false)),
            None,
            "one name only"
        );
    }

    #[test]
    fn definition_and_token() {
        assert!(is_definition_line("    async def run_task(x):", "run_task"));
        assert!(!is_definition_line("def run_task_later():", "run_task"));
        assert!(is_definition_line("class Foo(Base):", "Foo"));
        assert_eq!(token_at("x = _resolve_owners(a)", 5), "_resolve_owners");
    }
}
