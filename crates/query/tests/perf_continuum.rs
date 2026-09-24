//! Latency of the query functions over a copy of a real Python repo.
//! Run: GRAPHITE_PERF_REPO=~/work/sources/continuum cargo test --release -p graphite-query --test perf_continuum -- --ignored --nocapture

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use graphite_query::{blast_radius, diff_impact, estimate_tokens, Hunk, Options, QueryContext};
use graphite_store::{Adjacency, CozoStore, GraphStore, DEPENDENCY_KINDS};

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

fn time<T>(label: &str, runs: usize, mut f: impl FnMut() -> T) -> T {
    let mut last = None;
    let mut ds: Vec<Duration> = Vec::with_capacity(runs);
    for _ in 0..runs {
        let t = Instant::now();
        last = Some(f());
        ds.push(t.elapsed());
    }
    ds.sort();
    println!(
        "{label:<44} p50 {:>9.2?}  max {:>9.2?}  (n={runs})",
        ds[ds.len() / 2],
        ds[ds.len() - 1]
    );
    last.unwrap()
}

#[test]
#[ignore]
fn perf_continuum() {
    let Some(repo) = std::env::var_os("GRAPHITE_PERF_REPO").map(PathBuf::from) else {
        eprintln!("GRAPHITE_PERF_REPO unset; skipping");
        return;
    };
    let work = tempfile::tempdir().unwrap();
    let root = work.path().join("repo");
    let files = copy_py(&repo, &root);
    let db = tempfile::tempdir().unwrap();
    let store = CozoStore::open(db.path()).unwrap();
    let t = Instant::now();
    for rel in &files {
        let src = std::fs::read(root.join(rel)).unwrap();
        store
            .replace_file(&graphite_extract_python::extract(rel, &src))
            .unwrap();
    }
    let adj = Adjacency::rebuild(&store).unwrap();
    println!("indexed {} files in {:.2?}", files.len(), t.elapsed());

    let ctx = QueryContext {
        store: &store,
        adj: &adj,
        root: &root,
        stale: false,
        parse_failures: 0,
    };
    let opts = Options::default();

    // Worst hub by direct callers, and a typical symbol with a handful of dependents.
    let mut ids: Vec<_> = adj.snapshot().into_iter().map(|e| e.2).collect();
    ids.sort();
    ids.dedup();
    let mut ranked: Vec<_> = ids
        .iter()
        .map(|id| (adj.callers_of(*id).len(), *id))
        .collect();
    ranked.sort_by(|a, b| b.cmp(a));
    let hub = store.symbol(ranked[0].1).unwrap().unwrap();
    let typical = ranked
        .iter()
        .find(|(n, _)| *n == 3)
        .map(|(_, id)| store.symbol(*id).unwrap().unwrap())
        .unwrap();
    let load_yaml = store
        .symbols_by_name("load_yaml")
        .unwrap()
        .into_iter()
        .max_by_key(|s| adj.callers_of(s.id).len());
    println!(
        "hub {} ({} callers); typical {}",
        hub.qualified, ranked[0].0, typical.qualified
    );

    // Component costs.
    let deps = adj.blast_radius(hub.id, 3, DEPENDENCY_KINDS);
    let dep_ids: Vec<_> = deps.iter().map(|d| d.0).collect();
    time(
        &format!("store.symbol one-by-one x{}", dep_ids.len()),
        1,
        || {
            for id in &dep_ids {
                store.symbol(*id).unwrap();
            }
        },
    );
    time(
        &format!("store.symbols batch x{}", dep_ids.len()),
        5,
        || store.symbols(&dep_ids).unwrap().len(),
    );
    time("store.name_gaps (hub name)", 5, || {
        store.name_gaps(&hub.name).unwrap().len()
    });

    // End-to-end queries.
    for (label, sym) in [("hub", &hub), ("typical", &typical)]
        .into_iter()
        .chain(load_yaml.iter().map(|s| ("load_yaml", s)))
    {
        let env = time(&format!("blast_radius {label} (default)"), 5, || {
            blast_radius(&ctx, &sym.qualified, &opts).unwrap()
        });
        println!(
            "   -> total {} tier {:?} tokens {}",
            env.result.dependents.summary.total,
            env.tier,
            estimate_tokens(&env)
        );
        time(&format!("blast_radius {label} (compact d1)"), 5, || {
            blast_radius(
                &ctx,
                &sym.qualified,
                &Options {
                    compact: true,
                    depth: 1,
                    ..Options::default()
                },
            )
            .unwrap()
        });
    }
    let hunk = Hunk {
        path: hub.path.clone(),
        start_line: hub.start_line,
        line_count: 1,
    };
    time("diff_impact (one hunk on hub)", 5, || {
        diff_impact(&ctx, std::slice::from_ref(&hunk), &opts).unwrap()
    });
    time("diff_impact (no hunks)", 5, || {
        diff_impact(&ctx, &[], &opts).unwrap()
    });
}
