//! Text vs JSON answer sizes over a copy of a real Python repo.
//! Run: GRAPHITE_PERF_REPO=~/work/sources/continuum cargo test --release -p graphite-query --test text_continuum -- --ignored --nocapture

use std::path::{Path, PathBuf};

use graphite_query::{blast_radius, lookup, render_text, Options, QueryContext, TextOptions};
use graphite_store::{Adjacency, CozoStore, GraphStore};

const SKIP: &[&str] = &[
    ".git",
    ".venv",
    "venv",
    "node_modules",
    "target",
    "dist",
    "build",
    "__pycache__",
    "site-packages",
    ".worktrees",
];

fn copy_py(src: &Path, dst: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![src.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if !SKIP.contains(&name.as_str()) && !name.starts_with('.') {
                    stack.push(p);
                }
            } else if p.extension().is_some_and(|x| x == "py") {
                let rel = p
                    .strip_prefix(src)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let to = dst.join(&rel);
                std::fs::create_dir_all(to.parent().unwrap()).unwrap();
                std::fs::copy(&p, &to).unwrap();
                out.push(rel);
            }
        }
    }
    out.sort();
    out
}

/// Text answers stay small on the real hub and on the pilot's queries.
#[test]
#[ignore]
fn text_sizes_continuum() {
    let Some(repo) = std::env::var_os("GRAPHITE_PERF_REPO").map(PathBuf::from) else {
        eprintln!("GRAPHITE_PERF_REPO unset; skipping");
        return;
    };
    let work = tempfile::tempdir().unwrap();
    let root = work.path().join("repo");
    let files = copy_py(&repo, &root);
    let db = tempfile::tempdir().unwrap();
    let store = CozoStore::open(db.path()).unwrap();
    for rel in &files {
        let src = std::fs::read(root.join(rel)).unwrap();
        store
            .replace_file(&graphite_extract_python::extract(rel, &src))
            .unwrap();
    }
    let adj = Adjacency::rebuild(&store).unwrap();
    let ctx = QueryContext {
        store: &store,
        adj: &adj,
        root: &root,
        stale: false,
        parse_failures: 0,
    };

    let full = |depth: u32| Options {
        depth,
        ..Default::default()
    };
    let text_opts = |depth: u32| Options {
        depth,
        compact: true,
        ..Default::default()
    };
    let mut hub_text = 0;
    for (sym, depth) in [
        ("dao_cli.core.yaml_io.load_yaml", 3),
        ("resolve_owner", 1),
        ("post_hook_validate_pr_target", 2),
        (
            "sourcerer.wt.integrations.filesystem._resolve_externalized_worktrees_root",
            3,
        ),
        ("find_worktrees_root", 1),
    ] {
        let json = serde_json::to_string(&blast_radius(&ctx, sym, &full(depth)).unwrap()).unwrap();
        let env = blast_radius(&ctx, sym, &text_opts(depth)).unwrap();
        let text = render_text(&serde_json::to_value(&env).unwrap(), TextOptions::default());
        println!(
            "blast {sym:<40} depth {depth}  json {:>6} B  text {:>5} B",
            json.len(),
            text.len()
        );
        println!("{text}");
        if sym.ends_with("load_yaml") {
            hub_text = text.len();
        }
    }
    for sym in ["_pin_model", "resolve_owner"] {
        let env = lookup(&ctx, sym).unwrap();
        let json = serde_json::to_string(&env).unwrap();
        let text = render_text(&serde_json::to_value(&env).unwrap(), TextOptions::default());
        println!(
            "lookup {sym:<39}          json {:>6} B  text {:>5} B",
            json.len(),
            text.len()
        );
        println!("{text}");
    }
    assert!(hub_text > 0 && hub_text < 4096, "hub text {hub_text} bytes");
}
