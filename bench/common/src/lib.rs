//! Shared dataset generator, reference implementation and benchmark harness.
//!
//! Direction convention: an edge `src -> dst` means "src depends on dst"
//! (calls / imports / implements / contains). Blast radius of `t` is the set
//! of symbols that transitively depend on `t`, i.e. a walk over reversed edges.

use std::collections::{HashSet, VecDeque};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;

pub const KIND_CALLS: u8 = 0;
pub const KIND_IMPORTS: u8 = 1;
pub const KIND_IMPLEMENTS: u8 = 2;
pub const KIND_CONTAINS: u8 = 3;

pub const PROV_EXTRACTED: u8 = 0;
pub const PROV_INFERRED: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Edge {
    pub src: u32,
    pub dst: u32,
    pub kind: u8,
    pub prov: u8,
    pub resolved: bool,
}

impl Edge {
    /// The query-time confidence rule every engine must express natively.
    pub fn trusted(&self) -> bool {
        self.prov == PROV_EXTRACTED || self.resolved
    }
}

// ---------------------------------------------------------------- rng

#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }
}

// ---------------------------------------------------------------- dataset

#[derive(Clone, Debug)]
pub struct Config {
    pub name: &'static str,
    pub files: u32,
    pub syms_per_file: u32,
    pub avg_out: u32,
    pub files_per_module: u32,
    pub seed: u64,
}

impl Config {
    pub fn by_name(name: &str) -> Config {
        let files = match name {
            "small" => 500,
            "medium" => 5_000,
            "large" => 20_000,
            other => panic!("unknown size {other}"),
        };
        let name: &'static str = match name {
            "small" => "small",
            "medium" => "medium",
            _ => "large",
        };
        Config {
            name,
            files,
            syms_per_file: 10,
            avg_out: 5,
            files_per_module: 50,
            seed: 0x6772_6170_6869_7465,
        }
    }
    pub fn n_syms(&self) -> u32 {
        self.files * self.syms_per_file
    }
}

#[derive(Clone)]
enum Source {
    Synthetic(Config),
    /// Real graph: `pristine` is the extracted edge set per file, `dsts` the
    /// pool of real edge targets used to synthesize plausible edits.
    Real { pristine: Arc<Vec<Vec<Edge>>>, dsts: Arc<Vec<u32>> },
}

#[derive(Clone)]
pub struct Dataset {
    pub name: String,
    /// `file_start[f]..file_start[f + 1]` are the symbols of file `f`.
    pub file_start: Vec<u32>,
    /// Edges owned by each file (edges whose `src` lives in that file).
    pub edges_by_file: Vec<Vec<Edge>>,
    source: Source,
}

impl Dataset {
    pub fn generate(cfg: &Config) -> Dataset {
        let mut rng = Rng::new(cfg.seed);
        let edges_by_file = (0..cfg.files).map(|f| gen_file_edges(cfg, f, &mut rng)).collect();
        let file_start = (0..=cfg.files).map(|f| f * cfg.syms_per_file).collect();
        Dataset { name: cfg.name.to_string(), file_start, edges_by_file, source: Source::Synthetic(cfg.clone()) }
    }

    /// Load a graph exported by `bench/real-graph` (`data/<label>/{symbols,edges}.jsonl`).
    /// Symbols are renumbered so each file's symbols are contiguous; duplicate
    /// (src, dst, kind) edges are dropped.
    pub fn load_real(label: &str) -> Dataset {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../real-graph/data").join(label);
        let read = |f: &str| std::fs::read_to_string(dir.join(f)).unwrap_or_else(|e| panic!("{}: {e}", dir.join(f).display()));
        let mut syms: Vec<(String, u32)> = read("symbols.jsonl")
            .lines()
            .map(|l| {
                let v: serde_json::Value = serde_json::from_str(l).unwrap();
                (v["file"].as_str().unwrap().to_string(), v["i"].as_u64().unwrap() as u32)
            })
            .collect();
        syms.sort();
        let n = syms.len();
        let mut new_id = vec![u32::MAX; n];
        let mut file_start = Vec::new();
        let mut prev: Option<&str> = None;
        for (k, (file, i)) in syms.iter().enumerate() {
            if prev != Some(file.as_str()) {
                file_start.push(k as u32);
                prev = Some(file.as_str());
            }
            new_id[*i as usize] = k as u32;
        }
        file_start.push(n as u32);
        let n_files = file_start.len() - 1;
        let file_of = |sym: u32| (file_start.partition_point(|&s| s <= sym) - 1) as u32;
        let mut edges_by_file = vec![Vec::new(); n_files];
        let mut seen = HashSet::new();
        for l in read("edges.jsonl").lines() {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            let src = new_id[v["src"].as_u64().unwrap() as usize];
            let dst = new_id[v["dst"].as_u64().unwrap() as usize];
            let kind = match v["kind"].as_str().unwrap() {
                "calls" => KIND_CALLS,
                "imports" => KIND_IMPORTS,
                "implements" => KIND_IMPLEMENTS,
                "contains" => KIND_CONTAINS,
                k => panic!("unknown edge kind {k}"),
            };
            if !seen.insert((src, dst, kind)) {
                continue;
            }
            let extracted = v["prov"].as_str().unwrap() == "extracted";
            let prov = if extracted { PROV_EXTRACTED } else { PROV_INFERRED };
            edges_by_file[file_of(src) as usize].push(Edge { src, dst, kind, prov, resolved: extracted });
        }
        let dsts = edges_by_file.iter().flatten().filter(|e| e.kind == KIND_CALLS).map(|e| e.dst).collect();
        Dataset {
            name: label.to_string(),
            file_start,
            source: Source::Real { pristine: Arc::new(edges_by_file.clone()), dsts: Arc::new(dsts) },
            edges_by_file,
        }
    }

    /// `small|medium|large` (synthetic) or `real:<label>`.
    pub fn by_name(name: &str) -> Dataset {
        match name.strip_prefix("real:") {
            Some(label) => Dataset::load_real(label),
            None => Dataset::generate(&Config::by_name(name)),
        }
    }

    pub fn n_syms(&self) -> u32 {
        *self.file_start.last().unwrap()
    }
    pub fn n_files(&self) -> u32 {
        self.edges_by_file.len() as u32
    }
    pub fn file_of(&self, sym: u32) -> u32 {
        (self.file_start.partition_point(|&s| s <= sym) - 1) as u32
    }
    pub fn syms_of(&self, file: u32) -> std::ops::Range<u32> {
        self.file_start[file as usize]..self.file_start[file as usize + 1]
    }
    pub fn edges(&self) -> impl Iterator<Item = &Edge> {
        self.edges_by_file.iter().flatten()
    }
    pub fn n_edges(&self) -> usize {
        self.edges_by_file.iter().map(Vec::len).sum()
    }

    /// A new version of one file: same symbols, changed outgoing edges.
    /// Synthetic: regenerated. Real: the extracted edges with ~15% dropped
    /// and a few new calls to real call targets (a plausible edit).
    pub fn regen_file(&self, file: u32, rng: &mut Rng) -> Vec<Edge> {
        match &self.source {
            Source::Synthetic(cfg) => gen_file_edges(cfg, file, rng),
            Source::Real { pristine, dsts } => {
                let mut out: Vec<Edge> = pristine[file as usize].iter().filter(|_| rng.f64() >= 0.15).copied().collect();
                let syms = self.syms_of(file);
                let callers: Vec<u32> = syms.clone().collect();
                let adds = 1 + rng.below(4);
                for _ in 0..adds {
                    if callers.is_empty() || dsts.is_empty() {
                        break;
                    }
                    let src = callers[rng.below(callers.len() as u64) as usize];
                    let dst = dsts[rng.below(dsts.len() as u64) as usize];
                    if src != dst && !out.iter().any(|e| e.src == src && e.dst == dst && e.kind == KIND_CALLS) {
                        out.push(Edge { src, dst, kind: KIND_CALLS, prov: PROV_EXTRACTED, resolved: true });
                    }
                }
                out
            }
        }
    }
}

fn skewed(len: u32, u: f64, power: f64) -> u32 {
    ((len as f64 * u.powf(power)) as u32).min(len - 1)
}

fn gen_file_edges(cfg: &Config, file: u32, rng: &mut Rng) -> Vec<Edge> {
    // Layered, code-shaped graph: files depend mostly on earlier files of their
    // own module and on lower modules; a small share of back-edges creates
    // realistic cycles. Utility hubs live at the bottom (lowest symbol ids).
    let n = cfg.n_syms();
    let spf = cfg.syms_per_file;
    let fpm = cfg.files_per_module;
    let syms_per_module = fpm * spf;
    let n_modules = cfg.files.div_ceil(fpm);
    let hubs = (n / 100).max(50).min(n);
    let module = file / fpm;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for sym in file * spf..(file + 1) * spf {
        let degree = 1 + rng.below((2 * cfg.avg_out - 1) as u64) as u32;
        for _ in 0..degree {
            let k = rng.f64();
            let kind = if k < 0.60 {
                KIND_CALLS
            } else if k < 0.80 {
                KIND_IMPORTS
            } else if k < 0.85 {
                KIND_IMPLEMENTS
            } else {
                KIND_CONTAINS
            };
            let dst = if kind == KIND_CONTAINS {
                // container -> member, members follow their container
                let local = sym - file * spf;
                if local + 1 >= spf {
                    continue;
                }
                sym + 1 + rng.below((spf - local - 1) as u64) as u32
            } else {
                let r = rng.f64();
                if r < 0.08 {
                    skewed(hubs, rng.f64(), 3.0)
                } else if r < 0.70 {
                    // same module: a nearby earlier file (99.8%) or any file (0.2% => cycles)
                    let start = module * syms_per_module;
                    let len = syms_per_module.min(n - start);
                    let own_file = (sym - start) / spf;
                    if own_file == 0 {
                        // bottom file of a module only reaches down to the hubs
                        skewed(hubs, rng.f64(), 3.0)
                    } else if rng.f64() < 0.998 {
                        let back = 1 + skewed(own_file.min(10), rng.f64(), 2.0);
                        start + (own_file - back) * spf + rng.below(spf as u64) as u32
                    } else {
                        start + rng.below(len as u64) as u32
                    }
                } else {
                    // another module's public API (first 10% of its symbols):
                    // a nearby lower layer (99.9%) or any module (0.1% back-edge)
                    let m = if module > 0 && rng.f64() < 0.999 {
                        module - 1 - skewed(module.min(8), rng.f64(), 2.0)
                    } else {
                        rng.below(n_modules as u64) as u32
                    };
                    let start = m * syms_per_module;
                    let len = syms_per_module.min(n - start);
                    let api = (len / 10).max(1);
                    start + skewed(api, rng.f64(), 2.0)
                }
            };
            if dst == sym || !seen.insert((sym, dst, kind)) {
                continue;
            }
            let inferred = rng.f64() < 0.15;
            let prov = if inferred { PROV_INFERRED } else { PROV_EXTRACTED };
            let resolved = !inferred || rng.f64() < 0.5;
            out.push(Edge { src: sym, dst, kind, prov, resolved });
        }
    }
    out
}

// ---------------------------------------------------------------- reference

/// Plain-Rust ground truth used to validate every engine.
pub struct Reference {
    rev: Vec<Vec<(u32, bool)>>,
    fwd: Vec<Vec<u32>>,
}

impl Reference {
    pub fn new(ds: &Dataset) -> Self {
        let n = ds.n_syms() as usize;
        let mut rev = vec![Vec::new(); n];
        let mut fwd = vec![Vec::new(); n];
        for e in ds.edges() {
            rev[e.dst as usize].push((e.src, e.trusted()));
            fwd[e.src as usize].push(e.dst);
        }
        Reference { rev, fwd }
    }

    pub fn in_degree(&self, s: u32) -> usize {
        self.rev[s as usize].len()
    }

    /// Transitive dependents of `target` up to `max_depth`, shallowest depth
    /// per node, target excluded, sorted by node id.
    pub fn blast(&self, target: u32, max_depth: u32, trusted_only: bool) -> Vec<(u32, u32)> {
        let n = self.rev.len();
        let mut depth = vec![u32::MAX; n];
        let mut q = VecDeque::new();
        depth[target as usize] = 0;
        q.push_back(target);
        while let Some(y) = q.pop_front() {
            let d = depth[y as usize];
            if d >= max_depth {
                continue;
            }
            for &(x, trusted) in &self.rev[y as usize] {
                if trusted_only && !trusted {
                    continue;
                }
                if depth[x as usize] == u32::MAX {
                    depth[x as usize] = d + 1;
                    q.push_back(x);
                }
            }
        }
        (0..n as u32)
            .filter(|&x| x != target && depth[x as usize] != u32::MAX)
            .map(|x| (x, depth[x as usize]))
            .collect()
    }

    pub fn shortest_path(&self, a: u32, b: u32) -> Option<u32> {
        let mut dist = vec![u32::MAX; self.fwd.len()];
        let mut q = VecDeque::new();
        dist[a as usize] = 0;
        q.push_back(a);
        while let Some(y) = q.pop_front() {
            if y == b {
                return Some(dist[y as usize]);
            }
            for &x in &self.fwd[y as usize] {
                if dist[x as usize] == u32::MAX {
                    dist[x as usize] = dist[y as usize] + 1;
                    q.push_back(x);
                }
            }
        }
        None
    }

    /// Number of strongly connected components with more than one member.
    pub fn nontrivial_sccs(&self) -> usize {
        let comp = tarjan(&self.fwd);
        count_nontrivial(&comp)
    }
}

pub fn count_nontrivial(component_of: &[u32]) -> usize {
    let mut sizes = std::collections::HashMap::new();
    for &c in component_of {
        *sizes.entry(c).or_insert(0usize) += 1;
    }
    sizes.values().filter(|&&s| s > 1).count()
}

/// Iterative Tarjan SCC; returns component id per node.
pub fn tarjan(adj: &[Vec<u32>]) -> Vec<u32> {
    let n = adj.len();
    let mut index = vec![u32::MAX; n];
    let mut low = vec![0u32; n];
    let mut on_stack = vec![false; n];
    let mut comp = vec![u32::MAX; n];
    let mut stack = Vec::new();
    let mut next_index = 0u32;
    let mut next_comp = 0u32;
    let mut call: Vec<(u32, usize)> = Vec::new();
    for root in 0..n as u32 {
        if index[root as usize] != u32::MAX {
            continue;
        }
        call.push((root, 0));
        while let Some(&mut (v, ref mut i)) = call.last_mut() {
            let vu = v as usize;
            if *i == 0 && index[vu] == u32::MAX {
                index[vu] = next_index;
                low[vu] = next_index;
                next_index += 1;
                stack.push(v);
                on_stack[vu] = true;
            }
            if *i < adj[vu].len() {
                let w = adj[vu][*i];
                *i += 1;
                let wu = w as usize;
                if index[wu] == u32::MAX {
                    call.push((w, 0));
                } else if on_stack[wu] {
                    low[vu] = low[vu].min(index[wu]);
                }
            } else {
                call.pop();
                if let Some(&(p, _)) = call.last() {
                    low[p as usize] = low[p as usize].min(low[vu]);
                }
                if low[vu] == index[vu] {
                    loop {
                        let w = stack.pop().unwrap();
                        on_stack[w as usize] = false;
                        comp[w as usize] = next_comp;
                        if w == v {
                            break;
                        }
                    }
                    next_comp += 1;
                }
            }
        }
    }
    comp
}

// ---------------------------------------------------------------- engine trait

pub trait Engine: Send + Sync + 'static {
    fn label(&self) -> String;
    /// Bulk load the full dataset into an empty store.
    fn load(&self, ds: &Dataset);
    /// Blast-radius traversal strategies this engine implements; the first is the default.
    fn variants(&self) -> Vec<&'static str>;
    fn blast(&self, variant: &str, target: u32, max_depth: u32, trusted_only: bool) -> Vec<(u32, u32)>;
    fn shortest_path(&self, a: u32, b: u32) -> Option<u32>;
    /// Component id per node for every node the engine knows about.
    fn scc(&self) -> Option<Vec<u32>>;
    /// Number of communities, or None if unsupported.
    fn communities(&self) -> Option<usize>;
    /// Number of ranked nodes, or None if unsupported.
    fn pagerank(&self) -> Option<usize>;
    /// Replace all facts owned by `file` (its symbols + their outgoing edges).
    fn replace_file(&self, ds: &Dataset, file: u32, edges: &[Edge]);
    /// On-disk location, if persistent.
    fn disk_path(&self) -> Option<std::path::PathBuf> {
        None
    }
}

/// Factory: `fresh` builds an empty store; `reopen` opens the existing one
/// from disk (None for in-memory engines).
pub trait Factory {
    type E: Engine;
    fn fresh(&self, tag: &str) -> Self::E;
    fn reopen(&self, _tag: &str) -> Option<Self::E> {
        None
    }
    fn label(&self) -> String;
}

// ---------------------------------------------------------------- measurement

#[derive(Serialize, Clone, Debug, Default)]
pub struct Rec {
    pub engine: String,
    pub size: String,
    pub metric: String,
    pub n: usize,
    pub p50_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    pub max_us: f64,
    pub mean_us: f64,
    pub value: f64,
    pub note: String,
}

pub fn summarize(mut samples: Vec<f64>) -> (f64, f64, f64, f64, f64) {
    if samples.is_empty() {
        return (0.0, 0.0, 0.0, 0.0, 0.0);
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct = |p: f64| samples[((samples.len() - 1) as f64 * p).round() as usize];
    let mean = samples.iter().sum::<f64>() / samples.len() as f64;
    (pct(0.50), pct(0.95), pct(0.99), *samples.last().unwrap(), mean)
}

/// Run `f` until `budget` elapses or `max_iters` is hit (at least `min_iters`).
pub fn time_it<F: FnMut()>(min_iters: usize, max_iters: usize, budget: Duration, mut f: F) -> Vec<f64> {
    let start = Instant::now();
    let mut out = Vec::new();
    while out.len() < max_iters && (out.len() < min_iters || start.elapsed() < budget) {
        let t = Instant::now();
        f();
        out.push(t.elapsed().as_secs_f64() * 1e6);
    }
    out
}

pub fn peak_rss_mb() -> f64 {
    unsafe {
        let mut ru: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut ru);
        // macOS reports bytes, Linux kilobytes.
        if cfg!(target_os = "macos") {
            ru.ru_maxrss as f64 / (1024.0 * 1024.0)
        } else {
            ru.ru_maxrss as f64 / 1024.0
        }
    }
}

pub fn dir_size_mb(p: &Path) -> f64 {
    fn walk(p: &Path) -> u64 {
        match std::fs::metadata(p) {
            Ok(m) if m.is_file() => m.len(),
            Ok(m) if m.is_dir() => std::fs::read_dir(p)
                .map(|rd| rd.flatten().map(|e| walk(&e.path())).sum())
                .unwrap_or(0),
            _ => 0,
        }
    }
    walk(p) as f64 / (1024.0 * 1024.0)
}

struct Recorder {
    engine: String,
    size: String,
    path: std::path::PathBuf,
}

impl Recorder {
    fn emit(&self, metric: &str, samples: Vec<f64>, value: f64, note: impl Into<String>) {
        let n = samples.len();
        let (p50, p95, p99, max, mean) = summarize(samples);
        let rec = Rec {
            engine: self.engine.clone(),
            size: self.size.clone(),
            metric: metric.to_string(),
            n,
            p50_us: p50,
            p95_us: p95,
            p99_us: p99,
            max_us: max,
            mean_us: mean,
            value,
            note: note.into(),
        };
        println!(
            "{:<28} {:<7} {:<34} n={:<5} p50={:>11.1}us p95={:>11.1}us p99={:>11.1}us value={} {}",
            rec.engine, rec.size, rec.metric, n, p50, p95, p99, value, rec.note
        );
        let mut f = OpenOptions::new().create(true).append(true).open(&self.path).unwrap();
        writeln!(f, "{}", serde_json::to_string(&rec).unwrap()).unwrap();
    }
    fn value(&self, metric: &str, value: f64, note: impl Into<String>) {
        self.emit(metric, Vec::new(), value, note);
    }
}

pub struct Targets {
    pub hub: u32,
    pub p99: u32,
    pub median: u32,
    pub path: (u32, u32),
}

/// Pick blast targets by depth-10 reach: the worst hub, the p99 and the
/// median symbol among symbols with non-empty reach. Large graphs are sampled.
pub fn pick_targets(ds: &Dataset, reference: &Reference) -> Targets {
    let n = ds.n_syms();
    let mut rng = Rng::new(7);
    let mut cand: Vec<u32> = if n <= 60_000 {
        (0..n).collect()
    } else {
        (0..20_000).map(|_| rng.below(n as u64) as u32).collect()
    };
    let mut by_in: Vec<u32> = (0..n).collect();
    by_in.sort_by_key(|&s| std::cmp::Reverse(reference.in_degree(s)));
    cand.extend(by_in.into_iter().take(100));
    cand.sort_unstable();
    cand.dedup();
    let mut reach: Vec<(usize, u32)> = cand
        .into_iter()
        .map(|s| (reference.blast(s, 10, false).len(), s))
        .filter(|&(r, _)| r > 0)
        .collect();
    reach.sort_unstable();
    let at = |q: f64| reach[((reach.len() - 1) as f64 * q).round() as usize].1;
    let (hub, p99, median) = (reach.last().unwrap().1, at(0.99), at(0.50));
    // A pair with a real forward path, as long as possible (>= 4 preferred).
    let mut path = (hub, hub);
    let mut best = 0;
    for _ in 0..2_000 {
        let a = rng.below(n as u64) as u32;
        let b = rng.below(n as u64) as u32;
        if let Some(len) = reference.shortest_path(a, b) {
            if len > best {
                best = len;
                path = (a, b);
                if len >= 6 {
                    break;
                }
            }
        }
    }
    Targets { hub, p99, median, path }
}

/// Apply `count` random per-file updates to both `engine` and `ds`.
fn apply_updates<E: Engine>(engine: &E, ds: &mut Dataset, count: usize, seed: u64) -> Vec<f64> {
    let mut rng = Rng::new(seed);
    let mut lat = Vec::with_capacity(count);
    for _ in 0..count {
        let file = rng.below(ds.n_files() as u64) as u32;
        let edges = ds.regen_file(file, &mut rng);
        let t = Instant::now();
        engine.replace_file(ds, file, &edges);
        lat.push(t.elapsed().as_secs_f64() * 1e6);
        ds.edges_by_file[file as usize] = edges;
    }
    lat
}

pub struct RunOpts {
    pub size: String,
    pub results_dir: std::path::PathBuf,
    pub skip_algos: bool,
}

pub fn run_suite<F: Factory>(factory: &F, opts: &RunOpts) {
    let engine_label = factory.label();
    let t = Instant::now();
    let ds = Dataset::by_name(&opts.size);
    let path = opts.results_dir.join(format!("{}-{}.jsonl", engine_label, ds.name));
    let _ = std::fs::remove_file(&path);
    let rec = Recorder { engine: engine_label.clone(), size: ds.name.clone(), path };
    let reference = Reference::new(&ds);
    eprintln!(
        "[{engine_label}] dataset {}: {} syms, {} edges, loaded in {:?}",
        ds.name,
        ds.n_syms(),
        ds.n_edges(),
        t.elapsed()
    );
    rec.value("dataset_symbols", ds.n_syms() as f64, "");
    rec.value("dataset_edges", ds.n_edges() as f64, "");

    // ---- build
    let engine = factory.fresh("main");
    let t = Instant::now();
    engine.load(&ds);
    rec.value("build_ms", t.elapsed().as_secs_f64() * 1e3, "full bulk load");
    if let Some(p) = engine.disk_path() {
        rec.value("disk_mb", dir_size_mb(&p), p.display().to_string());
    }

    let tg = pick_targets(&ds, &reference);
    let (median, (pa, pb)) = (tg.median, tg.path);
    let named = [("hub", tg.hub), ("p99", tg.p99), ("median", tg.median)];
    for (tname, s) in named {
        rec.value(&format!("target_{tname}_blast10_size"), reference.blast(s, 10, false).len() as f64, format!("sym {s}"));
    }

    let variants = engine.variants();
    let dv = variants[0];

    // ---- cold first query (right after load)
    let t = Instant::now();
    let _ = engine.blast(dv, median, 10, false);
    rec.emit("cold_first_blast10_median", vec![t.elapsed().as_secs_f64() * 1e6], 0.0, "first query after load");

    // ---- correctness on the initial state (every variant)
    for &v in &variants {
        let mut mismatches = 0;
        for &(_, tg) in &named {
            for &d in &[3u32, 10] {
                for &tr in &[false, true] {
                    if engine.blast(v, tg, d, tr) != reference.blast(tg, d, tr) {
                        mismatches += 1;
                    }
                }
            }
        }
        rec.value(&format!("correct_vs_reference_initial[{v}]"), (mismatches == 0) as u8 as f64, format!("{mismatches} mismatching queries of 12"));
    }

    // ---- blast radius latencies (every variant)
    let budget = Duration::from_secs(3);
    for &v in &variants {
        for (tname, tg) in named {
            for d in [3u32, 10] {
                let s = time_it(5, 300, budget, || {
                    std::hint::black_box(engine.blast(v, tg, d, false));
                });
                rec.emit(&format!("blast_d{d}_{tname}[{v}]"), s, reference.blast(tg, d, false).len() as f64, "result size in value");
            }
            let s = time_it(5, 300, budget, || {
                std::hint::black_box(engine.blast(v, tg, 10, true));
            });
            rec.emit(&format!("blast_d10_{tname}_trusted[{v}]"), s, reference.blast(tg, 10, true).len() as f64, "confidence rule applied at query time");
        }
    }

    // ---- shortest path
    let expect = reference.shortest_path(pa, pb);
    let got = engine.shortest_path(pa, pb);
    let s = time_it(5, 300, budget, || {
        std::hint::black_box(engine.shortest_path(pa, pb));
    });
    rec.emit("shortest_path", s, got.map(|x| x as f64).unwrap_or(-1.0), format!("expected {:?}, got {:?}", expect, got));

    // ---- whole-graph algorithms
    if !opts.skip_algos {
        let expect = reference.nontrivial_sccs();
        let mut got = None;
        let s = time_it(3, 10, Duration::from_secs(5), || {
            got = engine.scc().map(|c| count_nontrivial(&c));
        });
        rec.emit("scc", s, got.map(|x| x as f64).unwrap_or(-1.0), format!("nontrivial SCCs expected {expect}, got {got:?}"));

        let mut comm = None;
        let s = time_it(3, 10, Duration::from_secs(10), || {
            comm = engine.communities();
        });
        if comm.is_some() {
            rec.emit("communities_louvain", s, comm.unwrap() as f64, "number of communities");
        } else {
            rec.value("communities_louvain", -1.0, "unsupported");
        }
        let mut pr = None;
        let s = time_it(3, 10, Duration::from_secs(10), || {
            pr = engine.pagerank();
        });
        if pr.is_some() {
            rec.emit("pagerank", s, pr.unwrap() as f64, "ranked nodes");
        } else {
            rec.value("pagerank", -1.0, "unsupported");
        }
    }

    // ---- incremental updates + correctness incremental == full rebuild
    let mut ds_inc = ds.clone();
    let lat = apply_updates(&engine, &mut ds_inc, 200, 99);
    rec.emit("incremental_replace_file", lat, 0.0, "delete+insert one file's facts (~10 syms / ~50 edges)");

    let reference_final = Reference::new(&ds_inc);
    let rebuilt = factory.fresh("rebuilt");
    rebuilt.load(&ds_inc);
    let mut rng = Rng::new(4242);
    let mut targets: Vec<u32> = (0..20).map(|_| rng.below(ds.n_syms() as u64) as u32).collect();
    targets.extend([tg.hub, tg.p99, tg.median]);
    for &v in &variants {
        let (mut inc_vs_full, mut inc_vs_ref, mut checks) = (0, 0, 0);
        for &tg in &targets {
            for &tr in &[false, true] {
                let a = engine.blast(v, tg, 10, tr);
                let b = rebuilt.blast(v, tg, 10, tr);
                let r = reference_final.blast(tg, 10, tr);
                checks += 1;
                if a != b {
                    inc_vs_full += 1;
                }
                if a != r {
                    inc_vs_ref += 1;
                }
            }
        }
        rec.value(
            &format!("correct_incremental_eq_full[{v}]"),
            (inc_vs_full == 0 && inc_vs_ref == 0) as u8 as f64,
            format!("after 200 updates: {inc_vs_full}/{checks} differ from full rebuild, {inc_vs_ref}/{checks} differ from reference"),
        );
    }
    drop(rebuilt);

    // ---- reopen from disk (persistent engines)
    drop(engine);
    let t_reopen = Instant::now();
    let reopened = factory.reopen("main");
    if reopened.is_some() {
        rec.value("reopen_ms", t_reopen.elapsed().as_secs_f64() * 1e3, "open persisted store (incl. any in-memory rebuild)");
    }
    let engine = match reopened {
        Some(e) => {
            let t = Instant::now();
            let _ = e.blast(dv, median, 10, false);
            rec.emit("reopen_cold_first_blast10_median", vec![t.elapsed().as_secs_f64() * 1e6], 0.0, "first query after reopening from disk");
            e
        }
        None => {
            let e = factory.fresh("main2");
            e.load(&ds_inc);
            e
        }
    };
    let ds_now = ds_inc;

    // ---- watcher scenario: reads while a writer applies per-file updates
    let engine = Arc::new(engine);
    for (mode, pace) in [("continuous", None), ("paced_20ps", Some(Duration::from_millis(50)))] {
        let stop = Arc::new(AtomicBool::new(false));
        let writes = Arc::new(AtomicU64::new(0));
        let w_engine = engine.clone();
        let w_stop = stop.clone();
        let w_writes = writes.clone();
        let w_ds = ds_now.clone();
        let writer = std::thread::spawn(move || {
            let mut ds = w_ds;
            let mut rng = Rng::new(1234);
            let mut lat = Vec::new();
            while !w_stop.load(Ordering::Relaxed) {
                let file = rng.below(ds.n_files() as u64) as u32;
                let edges = ds.regen_file(file, &mut rng);
                let t = Instant::now();
                w_engine.replace_file(&ds, file, &edges);
                lat.push(t.elapsed().as_secs_f64() * 1e6);
                ds.edges_by_file[file as usize] = edges;
                w_writes.fetch_add(1, Ordering::Relaxed);
                if let Some(p) = pace {
                    std::thread::sleep(p);
                }
            }
            lat
        });
        std::thread::sleep(Duration::from_millis(100));
        let s = time_it(20, usize::MAX, Duration::from_secs(4), || {
            std::hint::black_box(engine.blast(dv, median, 10, false));
        });
        stop.store(true, Ordering::Relaxed);
        let wlat = writer.join().unwrap();
        let nw = writes.load(Ordering::Relaxed);
        rec.emit(&format!("watcher_{mode}_read_blast10_median"), s, nw as f64, "value = writes applied during the window");
        rec.emit(&format!("watcher_{mode}_write"), wlat, nw as f64, "writer latency under concurrent reads");
    }

    rec.value("peak_rss_mb", peak_rss_mb(), "whole process incl. reference + rebuilt store");
}

pub fn parse_args() -> (Vec<String>, Vec<String>, RunOpts) {
    // usage: <bin> [--size small|medium|large]... [--backend X]... [--skip-algos]
    let mut sizes = Vec::new();
    let mut backends = Vec::new();
    let mut skip_algos = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--size" => sizes.push(args.next().unwrap()),
            "--backend" => backends.push(args.next().unwrap()),
            "--skip-algos" => skip_algos = true,
            other => panic!("unknown arg {other}"),
        }
    }
    if sizes.is_empty() {
        sizes.push("medium".into());
    }
    let results_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../results");
    std::fs::create_dir_all(&results_dir).unwrap();
    (sizes, backends, RunOpts { size: String::new(), results_dir, skip_algos })
}
