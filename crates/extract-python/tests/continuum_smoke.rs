//! Smoke run on real code from ~/work/sources/continuum (read-only). Skipped when absent.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use graphite_extract_python::extract;
use graphite_model::{EdgeKind, SymbolKind, Target};

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

fn python_files(root: &Path) -> Vec<PathBuf> {
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
}

#[test]
fn real_files_extract_with_consistent_facts() {
    let Some(root) = continuum_root() else {
        eprintln!("continuum not found; skipping");
        return;
    };
    let all = python_files(&root);
    assert!(!all.is_empty());
    let step = (all.len() / 60).max(1);
    let sample: Vec<_> = all.iter().step_by(step).collect();

    let (mut ok, mut symbols, mut calls) = (0usize, 0usize, 0usize);
    for path in &sample {
        let rel = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let source = std::fs::read(path).unwrap();
        let facts = extract(&rel, &source);
        ok += usize::from(facts.parse_ok);
        symbols += facts.symbols.len();
        calls += facts
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Calls)
            .count();

        let ids: HashSet<_> = facts.symbols.iter().map(|s| s.id).collect();
        assert_eq!(
            ids.len(),
            facts.symbols.len(),
            "{rel}: duplicate symbol ids"
        );
        assert_eq!(
            facts
                .symbols
                .iter()
                .filter(|s| s.kind == SymbolKind::Module)
                .count(),
            1,
            "{rel}"
        );
        for e in &facts.edges {
            assert!(ids.contains(&e.src), "{rel}: edge src not a local symbol");
            if let Target::Symbol(dst) = &e.dst {
                assert!(ids.contains(dst), "{rel}: edge dst not a local symbol");
            }
        }
        let contains = facts
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::Contains)
            .count();
        assert_eq!(
            contains,
            facts.symbols.len() - 1,
            "{rel}: one contains edge per non-module symbol"
        );
    }
    eprintln!(
        "sample={} parse_ok={ok} symbols={symbols} calls={calls}",
        sample.len()
    );
    assert!(ok * 100 >= sample.len() * 95, "parse_ok below 95%");
    assert!(symbols > sample.len(), "implausibly few symbols");
    assert!(calls > sample.len(), "implausibly few calls");
}
