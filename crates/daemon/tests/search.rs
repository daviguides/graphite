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
    assert_eq!(stats["excluded_matches"], 1, "{text}");
    // Every text match is accounted for: printed or counted.
    let total: u64 = c
        .as_object()
        .unwrap()
        .values()
        .map(|v| v.as_u64().unwrap())
        .sum();
    assert_eq!(total, res.hits.len() as u64);
    assert!(text.contains("pkg/alias.py:4:    return ro()"), "{text}");
    assert!(
        text.contains("not searched (default excludes): .venv/"),
        "{text}"
    );
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
    assert!(text.contains("# a (function L3-4)"), "{text}");
    assert!(text.contains("# g (function L1-2)"), "{text}");
    assert!(
        text.contains("pkg/use.py:4:    return resolve_owner()"),
        "{text}"
    );
    assert_eq!(stats["matches"], res.hits.len());
}

#[test]
fn caps_disclose_the_rest() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    std::fs::create_dir(root.join(".git")).unwrap();
    let body: String = (0..400).map(|i| format!("# needle {i}\n")).collect();
    write(&root, "notes.txt", &body);
    let engine = Engine::open(RepoPaths::new(&root)).unwrap();
    engine.index_all().unwrap();
    let s = spec(&root, "needle [0-9]+");
    let res = search::run(&s, &root).unwrap();
    let (text, _) = judge::render(&engine, &s, &res, false).unwrap();
    assert!(
        text.contains(
            "+250 more matches — all: graphite-hook run --all -- 'grep -rn needle [0-9]+ .'"
        ),
        "{text}"
    );
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
    assert!(text.contains("→ resolve_owner"), "{text}");
}
