//! CozoDB (mnestic fork) candidate: mem / sqlite / newrocksdb backends.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use common::{parse_args, run_suite, Dataset, Edge, Engine, Factory};
use cozo::{DataValue, DbInstance, NamedRows, ScriptMutability};

const SCHEMA: &str = r#"
{:create sym {file: Int, id: Int => name: String}}
{:create edge {src: Int, dst: Int, kind: Int => prov: Int, resolved: Bool}}
{::index create edge:rev {dst, src, kind, prov, resolved}}
"#;

struct CozoEngine {
    db: DbInstance,
    backend: String,
    path: Option<PathBuf>,
}

fn p(pairs: Vec<(&str, DataValue)>) -> BTreeMap<String, DataValue> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn int(v: &DataValue) -> u32 {
    v.get_int().expect("int") as u32
}

fn edge_row(e: &Edge) -> DataValue {
    DataValue::List(vec![
        DataValue::from(e.src as i64),
        DataValue::from(e.dst as i64),
        DataValue::from(e.kind as i64),
        DataValue::from(e.prov as i64),
        DataValue::Bool(e.resolved),
    ])
}

fn sym_rows(ds: &Dataset, file: u32) -> Vec<DataValue> {
    ds.syms_of(file)
        .map(|id| {
            DataValue::List(vec![
                DataValue::from(file as i64),
                DataValue::from(id as i64),
                DataValue::Str(format!("sym_{id}").into()),
            ])
        })
        .collect()
}

impl CozoEngine {
    fn run(&self, script: &str, params: BTreeMap<String, DataValue>, write: bool) -> NamedRows {
        let m = if write { ScriptMutability::Mutable } else { ScriptMutability::Immutable };
        self.db
            .run_script(script, params, m)
            .unwrap_or_else(|e| panic!("cozo script failed: {e:?}\n{script}"))
    }

    fn trusted_filter(trusted: bool) -> &'static str {
        if trusted {
            ", or(prov == 0, resolved)"
        } else {
            ""
        }
    }

    fn blast_recursive(&self, t: u32, d: u32, trusted: bool) -> Vec<(u32, u32)> {
        let f = Self::trusted_filter(trusted);
        let script = format!(
            "reach[x, min(d)] := *edge:rev{{dst: $t, src: x, prov, resolved}}{f}, d = 1\n\
             reach[x, min(d)] := reach[y, d0], d0 < $maxd, *edge:rev{{dst: y, src: x, prov, resolved}}{f}, d = d0 + 1\n\
             ?[x, d] := reach[x, d], x != $t"
        );
        self.collect(&script, t, d)
    }

    fn blast_unrolled(&self, t: u32, d: u32, trusted: bool) -> Vec<(u32, u32)> {
        let f = Self::trusted_filter(trusted);
        let mut s = format!("l1[x] := *edge:rev{{dst: $t, src: x, prov, resolved}}{f}\n");
        for i in 2..=d {
            s.push_str(&format!(
                "l{i}[x] := l{}[y], *edge:rev{{dst: y, src: x, prov, resolved}}{f}\n",
                i - 1
            ));
        }
        for i in 1..=d {
            s.push_str(&format!("?[x, min(d)] := l{i}[x], x != $t, d = {i}\n"));
        }
        self.collect(&s, t, d)
    }

    fn collect(&self, script: &str, t: u32, d: u32) -> Vec<(u32, u32)> {
        let rows = self.run(
            script,
            p(vec![("t", DataValue::from(t as i64)), ("maxd", DataValue::from(d as i64))]),
            false,
        );
        let mut out: Vec<(u32, u32)> = rows.rows.iter().map(|r| (int(&r[0]), int(&r[1]))).collect();
        out.sort_unstable();
        out
    }

    /// BFS driven from Rust: one indexed lookup query per level.
    fn blast_rust_bfs(&self, t: u32, d: u32, trusted: bool) -> Vec<(u32, u32)> {
        let f = Self::trusted_filter(trusted);
        let script = format!("?[x] := y in $f, *edge:rev{{dst: y, src: x, prov, resolved}}{f}");
        let mut depth: HashMap<u32, u32> = HashMap::new();
        let mut seen: HashSet<u32> = HashSet::from([t]);
        let mut frontier = vec![t];
        for level in 1..=d {
            if frontier.is_empty() {
                break;
            }
            let list = DataValue::List(frontier.iter().map(|&x| DataValue::from(x as i64)).collect());
            let rows = self.run(&script, p(vec![("f", list)]), false);
            let mut next = Vec::new();
            for r in &rows.rows {
                let x = int(&r[0]);
                if seen.insert(x) {
                    depth.insert(x, level);
                    next.push(x);
                }
            }
            frontier = next;
        }
        let mut out: Vec<(u32, u32)> = depth.into_iter().collect();
        out.sort_unstable();
        out
    }
}

impl Engine for CozoEngine {
    fn label(&self) -> String {
        format!("cozo-mnestic-{}", self.backend)
    }

    fn load(&self, ds: &Dataset) {
        self.run(SCHEMA, BTreeMap::new(), true);
        let syms: Vec<DataValue> = (0..ds.cfg.files).flat_map(|f| sym_rows(ds, f)).collect();
        for chunk in syms.chunks(50_000) {
            self.run(
                "?[file, id, name] <- $rows :put sym {file, id => name}",
                p(vec![("rows", DataValue::List(chunk.to_vec()))]),
                true,
            );
        }
        let edges: Vec<DataValue> = ds.edges().map(edge_row).collect();
        for chunk in edges.chunks(50_000) {
            self.run(
                "?[src, dst, kind, prov, resolved] <- $rows :put edge {src, dst, kind => prov, resolved}",
                p(vec![("rows", DataValue::List(chunk.to_vec()))]),
                true,
            );
        }
        self.run("::graph create g {edges: edge}", BTreeMap::new(), true);
    }

    fn variants(&self) -> Vec<&'static str> {
        vec!["recursive", "unrolled", "rust_bfs"]
    }

    fn blast(&self, variant: &str, t: u32, d: u32, trusted: bool) -> Vec<(u32, u32)> {
        match variant {
            "recursive" => self.blast_recursive(t, d, trusted),
            "unrolled" => self.blast_unrolled(t, d, trusted),
            "rust_bfs" => self.blast_rust_bfs(t, d, trusted),
            v => panic!("unknown variant {v}"),
        }
    }

    fn shortest_path(&self, a: u32, b: u32) -> Option<u32> {
        let rows = self.run(
            "start[x] <- [[$a]]\n\
             goal[x] <- [[$b]]\n\
             ?[s, g, cost, path] <~ ShortestPathDijkstra(start[], goal[], graph: 'g')",
            p(vec![("a", DataValue::from(a as i64)), ("b", DataValue::from(b as i64))]),
            false,
        );
        rows.rows.first().and_then(|r| match &r[3] {
            DataValue::List(path) => Some(path.len() as u32 - 1),
            _ => None,
        })
    }

    fn scc(&self) -> Option<Vec<u32>> {
        let rows = self.run(
            "?[n, c] <~ StronglyConnectedComponents(graph: 'g')",
            BTreeMap::new(),
            false,
        );
        Some(rows.rows.iter().map(|r| int(&r[1])).collect())
    }

    fn communities(&self) -> Option<usize> {
        let rows = self.run(
            "?[labels, n] <~ CommunityDetectionLouvain(graph: 'g')",
            BTreeMap::new(),
            false,
        );
        let distinct: HashSet<String> = rows.rows.iter().map(|r| format!("{:?}", r[0])).collect();
        Some(distinct.len())
    }

    fn pagerank(&self) -> Option<usize> {
        let rows = self.run("?[n, r] <~ PageRank(graph: 'g')", BTreeMap::new(), false);
        Some(rows.rows.len())
    }

    fn replace_file(&self, ds: &Dataset, file: u32, edges: &[Edge]) {
        // One chained script == one atomic transaction.
        let mut script = String::from(
            "{?[src, dst, kind] := *sym{file: $f, id: src}, *edge{src, dst, kind} :rm edge {src, dst, kind}}\n\
             {?[file, id, name] <- $syms :put sym {file, id => name}}\n",
        );
        if !edges.is_empty() {
            script.push_str(
                "{?[src, dst, kind, prov, resolved] <- $edges :put edge {src, dst, kind => prov, resolved}}\n",
            );
        }
        self.run(
            &script,
            p(vec![
                ("f", DataValue::from(file as i64)),
                ("syms", DataValue::List(sym_rows(ds, file))),
                ("edges", DataValue::List(edges.iter().map(edge_row).collect())),
            ]),
            true,
        );
    }

    fn disk_path(&self) -> Option<PathBuf> {
        self.path.clone()
    }
}

struct CozoFactory {
    backend: String,
    dir: PathBuf,
}

impl CozoFactory {
    fn path(&self, tag: &str) -> PathBuf {
        self.dir.join(format!("cozo-{}-{tag}", self.backend))
    }
    fn open(&self, tag: &str) -> CozoEngine {
        let path = self.path(tag);
        let (db, path) = if self.backend == "mem" {
            (DbInstance::new("mem", "", "").unwrap(), None)
        } else {
            (DbInstance::new(&self.backend, &path, "").unwrap(), Some(path))
        };
        CozoEngine { db, backend: self.backend.clone(), path }
    }
}

impl Factory for CozoFactory {
    type E = CozoEngine;
    fn fresh(&self, tag: &str) -> CozoEngine {
        let path = self.path(tag);
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::remove_file(&path);
        self.open(tag)
    }
    fn reopen(&self, tag: &str) -> Option<CozoEngine> {
        if self.backend == "mem" {
            None
        } else {
            Some(self.open(tag))
        }
    }
    fn label(&self) -> String {
        format!("cozo-mnestic-{}", self.backend)
    }
}

fn main() {
    let (sizes, mut backends, mut opts) = parse_args();
    if backends.is_empty() {
        backends = vec!["mem".into(), "sqlite".into(), "newrocksdb".into()];
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.data");
    std::fs::create_dir_all(&dir).unwrap();
    for size in &sizes {
        for backend in &backends {
            opts.size = size.clone();
            run_suite(&CozoFactory { backend: backend.clone(), dir: dir.clone() }, &opts);
        }
    }
}
