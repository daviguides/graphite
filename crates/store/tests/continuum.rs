//! Real-repo check over all of ~/work/sources/continuum's Python (read-only).
//! Run with `cargo test -p graphite-store --release --test continuum -- --ignored --nocapture`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use graphite_extract_python::{
    extract, DYNAMIC_TARGET, EXTERNAL_PREFIX, MODULE_TARGET, WILDCARD_TARGET,
};
use graphite_model::{EdgeKind, FileFacts, SymbolId, SymbolKind, Target};
use graphite_store::{Adjacency, CozoStore, EdgeKey, GraphStore, Outcome, DEPENDENCY_KINDS};

const SKIP_DIRS: &[&str] = &[
    ".git",
    ".venv",
    "venv",
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    "__pycache__",
];

fn continuum_root() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("HOME")?).join("work/sources/continuum");
    root.is_dir().then_some(root)
}

fn extract_all(root: &Path) -> Vec<FileFacts> {
    let mut files: Vec<PathBuf> = walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            !(e.file_type().is_dir()
                && SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
        })
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "py"))
        .map(|e| e.into_path())
        .collect();
    files.sort();
    files
        .iter()
        .map(|p| {
            let rel = p
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            extract(&rel, &std::fs::read(p).unwrap())
        })
        .collect()
}

struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// The file minus its last function/method and every edge touching it: a "deleted a function" edit.
fn drop_last_function(f: &FileFacts) -> FileFacts {
    let mut out = f.clone();
    let Some(pos) = out
        .symbols
        .iter()
        .rposition(|s| matches!(s.kind, SymbolKind::Function | SymbolKind::Method))
    else {
        return out;
    };
    let gone = out.symbols.remove(pos).id;
    out.edges
        .retain(|e| e.src != gone && !matches!(e.dst, Target::Symbol(d) if d == gone));
    out
}

fn open(dir: &Path) -> CozoStore {
    CozoStore::open(&dir.join("db")).unwrap()
}

/// Outcome per raw-edge class, so the report separates "not in repo" from "could not resolve".
fn breakdown(facts: &[FileFacts], store: &CozoStore) -> BTreeMap<(String, String), usize> {
    let mut class: HashMap<EdgeKey, String> = HashMap::new();
    for f in facts {
        for (i, e) in f.edges.iter().enumerate() {
            let c = match &e.dst {
                Target::Symbol(_) => "in-file",
                Target::Unresolved {
                    name,
                    qualifier,
                    import_path,
                } => {
                    if import_path
                        .as_deref()
                        .is_some_and(|p| p.starts_with(EXTERNAL_PREFIX))
                    {
                        "external"
                    } else if name == DYNAMIC_TARGET {
                        "dynamic"
                    } else if name == MODULE_TARGET || name == WILDCARD_TARGET {
                        "module-import"
                    } else if import_path.is_some() {
                        "import-path"
                    } else if matches!(qualifier.as_deref(), Some("self" | "cls" | "super()")) {
                        "self/cls/super"
                    } else if qualifier.is_some() {
                        "obj.attr"
                    } else {
                        "bare-name"
                    }
                }
            };
            let kind = match e.kind {
                EdgeKind::Calls => "calls",
                EdgeKind::Imports => "imports",
                EdgeKind::Inherits => "inherits",
                EdgeKind::Contains => "contains",
                EdgeKind::References => "references",
                EdgeKind::Tests => "tests",
            };
            class.insert(
                EdgeKey {
                    path: f.path.clone(),
                    idx: i as u32,
                },
                format!("{kind}:{c}"),
            );
        }
    }
    let mut out = BTreeMap::new();
    for r in store.resolve_all().unwrap() {
        let o = match r.outcome {
            Outcome::Resolved { provenance, .. } => provenance.as_str().to_string(),
            Outcome::Ambiguous { .. } => "ambiguous".into(),
            Outcome::Unresolved => "unresolved".into(),
        };
        *out.entry((class[&r.key].clone(), o)).or_insert(0) += 1;
    }
    out
}

fn print_breakdown(b: &BTreeMap<(String, String), usize>) {
    let mut totals: BTreeMap<&str, usize> = BTreeMap::new();
    for ((class, outcome), n) in b {
        println!("  {class:<28} {outcome:<11} {n:>6}");
        *totals.entry(outcome.as_str()).or_insert(0) += n;
    }
    println!("  totals: {totals:?}");
}

fn pct(mut v: Vec<Duration>, p: f64) -> Duration {
    v.sort();
    v[((v.len() - 1) as f64 * p) as usize]
}

#[test]
#[ignore = "heavy: full Continuum build; run with --ignored"]
fn continuum_incremental_equals_full() {
    let Some(root) = continuum_root() else {
        eprintln!("continuum not found; skipping");
        return;
    };
    let t = Instant::now();
    let facts = extract_all(&root);
    println!("extract: {} files in {:?}", facts.len(), t.elapsed());

    let tmp = tempfile::tempdir().unwrap();
    let full_dir = tmp.path().join("full");
    let inc_dir = tmp.path().join("inc");
    std::fs::create_dir_all(&full_dir).unwrap();
    std::fs::create_dir_all(&inc_dir).unwrap();

    let inc = open(&inc_dir);
    let t = Instant::now();
    for f in &facts {
        inc.replace_file(f).unwrap();
    }
    let load = t.elapsed();
    let t = Instant::now();
    let mut adj = Adjacency::rebuild(&inc).unwrap();
    println!(
        "full build: load {load:?} + resolve/adjacency {:?} ({} resolved edges)",
        t.elapsed(),
        adj.edge_count()
    );

    println!("resolution breakdown (edge class, outcome, count):");
    print_breakdown(&breakdown(&facts, &inc));

    // Churn mixing realistic edits: body-only rewrite (same symbols), delete one function, remove the
    // file, re-add it. Incremental store + adjacency must equal a fresh build of the final facts.
    let mut current: Vec<Option<FileFacts>> = facts.iter().cloned().map(Some).collect();
    let mut rng = Rng(0x5eed);
    let mut timings: BTreeMap<&str, (Vec<Duration>, Vec<Duration>)> = BTreeMap::new();
    for _ in 0..120 {
        let i = rng.below(current.len());
        let (op, next) = match (&current[i], rng.below(4)) {
            (None, _) => ("re-add", Some(facts[i].clone())),
            (Some(_), 0) => ("remove", None),
            (Some(f), 1) => ("delete-function", Some(drop_last_function(f))),
            (Some(f), _) => ("body-edit", Some(f.clone())),
        };
        let t = Instant::now();
        let delta = match &next {
            Some(f) => inc.replace_file(f).unwrap(),
            None => inc.remove_file(&facts[i].path).unwrap(),
        };
        let write = t.elapsed();
        let t = Instant::now();
        adj.apply(&inc, &delta).unwrap();
        let entry = timings.entry(op).or_default();
        entry.0.push(write);
        entry.1.push(t.elapsed());
        current[i] = next;
    }
    let mut all_apply = Vec::new();
    for (op, (w, a)) in &timings {
        println!(
            "per-file update [{op:<15}] n={:<3} write p50 {:?} p95 {:?} | adjacency apply p50 {:?} p95 {:?}",
            w.len(),
            pct(w.clone(), 0.5),
            pct(w.clone(), 0.95),
            pct(a.clone(), 0.5),
            pct(a.clone(), 0.95)
        );
        all_apply.extend(a.iter().copied());
    }
    println!(
        "per-file update [all] apply p50 {:?} p95 {:?} max {:?}",
        pct(all_apply.clone(), 0.5),
        pct(all_apply.clone(), 0.95),
        pct(all_apply, 1.0)
    );

    let full = open(&full_dir);
    for f in current.iter().flatten() {
        full.replace_file(f).unwrap();
    }
    let mut a = inc.resolve_all().unwrap();
    let mut b = full.resolve_all().unwrap();
    a.sort();
    b.sort();
    assert!(a == b, "store resolution diverged after churn");
    let rebuilt = Adjacency::rebuild(&full).unwrap();
    let (got, want) = (adj.snapshot(), rebuilt.snapshot());
    if got != want {
        let raw: HashMap<EdgeKey, String> = current
            .iter()
            .flatten()
            .flat_map(|f| {
                f.edges.iter().enumerate().map(move |(i, e)| {
                    (
                        EdgeKey {
                            path: f.path.clone(),
                            idx: i as u32,
                        },
                        format!("{:?}", e.dst),
                    )
                })
            })
            .collect();
        for x in got.symmetric_difference(&want).take(12) {
            let side = if got.contains(x) { "stale" } else { "missing" };
            println!(
                "DIVERGED {side} {}:{} {:?} {:?} {}",
                x.0.path,
                x.0.idx,
                x.3,
                x.4,
                raw.get(&x.0).cloned().unwrap_or_default()
            );
        }
    }
    assert!(got == want, "adjacency diverged after churn");
    assert_eq!(
        adj.test_set(),
        rebuilt.test_set(),
        "test set diverged after churn"
    );

    // Blast radius from the worst hub (most direct dependents).
    let mut fan_in: HashMap<SymbolId, usize> = HashMap::new();
    for (_, _, dst, kind, _) in adj.snapshot() {
        if DEPENDENCY_KINDS.contains(&kind) {
            *fan_in.entry(dst).or_insert(0) += 1;
        }
    }
    let (&hub, &n) = fan_in.iter().max_by_key(|(id, n)| (**n, **id)).unwrap();
    let name = full
        .symbol(hub)
        .unwrap()
        .map(|s| s.qualified)
        .unwrap_or_default();
    for depth in [3u32, 10] {
        let mut times = Vec::new();
        let mut size = 0;
        for _ in 0..50 {
            let t = Instant::now();
            size = adj.blast_radius(hub, depth, DEPENDENCY_KINDS).len();
            times.push(t.elapsed());
        }
        println!(
            "blast radius hub {name} (fan-in {n}) depth {depth}: {size} nodes, p50 {:?} p95 {:?}",
            pct(times.clone(), 0.5),
            pct(times, 0.95)
        );
    }

    // Edit -> visible latency on the hub's own file: write + adjacency apply.
    let hub_path = full.symbol(hub).unwrap().unwrap().path;
    let original = facts.iter().find(|f| f.path == hub_path).unwrap().clone();
    let without_hub = drop_symbol(&original, hub);
    let mut visible = |label: &str, f: &FileFacts| {
        let t = Instant::now();
        let delta = inc.replace_file(f).unwrap();
        adj.apply(&inc, &delta).unwrap();
        println!(
            "hub file {hub_path} [{label}]: edit->visible {:?} ({} names re-resolved)",
            t.elapsed(),
            delta.touched_names.len()
        );
    };
    visible("body edit", &original);
    visible("delete hub function", &without_hub);
    visible("restore hub function", &original);
}

fn drop_symbol(f: &FileFacts, gone: SymbolId) -> FileFacts {
    let mut out = f.clone();
    out.symbols.retain(|s| s.id != gone);
    out.edges
        .retain(|e| e.src != gone && !matches!(e.dst, Target::Symbol(d) if d == gone));
    out
}
