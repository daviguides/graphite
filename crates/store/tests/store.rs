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

const FUNCS: &[&str] = &["load", "save", "parse", "run"];
const CLASSES: &[&str] = &["Base", "Config"];
const METHODS: &[&str] = &["start", "stop", "run"];

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

fn unresolved(name: &str, qualifier: Option<&str>, import_path: Option<String>) -> Target {
    Target::Unresolved {
        name: name.to_string(),
        qualifier: qualifier.map(str::to_string),
        import_path,
    }
}

fn pick<'a>(rng: &mut Rng, pool: &[&'a str]) -> &'a str {
    pool[rng.below(pool.len() as u64) as usize]
}

/// Random Python-shaped file in the extractor's conventions: a module, functions and one class with
/// methods from small shared pools so names collide across files.
fn random_file(file: u64, n_files: u64, rng: &mut Rng) -> FileFacts {
    let path = format!("pkg/m{file}.py");
    let modq = module(file);
    let is_test = file == 0;
    let mut module_sym = sym(&path, &modq, &format!("m{file}"), SymbolKind::Module, 1);
    module_sym.is_test = is_test;
    let mut symbols = vec![module_sym.clone()];
    let mut funcs = FUNCS.to_vec();
    for n in 0..(1 + rng.below(3)) {
        let name = funcs.remove(rng.below(funcs.len() as u64) as usize);
        let mut s = sym(
            &path,
            &format!("{modq}.{name}"),
            name,
            SymbolKind::Function,
            n as u32 * 5 + 3,
        );
        s.parent = Some(module_sym.id);
        s.is_test = is_test;
        symbols.push(s);
    }
    let class_name = pick(rng, CLASSES);
    let mut class = sym(
        &path,
        &format!("{modq}.{class_name}"),
        class_name,
        SymbolKind::Class,
        40,
    );
    class.parent = Some(module_sym.id);
    class.is_test = is_test;
    symbols.push(class.clone());
    let mut methods = METHODS.to_vec();
    for n in 0..rng.below(3) {
        let name = methods.remove(rng.below(methods.len() as u64) as usize);
        let mut s = sym(
            &path,
            &format!("{modq}.{class_name}.{name}"),
            name,
            SymbolKind::Method,
            41 + n as u32 * 3,
        );
        s.parent = Some(class.id);
        s.is_test = is_test;
        symbols.push(s);
    }

    let mut edges = Vec::new();
    let other = |rng: &mut Rng| rng.below(n_files);
    if rng.below(2) == 0 {
        let base = pick(rng, CLASSES);
        edges.push(RawEdge {
            src: class.id,
            dst: unresolved(base, None, Some(format!("{}.{base}", module(other(rng))))),
            kind: EdgeKind::Inherits,
            site_line: 40,
            provenance: Provenance::Extracted,
        });
    }
    edges.push(RawEdge {
        src: module_sym.id,
        dst: unresolved("<module>", None, Some(module(other(rng)))),
        kind: EdgeKind::Imports,
        site_line: 1,
        provenance: Provenance::Extracted,
    });
    for s in symbols
        .iter()
        .filter(|s| s.kind != SymbolKind::Module && s.kind != SymbolKind::Class)
    {
        for _ in 0..(1 + rng.below(3)) {
            let dst = match rng.below(7) {
                0 => {
                    let name = pick(rng, FUNCS);
                    match symbols.iter().find(|t| t.name == name) {
                        Some(t) => Target::Symbol(t.id),
                        None => unresolved(name, None, None),
                    }
                }
                1 => {
                    let (name, f) = (pick(rng, FUNCS), other(rng));
                    let qual = (rng.below(2) == 0).then(|| format!("m{f}"));
                    unresolved(name, qual.as_deref(), Some(format!("{}.{name}", module(f))))
                }
                2 => unresolved(pick(rng, METHODS), Some("self"), None),
                3 => unresolved(pick(rng, METHODS), Some("obj"), None),
                4 => unresolved(pick(rng, FUNCS), Some(&format!("m{}", other(rng))), None),
                5 => unresolved("getcwd", Some("os"), Some("<external>:os.getcwd".into())),
                _ => unresolved(pick(rng, FUNCS), None, None),
            };
            edges.push(RawEdge {
                src: s.id,
                dst,
                kind: EdgeKind::Calls,
                site_line: s.start_line + 1,
                provenance: Provenance::Extracted,
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
