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

/// Answer-size budget taken from the agent's own `| head …` / `| tail …`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    pub bytes: Option<usize>,
    pub lines: Option<usize>,
    /// The pipeline stage it came from, echoed in the notice.
    pub source: String,
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

/// One line of the integrated list.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Item {
    /// Path as the agent sees it.
    pub path: String,
    pub line: u32,
    pub text: String,
    /// definition | call | graph_only | import | mock_in_test | string_comment | docs |
    /// unresolved_call | untracked_code | other_language | not_indexed | other_identifier | match
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

/// Facts that are not one matching line.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Footer {
    pub indirect_total: u32,
    /// `module N (top, top)` groups.
    pub indirect: Vec<String>,
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
    /// Integrated list in relevance order (imports included, not listed unless `all`).
    pub items: Vec<Item>,
    pub footer: Footer,
    /// Disclosures that are not items: hidden/excluded dirs, search cap, stale graph.
    pub omitted: Vec<String>,
    /// Notices about how the agent's pipeline was honored (budget, filters).
    pub notices: Vec<String>,
    /// List everything (no caps).
    pub all: bool,
    /// `graphite-hook run --all -- '<cmd>'`, empty when unknown.
    pub more_hint: String,
    pub budget: Option<Budget>,
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
        ("docs", _) => 8,
        ("other_language", _) => 9,
        ("not_indexed", _) => 10,
        ("other_identifier", _) => 11,
        _ => 12,
    }
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

/// The match line exactly as a filter in the agent's pipeline would see it.
pub fn item_line(it: &Item) -> String {
    if it.line == 0 {
        return format!("{}    ({})", it.path, it.text);
    }
    let ann = annotation(it);
    if ann.is_empty() {
        format!("{}:{}:{}", it.path, it.line, it.text)
    } else {
        format!("{}:{}:{}    {ann}", it.path, it.line, it.text)
    }
}

fn item_block(it: &Item) -> String {
    let mut out = String::new();
    for (n, t) in &it.before {
        let _ = writeln!(out, "{}-{n}-{t}", it.path);
    }
    out.push_str(&item_line(it));
    out.push('\n');
    for (n, t) in &it.after {
        let _ = writeln!(out, "{}-{n}-{t}", it.path);
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

/// The one header line.
pub fn header_line(a: &Answer) -> String {
    let mut h = format!(
        "# graphite: {} → {} matches in {} files",
        a.query, a.matches, a.files
    );
    if a.mode == "identifier" {
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

/// The footer, from facts plus what this rendering did not show.
fn footer_line(a: &Answer, not_shown: &BTreeMap<String, usize>, summarized: usize) -> String {
    let f = &a.footer;
    let compact = a.budget.is_some();
    let mut parts: Vec<String> = Vec::new();
    if a.mode == "identifier" && !a.targets.is_empty() {
        if f.indirect_total > 0 && compact {
            parts.push(format!("indirect (depth 2-3): {}", f.indirect_total));
        } else if f.indirect_total > 0 {
            parts.push(format!(
                "indirect (depth 2-3): {} — {}",
                f.indirect_total,
                f.indirect.join("; ")
            ));
        } else {
            parts.push("indirect: none".into());
        }
        if f.tests_total > 0 && compact {
            parts.push(format!("covering tests: {}", f.tests_total));
        } else if f.tests_total > 0 {
            parts.push(format!(
                "covering tests: {} (e.g. {})",
                f.tests_total,
                f.tests.join(", ")
            ));
        } else {
            parts.push("covering tests: none ⚠".into());
        }
        if !f.overrides.is_empty() {
            parts.push(format!("overrides: {}", f.overrides.join(", ")));
        }
    }
    if f.import_lines > 0 && !a.all {
        parts.push(format!(
            "imports: {} lines in {} files (not listed)",
            f.import_lines, f.import_files
        ));
    }
    for o in &a.omitted {
        let o = if compact {
            o.split(" — ").next().unwrap_or(o)
        } else {
            o
        };
        parts.push(o.to_string());
    }
    if summarized > 0 {
        parts.push(format!(
            "{summarized} call sites collapsed per file to fit the budget"
        ));
    }
    if !not_shown.is_empty() {
        let list: Vec<String> = not_shown
            .iter()
            .map(|(c, n)| format!("{} {n}", label(c)))
            .collect();
        parts.push(format!("not shown: {}", list.join(", ")));
    }
    let mut s = format!("# {}", parts.join(" · "));
    if (!not_shown.is_empty() || (f.import_lines > 0 && !a.all)) && !a.more_hint.is_empty() {
        let _ = write!(s, " — all: {}", a.more_hint);
    }
    s
}

/// Per-file collapsed line for call sites that don't fit individually.
fn collapsed(path: &str, items: &[&Item]) -> String {
    let lines: Vec<String> = items.iter().map(|i| i.line.to_string()).collect();
    let mut fns: Vec<&str> = items.iter().filter_map(|i| i.in_fn.as_deref()).collect();
    fns.sort_unstable();
    fns.dedup();
    format!(
        "{path}:{}    ← {} ({} sites)",
        lines.join(","),
        fns.join(", "),
        items.len()
    )
}

/// Model format: what the agent sees.
pub fn render_model(a: &Answer) -> String {
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
    let mut not_shown: BTreeMap<String, usize> = BTreeMap::new();

    // Unbudgeted: section caps only.
    if byte_cap.is_none() && line_cap.is_none() {
        let mut body = String::new();
        let (mut prio, mut other) = (0usize, 0usize);
        for it in &listable {
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
                body.push_str(&item_block(it));
            } else {
                *not_shown.entry(it.class.clone()).or_default() += 1;
            }
        }
        return format!("{head}{body}{}\n", footer_line(a, &not_shown, 0));
    }

    // Budgeted: header first; then definitions and direct call sites (full lines if they all
    // fit, else every call site collapsed per file); then the rest by rank. The footer is
    // recomputed and chunks are dropped from the tail until the whole answer fits exactly.
    let bytes = byte_cap.unwrap_or(usize::MAX);
    let lines_max = line_cap.unwrap_or(usize::MAX);
    let prio: Vec<&Item> = listable
        .iter()
        .copied()
        .filter(|i| is_priority(i))
        .collect();
    let rest: Vec<&Item> = listable
        .iter()
        .copied()
        .filter(|i| !is_priority(i))
        .collect();
    // (text, classes of the items it carries, collapsed call sites)
    let mut chunks: Vec<(String, Vec<String>, usize)> = Vec::new();
    let head_lines = head.lines().count();
    let prio_blocks: Vec<String> = prio.iter().map(|i| item_block(i)).collect();
    let prio_bytes: usize = prio_blocks.iter().map(String::len).sum();
    let prio_lines: usize = prio_blocks.iter().map(|b| b.lines().count()).sum();
    if head.len() + prio_bytes < bytes && head_lines + prio_lines < lines_max {
        for (blk, it) in prio_blocks.into_iter().zip(&prio) {
            chunks.push((blk, vec![it.class.clone()], 0));
        }
    } else {
        let (defs, calls): (Vec<&Item>, Vec<&Item>) =
            prio.iter().partition(|i| i.class == "definition");
        for it in defs {
            chunks.push((item_block(it), vec![it.class.clone()], 0));
        }
        let mut by_file: Vec<(String, Vec<&Item>)> = Vec::new();
        for it in calls {
            match by_file.iter_mut().find(|(p, _)| *p == it.path) {
                Some((_, v)) => v.push(it),
                None => by_file.push((it.path.clone(), vec![it])),
            }
        }
        for (path, its) in by_file {
            let classes = its.iter().map(|i| i.class.clone()).collect();
            chunks.push((collapsed(&path, &its) + "\n", classes, its.len()));
        }
    }
    for it in &rest {
        chunks.push((item_block(it), vec![it.class.clone()], 0));
    }
    // Greedy fill in order, then trim from the tail until header + body + footer fit.
    let mut kept: Vec<(String, Vec<String>, usize)> = Vec::new();
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
        }
    }
    loop {
        let summarized: usize = kept.iter().map(|c| c.2).sum();
        let body: String = kept.iter().map(|c| c.0.as_str()).collect();
        let out = format!("{head}{body}{}\n", footer_line(a, &not_shown, summarized));
        if (out.len() <= bytes && out.lines().count() <= lines_max) || kept.is_empty() {
            return out;
        }
        if let Some(ch) = kept.pop() {
            for c in ch.1 {
                *not_shown.entry(c).or_default() += 1;
            }
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
    let foot = footer_line(a, &BTreeMap::new(), 0);
    for part in foot.trim_start_matches("# ").split(" · ") {
        let _ = writeln!(out, "{}• {part}{reset}", c("\x1b[2m"));
    }
    let _ = writeln!(
        out,
        "\nlegend: ← enclosing fn ← its callers · [graph-only] reference text search can't see · [test] caller in test code · → definition it resolves to · [unresolved call] graph gap · [mock in test]/[docs]/[string/comment] non-call mentions"
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
    out.push_str(&footer_line(a, &BTreeMap::new(), 0));
    out.push('\n');
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
            more_hint: "graphite-hook run --all -- 'grep -rn f .'".into(),
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
        });
        let t = render_model(&a);
        assert!(t.len() <= 1500, "{} > 1500\n{t}", t.len());
        assert!(t.contains("a.py:1:"), "{t}");
        assert!(t.contains("collapsed per file"), "{t}");
        assert!(
            t.contains("(16 sites)"),
            "every call site kept, collapsed: {t}"
        );
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
        });
        let t = render_model(&a);
        assert!(t.lines().count() <= 12, "{t}");
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
        assert!(human.contains("covering tests: none") && model.contains("covering tests: none"));
    }
}
