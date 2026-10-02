use std::path::Path;

use graphite_daemon::search::{self, SearchSpec};
use graphite_daemon::{judge, Engine, RepoPaths};

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    std::fs::create_dir(root.join(".git")).unwrap();
    write(&root, "pkg/__init__.py", "");
    write(&root, "pkg/core.py", "def resolve_owner():\n    return 1\n");
    write(
        &root,
        "pkg/use.py",
        "from pkg.core import resolve_owner\n\ndef a():\n    return resolve_owner()\n\n\
         def b():\n    # resolve_owner is called above\n    s = \"resolve_owner\"\n    \
         f = resolve_owner\n    return _resolve_owners()\n",
    );
    write(
        &root,
        "pkg/alias.py",
        "from pkg.core import resolve_owner as ro\n\ndef c():\n    return ro()\n",
    );
    write(
        &root,
        "pkg/amb.py",
        "def g(obj):\n    return obj.resolve_owner()\n",
    );
    write(&root, "README.md", "call resolve_owner to find the owner\n");
    write(&root, "web/x.ts", "resolve_owner();\n");
    write(&root, ".venv/lib/y.py", "resolve_owner()\n");
    (d, root)
}

fn spec(root: &Path, pat: &str) -> SearchSpec {
    SearchSpec {
        patterns: vec![pat.into()],
        paths: vec![root.to_string_lossy().into()],
        cwd: root.to_string_lossy().into(),
        label: format!("grep -rn {pat} ."),
        ..Default::default()
    }
}

#[test]
fn identifier_search_accounts_for_every_match() {
    let (_d, root) = fixture();
    let engine = Engine::open(RepoPaths::new(&root)).unwrap();
    engine.index_all().unwrap();
    let s = spec(&root, "resolve_owner");
    let res = search::run(&s, &root).unwrap();
    let (text, stats) = judge::render(&engine, &s, &res, false).unwrap();
    let c = &stats["classes"];
    assert_eq!(c["definition"], 1, "{text}");
    // one call and obj.resolve_owner() bound by unique-name guess; two import lines
    assert_eq!(c["reference"], 2, "{text}");
    assert_eq!(c["import"], 2, "{text}");
    assert_eq!(c["string_or_comment"], 2, "{text}");
    assert_eq!(c["code_untracked"], 1, "{text}"); // f = resolve_owner
    assert_eq!(c["other_identifier"], 1, "{text}"); // _resolve_owners
    assert_eq!(c["docs_config"], 1, "{text}");
    assert_eq!(c["other_language"], 1, "{text}");
    assert_eq!(stats["alias_refs"], 1, "{text}"); // ro() via `as ro`
    assert_eq!(stats["secrets_skipped"], 0, "{text}");
    // Every text match is accounted for: printed or counted.
    let total: u64 = c
        .as_object()
        .unwrap()
        .values()
        .map(|v| v.as_u64().unwrap())
        .sum();
    assert_eq!(total, res.hits.len() as u64);
    assert!(text.contains("pkg/alias.py:4:    return ro()"), "{text}");
    // .venv is noise: never searched, never mentioned.
    assert!(!text.contains(".venv"), "{text}");
    assert!(text.contains("name guess"), "{text}");
    assert!(!text.contains(".git/"), "{text}");
}

#[test]
fn non_identifier_patterns_group_by_enclosing_symbol() {
    let (_d, root) = fixture();
    let engine = Engine::open(RepoPaths::new(&root)).unwrap();
    engine.index_all().unwrap();
    let mut s = spec(&root, "return .*\\(\\)");
    s.label = "grep -rnE 'return .*\\(\\)' .".into();
    let res = search::run(&s, &root).unwrap();
    let (text, stats) = judge::render(&engine, &s, &res, false).unwrap();
    assert_eq!(stats["mode"], "grouped");
    assert!(
        text.contains("pkg/use.py:4:    return resolve_owner()    [in a]"),
        "{text}"
    );
    assert!(
        text.contains("pkg/amb.py:2:    return obj.resolve_owner()    [in g]"),
        "{text}"
    );
    assert_eq!(stats["matches"], res.hits.len());
}

// kinhin: decision(ref="docs/foundation/interception.md#4-size-budget-never-byte-cut")
#[test]
fn default_budget_discloses_the_rest() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    std::fs::create_dir(root.join(".git")).unwrap();
    let body: String = (0..400).map(|i| format!("# needle {i}\n")).collect();
    write(&root, "notes.txt", &body);
    let engine = Engine::open(RepoPaths::new(&root)).unwrap();
    engine.index_all().unwrap();
    let s = spec(&root, "needle [0-9]+");
    let res = search::run(&s, &root).unwrap();
    let (text, stats) = judge::render(&engine, &s, &res, false).unwrap();
    let grep_bytes = stats["raw_bytes"].as_u64().unwrap() as usize;
    // `notes.txt:N:# needle N` for 400 lines, what `grep -rn` prints.
    let plain: usize = (0..400)
        .map(|i| format!("notes.txt:{}:# needle {i}\n", i + 1).len())
        .sum();
    assert_eq!(grep_bytes, plain);
    let head: usize = text
        .lines()
        .take_while(|l| l.starts_with("# graphite:"))
        .map(|l| l.len() + 1)
        .sum();
    assert!(
        text.len() - head <= grep_bytes + graphite_daemon::answer::overhead(grep_bytes),
        "{}",
        text.len()
    );
    let shown = text.lines().filter(|l| l.starts_with("notes.txt:")).count();
    let footer = text.lines().last().unwrap();
    let hidden: usize = footer
        .split("not shown: docs ")
        .nth(1)
        .and_then(|r| r.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("{footer}"));
    assert_eq!(shown + hidden, 400, "{footer}");
    assert!(
        footer.contains("— to see them: grep -rn needle [0-9]+ . | head -c "),
        "{footer}"
    );
}

// kinhin: decision(ref="docs/foundation/interception.md#4-size-budget-never-byte-cut")
#[test]
fn a_line_budget_also_costs_at_most_those_grep_lines() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    std::fs::create_dir(root.join(".git")).unwrap();
    let body: String = (0..400).map(|i| format!("# needle {i}\n")).collect();
    write(&root, "notes.txt", &body);
    let engine = Engine::open(RepoPaths::new(&root)).unwrap();
    engine.index_all().unwrap();
    let mut s = spec(&root, "needle [0-9]+");
    s.budget = Some(graphite_daemon::answer::Budget {
        lines: Some(40),
        source: "head -40".into(),
        ..Default::default()
    });
    let res = search::run(&s, &root).unwrap();
    let (text, stats) = judge::render(&engine, &s, &res, false).unwrap();
    // A tenth of the plain grep output (`| head -40` of 400 lines), plus the bounded overhead,
    // on top of the header.
    let share = stats["raw_bytes"].as_u64().unwrap() as usize * 40 / 400;
    let head: usize = text
        .lines()
        .take_while(|l| l.starts_with("# graphite:"))
        .map(|l| l.len() + 1)
        .sum();
    let body = text.len() - head;
    assert!(
        body <= share + graphite_daemon::answer::overhead(share),
        "{body}\n{text}"
    );
    assert!(text.lines().count() <= 40, "{text}");
}

// kinhin: incident(ref="bench/interception replay of pilot C, 2026-10-02: grep -n X -A70 FILE | head -130 lost its only match")
#[test]
fn single_file_context_search_keeps_its_match_under_the_default_budget() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    std::fs::create_dir(root.join(".git")).unwrap();
    let body: String = std::iter::once("def recorder():\n".to_string())
        .chain((0..80).map(|i| format!("    step_{i} = {i}\n")))
        .collect();
    write(&root, "tests/test_delivery.py", &body);
    let engine = Engine::open(RepoPaths::new(&root)).unwrap();
    engine.index_all().unwrap();
    let file = root.join("tests/test_delivery.py");
    let mut s = spec(&root, "def recorder");
    s.paths = vec![file.to_string_lossy().into()];
    s.after = 70;
    s.no_filename = true;
    s.label = "grep -n \"def recorder\" -A70 tests/test_delivery.py".into();
    let res = search::run(&s, &root).unwrap();
    let (text, _) = judge::render(&engine, &s, &res, false).unwrap();
    // grep's own shape for one file: no path on the lines (the header names the file).
    assert!(text.contains("\n1:def recorder():"), "{text}");
    assert!(text.contains("\n71-    step_69 = 69\n"), "{text}");
    assert!(!text.contains("tests/test_delivery.py:1:"), "{text}");
}

// kinhin: decision(ref="docs/foundation/interception.md#2-the-answer-an-enriched-grep")
#[test]
fn alternation_of_identifiers_gets_a_verdict_per_name() {
    let (_d, root) = fixture();
    write(&root, "other/root.py", "def find_root():\n    return 2\n");
    write(
        &root,
        "other/use3.py",
        "from other.root import find_root\n\ndef z():\n    return find_root()\n",
    );
    let engine = Engine::open(RepoPaths::new(&root)).unwrap();
    engine.index_all().unwrap();
    // `grep -rn "resolve_owner\|find_root" .` and `rg -e resolve_owner -e find_root`
    let mut alt = spec(&root, "resolve_owner|find_root");
    alt.label = "grep -rn \"resolve_owner\\|find_root\" .".into();
    let mut two = spec(&root, "resolve_owner");
    two.patterns.push("find_root".into());
    for s in [alt, two] {
        let res = search::run(&s, &root).unwrap();
        let (text, stats) = judge::render(&engine, &s, &res, false).unwrap();
        assert_eq!(stats["mode"], "identifier", "{text}");
        let header = text.lines().next().unwrap();
        assert!(
            header.contains(" · 2 identifiers: `resolve_owner` "),
            "{header}"
        );
        assert!(
            header.contains(
                "`find_root` graph COMPLETE, def other/root.py:1 · 1 call sites in 1 files"
            ),
            "{header}"
        );
        assert_eq!(stats["names"][1]["verdict"], "complete", "{stats}");
        // Each call site annotated by the graph of its own name.
        assert!(
            text.contains("other/use3.py:4:    return find_root()    ← z"),
            "{text}"
        );
        assert!(
            text.contains("pkg/use.py:4:    return resolve_owner()    ← a"),
            "{text}"
        );
        assert!(
            text.contains("other/root.py:1:def find_root():    [definition]"),
            "{text}"
        );
        assert!(
            text.contains("pkg/alias.py:4:    return ro()"),
            "graph-only kept: {text}"
        );
        let total: u64 = stats["classes"]
            .as_object()
            .unwrap()
            .values()
            .map(|v| v.as_u64().unwrap())
            .sum();
        assert_eq!(total, res.hits.len() as u64, "every match judged");
        assert!(
            footer(&text).contains("no test covers `resolve_owner`, `find_root` ⚠"),
            "{text}"
        );
        // One definition per name: both bodies come after the footer.
        assert!(text.contains("# graphite: body of find_root (other/root.py:1-"), "{text}");
        assert!(text.contains("# graphite: body of resolve_owner (pkg/core.py:1-"), "{text}");
    }
}

#[test]
fn imports_counted_and_multi_definitions_listed() {
    let (_d, root) = fixture();
    write(
        &root,
        "other/core.py",
        "def resolve_owner(x):\n    return x\n",
    );
    write(
        &root,
        "other/use2.py",
        "from other.core import resolve_owner\n\ndef z():\n    return resolve_owner(1)\n",
    );
    let engine = Engine::open(RepoPaths::new(&root)).unwrap();
    engine.index_all().unwrap();
    let s = spec(&root, "resolve_owner");
    let res = search::run(&s, &root).unwrap();
    let (text, _) = judge::render(&engine, &s, &res, false).unwrap();
    assert!(text.contains("has 2 definitions"), "{text}");
    assert!(text.contains("imports: "), "{text}");
    assert!(!text.contains("pkg/use.py:1:from pkg.core"), "{text}");
    assert!(
        text.contains("pkg/use.py:4:    return resolve_owner()    ← a → pkg/core.py:1"),
        "{text}"
    );
    assert!(
        text.contains("other/use2.py:4:    return resolve_owner(1)    ← z → other/core.py:1"),
        "{text}"
    );
}

/// The footer: the last `#` line that is not a body's.
fn footer(text: &str) -> &str {
    text.lines()
        .filter(|l| l.starts_with("# ") && !l.starts_with("# graphite: body of"))
        .last()
        .unwrap_or_default()
}

fn build(root: &Path, s: &SearchSpec) -> (graphite_daemon::answer::Answer, String) {
    let engine = Engine::open(RepoPaths::new(root)).unwrap();
    engine.index_all().unwrap();
    let res = search::run(s, root).unwrap();
    let (a, _) = judge::build(&engine, s, &res, false).unwrap();
    let text = graphite_daemon::answer::render_model(&a);
    (a, text)
}

#[test]
fn integrated_list_is_relevance_ordered_single_list() {
    let (_d, root) = fixture();
    let (a, text) = build(&root, &spec(&root, "resolve_owner"));
    let first = text.lines().nth(1).unwrap();
    assert!(
        first.starts_with("pkg/core.py:1:def resolve_owner"),
        "{text}"
    );
    assert!(first.ends_with("[definition]"), "{text}");
    // one header, one footer, everything else is a self-contained `path:line:` match line or a
    // `path-line-` body line (grep's context shape)
    let body: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    let context_line = |l: &str| {
        l.split_once(".py-")
            .is_some_and(|(_, r)| r.split('-').next().unwrap().parse::<u32>().is_ok())
    };
    assert!(
        body.iter().all(|l| l.contains(':') || context_line(l)),
        "{text}"
    );
    assert_eq!(
        text.lines()
            .filter(|l| l.starts_with("# ") && !l.starts_with("# graphite: body of"))
            .count(),
        2,
        "{text}"
    );
    // Bodies after the footer, in grep's context shape: the definition first, then the
    // production callers' functions.
    let bodies: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("# graphite: body of"))
        .collect();
    assert!(
        bodies[0].starts_with("# graphite: body of resolve_owner (pkg/core.py:1-2)"),
        "{text}"
    );
    assert!(text.contains("pkg/core.py-1-def resolve_owner():\npkg/core.py-2-    return 1\n"));
    assert!(text.contains("# graphite: body of a (pkg/use.py:3-4)"), "{text}");
    assert!(text.find("# no test covers").unwrap() < text.find("# graphite: body of").unwrap());
    assert!(text.lines().next().unwrap().contains("graph "), "{text}");
    // ranks never decrease
    let ranks: Vec<u8> = a
        .items
        .iter()
        .map(|i| graphite_daemon::answer::rank(&i.class, i.test))
        .collect();
    assert!(ranks.windows(2).all(|w| w[0] <= w[1]), "{ranks:?}");
}

#[test]
fn drop_tests_is_semantic_and_disclosed() {
    let (_d, root) = fixture();
    write(&root, "tests/test_core.py", "from unittest.mock import patch\nfrom pkg.use import a\n\ndef test_x():\n    with patch(\"pkg.core.resolve_owner\"):\n        a()\n");
    let mut s = spec(&root, "resolve_owner");
    s.drop_tests = Some("grep -v test".into());
    let (_a, text) = build(&root, &s);
    assert!(!text.contains("tests/test_core.py"), "{text}");
    assert!(
        text.contains("`grep -v test` → 1 test matches (1 mocks) omitted"),
        "{text}"
    );
}

#[test]
fn multiline_patch_target_is_a_mock() {
    let (_d, root) = fixture();
    write(&root, "tests/test_m.py", "from unittest.mock import patch\n\ndef test_m():\n    with patch(\n        \"pkg.core.resolve_owner\",\n    ):\n        pass\n");
    let (_a, text) = build(&root, &spec(&root, "resolve_owner"));
    assert!(
        text.contains("tests/test_m.py:5:        \"pkg.core.resolve_owner\",    [mock in test]"),
        "{text}"
    );
}

#[test]
fn line_filter_keeps_header_and_footer() {
    let (_d, root) = fixture();
    let mut s = spec(&root, "resolve_owner");
    s.line_filters = vec![graphite_daemon::answer::LineFilter {
        pattern: "alias".into(),
        source: "grep alias".into(),
        ..Default::default()
    }];
    let (_a, text) = build(&root, &s);
    let body: Vec<&str> = text.lines().filter(|l| !l.starts_with('#')).collect();
    assert_eq!(body.len(), 1, "{text}");
    assert!(body[0].starts_with("pkg/alias.py:4:"), "{text}");
    assert!(text.starts_with("# graphite:"), "{text}");
    assert!(
        text.lines()
            .last()
            .unwrap()
            .contains("no test covers `resolve_owner` ⚠"),
        "{text}"
    );
}

#[test]
fn budget_replaces_byte_cut() {
    let (_d, root) = fixture();
    let mut s = spec(&root, "resolve_owner");
    s.budget = Some(graphite_daemon::answer::Budget {
        bytes: Some(900),
        lines: None,
        source: "head -c 900".into(),
        ..Default::default()
    });
    let (_a, text) = build(&root, &s);
    assert!(text.len() <= 900, "{}\n{text}", text.len());
    assert!(text.contains("pkg/core.py:1:def resolve_owner"), "{text}");
    assert!(
        text.contains("`head -c 900` applied as answer budget"),
        "{text}"
    );
    assert!(text.lines().last().unwrap().starts_with("# "), "{text}");
}
