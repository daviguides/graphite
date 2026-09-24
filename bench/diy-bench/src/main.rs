//! DIY candidate: per-file facts persisted in redb, graph held in memory,
//! traversal in Rust (BFS) or in Ascent (in-process Datalog, per query).
//!
//! Storage holds only per-file facts (file -> encoded outgoing edges), so a
//! file update is a single key overwrite and "incremental == full rebuild"
//! holds by construction; derived state lives only in memory.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::RwLock;

use ascent::{ascent, Dual};
use common::{parse_args, run_suite, tarjan, Dataset, Edge, Engine, Factory};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};

const FILES: TableDefinition<u32, &[u8]> = TableDefinition::new("file_facts");

ascent! {
    struct BlastProg;
    relation rev(u32, u32, bool);
    relation seed(u32);
    relation max_depth(u32);
    relation trusted_only(bool);
    lattice reach(u32, Dual<u32>);

    reach(x, Dual(1)) <-- seed(t), trusted_only(tr), rev(t, x, ok), if !*tr || *ok;
    reach(x, Dual(d + 1)) <--
        reach(y, ?Dual(d)), max_depth(m), if d < m,
        trusted_only(tr), rev(y, x, ok), if !*tr || *ok;
}

fn encode(edges: &[Edge]) -> Vec<u8> {
    let mut out = Vec::with_capacity(edges.len() * 10);
    for e in edges {
        out.extend_from_slice(&e.src.to_le_bytes());
        out.extend_from_slice(&e.dst.to_le_bytes());
        out.push(e.kind);
        out.push(e.prov | ((e.resolved as u8) << 1));
    }
    out
}

fn decode(bytes: &[u8]) -> Vec<Edge> {
    bytes
        .chunks_exact(10)
        .map(|c| Edge {
            src: u32::from_le_bytes(c[0..4].try_into().unwrap()),
            dst: u32::from_le_bytes(c[4..8].try_into().unwrap()),
            kind: c[8],
            prov: c[9] & 1,
            resolved: c[9] & 2 != 0,
        })
        .collect()
}

#[derive(Default)]
struct Graph {
    /// rev[dst] = (src, trusted)
    rev: Vec<Vec<(u32, bool)>>,
    fwd: Vec<Vec<u32>>,
    by_file: Vec<Vec<Edge>>,
}

impl Graph {
    fn with_size(n_syms: usize, n_files: usize) -> Self {
        Graph { rev: vec![Vec::new(); n_syms], fwd: vec![Vec::new(); n_syms], by_file: vec![Vec::new(); n_files] }
    }
    fn add(&mut self, e: &Edge) {
        self.rev[e.dst as usize].push((e.src, e.trusted()));
        self.fwd[e.src as usize].push(e.dst);
    }
    fn replace(&mut self, file: u32, edges: &[Edge]) {
        let old = std::mem::take(&mut self.by_file[file as usize]);
        for e in &old {
            self.rev[e.dst as usize].retain(|&(s, _)| s != e.src);
            self.fwd[e.src as usize].clear();
        }
        for e in edges {
            self.add(e);
        }
        self.by_file[file as usize] = edges.to_vec();
    }
}

struct DiyEngine {
    db: Database,
    path: PathBuf,
    g: RwLock<Graph>,
}

impl DiyEngine {
    fn blast_bfs(&self, t: u32, d: u32, trusted: bool) -> Vec<(u32, u32)> {
        let g = self.g.read().unwrap();
        let mut depth = vec![u32::MAX; g.rev.len()];
        depth[t as usize] = 0;
        let mut q = VecDeque::from([t]);
        let mut out = Vec::new();
        while let Some(y) = q.pop_front() {
            let dy = depth[y as usize];
            if dy >= d {
                continue;
            }
            for &(x, ok) in &g.rev[y as usize] {
                if (!trusted || ok) && depth[x as usize] == u32::MAX {
                    depth[x as usize] = dy + 1;
                    out.push((x, dy + 1));
                    q.push_back(x);
                }
            }
        }
        out.sort_unstable();
        out
    }

    fn blast_ascent(&self, t: u32, d: u32, trusted: bool) -> Vec<(u32, u32)> {
        let mut prog = BlastProg::default();
        {
            let g = self.g.read().unwrap();
            prog.rev = g
                .rev
                .iter()
                .enumerate()
                .flat_map(|(dst, srcs)| srcs.iter().map(move |&(s, ok)| (dst as u32, s, ok)))
                .collect();
        }
        prog.seed = vec![(t,)];
        prog.max_depth = vec![(d,)];
        prog.trusted_only = vec![(trusted,)];
        prog.run();
        let mut out: Vec<(u32, u32)> =
            prog.reach.iter().filter(|(x, _)| *x != t).map(|(x, Dual(dd))| (*x, *dd)).collect();
        out.sort_unstable();
        out
    }

    fn load_from_disk(&self, n_syms: usize, n_files: usize) {
        let mut g = Graph::with_size(n_syms, n_files);
        let rtx = self.db.begin_read().unwrap();
        let table = rtx.open_table(FILES).unwrap();
        for item in table.iter().unwrap() {
            let (k, v) = item.unwrap();
            let edges = decode(v.value());
            for e in &edges {
                g.add(e);
            }
            g.by_file[k.value() as usize] = edges;
        }
        *self.g.write().unwrap() = g;
    }
}

impl Engine for DiyEngine {
    fn label(&self) -> String {
        "diy-ascent-redb".into()
    }

    fn load(&self, ds: &Dataset) {
        let wtx = self.db.begin_write().unwrap();
        {
            let mut table = wtx.open_table(FILES).unwrap();
            for (f, edges) in ds.edges_by_file.iter().enumerate() {
                table.insert(f as u32, encode(edges).as_slice()).unwrap();
            }
        }
        wtx.commit().unwrap();
        self.load_from_disk(ds.n_syms() as usize, ds.cfg.files as usize);
    }

    fn variants(&self) -> Vec<&'static str> {
        vec!["rust_bfs", "ascent_per_query"]
    }

    fn blast(&self, variant: &str, t: u32, d: u32, trusted: bool) -> Vec<(u32, u32)> {
        match variant {
            "rust_bfs" => self.blast_bfs(t, d, trusted),
            "ascent_per_query" => self.blast_ascent(t, d, trusted),
            v => panic!("unknown variant {v}"),
        }
    }

    fn shortest_path(&self, a: u32, b: u32) -> Option<u32> {
        let g = self.g.read().unwrap();
        let mut dist = vec![u32::MAX; g.fwd.len()];
        dist[a as usize] = 0;
        let mut q = VecDeque::from([a]);
        while let Some(y) = q.pop_front() {
            if y == b {
                return Some(dist[y as usize]);
            }
            for &x in &g.fwd[y as usize] {
                if dist[x as usize] == u32::MAX {
                    dist[x as usize] = dist[y as usize] + 1;
                    q.push_back(x);
                }
            }
        }
        None
    }

    fn scc(&self) -> Option<Vec<u32>> {
        Some(tarjan(&self.g.read().unwrap().fwd))
    }

    fn communities(&self) -> Option<usize> {
        None // Louvain would have to be written (~200 LOC); not available off the shelf.
    }

    fn pagerank(&self) -> Option<usize> {
        let g = self.g.read().unwrap();
        let n = g.fwd.len();
        let mut rank = vec![1.0 / n as f64; n];
        for _ in 0..20 {
            let mut next = vec![0.15 / n as f64; n];
            for (v, outs) in g.fwd.iter().enumerate() {
                if outs.is_empty() {
                    continue;
                }
                let share = 0.85 * rank[v] / outs.len() as f64;
                for &w in outs {
                    next[w as usize] += share;
                }
            }
            rank = next;
        }
        Some(rank.len())
    }

    fn replace_file(&self, _ds: &Dataset, file: u32, edges: &[Edge]) {
        // Durable first (single key overwrite), then a short in-memory patch.
        let wtx = self.db.begin_write().unwrap();
        {
            let mut table = wtx.open_table(FILES).unwrap();
            table.insert(file, encode(edges).as_slice()).unwrap();
        }
        wtx.commit().unwrap();
        self.g.write().unwrap().replace(file, edges);
    }

    fn disk_path(&self) -> Option<PathBuf> {
        Some(self.path.clone())
    }
}

struct DiyFactory {
    dir: PathBuf,
    size: String,
}

impl Factory for DiyFactory {
    type E = DiyEngine;
    fn fresh(&self, tag: &str) -> DiyEngine {
        let path = self.dir.join(format!("diy-redb-{tag}.redb"));
        let _ = std::fs::remove_file(&path);
        DiyEngine { db: Database::create(&path).unwrap(), path, g: RwLock::new(Graph::default()) }
    }
    fn reopen(&self, tag: &str) -> Option<DiyEngine> {
        let path = self.dir.join(format!("diy-redb-{tag}.redb"));
        let cfg = common::Config::by_name(&self.size);
        let e = DiyEngine { db: Database::create(&path).unwrap(), path, g: RwLock::new(Graph::default()) };
        let t = std::time::Instant::now();
        e.load_from_disk(cfg.n_syms() as usize, cfg.files as usize);
        eprintln!("[diy] reopen: rebuilt in-memory graph from redb in {:?}", t.elapsed());
        Some(e)
    }
    fn label(&self) -> String {
        "diy-ascent-redb".into()
    }
}

fn main() {
    let (sizes, _backends, mut opts) = parse_args();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.data");
    std::fs::create_dir_all(&dir).unwrap();
    for size in &sizes {
        opts.size = size.clone();
        run_suite(&DiyFactory { dir: dir.clone(), size: size.clone() }, &opts);
    }
}
