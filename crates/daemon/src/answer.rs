//! Canonical record of a judged search and its renderers.
//!
//! One `Answer` per search; every output format is a view of the same record, so what a human
//! reads (`--human`, `--explain`) never diverges from what the agent got (model format, default).
//! Model format = phase-1 winner: integrated `path:line:text    <annotation>` lines in relevance
//! order, one `# ` header line (+ notices) and one `# ` footer line. Nothing is dropped silently:
//! every item is printed or counted in the footer.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

/// Output view of an answer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutFormat {
    /// What the agent sees (default).
    #[default]
    Model,
    /// Grouped by file, colored, with a legend.
    Human,
    /// The full record.
    Json,
    /// Per-line reason for every classification.
    Explain,
}

/// Answer-size budget taken from the agent's own `| head …` (`| tail` runs the raw command), or
/// the default one: never more than the agent's own command would have printed, plus a bounded
/// overhead for the header, verdict, annotations and footer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    pub bytes: Option<usize>,
    pub lines: Option<usize>,
    /// The pipeline stage it came from, echoed in the notice.
    pub source: String,
    /// The default budget (no `| head` in the agent's pipeline).
    #[serde(default)]
    pub implicit: bool,
}

/// Bounds of what a default budget may add on top of the plain grep output (header line,
/// verdict, per-line annotations, footer): a quarter of that output, never under the header's
/// own size, never over 1 KB.
pub const OVERHEAD_MIN: usize = 384;
pub const OVERHEAD_MAX: usize = 1024;

/// Bytes an answer may add over a plain output of `grep_bytes`.
pub fn overhead(grep_bytes: usize) -> usize {
    (grep_bytes / 4).clamp(OVERHEAD_MIN, OVERHEAD_MAX)
}

impl Budget {
    /// The default budget for a search whose plain output is `grep_bytes` long.
    pub fn implicit(grep_bytes: usize) -> Self {
        Self {
            bytes: Some(grep_bytes + overhead(grep_bytes)),
            implicit: true,
            ..Default::default()
        }
    }
}

/// Lines the agent's own command would have printed for these items (context and `--` included).
pub fn grep_output_lines(a: &Answer) -> usize {
    a.items
        .iter()
        .filter(|i| i.class != "graph_only")
        .map(|it| {
            let ctx = it.before.len() + it.after.len();
            1 + ctx + usize::from(ctx > 0)
        })
        .sum()
}

/// Bytes the agent's own command would have printed for these items: `path:line:text` (no path
/// when a single file was searched), context lines and `--` separators. Graph-only items are not
/// in a text search's output.
pub fn grep_output_bytes(a: &Answer, with_path: bool) -> usize {
    let prefix = |it: &Item| if with_path { it.path.len() + 1 } else { 0 };
    a.items
        .iter()
        .filter(|i| i.class != "graph_only")
        .map(|it| {
            if it.line == 0 {
                return it.path.len() + 1; // `-l`
            }
            let digits = |n: u32| n.to_string().len();
            let ctx: usize = it
                .before
                .iter()
                .chain(&it.after)
                .map(|(n, t)| prefix(it) + digits(*n) + 1 + t.len() + 1)
                .sum();
            let sep = if ctx > 0 { 3 } else { 0 };
            prefix(it) + digits(it.line) + 1 + it.text.len() + 1 + ctx + sep
        })
        .sum()
}

/// A `grep`-style filter the agent piped the output through, applied to match lines only.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineFilter {
    pub pattern: String,
    #[serde(default)]
    pub invert: bool,
    #[serde(default)]
    pub ignore_case: bool,
    #[serde(default)]
    pub fixed: bool,
    #[serde(default)]
    pub word: bool,
    pub source: String,
}

/// A definition the searched name resolves to.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Target {
    /// `path:line` as the agent sees paths.
    pub def: String,
    pub signature: String,
    pub callers: u32,
    pub sites: u32,
    pub files: u32,
    pub prod: u32,
    pub test: u32,
    pub risk: Option<String>,
}

/// One identifier of an alternation search (`grep 'a\|b'`): its own verdict and definitions.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NameVerdict {
    pub name: String,
    /// complete | lower_bound | none
    pub verdict: String,
    /// Short reason for a non-complete verdict.
    pub note: String,
    pub targets: Vec<Target>,
    pub tests_total: u32,
}

/// One line of the integrated list.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Path as the agent sees it.
    pub path: String,
    pub line: u32,
    pub text: String,
    /// definition | call | graph_only | import | mock_in_test | string_comment | ci | config |
    /// docs | unresolved_call | untracked_code | other_language | not_indexed |
    /// other_identifier | match
    pub class: String,
    /// Enclosing function of a reference (or of a match, for non-identifier searches).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub in_fn: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub called_by: Vec<String>,
    #[serde(default)]
    pub called_by_total: u32,
    /// `path:line` of the definition this reference resolves to (multi-definition names).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolves_to: Option<String>,
    /// "inferred" / "name guess" when not extracted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    /// Why a call could not be resolved (unresolved_call).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gap: Option<String>,
    /// Match is in test code.
    #[serde(default)]
    pub test: bool,
    /// Why the item got its class (for `--explain`).
    pub why: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<(u32, String)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<(u32, String)>,
}

/// Indirect dependents of one module.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct IndirectGroup {
    pub module: String,
    pub count: u32,
    pub top: Vec<String>,
}

/// Covering tests listed by name (closest first); the total is always given.
pub const MAX_TESTS_LISTED: usize = 5;

/// The whole source of a function the answer points at (its definition, or the one function every
/// production match falls in), so the agent does not spend a turn reading it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Body {
    /// `Class.method` / `function`.
    pub label: String,
    /// Path as the agent sees it.
    pub path: String,
    pub start: u32,
    pub end: u32,
    pub lines: Vec<(u32, String)>,
}

/// Bodies are given only when the production hits fall in at most this many functions (more
/// means the agent hasn't narrowed down yet)…
pub const MAX_BODIES: usize = 6;
/// …each at most this long (longer functions are left to the agent's own read)…
pub const MAX_BODY_LINES: u32 = 60;
/// …and together at most this many bytes.
pub const MAX_BODY_BYTES: usize = 6000;

/// Facts that are not one matching line.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Footer {
    pub indirect_total: u32,
    pub indirect: Vec<IndirectGroup>,
    pub tests_total: u32,
    pub tests: Vec<String>,
    pub overrides: Vec<String>,
    pub import_lines: usize,
    pub import_files: usize,
}

/// The judged search.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    /// The command as the agent wrote it.
    pub query: String,
    /// identifier | grouped | files
    pub mode: String,
    pub matches: usize,
    pub files: usize,
    /// complete | lower_bound | none
    pub verdict: String,
    pub verdict_note: String,
    pub name: Option<String>,
    pub targets: Vec<Target>,
    /// Lines without their path (`N:text`), as grep prints a single file.
    #[serde(default)]
    pub no_filename: bool,
    /// Per identifier, when the search alternates several (`a\|b`); empty for one name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub names: Vec<NameVerdict>,
    /// Integrated list in relevance order (imports included, not listed unless `all`).
    pub items: Vec<Item>,
    pub footer: Footer,
    /// Disclosures that are not items: hidden/excluded dirs, search cap, stale graph.
    pub omitted: Vec<String>,
    /// Notices about how the agent's pipeline was honored (budget, filters).
    pub notices: Vec<String>,
    /// List everything (no caps).
    pub all: bool,
    /// The agent's search as a plain grep (`grep -rn 'pat'`), for "see more" hints on a narrower path.
    #[serde(default)]
    pub hint_base: String,
    pub budget: Option<Budget>,
    /// Inlined function bodies, printed after the footer.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bodies: Vec<Body>,
}

pub const CAP_PRIORITY: usize = 60;
pub const CAP_OTHER: usize = 24;

/// Relevance rank; lower is shown first.
pub fn rank(class: &str, test: bool) -> u8 {
    match (class, test) {
        ("definition", _) => 0,
        ("call", false) => 1,
        ("graph_only", false) => 2,
        ("call", true) | ("graph_only", true) => 3,
        ("mock_in_test", _) => 4,
        ("unresolved_call", _) => 5,
        ("untracked_code", _) => 6,
        ("match", _) => 6,
        ("string_comment", _) => 7,
        ("ci", _) => 8,
        ("config", _) => 9,
        ("docs", _) => 10,
        ("other_language", _) => 11,
        ("not_indexed", _) => 12,
        ("other_identifier", _) => 13,
        _ => 14,
    }
}

/// Definitions and direct uses (prod or test): kept under any budget, folded per file if needed.
fn is_direct(it: &Item) -> bool {
    matches!(it.class.as_str(), "definition" | "call" | "graph_only")
}

fn is_priority(it: &Item) -> bool {
    matches!(it.class.as_str(), "definition" | "call" | "graph_only") && !it.test
        || it.class == "definition"
}

fn label(class: &str) -> &'static str {
    match class {
        "definition" => "definition",
        "call" => "call",
        "graph_only" => "graph-only",
        "import" => "import",
        "mock_in_test" => "mock in test",
        "string_comment" => "string/comment",
        "ci" => "ci",
        "config" => "config",
        "docs" => "docs",
        "unresolved_call" => "unresolved call",
        "untracked_code" => "untracked code use",
        "other_language" => "other language",
        "not_indexed" => "not indexed",
        "other_identifier" => "different identifier",
        _ => "match",
    }
}

/// The annotation after a match line.
pub fn annotation(it: &Item) -> String {
    let mut s = String::new();
    match it.class.as_str() {
        "call" | "graph_only" => {
            if let Some(f) = &it.in_fn {
                let _ = write!(s, "← {f}");
            }
            if !it.called_by.is_empty() {
                let _ = write!(s, " ← {}", it.called_by.join(", "));
                let more = it.called_by_total as usize
                    - it.called_by.len().min(it.called_by_total as usize);
                if more > 0 {
                    let _ = write!(s, " (+{more})");
                }
            }
            let mut tags = Vec::new();
            if it.class == "graph_only" {
                tags.push("graph-only".to_string());
            }
            if it.test {
                tags.push("test".to_string());
            }
            if let Some(c) = &it.confidence {
                tags.push(c.clone());
            }
            if !tags.is_empty() {
                let _ = write!(s, " [{}]", tags.join(", "));
            }
        }
        "unresolved_call" => {
            let _ = write!(
                s,
                "[unresolved call{}]",
                it.gap
                    .as_ref()
                    .map(|g| format!(": {g}"))
                    .unwrap_or_default()
            );
            if let Some(f) = &it.in_fn {
                let _ = write!(s, " in {f}");
            }
        }
        "match" => {
            if let Some(f) = &it.in_fn {
                let _ = write!(s, "[in {f}]");
            }
        }
        c => {
            let _ = write!(s, "[{}]", label(c));
        }
    }
    if let Some(r) = &it.resolves_to {
        let _ = write!(s, " → {r}");
    }
    s.trim_start().to_string()
}

/// `path` + `sep`, or nothing when lines go without their path.
fn path_prefix(it: &Item, names: bool, sep: char) -> String {
    if names {
        format!("{}{sep}", it.path)
    } else {
        String::new()
    }
}

/// The match line exactly as a filter in the agent's pipeline would see it.
pub fn item_line(it: &Item, names: bool) -> String {
    if it.line == 0 {
        return format!("{}    ({})", it.path, it.text);
    }
    let p = path_prefix(it, names, ':');
    let ann = annotation(it);
    if ann.is_empty() {
        format!("{p}{}:{}", it.line, it.text)
    } else {
        format!("{p}{}:{}    {ann}", it.line, it.text)
    }
}

fn item_block(it: &Item, names: bool) -> String {
    let mut out = String::new();
    let p = path_prefix(it, names, '-');
    for (n, t) in &it.before {
        let _ = writeln!(out, "{p}{n}-{t}");
    }
    out.push_str(&item_line(it, names));
    out.push('\n');
    for (n, t) in &it.after {
        let _ = writeln!(out, "{p}{n}-{t}");
    }
    if !it.before.is_empty() || !it.after.is_empty() {
        out.push_str("--\n");
    }
    out
}

fn verdict_words(a: &Answer) -> String {
    match a.verdict.as_str() {
        "complete" => "graph COMPLETE".into(),
        "lower_bound" => format!("graph LOWER-BOUND ({})", a.verdict_note),
        _ => a.verdict_note.clone(),
    }
}

/// One identifier of an alternation, in the header: verdict, then where it is defined.
fn name_words(n: &NameVerdict) -> String {
    let verdict = match n.verdict.as_str() {
        "complete" => "graph COMPLETE".to_string(),
        "lower_bound" => format!("graph LOWER-BOUND ({})", n.note),
        _ => n.note.clone(),
    };
    match n.targets.as_slice() {
        [] => format!("`{}` {verdict}", n.name),
        [t] => format!(
            "`{}` {verdict}, def {} · {} call sites in {} files",
            n.name, t.def, t.sites, t.files
        ),
        many => format!(
            "`{}` {verdict}, {} definitions — each reference → the one it resolves to",
            n.name,
            many.len()
        ),
    }
}

/// The one header line.
pub fn header_line(a: &Answer) -> String {
    let mut h = format!(
        "# graphite: {} → {} matches in {} files",
        a.query, a.matches, a.files
    );
    if a.mode == "identifier" && !a.names.is_empty() {
        let _ = write!(h, " · {} identifiers:", a.names.len());
        let per: Vec<String> = a.names.iter().map(name_words).collect();
        let _ = write!(h, " {}", per.join(" · "));
    } else if a.mode == "identifier" {
        let _ = write!(h, " · {}", verdict_words(a));
        let name = a.name.as_deref().unwrap_or("");
        match a.targets.len() {
            0 => {}
            1 => {
                let t = &a.targets[0];
                let _ = write!(
                    h,
                    " · `{name}` def {} · {} call sites, {} callers in {} files",
                    t.def, t.sites, t.callers, t.files
                );
                if let Some(r) = &t.risk {
                    let _ = write!(h, " · risk {r} (prod {}, test {})", t.prod, t.test);
                }
            }
            n => {
                let _ = write!(
                    h,
                    " · `{name}` has {n} definitions — each reference → the one it resolves to"
                );
            }
        }
    } else if !a.verdict_note.is_empty() {
        let _ = write!(h, " · {}", a.verdict_note);
    }
    h
}

/// Longest common directory prefix of `paths` (ends with `/`), or empty.
fn common_dir(paths: &[&str]) -> String {
    let Some(first) = paths.first() else {
        return String::new();
    };
    let mut prefix: Vec<&str> = first.split('/').collect();
    prefix.pop(); // last component may be a file or the leaf module itself
    for p in &paths[1..] {
        let comps: Vec<&str> = p.split('/').collect();
        let n = prefix
            .iter()
            .zip(&comps)
            .take_while(|(a, b)| a == b)
            .count()
            .min(comps.len().saturating_sub(1));
        prefix.truncate(n);
    }
    if prefix.is_empty() {
        String::new()
    } else {
        format!("{}/", prefix.join("/"))
    }
}

/// `pytest a/b/{x.py::T::t,y.py::U::u}` — runnable (shell brace expansion) and short.
fn tests_run_line(ids: &[String]) -> String {
    let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
    let files: Vec<&str> = refs
        .iter()
        .map(|i| i.split("::").next().unwrap_or(i))
        .collect();
    let pre = common_dir(&files);
    if ids.len() < 2 || pre.is_empty() {
        return format!("pytest {}", ids.join(" "));
    }
    let rest: Vec<&str> = refs.iter().map(|i| &i[pre.len()..]).collect();
    format!("pytest {pre}{{{}}}", rest.join(","))
}

/// Narrowest path covering the cut items (a file, or the directory holding most of them), for a
/// re-run of the agent's own grep that fits.
fn hint_dir(cut: &[String]) -> Option<String> {
    let first = cut.first()?;
    if cut.iter().all(|p| p == first) {
        return Some(first.clone());
    }
    let refs: Vec<&str> = cut.iter().map(String::as_str).collect();
    let common = common_dir(&refs);
    if !common.is_empty() {
        return Some(common);
    }
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for p in cut {
        let comps: Vec<&str> = p.split('/').collect();
        let dirs = comps.len().saturating_sub(1).min(3);
        let d = if dirs == 0 {
            ".".to_string()
        } else {
            comps[..dirs].join("/") + "/"
        };
        *counts.entry(d).or_default() += 1;
    }
    counts
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1).then_with(|| b.0.cmp(&a.0)))
        .map(|(d, _)| d)
}

/// One footer item with degraded forms, richest first. Priority: lower is kept longer.
struct Part {
    priority: u8,
    /// Disclosures that must survive any budget (tests warning, cuts, stale graph).
    mandatory: bool,
    forms: Vec<String>,
}

/// The footer: each fact judged on its own (sent only when it carries information), rendered as
/// rich as the allowance permits. Degrade order under a tight allowance: examples go first, then
/// low-priority parts (imports < indirect < tests to run); mandatory disclosures always stay.
fn footer_line(
    a: &Answer,
    not_shown: &BTreeMap<String, usize>,
    cut_paths: &[String],
    summarized: usize,
    allowance: Option<usize>,
    more: Option<&str>,
) -> String {
    let f = &a.footer;
    let mut parts: Vec<Part> = Vec::new();
    if a.mode == "identifier" && !a.targets.is_empty() {
        let untested: Vec<String> = if a.names.is_empty() {
            (f.tests_total == 0)
                .then(|| format!("`{}`", a.name.as_deref().unwrap_or("target")))
                .into_iter()
                .collect()
        } else {
            a.names
                .iter()
                .filter(|n| !n.targets.is_empty() && n.tests_total == 0)
                .map(|n| format!("`{}`", n.name))
                .collect()
        };
        if !untested.is_empty() {
            parts.push(Part {
                priority: 0,
                mandatory: true,
                forms: vec![format!("no test covers {} ⚠", untested.join(", "))],
            });
        }
        if f.tests_total > 0 {
            let mut forms = Vec::new();
            if !f.tests.is_empty() {
                forms.push(format!(
                    "tests: {} — closest: {}",
                    f.tests_total,
                    tests_run_line(&f.tests)
                ));
            }
            if f.tests.len() > 2 {
                forms.push(format!(
                    "tests: {} — closest: {}",
                    f.tests_total,
                    tests_run_line(&f.tests[..2])
                ));
            }
            forms.push(format!("tests: {}", f.tests_total));
            parts.push(Part {
                priority: 1,
                mandatory: false,
                forms,
            });
        }
        if f.indirect_total > 0 {
            let mods: Vec<&str> = f.indirect.iter().map(|g| g.module.as_str()).collect();
            let pre = common_dir(&mods);
            let short = |m: &str| m.strip_prefix(pre.as_str()).unwrap_or(m).to_string();
            let where_ = if pre.is_empty() {
                String::new()
            } else {
                format!(" in {pre}")
            };
            let with_top: Vec<String> = f
                .indirect
                .iter()
                .map(|g| format!("{} {} ({})", short(&g.module), g.count, g.top.join(", ")))
                .collect();
            let counts: Vec<String> = f
                .indirect
                .iter()
                .map(|g| format!("{} {}", short(&g.module), g.count))
                .collect();
            let n = f.indirect_total;
            parts.push(Part {
                priority: 2,
                mandatory: false,
                forms: vec![
                    format!(
                        "indirect (depth 2-3): {n}{where_} — {}",
                        with_top.join("; ")
                    ),
                    format!("indirect (depth 2-3): {n}{where_} — {}", counts.join(", ")),
                    format!(
                        "indirect (depth 2-3): {n}{where_} — {}",
                        counts
                            .iter()
                            .take(2)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    format!("indirect (depth 2-3): {n}"),
                ],
            });
        }
        if !f.overrides.is_empty() {
            let shown: Vec<String> = f.overrides.iter().take(3).cloned().collect();
            let more = f.overrides.len().saturating_sub(shown.len());
            let tail = if more > 0 {
                format!(" (+{more})")
            } else {
                String::new()
            };
            parts.push(Part {
                priority: 1,
                mandatory: false,
                forms: vec![
                    format!("overrides: {}{tail}", shown.join(", ")),
                    format!("overrides: {}", f.overrides.len()),
                ],
            });
        }
    }
    if f.import_lines > 0 && !a.all {
        parts.push(Part {
            priority: 3,
            mandatory: false,
            forms: vec![format!(
                "imports: {} lines in {} files (not listed)",
                f.import_lines, f.import_files
            )],
        });
    }
    for o in &a.omitted {
        let secret = o.starts_with("secret-looking");
        parts.push(Part {
            priority: if secret { 4 } else { 0 },
            mandatory: !secret,
            forms: vec![o.clone()],
        });
    }
    let mut cut_note = Vec::new();
    if summarized > 0 {
        cut_note.push(format!("{summarized} call sites collapsed per file"));
    }
    if !not_shown.is_empty() {
        let list: Vec<String> = not_shown
            .iter()
            .map(|(c, n)| format!("{} {n}", label(c)))
            .collect();
        cut_note.push(format!("not shown: {}", list.join(", ")));
    }
    if !cut_note.is_empty() {
        let mut t = cut_note.join(" · ");
        if let Some(m) = more {
            let _ = write!(t, " — to see them: {m}");
        } else if let Some(dir) = hint_dir(cut_paths).filter(|_| !a.hint_base.is_empty()) {
            let _ = write!(t, " — to see them: {} {dir}", a.hint_base);
        }
        parts.push(Part {
            priority: 0,
            mandatory: true,
            forms: vec![t],
        });
    }
    if parts.is_empty() {
        return String::new();
    }
    parts.sort_by_key(|p| p.priority);
    let mut level = vec![0usize; parts.len()];
    let mut keep = vec![true; parts.len()];
    let text = |level: &[usize], keep: &[bool]| -> String {
        let items: Vec<&str> = parts
            .iter()
            .enumerate()
            .filter(|(i, _)| keep[*i])
            .map(|(i, p)| p.forms[level[i].min(p.forms.len() - 1)].as_str())
            .collect();
        format!("# {}", items.join(" · "))
    };
    let Some(max) = allowance else {
        return text(&level, &keep);
    };
    // Degrade lowest priority first, one step at a time; then drop optional parts from the bottom.
    loop {
        if text(&level, &keep).len() <= max {
            return text(&level, &keep);
        }
        let step = (0..parts.len())
            .rev()
            .find(|&i| keep[i] && level[i] + 1 < parts[i].forms.len());
        if let Some(i) = step {
            level[i] += 1;
            continue;
        }
        match (0..parts.len())
            .rev()
            .find(|&i| keep[i] && !parts[i].mandatory)
        {
            Some(i) => keep[i] = false,
            None => return text(&level, &keep),
        }
    }
}

fn with_nl(s: String) -> String {
    if s.is_empty() {
        s
    } else {
        s + "\n"
    }
}

/// Per-file collapsed line for call sites that don't fit individually.
fn collapsed(path: &str, items: &[&Item], names: bool) -> String {
    let lines: Vec<String> = items.iter().map(|i| i.line.to_string()).collect();
    let mut fns: Vec<&str> = items.iter().filter_map(|i| i.in_fn.as_deref()).collect();
    fns.sort_unstable();
    fns.dedup();
    let prefix = if names {
        format!("{path}:")
    } else {
        String::new()
    };
    format!(
        "{prefix}{}    ← {} ({} sites)",
        lines.join(","),
        fns.join(", "),
        items.len()
    )
}

/// `(text, classes of the items it carries, collapsed call sites, their paths)`.
type Chunk = (String, Vec<String>, usize, Vec<String>);

fn single(it: &Item, names: bool) -> Chunk {
    (
        item_block(it, names),
        vec![it.class.clone()],
        0,
        vec![it.path.clone()],
    )
}

/// Definitions and direct call sites: the first `full` in full, every remaining call site
/// collapsed per file (definitions are never collapsed).
fn priority_chunks(prio: &[&Item], full: usize, names: bool) -> Vec<Chunk> {
    let mut chunks: Vec<Chunk> = prio.iter().take(full).map(|i| single(i, names)).collect();
    let mut by_file: Vec<(String, Vec<&Item>)> = Vec::new();
    for it in prio.iter().skip(full) {
        if it.class == "definition" {
            chunks.push(single(it, names));
            continue;
        }
        match by_file.iter_mut().find(|(p, _)| *p == it.path) {
            Some((_, v)) => v.push(it),
            None => by_file.push((it.path.clone(), vec![it])),
        }
    }
    for (path, its) in by_file {
        let classes = its.iter().map(|i| i.class.clone()).collect();
        let paths = vec![path.clone(); its.len()];
        chunks.push((
            collapsed(&path, &its, names) + "\n",
            classes,
            its.len(),
            paths,
        ));
    }
    chunks
}

fn size(chunks: &[Chunk]) -> (usize, usize) {
    chunks
        .iter()
        .fold((0, 0), |(b, l), c| (b + c.0.len(), l + c.0.lines().count()))
}

/// Unbudgeted model view: section caps only.
fn render_capped(a: &Answer, head: &str, listable: &[&Item]) -> String {
    let mut not_shown: BTreeMap<String, usize> = BTreeMap::new();
    let mut cut_paths: Vec<String> = Vec::new();
    let mut body = String::new();
    let (mut prio, mut other) = (0usize, 0usize);
    for it in listable {
        let fits = if a.all {
            true
        } else if is_priority(it) {
            prio += 1;
            prio <= CAP_PRIORITY
        } else {
            other += 1;
            other <= CAP_OTHER
        };
        if fits {
            body.push_str(&item_block(it, !a.no_filename));
        } else {
            *not_shown.entry(it.class.clone()).or_default() += 1;
            cut_paths.push(it.path.clone());
        }
    }
    format!(
        "{head}{body}{}",
        with_nl(footer_line(a, &not_shown, &cut_paths, 0, None, None))
    )
}

/// A default budget's escape hatch, in the agent's own vocabulary: the same command with a
/// `| head -c N` large enough for the whole answer.
fn whole_answer_hint(a: &Answer) -> String {
    let mut whole = a.clone();
    whole.budget = Some(Budget {
        bytes: Some(usize::MAX),
        source: "head -c N".into(),
        ..Default::default()
    });
    let need = render_model_items(&whole).len() + 120;
    let n = need.div_ceil(1000) * 1000;
    format!("{} | head -c {n}", a.query)
}

/// The inlined bodies in grep's context shape (`path-N-text`), one `#` line naming each.
pub fn bodies_block(a: &Answer) -> String {
    let mut out = String::new();
    for b in &a.bodies {
        let _ = writeln!(
            out,
            "# graphite: body of {} ({}:{}-{}), whole — no need to read it",
            b.label, b.path, b.start, b.end
        );
        for (n, t) in &b.lines {
            if a.no_filename {
                let _ = writeln!(out, "{n}-{t}");
            } else {
                let _ = writeln!(out, "{}-{n}-{t}", b.path);
            }
        }
    }
    out
}

/// Model format: what the agent sees. Bodies come after the footer. They replace the read the
/// agent would do next, so a default budget doesn't count them (they are capped on their own);
/// an explicit `| head` budget does: they appear only if the whole answer still fits it.
pub fn render_model(a: &Answer) -> String {
    let base = render_model_items(a);
    let block = bodies_block(a);
    if block.is_empty() {
        return base;
    }
    // The agent's own `| head` (its stage is the budget's source, even once tightened to the
    // grep-output cap) binds the bodies too; the default budget alone does not.
    let explicit = a.budget.as_ref().filter(|b| !b.source.is_empty());
    let fits = explicit.is_none_or(|b| {
        b.bytes.is_none_or(|n| base.len() + block.len() <= n)
            && b.lines
                .is_none_or(|n| base.lines().count() + block.lines().count() <= n)
    });
    if fits {
        format!("{base}{block}")
    } else {
        base
    }
}

fn render_model_items(a: &Answer) -> String {
    let mut head = header_line(a) + "\n";
    for n in &a.notices {
        let _ = writeln!(head, "# graphite: {n}");
    }
    let listable: Vec<&Item> = a
        .items
        .iter()
        .filter(|i| a.all || i.class != "import")
        .collect();
    let byte_cap = a.budget.as_ref().and_then(|b| b.bytes);
    let line_cap = a.budget.as_ref().and_then(|b| b.lines);
    let implicit = a.budget.as_ref().is_some_and(|b| b.implicit);
    if byte_cap.is_none() && line_cap.is_none() {
        return render_capped(a, &head, &listable);
    }
    // A default budget replaces the section caps: every direct site stays (folded if needed) and
    // the answer never costs more than the agent's own command plus a bounded overhead.

    // Budgeted: header first; then definitions and direct call sites (as many full lines as fit,
    // every other call site collapsed per file); then the rest by rank. The footer is recomputed
    // and chunks are dropped from the tail until the whole answer fits exactly.
    // A default budget covers the body and footer: the header (the agent's own command echoed,
    // the verdict, pipeline notices) is always there on top of it.
    let bytes = byte_cap.map_or(usize::MAX, |b| if implicit { b + head.len() } else { b });
    let lines_max = line_cap.unwrap_or(usize::MAX);
    // The footer gets a share of a byte budget; facts degrade inside it (see `footer_line`).
    let allowance = byte_cap.map(|b| (b / 4).max(160));
    let more = implicit.then(|| whole_answer_hint(a));
    let names = !a.no_filename;
    let mut prio: Vec<&Item> = listable.iter().copied().filter(|i| is_direct(i)).collect();
    prio.sort_by_key(|i| i.class != "definition");
    let rest: Vec<&Item> = listable.iter().copied().filter(|i| !is_direct(i)).collect();
    let mut not_shown: BTreeMap<String, usize> = BTreeMap::new();
    let mut cut_paths: Vec<String> = Vec::new();

    // Room for the priority section: everything but the header and a footer that discloses the
    // whole rest as cut (an upper bound of the real one).
    let worst_cut: BTreeMap<String, usize> = rest.iter().fold(BTreeMap::new(), |mut m, i| {
        *m.entry(i.class.clone()).or_default() += 1;
        m
    });
    let worst_paths: Vec<String> = rest.iter().map(|i| i.path.clone()).collect();
    let foot = footer_line(
        a,
        &worst_cut,
        &worst_paths,
        prio.len(),
        allowance,
        more.as_deref(),
    );
    let head_lines = head.lines().count();
    let room = bytes.saturating_sub(head.len() + foot.len() + 1);
    let room_lines = lines_max.saturating_sub(head_lines + 1);
    let fits = |c: &[Chunk]| {
        let (b, l) = size(c);
        b <= room && l <= room_lines
    };
    // Most full lines whose collapsed remainder still fits (binary search on the prefix).
    let (mut lo, mut hi) = (0usize, prio.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(&priority_chunks(&prio, mid, names)) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let chunks: Vec<Chunk> = priority_chunks(&prio, lo, names)
        .into_iter()
        .chain(rest.iter().map(|i| single(i, names)))
        .collect();

    // Greedy fill in order, then trim from the tail until header + body + footer fit.
    let mut kept: Vec<Chunk> = Vec::new();
    let mut used = head.len();
    let mut nl = head_lines + 1;
    for ch in chunks {
        if used + ch.0.len() <= bytes && nl + ch.0.lines().count() <= lines_max {
            used += ch.0.len();
            nl += ch.0.lines().count();
            kept.push(ch);
        } else {
            for c in ch.1 {
                *not_shown.entry(c).or_default() += 1;
            }
            cut_paths.extend(ch.3);
        }
    }
    loop {
        let summarized: usize = kept.iter().map(|c| c.2).sum();
        let body: String = kept.iter().map(|c| c.0.as_str()).collect();
        // Collapsed call sites lost their text too: point the narrower re-run at them as well.
        let mut hint_paths = cut_paths.clone();
        hint_paths.extend(
            kept.iter()
                .filter(|c| c.2 > 0)
                .flat_map(|c| c.3.iter().cloned()),
        );
        let out = format!(
            "{head}{body}{}",
            with_nl(footer_line(
                a,
                &not_shown,
                &hint_paths,
                summarized,
                allowance,
                more.as_deref(),
            ))
        );
        if (out.len() <= bytes && out.lines().count() <= lines_max) || kept.is_empty() {
            return out;
        }
        if let Some(ch) = kept.pop() {
            for c in ch.1 {
                *not_shown.entry(c).or_default() += 1;
            }
            cut_paths.extend(ch.3);
        }
    }
}

const RESET: &str = "\x1b[0m";

fn color(class: &str) -> &'static str {
    match class {
        "definition" => "\x1b[1;32m",
        "call" | "graph_only" => "\x1b[36m",
        "mock_in_test" => "\x1b[35m",
        "unresolved_call" => "\x1b[33m",
        "ci" | "config" => "\x1b[34m",
        "docs" | "string_comment" => "\x1b[90m",
        _ => "\x1b[2m",
    }
}

/// Human format: same record, grouped by file, colored, with a legend.
pub fn render_human(a: &Answer, colored: bool) -> String {
    let c = |code: &'static str| if colored { code } else { "" };
    let reset = if colored { RESET } else { "" };
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{}{}{reset}",
        c("\x1b[1m"),
        header_line(a).trim_start_matches("# ")
    );
    for n in &a.notices {
        let _ = writeln!(out, "  note: {n}");
    }
    out.push('\n');
    let mut order: Vec<String> = Vec::new();
    let mut groups: BTreeMap<String, Vec<&Item>> = BTreeMap::new();
    for it in a.items.iter().filter(|i| a.all || i.class != "import") {
        if !groups.contains_key(&it.path) {
            order.push(it.path.clone());
        }
        groups.entry(it.path.clone()).or_default().push(it);
    }
    for path in order {
        let _ = writeln!(out, "{}{path}{reset}", c("\x1b[1;35m"));
        let mut its = groups.remove(&path).unwrap_or_default();
        its.sort_by_key(|i| i.line);
        for it in its {
            let _ = writeln!(
                out,
                "  {}{:>5}{reset}: {}    {}{}{reset}",
                c("\x1b[32m"),
                it.line,
                it.text.trim_end(),
                c(color(&it.class)),
                annotation(it)
            );
        }
        out.push('\n');
    }
    let foot = footer_line(a, &BTreeMap::new(), &[], 0, None, None);
    for part in foot
        .trim_start_matches("# ")
        .split(" · ")
        .filter(|p| !p.is_empty())
    {
        let _ = writeln!(out, "{}• {part}{reset}", c("\x1b[2m"));
    }
    out.push_str(&bodies_block(a));
    let _ = writeln!(
        out,
        "\nlegend: ← enclosing fn ← its production callers · [graph-only] reference text search can't see · [test] caller in test code · → definition it resolves to · [unresolved call] graph gap · [mock in test]/[ci]/[config]/[docs]/[string/comment] non-call mentions"
    );
    out
}

/// Explain format: every line with the reason for its class.
pub fn render_explain(a: &Answer) -> String {
    let mut out = header_line(a) + "\n";
    let _ = writeln!(out, "# verdict: {} — {}", a.verdict, a.verdict_note);
    for it in &a.items {
        let _ = writeln!(
            out,
            "{}:{}  class={}{}  why: {}",
            it.path,
            it.line,
            it.class,
            if it.test { " test" } else { "" },
            it.why
        );
    }
    out.push_str(&with_nl(footer_line(
        a,
        &BTreeMap::new(),
        &[],
        0,
        None,
        None,
    )));
    out
}

/// Render in the requested view.
pub fn render(a: &Answer, format: OutFormat) -> String {
    match format {
        OutFormat::Model => render_model(a),
        OutFormat::Human => render_human(a, std::env::var_os("NO_COLOR").is_none()),
        OutFormat::Json => serde_json::to_string_pretty(a).unwrap_or_default() + "\n",
        OutFormat::Explain => render_explain(a),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(path: &str, line: u32, class: &str) -> Item {
        Item {
            path: path.into(),
            line,
            text: format!("x = f()  # {line}"),
            class: class.into(),
            in_fn: Some("g".into()),
            why: "fixture".into(),
            ..Default::default()
        }
    }

    fn answer(n_calls: u32) -> Answer {
        let mut items = vec![item("a.py", 1, "definition")];
        for i in 0..n_calls {
            items.push(item(&format!("m{}.py", i % 5), 10 + i, "call"));
        }
        items.push(item("docs/x.md", 3, "docs"));
        items.push(item("t/test_a.py", 5, "mock_in_test"));
        Answer {
            query: "grep -rn f .".into(),
            mode: "identifier".into(),
            matches: items.len(),
            files: 7,
            verdict: "complete".into(),
            name: Some("f".into()),
            targets: vec![Target {
                def: "a.py:1".into(),
                signature: "def f()".into(),
                ..Default::default()
            }],
            items,
            hint_base: "grep -rn f".into(),
            ..Default::default()
        }
    }

    #[test]
    fn model_lists_every_item_or_counts_it() {
        let a = answer(8);
        let t = render_model(&a);
        for it in &a.items {
            assert!(t.contains(&format!("{}:{}:", it.path, it.line)), "{t}");
        }
        assert!(
            t.starts_with("# graphite: grep -rn f . → 11 matches"),
            "{t}"
        );
        assert!(t.lines().last().unwrap().starts_with("# "), "{t}");
    }

    #[test]
    fn budget_keeps_header_def_and_calls_first() {
        let mut a = answer(80);
        a.budget = Some(Budget {
            bytes: Some(1500),
            lines: None,
            source: "head -c 1500".into(),
            ..Default::default()
        });
        let t = render_model(&a);
        assert!(t.len() <= 1500, "{} > 1500\n{t}", t.len());
        assert!(t.contains("a.py:1:"), "{t}");
        assert!(t.contains("collapsed per file"), "{t}");
        assert_every_direct_site_kept(&a, &t);
        // no line is cut mid-way: every non-comment line is a full item or collapsed line
        for l in t.lines().filter(|l| !l.starts_with('#')) {
            assert!(l.contains("    "), "partial line: {l}");
        }
    }

    #[test]
    fn line_budget_counts_lines() {
        let mut a = answer(40);
        a.budget = Some(Budget {
            bytes: None,
            lines: Some(12),
            source: "head -12".into(),
            ..Default::default()
        });
        let t = render_model(&a);
        assert!(t.lines().count() <= 12, "{t}");
    }

    fn with_body(mut a: Answer) -> Answer {
        a.bodies = vec![Body {
            label: "f".into(),
            path: "a.py".into(),
            start: 1,
            end: 3,
            lines: vec![
                (1, "def f():".into()),
                (2, "    x = 1".into()),
                (3, "    return x".into()),
            ],
        }];
        a
    }

    // kinhin: decision(ref="docs/foundation/interception.md#2-the-answer-an-enriched-grep")
    #[test]
    fn body_follows_the_footer_in_grep_context_shape() {
        let a = with_body(answer(2));
        let t = render_model(&a);
        let foot = t.find("no test covers").unwrap();
        let body = t.find("# graphite: body of f (a.py:1-3)").unwrap();
        assert!(foot < body, "{t}");
        assert!(t.ends_with("a.py-1-def f():\na.py-2-    x = 1\na.py-3-    return x\n"), "{t}");
        assert!(render_human(&a, false).contains("a.py-2-    x = 1"));
    }

    // kinhin: decision(ref="docs/foundation/interception.md#4-size-budget-never-byte-cut")
    #[test]
    fn explicit_budget_drops_the_body_whole_never_part_of_it() {
        let mut a = with_body(answer(2));
        let full = render_model(&a);
        a.budget = Some(Budget {
            bytes: Some(full.len() - 1),
            source: "head -c".into(),
            ..Default::default()
        });
        let t = render_model(&a);
        assert!(!t.contains("body of f") && !t.contains("a.py-2-"), "{t}");
        a.budget.as_mut().unwrap().bytes = Some(full.len());
        assert_eq!(render_model(&a), full);
        // A default budget bounds the grep part only.
        a.budget = Some(Budget::implicit(10_000));
        assert!(render_model(&a).contains("a.py-3-    return x"));
    }

    #[test]
    fn views_carry_the_same_items() {
        let a = answer(8);
        let model = render_model(&a);
        let human = render_human(&a, false);
        let explain = render_explain(&a);
        let json: Answer = serde_json::from_str(&render(&a, OutFormat::Json)).unwrap();
        assert_eq!(json, a);
        for it in &a.items {
            assert!(model.contains(&format!("{}:{}:", it.path, it.line)));
            assert!(human.contains(&it.path) && human.contains(&format!("{:>5}:", it.line)));
            assert!(explain.contains(&format!("{}:{}  class={}", it.path, it.line, it.class)));
        }
        assert!(human.contains("no test covers `f` ⚠") && model.contains("no test covers `f` ⚠"));
    }

    /// Bytes after the header lines (what a default budget covers).
    fn body_len(t: &str) -> usize {
        let head: usize = t
            .lines()
            .take_while(|l| l.starts_with("# graphite:"))
            .map(|l| l.len() + 1)
            .sum();
        t.len() - head
    }

    /// Every definition and direct call site is in `t`, as a full line or in a collapsed one.
    fn assert_every_direct_site_kept(a: &Answer, t: &str) {
        for it in a
            .items
            .iter()
            .filter(|i| i.class == "definition" || i.class == "call")
        {
            let full = t.contains(&format!("{}:{}:", it.path, it.line));
            let folded = t.lines().any(|l| {
                l.strip_prefix(&format!("{}:", it.path))
                    .and_then(|r| r.split_whitespace().next())
                    .is_some_and(|nums| nums.split(',').any(|n| n == it.line.to_string()))
            });
            assert!(full || folded, "{}:{} lost\n{t}", it.path, it.line);
        }
    }

    // kinhin: decision(ref="docs/foundation/interception.md#4-size-budget-never-byte-cut")
    #[test]
    fn default_budget_costs_at_most_the_grep_output_plus_overhead() {
        let mut a = answer(300);
        let grep = grep_output_bytes(&a, true);
        a.budget = Some(Budget::implicit(grep));
        let t = render_model(&a);
        assert!(
            body_len(&t) <= grep + overhead(grep),
            "{} > {grep}+overhead",
            t.len()
        );
        assert!(
            t.starts_with("# graphite: grep -rn f . → 303 matches"),
            "{t}"
        );
        assert!(t.contains("graph COMPLETE"), "verdict survives: {t}");
        assert_every_direct_site_kept(&a, &t);
        // Most sites stay full lines; only the overflow is folded.
        assert!(t.matches("    ← g").count() > 200, "{t}");
        // The way back to everything, in grep vocabulary, big enough for it.
        let f = footer_of(&t);
        let n: usize = f
            .split("to see them: grep -rn f . | head -c ")
            .nth(1)
            .and_then(|r| r.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("no head -c hint: {f}"));
        let mut whole = a.clone();
        whole.budget = Some(Budget {
            bytes: Some(n),
            source: format!("head -c {n}"),
            ..Default::default()
        });
        let all = render_model(&whole);
        assert!(!footer_of(&all).contains("not shown"), "{all}");
        assert!(!all.contains("collapsed per file"), "{all}");
    }

    // kinhin: decision(ref="docs/foundation/interception.md#4-size-budget-never-byte-cut")
    #[test]
    fn default_budget_changes_nothing_when_the_answer_fits() {
        let a = answer(8);
        let mut b = a.clone();
        b.budget = Some(Budget::implicit(grep_output_bytes(&a, true)));
        assert_eq!(render_model(&b), render_model(&a));
    }

    // kinhin: decision(ref="docs/foundation/interception.md#4-size-budget-never-byte-cut")
    #[test]
    fn overhead_is_a_quarter_of_the_grep_output_within_bounds() {
        assert_eq!(overhead(0), OVERHEAD_MIN);
        assert_eq!(overhead(1000), OVERHEAD_MIN);
        assert_eq!(overhead(3000), 750);
        assert_eq!(overhead(1_000_000), OVERHEAD_MAX);
    }

    // kinhin: decision(ref="docs/foundation/interception.md#4-size-budget-never-byte-cut")
    #[test]
    fn test_call_sites_are_folded_not_dropped() {
        let mut a = answer(40);
        for (i, it) in a
            .items
            .iter_mut()
            .enumerate()
            .filter(|(_, i)| i.class == "call")
        {
            it.test = i % 2 == 0;
        }
        let grep = grep_output_bytes(&a, true);
        a.budget = Some(Budget::implicit(grep / 2));
        let t = render_model(&a);
        assert!(body_len(&t) <= grep / 2 + overhead(grep / 2), "{t}");
        assert_every_direct_site_kept(&a, &t);
    }

    #[test]
    fn grep_output_counts_context_and_single_file_shape() {
        let mut a = answer(0);
        a.items = vec![Item {
            path: "a.py".into(),
            line: 12,
            text: "def f():".into(),
            class: "definition".into(),
            after: vec![(13, "    pass".into())],
            ..Default::default()
        }];
        // a.py:12:def f():\n a.py-13-    pass\n --\n
        assert_eq!(grep_output_bytes(&a, true), 17 + 17 + 3);
        assert_eq!(grep_output_bytes(&a, false), 12 + 12 + 3);
        assert_eq!(grep_output_lines(&a), 3);
    }

    fn footer_of(t: &str) -> &str {
        t.lines().last().unwrap()
    }

    #[test]
    fn footer_judges_each_fact() {
        // Zero tests: the warning is always there; no indirect → no indirect part at all.
        let a = answer(3);
        let f = footer_of(&render_model(&a)).to_string();
        assert!(f.contains("no test covers `f` ⚠"), "{f}");
        assert!(
            !f.contains("indirect"),
            "no indirect part when there is none: {f}"
        );
        assert!(
            !f.contains("to see them"),
            "no hint when nothing was cut: {f}"
        );

        // Many tests: total + only the closest few, runnable, prefix factored out.
        let mut a = answer(3);
        a.footer.tests_total = 42;
        a.footer.tests = (0..MAX_TESTS_LISTED)
            .map(|i| format!("pkg/tests/test_m{i}.py::T::t{i}"))
            .collect();
        a.footer.indirect_total = 9;
        a.footer.indirect = vec![
            IndirectGroup {
                module: "tools/orch/runner/core".into(),
                count: 6,
                top: vec!["run".into()],
            },
            IndirectGroup {
                module: "tools/orch/dao/cli".into(),
                count: 3,
                top: vec!["init".into()],
            },
        ];
        let f = footer_of(&render_model(&a)).to_string();
        assert!(!f.contains("no test covers"), "{f}");
        assert!(
            f.contains("tests: 42 — closest: pytest pkg/tests/{test_m0.py::T::t0,"),
            "{f}"
        );
        assert_eq!(f.matches("::T::").count(), MAX_TESTS_LISTED, "{f}");
        assert!(
            f.contains(
                "indirect (depth 2-3): 9 in tools/orch/ — runner/core 6 (run); dao/cli 3 (init)"
            ),
            "{f}"
        );
    }

    #[test]
    fn tight_budget_degrades_footer_by_priority() {
        let mut a = answer(40);
        a.footer.tests_total = 42;
        a.footer.tests = (0..5)
            .map(|i| format!("pkg/tests/test_m{i}.py::T::t{i}"))
            .collect();
        a.footer.indirect_total = 9;
        a.footer.indirect = (0..5)
            .map(|i| IndirectGroup {
                module: format!("mod{i}/deep/path"),
                count: 2,
                top: vec!["a".into(), "b".into()],
            })
            .collect();
        a.footer.import_lines = 7;
        a.footer.import_files = 7;
        a.budget = Some(Budget {
            bytes: Some(900),
            lines: None,
            source: "head -c 900".into(),
            ..Default::default()
        });
        let t = render_model(&a);
        assert!(t.len() <= 900, "{t}");
        let f = footer_of(&t);
        // tests to run outrank indirect, which outranks imports; cuts are always disclosed.
        assert!(f.contains("tests: 42"), "{f}");
        assert!(f.contains("not shown") || f.contains("collapsed"), "{f}");
        assert!(
            f.contains("to see them: grep -rn f "),
            "grep-form hint: {f}"
        );
        if f.contains("imports") {
            assert!(
                f.contains("indirect"),
                "imports kept only if indirect kept: {f}"
            );
        }
    }

    #[test]
    fn answers_never_mention_graphite_commands_to_the_agent() {
        let mut a = answer(200);
        a.budget = Some(Budget {
            bytes: Some(1200),
            lines: None,
            source: "head -c 1200".into(),
            ..Default::default()
        });
        let t = render_model(&a);
        assert!(
            !t.contains("graphite-hook") && !t.contains("--all") && !t.contains("--hidden"),
            "{t}"
        );
    }
}
