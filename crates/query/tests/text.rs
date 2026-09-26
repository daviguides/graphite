//! Agent text format and call-site lines.

use std::path::{Path, PathBuf};

use graphite_query::{
    blast_radius, diff_impact, lookup, render_text, Hunk, Options, QueryContext, TextOptions,
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

fn shapes() -> Fixture {
    build(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/shapes"))
}

fn text<T: Serialize>(env: &T) -> String {
    render_text(&serde_json::to_value(env).unwrap(), TextOptions::default())
}

fn golden(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() || !path.exists() {
        std::fs::write(&path, actual).unwrap();
        return;
    }
    let expected = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        expected, actual,
        "golden {name} differs; rerun with UPDATE_GOLDEN=1 after checking the diff"
    );
}

#[test]
fn call_sites_carry_every_line() {
    let f = shapes();
    let env = blast_radius(&f.ctx(), "shop.util.fmt", &Options::default()).unwrap();
    let r = &env.result;
    let lines: Vec<(&str, Vec<u32>)> = r
        .call_sites
        .iter()
        .map(|c| (c.caller.qualified.as_str(), c.lines.clone()))
        .collect();
    assert_eq!(
        lines,
        vec![
            ("shop.handlers.JsonHandler.handle", vec![7]),
            ("shop.handlers.XmlHandler.handle", vec![12]),
            ("shop.util.render", vec![6, 7, 8]),
        ]
    );
    assert_eq!(r.direct.callers, 3);
    assert_eq!(r.direct.sites, 5);
    assert_eq!(r.direct.files, 2);
    let render = r
        .call_sites
        .iter()
        .find(|c| c.caller.qualified == "shop.util.render")
        .unwrap();
    assert_eq!(render.called_by_total, 3);
    let render_dep = r
        .dependents
        .items
        .iter()
        .find(|i| i.symbol.qualified == "shop.util.render")
        .unwrap();
    assert_eq!(
        render_dep.lines,
        vec![6, 7, 8],
        "dependent items carry lines too"
    );
    let serve = r
        .dependents
        .items
        .iter()
        .find(|i| i.symbol.qualified == "shop.api.serve_twice")
        .unwrap();
    assert_eq!(serve.depth, 2);
    assert_eq!(serve.lines, vec![9, 10], "lines into its via symbol");
    assert_eq!(
        r.tests
            .items
            .iter()
            .map(|t| t.node_id.as_str())
            .collect::<Vec<_>>(),
        vec!["tests/test_util.py::test_render"]
    );
}

#[test]
fn method_overrides_both_directions() {
    let f = shapes();
    let ctx = f.ctx();
    let base = blast_radius(&ctx, "shop.base.Handler.handle", &Options::default()).unwrap();
    let ov: Vec<(&str, &str)> = base
        .result
        .overrides
        .iter()
        .map(|o| (o.relation, o.symbol.qualified.as_str()))
        .collect();
    assert_eq!(
        ov,
        vec![
            ("overridden_by", "shop.handlers.JsonHandler.handle"),
            ("overridden_by", "shop.handlers.XmlHandler.handle"),
        ]
    );
    let mid = blast_radius(
        &ctx,
        "shop.handlers.JsonHandler.handle",
        &Options::default(),
    )
    .unwrap();
    let ov: Vec<(&str, &str)> = mid
        .result
        .overrides
        .iter()
        .map(|o| (o.relation, o.symbol.qualified.as_str()))
        .collect();
    assert_eq!(
        ov,
        vec![
            ("overridden_by", "shop.handlers.XmlHandler.handle"),
            ("overrides", "shop.base.Handler.handle"),
        ]
    );
    golden("text_blast_override", &text(&base));
}

#[test]
fn text_goldens_small() {
    let f = shapes();
    let ctx = f.ctx();
    let blast = text(&blast_radius(&ctx, "fmt", &Options::default()).unwrap());
    assert!(blast.len() < 2048, "{} bytes:\n{blast}", blast.len());
    let first: Vec<&str> = blast.lines().take(2).collect();
    assert!(
        first[0].starts_with("def fmt(x)  shop/util.py:1  [function]"),
        "{blast}"
    );
    assert!(
        first[1].starts_with(
            "COMPLETE · 5 call sites in 3 callers across 2 files (imported in 1 file)"
        ),
        "{blast}"
    );
    assert!(
        blast.contains("shop/util.py:6,7,8  in render  ← called by"),
        "{blast}"
    );
    golden("text_blast_fmt", &blast);

    golden("text_lookup_found", &text(&lookup(&ctx, "render").unwrap()));
    golden(
        "text_lookup_ambiguous",
        &text(&lookup(&ctx, "handle").unwrap()),
    );
    golden(
        "text_lookup_not_found",
        &text(&lookup(&ctx, "nope").unwrap()),
    );

    let hunks = vec![
        Hunk {
            path: "shop/util.py".into(),
            start_line: 2,
            line_count: 1,
        },
        Hunk {
            path: "README.md".into(),
            start_line: 1,
            line_count: 1,
        },
    ];
    let diff = text(&diff_impact(&ctx, &hunks, &Options::default()).unwrap());
    assert!(diff.len() < 2048, "{} bytes:\n{diff}", diff.len());
    assert!(diff.contains("tests to run: 1"), "{diff}");
    golden("text_diff_fmt", &diff);
}

#[test]
fn json_keeps_full_envelope_with_call_sites() {
    let f = shapes();
    let env = blast_radius(&f.ctx(), "fmt", &Options::default()).unwrap();
    let v = serde_json::to_value(&env).unwrap();
    let r = &v["result"];
    assert!(r["dependents"]["items"].is_array());
    assert!(r["call_sites"][0]["lines"].is_array());
    assert_eq!(r["direct"]["sites"], 5);
}

/// Synthetic hub: `load_yaml` called from 40 modules, twice each, plus callers of those callers.
fn hub_repo(dir: &Path) {
    let w = |rel: &str, body: String| {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    };
    w(
        "core/io.py",
        "def load_yaml(path):\n    return path\n".into(),
    );
    for i in 0..40 {
        w(
            &format!("svc/m{i:02}.py"),
            format!(
                "from core.io import load_yaml\n\n\ndef read_{i}(p):\n    return load_yaml(p)\n\n\ndef use_{i}(p):\n    x = read_{i}(p)\n    return load_yaml(x)\n"
            ),
        );
    }
    let mut main = String::new();
    for i in 0..10 {
        main.push_str(&format!("from svc.m{i:02} import use_{i}\n"));
    }
    main.push_str("\n\ndef main(p):\n");
    for i in 0..10 {
        main.push_str(&format!("    use_{i}(p)\n"));
    }
    w("app/main.py", main);
    for i in 0..5 {
        w(
            &format!("tests/test_m{i:02}.py"),
            format!("from svc.m{i:02} import read_{i}\n\n\ndef test_read_{i}():\n    assert read_{i}(1) == 1\n"),
        );
    }
}

#[test]
fn text_golden_hub_stays_small() {
    let tmp = tempfile::tempdir().unwrap();
    hub_repo(tmp.path());
    let f = build(tmp.path());
    let ctx = f.ctx();
    let opts = Options {
        compact: true,
        ..Default::default()
    };
    let env = blast_radius(&ctx, "load_yaml", &opts).unwrap();
    assert_eq!(env.result.direct.callers, 80);
    assert_eq!(env.result.direct.sites, 80);
    let hub = text(&env);
    let json = serde_json::to_string(&env).unwrap();
    println!("hub text {} bytes, json {} bytes", hub.len(), json.len());
    assert!(hub.len() < 3072, "{} bytes:\n{hub}", hub.len());
    assert!(
        hub.lines().nth(1).unwrap().starts_with(
            "COMPLETE · 80 call sites in 80 callers across 40 files (imported in 40 files)"
        ),
        "{hub}"
    );
    assert!(hub.contains("… 68 more callers"), "{hub}");
    golden("text_blast_hub", &hub);

    let all = render_text(
        &serde_json::to_value(&env).unwrap(),
        TextOptions { all: true },
    );
    let listed = all.lines().filter(|l| l.contains("  in ")).count();
    assert_eq!(
        listed,
        env.result.call_sites.len(),
        "--all lists every call site kept"
    );
}
