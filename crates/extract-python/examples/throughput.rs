//! Extraction throughput and fact stats over every Python file under a root:
//! `cargo run --release --example throughput -- ~/work/sources/continuum`

use std::path::PathBuf;
use std::time::Instant;

use graphite_extract_python::{extract, EXTERNAL_PREFIX};
use graphite_model::{EdgeKind, FileFacts, Target};

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

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("usage: throughput <root>"));
    let mut files: Vec<(String, Vec<u8>)> = walkdir::WalkDir::new(&root)
        .into_iter()
        .filter_entry(|e| {
            !(e.file_type().is_dir()
                && SKIP_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
        })
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|x| x == "py"))
        .map(|e| {
            let rel = e
                .path()
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let bytes = std::fs::read(e.path()).unwrap();
            (rel, bytes)
        })
        .collect();
    files.sort();
    let bytes: usize = files.iter().map(|(_, b)| b.len()).sum();

    // warm-up: grammar + query compilation
    let _ = extract("warmup.py", b"def f():\n    pass\n");

    let t = Instant::now();
    let facts: Vec<FileFacts> = files.iter().map(|(p, b)| extract(p, b)).collect();
    let single = t.elapsed();

    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let t = Instant::now();
    std::thread::scope(|s| {
        for chunk in files.chunks(files.len().div_ceil(threads)) {
            s.spawn(move || {
                chunk
                    .iter()
                    .map(|(p, b)| extract(p, b).symbols.len())
                    .sum::<usize>()
            });
        }
    });
    let parallel = t.elapsed();

    let n = files.len() as f64;
    println!("files={} bytes={:.1}MB", files.len(), bytes as f64 / 1e6);
    println!(
        "single-thread: {:.0} ms  {:.0} files/s  {:.1} MB/s",
        single.as_secs_f64() * 1e3,
        n / single.as_secs_f64(),
        bytes as f64 / 1e6 / single.as_secs_f64()
    );
    println!(
        "{threads} threads:    {:.0} ms  {:.0} files/s",
        parallel.as_secs_f64() * 1e3,
        n / parallel.as_secs_f64()
    );

    let parse_ok = facts.iter().filter(|f| f.parse_ok).count();
    let symbols: usize = facts.iter().map(|f| f.symbols.len()).sum();
    println!("parse_ok={parse_ok}/{} symbols={symbols}", facts.len());
    for kind in EdgeKind::ALL {
        let edges: Vec<_> = facts
            .iter()
            .flat_map(|f| &f.edges)
            .filter(|e| e.kind == kind)
            .collect();
        if edges.is_empty() {
            continue;
        }
        let in_file = edges
            .iter()
            .filter(|e| matches!(e.dst, Target::Symbol(_)))
            .count();
        let external = edges
            .iter()
            .filter(|e| matches!(&e.dst, Target::Unresolved { import_path: Some(p), .. } if p.starts_with(EXTERNAL_PREFIX)))
            .count();
        let with_path = edges
            .iter()
            .filter(|e| matches!(&e.dst, Target::Unresolved { import_path: Some(p), .. } if !p.starts_with(EXTERNAL_PREFIX)))
            .count();
        let bare = edges.len() - in_file - external - with_path;
        println!(
            "{:<10} total={:>6} in_file={:>6} external={:>6} unresolved_with_import_path={:>6} unresolved_bare={:>6}",
            kind.as_str(),
            edges.len(),
            in_file,
            external,
            with_path,
            bare
        );
    }
}
