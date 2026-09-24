use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use graphite_model::{
    EdgeKind, FileFacts, Lang, Provenance, RawEdge, Symbol, SymbolId, SymbolKind, Target,
};
use graphite_store::{Adjacency, CozoStore, GraphStore, Outcome, Resolution, DEPENDENCY_KINDS};

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const NAMES: &[&str] = &["load", "save", "parse", "run", "check", "Config"];

fn module(file: u64) -> String {
    format!("pkg.m{file}")
}

fn sym(path: &str, qualified: &str, name: &str, kind: SymbolKind, line: u32) -> Symbol {
    Symbol {
        id: SymbolId::new(Lang::Python, path, qualified, kind, 0),
        lang: Lang::Python,
        path: path.to_string(),
        name: name.to_string(),
        qualified: qualified.to_string(),
        kind,
        start_line: line,
        end_line: line + 2,
        start_byte: line * 10,
        end_byte: line * 10 + 20,
        exported: true,
        signature: format!("def {name}()"),
        parent: None,
        is_test: false,
    }
}

/// Random Python-shaped file: a few functions from a small shared name pool so names collide across files.
fn random_file(file: u64, n_files: u64, rng: &mut Rng) -> FileFacts {
    let path = format!("pkg/m{file}.py");
    let mut names: Vec<&str> = NAMES.to_vec();
    let mut symbols = Vec::new();
    for n in 0..(1 + rng.below(4)) {
        let idx = rng.below(names.len() as u64) as usize;
        let name = names.remove(idx);
        let kind = if name == "Config" {
            SymbolKind::Class
        } else {
            SymbolKind::Function
        };
        symbols.push(sym(
            &path,
            &format!("{}.{name}", module(file)),
            name,
            kind,
            n as u32 * 5 + 1,
        ));
    }
    let mut edges = Vec::new();
    for s in &symbols {
        for _ in 0..rng.below(4) {
            let name = match rng.below(10) {
                0 => "print".to_string(),
                _ => NAMES[rng.below(NAMES.len() as u64) as usize].to_string(),
            };
            let dst = match rng.below(4) {
                0 => match symbols.iter().find(|t| t.name == name) {
                    Some(t) => Target::Symbol(t.id),
                    None => Target::Unresolved {
                        name,
                        qualifier: None,
                        import_path: None,
                    },
                },
                1 => Target::Unresolved {
                    name,
                    qualifier: None,
                    import_path: Some(module(rng.below(n_files))),
                },
                2 => Target::Unresolved {
                    name,
                    qualifier: Some(format!("m{}", rng.below(n_files))),
                    import_path: None,
                },
                _ => Target::Unresolved {
                    name,
                    qualifier: None,
                    import_path: None,
                },
            };
            let provenance = if matches!(dst, Target::Symbol(_)) {
                Provenance::Extracted
            } else {
                Provenance::NameGuess
            };
            edges.push(RawEdge {
                src: s.id,
                dst,
                kind: EdgeKind::Calls,
                site_line: s.start_line + 1,
                provenance,
            });
        }
    }
    FileFacts {
        path,
        lang: Lang::Python,
        content_hash: [file as u8; 32],
        symbols,
        edges,
        parse_ok: true,
    }
}

fn sorted(mut v: Vec<Resolution>) -> Vec<Resolution> {
    v.sort();
    v
}

fn open() -> (tempfile::TempDir, CozoStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = CozoStore::open(&dir.path().join("db")).unwrap();
    (dir, store)
}

#[test]
fn incremental_equals_full_rebuild() {
    const N_FILES: u64 = 8;
    let mut seen = BTreeSet::new();
    for seed in [7u64, 42, 1234, 777, 31337] {
        let mut rng = Rng(seed);
        let (_d1, inc) = open();
        let mut current: Vec<Option<FileFacts>> = (0..N_FILES).map(|_| None).collect();
        let mut adj = Adjacency::rebuild(&inc).unwrap();

        for _ in 0..120 {
            let f = rng.below(N_FILES);
            let delta = if rng.below(6) == 0 && current[f as usize].is_some() {
                current[f as usize] = None;
                inc.remove_file(&format!("pkg/m{f}.py")).unwrap()
            } else {
                let facts = random_file(f, N_FILES, &mut rng);
                let delta = inc.replace_file(&facts).unwrap();
                current[f as usize] = Some(facts);
                delta
            };
            adj.apply(&inc, &delta).unwrap();
        }

        let (_d2, full) = open();
        for facts in current.iter().flatten() {
            full.replace_file(facts).unwrap();
        }

        let inc_res = sorted(inc.resolve_all().unwrap());
        let full_res = sorted(full.resolve_all().unwrap());
        assert_eq!(inc_res, full_res, "seed {seed}: store resolution diverged");
        for r in &inc_res {
            seen.insert(match r.outcome {
                Outcome::Resolved { provenance, .. } => provenance.as_str(),
                Outcome::Ambiguous { .. } => "ambiguous",
                Outcome::Unresolved => "unresolved",
            });
        }

        let rebuilt = Adjacency::rebuild(&full).unwrap();
        assert_eq!(
            adj.snapshot(),
            rebuilt.snapshot(),
            "seed {seed}: incremental adjacency diverged"
        );
        assert_eq!(adj.snapshot(), Adjacency::rebuild(&inc).unwrap().snapshot());
        assert_eq!(adj.rev(), inc.graph_rev());
    }
    for tier in [
        "extracted",
        "resolved",
        "inferred",
        "name_guess",
        "ambiguous",
        "unresolved",
    ] {
        assert!(
            seen.contains(tier),
            "fixture never exercised {tier}: {seen:?}"
        );
    }
}

/// a <- b <- c, a <- d, d <- c (diamond: c reachable at depth 2 twice), c <- e, a contains x (ignored).
#[test]
fn blast_radius_shallowest_depth() {
    let (_d, store) = open();
    let path = "pkg/g.py";
    let names = ["a", "b", "c", "d", "e", "x"];
    let syms: Vec<Symbol> = names
        .iter()
        .enumerate()
        .map(|(n, name)| {
            sym(
                path,
                &format!("pkg.g.{name}"),
                name,
                SymbolKind::Function,
                n as u32 * 10,
            )
        })
        .collect();
    let id = |n: &str| syms[names.iter().position(|x| *x == n).unwrap()].id;
    let edge = |src: &str, dst: &str, kind: EdgeKind| RawEdge {
        src: id(src),
        dst: Target::Symbol(id(dst)),
        kind,
        site_line: 1,
        provenance: Provenance::Extracted,
    };
    let facts = FileFacts {
        path: path.into(),
        lang: Lang::Python,
        content_hash: [1; 32],
        symbols: syms.clone(),
        edges: vec![
            edge("b", "a", EdgeKind::Calls),
            edge("c", "b", EdgeKind::Calls),
            edge("d", "a", EdgeKind::Calls),
            edge("c", "d", EdgeKind::Calls),
            edge("e", "c", EdgeKind::Calls),
            edge("x", "a", EdgeKind::Contains),
        ],
        parse_ok: true,
    };
    store.replace_file(&facts).unwrap();
    let adj = Adjacency::rebuild(&store).unwrap();

    let got = adj.blast_radius(id("a"), 10, DEPENDENCY_KINDS);
    let mut want = vec![(id("b"), 1), (id("d"), 1), (id("c"), 2), (id("e"), 3)];
    want.sort_by_key(|(i, d)| (*d, *i));
    assert_eq!(got, want);
    assert_eq!(adj.blast_radius(id("a"), 1, DEPENDENCY_KINDS).len(), 2);

    let callers: BTreeSet<SymbolId> = store
        .callers(id("a"))
        .unwrap()
        .into_iter()
        .map(|r| r.src)
        .collect();
    assert_eq!(callers, BTreeSet::from([id("b"), id("d"), id("x")]));
    assert_eq!(adj.callers_of(id("c")).len(), 1);
}

#[test]
fn reads_proceed_while_writer_runs() {
    const N_FILES: u64 = 6;
    let (_d, store) = open();
    let store = Arc::new(store);
    let mut rng = Rng(99);
    for f in 0..N_FILES {
        store
            .replace_file(&random_file(f, N_FILES, &mut rng))
            .unwrap();
    }
    let probe = store.symbols_in_file("pkg/m0.py").unwrap()[0].id;
    let done = Arc::new(AtomicBool::new(false));

    let writer = {
        let (store, done) = (store.clone(), done.clone());
        std::thread::spawn(move || {
            let mut rng = Rng(5);
            for _ in 0..150 {
                let f = 1 + rng.below(N_FILES - 1);
                store
                    .replace_file(&random_file(f, N_FILES, &mut rng))
                    .unwrap();
            }
            done.store(true, Ordering::SeqCst);
        })
    };
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let (store, done) = (store.clone(), done.clone());
            std::thread::spawn(move || {
                let mut reads = 0u32;
                let mut worst = Duration::ZERO;
                while !done.load(Ordering::SeqCst) {
                    let t = Instant::now();
                    store.callers(probe).unwrap();
                    store.symbols_by_name("load").unwrap();
                    worst = worst.max(t.elapsed());
                    reads += 1;
                }
                (reads, worst)
            })
        })
        .collect();

    writer.join().unwrap();
    for r in readers {
        let (reads, worst) = r.join().unwrap();
        assert!(reads > 0);
        assert!(
            worst < Duration::from_secs(1),
            "a read stalled for {worst:?}"
        );
    }
}

#[test]
fn reopen_keeps_facts_and_rev() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let mut rng = Rng(3);
    let facts = random_file(0, 1, &mut rng);
    let rev = {
        let store = CozoStore::open(&path).unwrap();
        store.replace_file(&facts).unwrap();
        store.graph_rev()
    };
    let store = CozoStore::open(&path).unwrap();
    assert_eq!(store.graph_rev(), rev);
    assert_eq!(
        store.symbols_in_file(&facts.path).unwrap().len(),
        facts.symbols.len()
    );
    assert_eq!(store.file_paths().unwrap(), vec![facts.path.clone()]);
}
