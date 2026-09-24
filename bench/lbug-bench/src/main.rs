//! LadybugDB (lbug, Kuzu fork) candidate: on-disk and in-memory databases.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use common::{parse_args, run_suite, Dataset, Edge, Engine, Factory};
use lbug::{Connection, Database, LogicalType, SystemConfig, Value};

const VARLEN_TIMEOUT_MS: u64 = 5_000;

/// Default buffer pool is ~80% of RAM; cap it so RSS is comparable.
fn config() -> SystemConfig {
    SystemConfig::default().buffer_pool_size(1 << 30)
}

struct LbugEngine {
    db: *mut Database,
    pool: Mutex<Vec<Connection<'static>>>,
    backend: String,
    path: Option<PathBuf>,
    scratch: PathBuf,
    algo: Mutex<Option<bool>>,
}

// Connections are synchronized on the C++ side; the raw Database pointer is
// only freed in Drop after every pooled connection is gone.
unsafe impl Send for LbugEngine {}
unsafe impl Sync for LbugEngine {}

impl Drop for LbugEngine {
    fn drop(&mut self) {
        self.pool.lock().unwrap().clear();
        unsafe { drop(Box::from_raw(self.db)) };
    }
}

fn int(v: &Value) -> u32 {
    match v {
        Value::Int64(i) => *i as u32,
        other => panic!("expected INT64, got {other:?}"),
    }
}

fn id_list(ids: &[u32]) -> Value {
    Value::List(LogicalType::Int64, ids.iter().map(|&x| Value::Int64(x as i64)).collect())
}

impl LbugEngine {
    fn open(backend: &str, path: Option<PathBuf>, scratch: PathBuf) -> Self {
        let db = match &path {
            Some(p) => Database::new(p, config()).unwrap(),
            None => Database::in_memory(config()).unwrap(),
        };
        LbugEngine {
            db: Box::into_raw(Box::new(db)),
            pool: Mutex::new(Vec::new()),
            backend: backend.to_string(),
            path,
            scratch,
            algo: Mutex::new(None),
        }
    }

    fn with_conn<R>(&self, f: impl FnOnce(&Connection<'static>) -> R) -> R {
        let conn = self.pool.lock().unwrap().pop();
        let conn = conn.unwrap_or_else(|| {
            let db: &'static Database = unsafe { &*self.db };
            Connection::new(db).unwrap()
        });
        let out = f(&conn);
        self.pool.lock().unwrap().push(conn);
        out
    }

    fn q(conn: &Connection<'static>, cypher: &str) {
        conn.query(cypher).unwrap_or_else(|e| panic!("lbug query failed: {e}\n{cypher}"));
    }

    fn rows(conn: &Connection<'static>, cypher: &str, params: Vec<(&str, Value)>) -> Result<Vec<Vec<Value>>, lbug::Error> {
        let mut stmt = conn.prepare(cypher)?;
        Ok(conn.execute(&mut stmt, params)?.collect())
    }

    fn rel_filter(trusted: bool) -> &'static str {
        if trusted {
            " (r, n | WHERE r.prov = 0 OR r.resolved)"
        } else {
            ""
        }
    }

    fn min_depth(rows: Vec<Vec<Value>>) -> Vec<(u32, u32)> {
        let mut best: HashMap<u32, u32> = HashMap::new();
        for r in rows {
            let (x, d) = (int(&r[0]), int(&r[1]));
            best.entry(x).and_modify(|b| *b = (*b).min(d)).or_insert(d);
        }
        let mut out: Vec<(u32, u32)> = best.into_iter().collect();
        out.sort_unstable();
        out
    }

    fn blast_shortest(&self, t: u32, d: u32, trusted: bool) -> Vec<(u32, u32)> {
        let f = Self::rel_filter(trusted);
        let cypher = format!(
            "MATCH (t:Sym {{id: $t}})<-[e:Dep* SHORTEST 1..{d}{f}]-(x:Sym) WHERE x.id <> $t RETURN x.id, length(e)"
        );
        self.with_conn(|c| Self::min_depth(Self::rows(c, &cypher, vec![("t", Value::Int64(t as i64))]).unwrap()))
    }

    fn blast_varlen(&self, t: u32, d: u32, trusted: bool) -> Vec<(u32, u32)> {
        let f = Self::rel_filter(trusted);
        let cypher = format!(
            "MATCH (t:Sym {{id: $t}})<-[e:Dep*1..{d}{f}]-(x:Sym) WHERE x.id <> $t RETURN x.id, min(length(e))"
        );
        self.with_conn(|c| {
            c.set_query_timeout(VARLEN_TIMEOUT_MS);
            let r = Self::rows(c, &cypher, vec![("t", Value::Int64(t as i64))]);
            c.set_query_timeout(0);
            match r {
                Ok(rows) => Self::min_depth(rows),
                // Timeout / out of memory: reported as a correctness mismatch.
                Err(_) => Vec::new(),
            }
        })
    }

    fn blast_rust_bfs(&self, t: u32, d: u32, trusted: bool) -> Vec<(u32, u32)> {
        let w = if trusted { " WHERE r.prov = 0 OR r.resolved" } else { "" };
        let cypher = format!("UNWIND $f AS y MATCH (a:Sym {{id: y}})<-[r:Dep]-(x:Sym){w} RETURN DISTINCT x.id");
        self.with_conn(|c| {
            let mut stmt = c.prepare(&cypher).unwrap();
            let mut seen: HashSet<u32> = HashSet::from([t]);
            let mut out = Vec::new();
            let mut frontier = vec![t];
            for level in 1..=d {
                if frontier.is_empty() {
                    break;
                }
                let res = c.execute(&mut stmt, vec![("f", id_list(&frontier))]).unwrap();
                let mut next = Vec::new();
                for r in res {
                    let x = int(&r[0]);
                    if seen.insert(x) {
                        out.push((x, level));
                        next.push(x);
                    }
                }
                frontier = next;
            }
            out.sort_unstable();
            out
        })
    }

    /// Project the graph for the algo extension; remembers whether it works.
    fn ensure_algo(&self) -> bool {
        let mut algo = self.algo.lock().unwrap();
        if let Some(ok) = *algo {
            return ok;
        }
        let ok = self.with_conn(|c| {
            for load in ["INSTALL algo", "LOAD algo"] {
                if let Err(e) = c.query(load) {
                    eprintln!("[lbug] `{load}` failed: {e}");
                }
            }
            let _ = c.query("CALL drop_projected_graph('G')");
            match c.query("CALL project_graph('G', ['Sym'], ['Dep'])") {
                Ok(_) => c.query("CALL page_rank('G') RETURN count(*)").map_err(|e| eprintln!("[lbug] algo check: {e}")).is_ok(),
                Err(e) => {
                    eprintln!("[lbug] project_graph unavailable: {e}");
                    false
                }
            }
        });
        *algo = Some(ok);
        ok
    }
}

impl Engine for LbugEngine {
    fn label(&self) -> String {
        format!("lbug-{}", self.backend)
    }

    fn load(&self, ds: &Dataset) {
        std::fs::create_dir_all(&self.scratch).unwrap();
        let syms = self.scratch.join("syms.csv");
        let deps = self.scratch.join("deps.csv");
        {
            let mut f = std::io::BufWriter::new(std::fs::File::create(&syms).unwrap());
            for file in 0..ds.n_files() {
                for id in ds.syms_of(file) {
                    writeln!(f, "{id},{file},sym_{id}").unwrap();
                }
            }
            let mut f = std::io::BufWriter::new(std::fs::File::create(&deps).unwrap());
            for e in ds.edges() {
                writeln!(f, "{},{},{},{},{}", e.src, e.dst, e.kind, e.prov, e.resolved).unwrap();
            }
        }
        self.with_conn(|c| {
            Self::q(c, "CREATE NODE TABLE Sym(id INT64, file INT64, name STRING, PRIMARY KEY(id))");
            Self::q(c, "CREATE REL TABLE Dep(FROM Sym TO Sym, kind INT64, prov INT64, resolved BOOLEAN)");
            Self::q(c, &format!("COPY Sym FROM '{}' (HEADER=false)", syms.display()));
            Self::q(c, &format!("COPY Dep FROM '{}' (HEADER=false)", deps.display()));
        });
    }

    fn variants(&self) -> Vec<&'static str> {
        // LBUG_SKIP_VARLEN=1: the naive var-length query segfaults the process
        // when it hits the query timeout on large graphs (see study).
        if std::env::var("LBUG_SKIP_VARLEN").is_ok() {
            vec!["cypher_shortest", "rust_bfs"]
        } else {
            vec!["cypher_shortest", "rust_bfs", "cypher_varlen"]
        }
    }

    fn blast(&self, variant: &str, t: u32, d: u32, trusted: bool) -> Vec<(u32, u32)> {
        match variant {
            "cypher_shortest" => self.blast_shortest(t, d, trusted),
            "rust_bfs" => self.blast_rust_bfs(t, d, trusted),
            "cypher_varlen" => self.blast_varlen(t, d, trusted),
            v => panic!("unknown variant {v}"),
        }
    }

    fn shortest_path(&self, a: u32, b: u32) -> Option<u32> {
        let cypher = "MATCH (a:Sym {id: $a})-[e:Dep* SHORTEST 1..30]->(b:Sym {id: $b}) RETURN length(e)";
        self.with_conn(|c| {
            Self::rows(c, cypher, vec![("a", Value::Int64(a as i64)), ("b", Value::Int64(b as i64))])
                .unwrap()
                .first()
                .map(|r| int(&r[0]))
        })
    }

    fn scc(&self) -> Option<Vec<u32>> {
        if !self.ensure_algo() {
            return None;
        }
        self.with_conn(|c| {
            Self::rows(c, "CALL strongly_connected_components('G') RETURN node.id, group_id", vec![])
                .map_err(|e| eprintln!("[lbug] scc: {e}"))
                .ok()
                .map(|rows| rows.iter().map(|r| int(&r[1])).collect())
        })
    }

    fn communities(&self) -> Option<usize> {
        if !self.ensure_algo() {
            return None;
        }
        self.with_conn(|c| {
            Self::rows(c, "CALL louvain('G') RETURN node.id, louvain_id", vec![])
                .map_err(|e| eprintln!("[lbug] louvain: {e}"))
                .ok()
                .map(|rows| rows.iter().map(|r| format!("{:?}", r[1])).collect::<HashSet<_>>().len())
        })
    }

    fn pagerank(&self) -> Option<usize> {
        if !self.ensure_algo() {
            return None;
        }
        self.with_conn(|c| {
            Self::rows(c, "CALL page_rank('G') RETURN node.id, rank", vec![])
                .map_err(|e| eprintln!("[lbug] page_rank: {e}"))
                .ok()
                .map(|rows| rows.len())
        })
    }

    fn replace_file(&self, ds: &Dataset, file: u32, edges: &[Edge]) {
        let ids: Vec<u32> = ds.syms_of(file).collect();
        self.with_conn(|c| {
            Self::q(c, "BEGIN TRANSACTION");
            let mut del = c.prepare("UNWIND $ids AS i MATCH (s:Sym {id: i})-[r:Dep]->() DELETE r").unwrap();
            c.execute(&mut del, vec![("ids", id_list(&ids))]).unwrap();
            let mut up = c
                .prepare("UNWIND $ids AS i MERGE (s:Sym {id: i}) SET s.file = $f, s.name = 'sym_' + CAST(i AS STRING)")
                .unwrap();
            c.execute(&mut up, vec![("ids", id_list(&ids)), ("f", Value::Int64(file as i64))]).unwrap();
            let mut ins = c
                .prepare(
                    "MATCH (a:Sym {id: $s}), (b:Sym {id: $d}) \
                     CREATE (a)-[:Dep {kind: $k, prov: $p, resolved: $r}]->(b)",
                )
                .unwrap();
            for e in edges {
                c.execute(
                    &mut ins,
                    vec![
                        ("s", Value::Int64(e.src as i64)),
                        ("d", Value::Int64(e.dst as i64)),
                        ("k", Value::Int64(e.kind as i64)),
                        ("p", Value::Int64(e.prov as i64)),
                        ("r", Value::Bool(e.resolved)),
                    ],
                )
                .unwrap();
            }
            Self::q(c, "COMMIT");
        });
    }

    fn disk_path(&self) -> Option<PathBuf> {
        self.path.clone()
    }
}

struct LbugFactory {
    backend: String,
    dir: PathBuf,
}

impl LbugFactory {
    fn path(&self, tag: &str) -> PathBuf {
        self.dir.join(format!("lbug-{}-{tag}", self.backend))
    }
}

impl Factory for LbugFactory {
    type E = LbugEngine;
    fn fresh(&self, tag: &str) -> LbugEngine {
        let path = self.path(tag);
        let _ = std::fs::remove_dir_all(&path);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("wal"));
        let scratch = self.dir.join(format!("lbug-{}-{tag}-csv", self.backend));
        let disk = (self.backend == "disk").then_some(path);
        LbugEngine::open(&self.backend, disk, scratch)
    }
    fn reopen(&self, tag: &str) -> Option<LbugEngine> {
        (self.backend == "disk").then(|| {
            let scratch = self.dir.join(format!("lbug-{}-{tag}-csv", self.backend));
            LbugEngine::open(&self.backend, Some(self.path(tag)), scratch)
        })
    }
    fn label(&self) -> String {
        format!("lbug-{}", self.backend)
    }
}

fn main() {
    let (sizes, mut backends, mut opts) = parse_args();
    if backends.is_empty() {
        backends = vec!["disk".into(), "mem".into()];
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.data");
    std::fs::create_dir_all(&dir).unwrap();
    for size in &sizes {
        for backend in &backends {
            opts.size = size.clone();
            run_suite(&LbugFactory { backend: backend.clone(), dir: dir.clone() }, &opts);
        }
    }
}
