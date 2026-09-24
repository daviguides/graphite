use std::path::{Path, PathBuf};

use graphite_query::{
    blast_radius, diff_impact, estimate_tokens, lookup, parse_unified_diff, CompletenessStatus,
    Hunk, LookupStatus, Options, QueryContext, RiskLevel, Role, SourceState, Tier,
};
use graphite_store::{Adjacency, CozoStore, GraphStore};
use serde::Serialize;

struct Fixture {
    _db: tempfile::TempDir,
    store: CozoStore,
    adj: Adjacency,
    root: PathBuf,
}

impl Fixture {
    fn ctx(&self) -> QueryContext<'_> {
        QueryContext {
            store: &self.store,
            adj: &self.adj,
            root: &self.root,
            stale: false,
            parse_failures: 0,
        }
    }
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/repo")
}

fn py_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "py") {
                out.push(
                    p.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    out.sort();
    out
}

fn build(root: &Path) -> Fixture {
    let db = tempfile::tempdir().unwrap();
    let store = CozoStore::open(db.path()).unwrap();
    for rel in py_files(root) {
        let src = std::fs::read(root.join(&rel)).unwrap();
        store
            .replace_file(&graphite_extract_python::extract(&rel, &src))
            .unwrap();
    }
    let adj = Adjacency::rebuild(&store).unwrap();
    Fixture {
        _db: db,
        store,
        adj,
        root: root.to_path_buf(),
    }
}

fn golden<T: Serialize>(name: &str, value: &T) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.json"));
    let actual = serde_json::to_string_pretty(value).unwrap() + "\n";
    if std::env::var_os("UPDATE_GOLDEN").is_some() || !path.exists() {
        std::fs::write(&path, &actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        expected, actual,
        "golden {name} differs; rerun with UPDATE_GOLDEN=1 after checking the diff"
    );
}

fn big_budget() -> Options {
    Options {
        token_budget: 1_000_000,
        ..Default::default()
    }
}

#[test]
fn lookup_found_ambiguous_notfound() {
    let f = build(&fixture_root());
    let ctx = f.ctx();

    let found = lookup(&ctx, "app.core.Config").unwrap();
    assert_eq!(found.result.status, LookupStatus::Found);
    golden("lookup_found", &found);

    let suffix = lookup(&ctx, "core.load").unwrap();
    assert_eq!(suffix.result.status, LookupStatus::Found);

    let amb = lookup(&ctx, "run").unwrap();
    assert_eq!(amb.result.status, LookupStatus::Ambiguous);
    assert_eq!(amb.result.candidates.len(), 2);
    assert!(amb.result.symbol.is_none());
    golden("lookup_ambiguous", &amb);

    let by_id = lookup(&ctx, &found.result.symbol.as_ref().unwrap().id).unwrap();
    assert_eq!(by_id.result.symbol, found.result.symbol);

    assert_eq!(
        lookup(&ctx, "nope").unwrap().result.status,
        LookupStatus::NotFound
    );
}

#[test]
fn blast_radius_golden_and_invariants() {
    let f = build(&fixture_root());
    let ctx = f.ctx();
    let env = blast_radius(&ctx, "app.core.Config", &big_budget()).unwrap();
    golden("blast_config", &env);

    assert_eq!(env.tier, Tier::Full);
    let deps = &env.result.dependents;
    let names: Vec<&str> = deps
        .items
        .iter()
        .map(|i| i.symbol.qualified.as_str())
        .collect();
    assert!(names.contains(&"app.service.build"), "{names:?}");
    assert!(names.contains(&"app.api.handler"), "{names:?}");
    assert!(
        names.contains(&"tests.test_service.test_build"),
        "{names:?}"
    );
    for w in deps.items.windows(2) {
        assert!(w[0].depth <= w[1].depth, "ranked by depth first");
    }
    let build = deps
        .items
        .iter()
        .find(|i| i.symbol.qualified == "app.service.build")
        .unwrap();
    assert_eq!(build.depth, 1);
    assert_eq!(
        deps.summary.total as usize,
        deps.items.len(),
        "tier full lists every dependent"
    );
    assert_eq!(deps.summary.prod + deps.summary.test, deps.summary.total);
    let test = deps
        .items
        .iter()
        .find(|i| i.symbol.qualified.ends_with("test_build"))
        .unwrap();
    assert_eq!(test.symbol.role, Role::Test);
    let src = env.result.source.as_ref().unwrap();
    assert_eq!(src.state, SourceState::Fresh);
    assert!(src.text.starts_with("9| class Config:"), "{}", src.text);
}

#[test]
fn zero_callers_with_ambiguous_refs_is_unknown_not_low() {
    let f = build(&fixture_root());
    let ctx = f.ctx();
    let env = blast_radius(&ctx, "app.jobs.run", &big_budget()).unwrap();
    golden("blast_ambiguous_unknown", &env);
    let risk = env.result.risk.as_ref().unwrap();
    assert_eq!(env.result.dependents.summary.total, 0);
    assert_eq!(risk.level, RiskLevel::Unknown);
    assert_eq!(env.completeness.status, CompletenessStatus::LowerBound);
    let c = &env.completeness.causes;
    assert!(c.ambiguous_refs + c.unresolved_refs >= 1);

    let clean = blast_radius(&ctx, "app.api.unrelated", &big_budget()).unwrap();
    assert_eq!(clean.result.dependents.summary.total, 0);
    let c = &clean.completeness.causes;
    if c.ambiguous_refs + c.unresolved_refs == 0 {
        assert_eq!(clean.result.risk.as_ref().unwrap().level, RiskLevel::Low);
        assert_eq!(clean.completeness.status, CompletenessStatus::Complete);
    }
}

#[test]
fn ambiguous_target_is_not_guessed() {
    let f = build(&fixture_root());
    let env = blast_radius(&f.ctx(), "run", &big_budget()).unwrap();
    assert_eq!(env.result.target.status, LookupStatus::Ambiguous);
    assert!(env.result.risk.is_none());
    assert!(env.result.dependents.items.is_empty());
}

#[test]
fn compression_tiers_under_budget_are_disclosed() {
    let f = build(&fixture_root());
    let ctx = f.ctx();
    let full = blast_radius(&ctx, "app.core.Config", &big_budget()).unwrap();
    let total = full.result.dependents.summary.total;
    assert!(total >= 3);

    let compact = blast_radius(
        &ctx,
        "app.core.Config",
        &Options {
            compact: true,
            token_budget: 1_000_000,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(compact.tier, Tier::Summary);
    assert!(compact
        .result
        .dependents
        .items
        .iter()
        .all(|i| i.source.is_none()));
    assert_eq!(compact.result.dependents.items.len() as u32, total);

    let summary_size = estimate_tokens(&compact);
    let by_file = blast_radius(
        &ctx,
        "app.core.Config",
        &Options {
            token_budget: summary_size - 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(by_file.tier >= Tier::ByFile, "{:?}", by_file.tier);
    assert!(by_file.result.dependents.items.is_empty());
    assert!(by_file
        .disclosures
        .iter()
        .any(|d| d.what == "dependents" && d.omitted == total));
    if by_file.tier == Tier::ByFile {
        let grouped: u32 = by_file
            .result
            .dependents
            .by_file
            .iter()
            .map(|g| g.count)
            .sum();
        assert_eq!(grouped, total, "grouping keeps every dependent counted");
    }

    let tiny = blast_radius(
        &ctx,
        "app.core.Config",
        &Options {
            token_budget: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(tiny.tier, Tier::ByDirectory);
    assert!(tiny.result.dependents.by_directory.is_empty());
    assert!(tiny
        .disclosures
        .iter()
        .any(|d| d.what == "directory groups"));
    assert_eq!(
        tiny.result.dependents.summary.total, total,
        "counts survive every cut"
    );
    golden("blast_config_tiny_budget", &tiny);
}

#[test]
fn unified_diff_parsing() {
    let text = "\
diff --git a/app/core.py b/app/core.py
--- a/app/core.py
+++ b/app/core.py
@@ -5,2 +5,3 @@ def parse(text):
-    return text.split()
+    words = text.split()
+    return words
@@ -14 +15 @@ class Config:
-        return self.data
+        return self.data[key]
diff --git a/gone.py b/gone.py
--- a/gone.py
+++ /dev/null
@@ -1,2 +0,0 @@
-x = 1
diff --git a/new.py b/new.py
--- /dev/null
+++ b/new.py
@@ -0,0 +1 @@
+y = 2
";
    assert_eq!(
        parse_unified_diff(text),
        vec![
            Hunk {
                path: "app/core.py".into(),
                start_line: 5,
                line_count: 3
            },
            Hunk {
                path: "app/core.py".into(),
                start_line: 15,
                line_count: 1
            },
            Hunk {
                path: "new.py".into(),
                start_line: 1,
                line_count: 1
            },
        ]
    );
}

#[test]
fn diff_hunks_map_to_innermost_symbols() {
    let f = build(&fixture_root());
    let ctx = f.ctx();
    let hunks = vec![
        Hunk {
            path: "app/core.py".into(),
            start_line: 6,
            line_count: 1,
        },
        Hunk {
            path: "app/core.py".into(),
            start_line: 14,
            line_count: 1,
        },
        Hunk {
            path: "app/missing.py".into(),
            start_line: 1,
            line_count: 2,
        },
    ];
    let env = diff_impact(&ctx, &hunks, &big_budget()).unwrap();
    golden("diff_impact", &env);
    let changed: Vec<&str> = env
        .result
        .changed
        .iter()
        .map(|c| c.symbol.qualified.as_str())
        .collect();
    assert_eq!(
        changed,
        vec!["app.core.parse", "app.core.Config.get"],
        "innermost only, never the class"
    );
    assert_eq!(env.result.unmapped.len(), 1);
    assert_eq!(env.result.unmapped[0].reason, "file not indexed");
    assert!(env
        .disclosures
        .iter()
        .any(|d| d.what == "hunks" && d.omitted == 1));
    let deps: Vec<&str> = env
        .result
        .dependents
        .items
        .iter()
        .map(|i| i.symbol.qualified.as_str())
        .collect();
    assert!(deps.contains(&"app.core.Config.__init__"), "{deps:?}");
    assert!(
        !deps.contains(&"app.core.parse"),
        "roots are not their own dependents"
    );
}

#[test]
fn diff_impact_collects_covering_tests() {
    let f = build(&fixture_root());
    let hunks = vec![Hunk {
        path: "app/service.py".into(),
        start_line: 5,
        line_count: 1,
    }];
    let env = diff_impact(&f.ctx(), &hunks, &big_budget()).unwrap();
    assert_eq!(env.result.changed[0].symbol.qualified, "app.service.build");
    let tests: Vec<&str> = env
        .result
        .covering_tests
        .iter()
        .map(|t| t.qualified.as_str())
        .collect();
    assert_eq!(tests, vec!["tests.test_service.test_build"]);
    assert!(env.result.changed[0].direct_test_callers >= 1);
}

#[test]
fn changed_file_source_is_withheld_and_counted() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("repo");
    copy_dir(&fixture_root(), &root);
    let f = build(&root);
    let path = root.join("app/core.py");
    let orig = std::fs::read_to_string(&path).unwrap();
    std::fs::write(&path, format!("import os\n{orig}")).unwrap();

    let env = blast_radius(&f.ctx(), "app.core.load", &big_budget()).unwrap();
    assert_eq!(
        env.result.source.as_ref().unwrap().state,
        SourceState::Changed
    );
    assert!(env.result.source.as_ref().unwrap().text.is_empty());
    assert!(env.completeness.causes.source_changed >= 1);
    assert_eq!(env.completeness.status, CompletenessStatus::LowerBound);
}

#[test]
fn stale_context_is_flagged() {
    let f = build(&fixture_root());
    let mut ctx = f.ctx();
    ctx.stale = true;
    let env = blast_radius(&ctx, "app.core.load", &big_budget()).unwrap();
    assert!(env.stale);
    assert!(env.disclosures.iter().any(|d| d.what == "freshness"));
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let p = e.unwrap().path();
        let dst = to.join(p.file_name().unwrap());
        if p.is_dir() {
            copy_dir(&p, &dst);
        } else {
            std::fs::copy(&p, &dst).unwrap();
        }
    }
}
