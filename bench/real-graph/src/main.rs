//! Extract a real code graph from a repo and measure its shape.
//! Usage: real-graph <repo-root> <label> [prefix=depth ...]
//!   prefix=depth: group files under `prefix/` by their first `depth` path components.

mod extract;

use extract::{Extracted, Fam, Lang};
use serde::Serialize;
use std::collections::{HashMap, HashSet, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};

const SKIP_DIRS: &[&str] = &[
    "target", "node_modules", "dist", "build", ".venv", "venv", "vendor", ".git", ".worktrees",
    "__pycache__", ".next", "site-packages", "coverage",
];

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Kind {
    Calls,
    Imports,
    Implements,
    Contains,
}
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Calls => "calls",
            Kind::Imports => "imports",
            Kind::Implements => "implements",
            Kind::Contains => "contains",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Prov {
    Extracted,
    Inferred,
}

enum Res {
    Ex(usize),
    Inf(usize),
    Ambiguous,
    External,
}

struct Ctx<'a> {
    x: &'a Extracted,
    root: PathBuf,
    by_fam_name: HashMap<(Fam, String), Vec<usize>>,
    by_file_name: HashMap<(usize, String), Vec<usize>>,
    by_dir_name: HashMap<(Fam, String, String), Vec<usize>>,
    by_unit_name: HashMap<(Fam, String, String), Vec<usize>>,
    file_by_path: HashMap<String, usize>,
    files_by_dir: HashMap<String, Vec<usize>>,
    py_by_last: HashMap<String, Vec<usize>>,
    file_unit: Vec<String>,
    go_modules: Vec<(String, String)>, // (module path, dir)
    name_maps: Vec<HashMap<String, (Vec<usize>, String)>>,
    mod_maps: Vec<HashMap<String, Vec<usize>>>,
    /// import aliases/names that resolve outside the repo (stdlib, third-party)
    external_aliases: Vec<HashSet<String>>,
}

/// Method names so common on stdlib/third-party receivers that an unknown-receiver
/// name match is almost always wrong.
const COMMON_METHODS: &[&str] = &[
    "get", "set", "put", "post", "delete", "push", "pop", "append", "extend", "insert", "remove", "clear",
    "update", "add", "has", "keys", "values", "items", "map", "filter", "reduce", "forEach", "find",
    "some", "every", "includes", "indexOf", "slice", "splice", "join", "split", "sort", "concat", "then",
    "catch", "finally", "toString", "format", "replace", "strip", "lower", "upper", "startswith",
    "endswith", "encode", "decode", "read", "write", "close", "open", "send", "emit", "on", "off", "next",
    "copy", "len", "count", "index", "json", "text", "Lock", "Unlock", "RLock", "RUnlock", "Now", "Is",
    "As", "Close", "Error", "String", "Wait", "Done", "Add", "Len", "Read", "Write", "Get", "Set", "Delete",
    "Load", "Store", "Run", "Start", "Stop", "Err", "Value", "Scan", "Exec", "Query", "Info", "Debug",
    "Warn", "Errorf", "Infof", "Debugf", "Warnf", "info", "debug", "warning", "error", "exception",
    "log", "print", "run", "start", "stop", "execute", "call", "apply", "bind", "resolve", "reject",
];

/// RG_PERMISSIVE=1: unbounded name-guess resolution (upper bound on edges/blast).
fn permissive() -> bool {
    std::env::var("RG_PERMISSIVE").map(|v| v == "1").unwrap_or(false)
}

fn dir_of(p: &str) -> String {
    match p.rfind('/') {
        Some(i) => p[..i].to_string(),
        None => String::new(),
    }
}

fn norm(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

fn join(a: &str, b: &str) -> String {
    if a.is_empty() {
        norm(b)
    } else {
        norm(&format!("{a}/{b}"))
    }
}

impl<'a> Ctx<'a> {
    fn new(x: &'a Extracted, root: &Path) -> Self {
        let mut c = Ctx {
            x,
            root: root.to_path_buf(),
            by_fam_name: HashMap::new(),
            by_file_name: HashMap::new(),
            by_dir_name: HashMap::new(),
            by_unit_name: HashMap::new(),
            file_by_path: HashMap::new(),
            files_by_dir: HashMap::new(),
            py_by_last: HashMap::new(),
            file_unit: Vec::new(),
            go_modules: Vec::new(),
            name_maps: Vec::new(),
            mod_maps: Vec::new(),
            external_aliases: Vec::new(),
        };
        let mut unit_cache: HashMap<String, String> = HashMap::new();
        for (i, f) in x.files.iter().enumerate() {
            c.file_by_path.insert(f.path.clone(), i);
            let d = dir_of(&f.path);
            c.files_by_dir.entry(d.clone()).or_default().push(i);
            if f.lang == Lang::Py {
                let stem = f.path.trim_end_matches(".py");
                let stem = stem.strip_suffix("/__init__").unwrap_or(stem);
                let last = stem.rsplit('/').next().unwrap_or(stem).to_string();
                c.py_by_last.entry(last).or_default().push(i);
            }
            let unit = c.unit_for(&d, &mut unit_cache);
            c.file_unit.push(unit);
        }
        // go.mod module paths
        let mut seen = HashSet::new();
        for u in c.file_unit.clone() {
            if !seen.insert(u.clone()) {
                continue;
            }
            let gm = c.root.join(&u).join("go.mod");
            if let Ok(s) = std::fs::read_to_string(&gm) {
                if let Some(line) = s.lines().find(|l| l.starts_with("module ")) {
                    c.go_modules.push((line[7..].trim().to_string(), u.clone()));
                }
            }
        }
        c.go_modules.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        for (i, s) in x.syms.iter().enumerate() {
            if s.kind == "module" || s.name.is_empty() {
                continue;
            }
            let f = &x.files[s.file];
            let fam = f.lang.fam();
            c.by_fam_name.entry((fam, s.name.clone())).or_default().push(i);
            c.by_file_name.entry((s.file, s.name.clone())).or_default().push(i);
            c.by_dir_name
                .entry((fam, dir_of(&f.path), s.name.clone()))
                .or_default()
                .push(i);
            c.by_unit_name
                .entry((fam, c.file_unit[s.file].clone(), s.name.clone()))
                .or_default()
                .push(i);
        }
        // per-file import maps
        for (i, f) in x.files.iter().enumerate() {
            let mut nm = HashMap::new();
            for (local, spec, orig) in &f.imports.names {
                let files = c.resolve_spec(i, spec);
                nm.insert(local.clone(), (files, orig.clone()));
            }
            let mut ext = HashSet::new();
            for (local, (files, _)) in &nm {
                if files.is_empty() && f.lang.fam() != Fam::Rust {
                    ext.insert(local.clone());
                }
            }
            let mut mm = HashMap::new();
            for (alias, spec) in &f.imports.modules {
                let files = c.resolve_spec(i, spec);
                if !files.is_empty() {
                    mm.insert(alias.clone(), files);
                } else {
                    ext.insert(alias.clone());
                }
            }
            c.name_maps.push(nm);
            c.mod_maps.push(mm);
            c.external_aliases.push(ext);
        }
        c
    }

    fn unit_for(&self, dir: &str, cache: &mut HashMap<String, String>) -> String {
        if let Some(u) = cache.get(dir) {
            return u.clone();
        }
        const MANIFESTS: &[&str] = &["Cargo.toml", "package.json", "pyproject.toml", "setup.py", "go.mod"];
        let mut d = dir.to_string();
        let found = loop {
            if MANIFESTS.iter().any(|m| self.root.join(&d).join(m).exists()) {
                break d.clone();
            }
            if d.is_empty() {
                break String::new();
            }
            d = dir_of(&d);
        };
        cache.insert(dir.to_string(), found.clone());
        found
    }

    fn fam(&self, file: usize) -> Fam {
        self.x.files[file].lang.fam()
    }

    /// Resolve a module/import spec to repo files.
    fn resolve_spec(&self, file: usize, spec: &str) -> Vec<usize> {
        let f = &self.x.files[file];
        let dir = dir_of(&f.path);
        match f.lang.fam() {
            Fam::Py => {
                if spec.starts_with('.') {
                    let dots = spec.chars().take_while(|c| *c == '.').count();
                    let mut base = dir.clone();
                    for _ in 1..dots {
                        base = dir_of(&base);
                    }
                    let rest = spec[dots..].replace('.', "/");
                    let stem = if rest.is_empty() { base } else { join(&base, &rest) };
                    return self.py_stem(&stem).into_iter().collect();
                }
                let parts: Vec<&str> = spec.split('.').collect();
                let last = parts.last().copied().unwrap_or("");
                let tail = parts.join("/");
                let mut best: Option<(usize, usize)> = None;
                if let Some(cands) = self.py_by_last.get(last) {
                    for &cf in cands {
                        let p = &self.x.files[cf].path;
                        let stem = p.trim_end_matches(".py");
                        let stem = stem.strip_suffix("/__init__").unwrap_or(stem);
                        if stem == tail || stem.ends_with(&format!("/{tail}")) {
                            let common = p.chars().zip(f.path.chars()).take_while(|(a, b)| a == b).count();
                            if best.map(|(_, c)| common > c).unwrap_or(true) {
                                best = Some((cf, common));
                            }
                        }
                    }
                }
                best.map(|(f, _)| vec![f]).unwrap_or_default()
            }
            Fam::Js => {
                let base = if spec.starts_with('.') {
                    join(&dir, spec)
                } else if let Some(rest) = spec.strip_prefix("@/").or_else(|| spec.strip_prefix("~/")) {
                    let unit = &self.file_unit[file];
                    let with_src = join(unit, &format!("src/{rest}"));
                    if !self.js_try(&with_src).is_empty() {
                        return self.js_try(&with_src);
                    }
                    join(unit, rest)
                } else {
                    return Vec::new();
                };
                self.js_try(&base)
            }
            Fam::Go => {
                for (m, mdir) in &self.go_modules {
                    if spec == m || spec.starts_with(&format!("{m}/")) {
                        let suffix = spec[m.len()..].trim_start_matches('/');
                        let d = join(mdir, suffix);
                        return self
                            .files_by_dir
                            .get(&d)
                            .map(|v| v.iter().copied().filter(|&i| self.x.files[i].lang == Lang::Go).collect())
                            .unwrap_or_default();
                    }
                }
                Vec::new()
            }
            Fam::Rust => {
                let head = spec.split('{').next().unwrap_or(spec).trim_end_matches("::");
                let mut segs: Vec<&str> = head.split("::").map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
                let unit = self.file_unit[file].clone();
                let mut base = match segs.first().copied() {
                    Some("crate") => {
                        segs.remove(0);
                        join(&unit, "src")
                    }
                    Some("super") => {
                        segs.remove(0);
                        let mut b = dir.clone();
                        let fname = f.path.rsplit('/').next().unwrap_or("");
                        if fname == "mod.rs" || fname == "lib.rs" || fname == "main.rs" {
                            b = dir_of(&b);
                        }
                        b
                    }
                    Some("self") => {
                        segs.remove(0);
                        dir.clone()
                    }
                    _ => return Vec::new(),
                };
                while segs.first() == Some(&"super") {
                    segs.remove(0);
                    base = dir_of(&base);
                }
                while !segs.is_empty() {
                    let p = join(&base, &segs.join("/"));
                    for cand in [format!("{p}.rs"), format!("{p}/mod.rs")] {
                        if let Some(&i) = self.file_by_path.get(&cand) {
                            return vec![i];
                        }
                    }
                    segs.pop();
                }
                Vec::new()
            }
        }
    }

    fn py_stem(&self, stem: &str) -> Option<usize> {
        self.file_by_path
            .get(&format!("{stem}.py"))
            .or_else(|| self.file_by_path.get(&format!("{stem}/__init__.py")))
            .copied()
    }

    fn js_try(&self, base: &str) -> Vec<usize> {
        for ext in [
            "", ".ts", ".tsx", ".js", ".jsx", ".mjs", "/index.ts", "/index.tsx", "/index.js", "/index.jsx",
        ] {
            if let Some(&i) = self.file_by_path.get(&format!("{base}{ext}")) {
                return vec![i];
            }
        }
        Vec::new()
    }

    fn pick(&self, cands: &[usize], extracted: bool) -> Option<Res> {
        match cands.len() {
            0 => None,
            1 => Some(if extracted { Res::Ex(cands[0]) } else { Res::Inf(cands[0]) }),
            _ => Some(Res::Ambiguous),
        }
    }

    fn filt(&self, v: Option<&Vec<usize>>, pred: impl Fn(&extract::Sym) -> bool) -> Vec<usize> {
        v.map(|v| v.iter().copied().filter(|&i| pred(&self.x.syms[i])).collect())
            .unwrap_or_default()
    }

    fn in_files(&self, files: &[usize], name: &str, pred: impl Fn(&extract::Sym) -> bool + Copy) -> Vec<usize> {
        let mut out = Vec::new();
        for &f in files {
            out.extend(self.filt(self.by_file_name.get(&(f, name.to_string())), pred));
        }
        out
    }

    /// Scope ladder: file -> dir -> unit -> repo. Returns first non-empty scope result.
    fn ladder(
        &self,
        file: usize,
        name: &str,
        pred: impl Fn(&extract::Sym) -> bool + Copy,
        extracted_file: bool,
        extracted_dir: bool,
    ) -> Res {
        self.ladder_to(file, name, pred, extracted_file, extracted_dir, true)
    }

    fn ladder_to(
        &self,
        file: usize,
        name: &str,
        pred: impl Fn(&extract::Sym) -> bool + Copy,
        extracted_file: bool,
        extracted_dir: bool,
        repo_scope: bool,
    ) -> Res {
        let fam = self.fam(file);
        let path = &self.x.files[file].path;
        let c = self.filt(self.by_file_name.get(&(file, name.to_string())), pred);
        if let Some(r) = self.pick(&c, extracted_file) {
            return r;
        }
        let c = self.filt(self.by_dir_name.get(&(fam, dir_of(path), name.to_string())), pred);
        if let Some(r) = self.pick(&c, extracted_dir) {
            return r;
        }
        let c = self.filt(
            self.by_unit_name.get(&(fam, self.file_unit[file].clone(), name.to_string())),
            pred,
        );
        if let Some(r) = self.pick(&c, false) {
            return r;
        }
        if !repo_scope {
            return Res::External;
        }
        let c = self.filt(self.by_fam_name.get(&(fam, name.to_string())), pred);
        self.pick(&c, false).unwrap_or(Res::External)
    }

    /// Member call on a receiver of unknown type: name guess, bounded to the unit.
    fn member_guess(&self, file: usize, name: &str) -> Res {
        if permissive() {
            return self.ladder(file, name, |s| s.kind == "method", false, false);
        }
        if COMMON_METHODS.contains(&name) {
            return Res::External;
        }
        self.ladder_to(file, name, |s| s.kind == "method", false, false, false)
    }

    fn resolve_call(&self, call: &extract::RawCall) -> Res {
        let file = self.x.syms[call.caller].file;
        let fam = self.fam(file);
        let name = call.name.as_str();
        let any = |_: &extract::Sym| true;

        if call.is_self {
            if let Some(t) = &call.self_type {
                let scope: Vec<usize> = if fam == Fam::Go || fam == Fam::Rust {
                    self.filt(
                        self.by_unit_name.get(&(fam, self.file_unit[file].clone(), name.to_string())),
                        |s| s.parent_type.as_deref() == Some(t.as_str()),
                    )
                } else {
                    self.filt(self.by_file_name.get(&(file, name.to_string())), |s| {
                        s.parent_type.as_deref() == Some(t.as_str())
                    })
                };
                if let Some(r) = self.pick(&scope, true) {
                    return r;
                }
            }
            return self.member_guess(file, name);
        }

        if let Some(q) = &call.qualifier {
            // module / package alias
            let head = q.split('.').next().unwrap_or(q);
            if let Some(files) = self.mod_maps[file].get(q).or_else(|| self.mod_maps[file].get(head)) {
                let c = self.in_files(files, name, any);
                if let Some(r) = self.pick(&c, true) {
                    return r;
                }
                return Res::External;
            }
            if !permissive() && (self.external_aliases[file].contains(q) || self.external_aliases[file].contains(head)) {
                return Res::External;
            }
            // imported type used as qualifier: Foo.bar() / Foo::bar()
            let q_last = q.rsplit(|c| c == ':' || c == '.').next().unwrap_or(q).to_string();
            if let Some((files, orig)) = self.name_maps[file].get(&q_last) {
                let c = self.in_files(files, name, |s| s.parent_type.as_deref() == Some(orig.as_str()));
                if let Some(r) = self.pick(&c, true) {
                    return r;
                }
            }
            if fam == Fam::Rust {
                let upper = q_last.chars().next().map(|c| c.is_ascii_uppercase()).unwrap_or(false);
                if upper || q_last == "Self" {
                    let t = if q_last == "Self" { call.self_type.clone().unwrap_or_default() } else { q_last.clone() };
                    let c = self.filt(
                        self.by_unit_name.get(&(fam, self.file_unit[file].clone(), name.to_string())),
                        |s| s.parent_type.as_deref() == Some(t.as_str()),
                    );
                    if let Some(r) = self.pick(&c, true) {
                        return r;
                    }
                    let c = self.filt(self.by_fam_name.get(&(fam, name.to_string())), |s| {
                        s.parent_type.as_deref() == Some(t.as_str())
                    });
                    return self.pick(&c, true).unwrap_or(Res::External);
                } else if q.contains("::") || !q.contains('.') {
                    // module-qualified free function: prefer files whose stem/dir == q_last
                    let c = self.filt(
                        self.by_unit_name.get(&(fam, self.file_unit[file].clone(), name.to_string())),
                        |s| {
                            let p = &self.x.files[s.file].path;
                            s.parent_type.is_none()
                                && (p.ends_with(&format!("/{q_last}.rs")) || p.ends_with(&format!("/{q_last}/mod.rs")))
                        },
                    );
                    if let Some(r) = self.pick(&c, true) {
                        return r;
                    }
                }
            }
            // q is a local variable / receiver / expression: unknown type
            return self.member_guess(file, name);
        }

        // plain name
        let c = self.filt(self.by_file_name.get(&(file, name.to_string())), |s| s.kind != "method");
        if let Some(r) = self.pick(&c, true) {
            return r;
        }
        if let Some((files, orig)) = self.name_maps[file].get(name) {
            if !files.is_empty() {
                let o = if orig == "default" { name } else { orig.as_str() };
                let c = self.in_files(files, o, |s| s.kind != "method");
                if let Some(r) = self.pick(&c, true) {
                    return r;
                }
                // python: `from pkg import mod` then mod() is rare; fall through
            } else if fam == Fam::Rust {
                // `use` path not mapped to a file: unique in crate, then repo
                let o = orig.as_str();
                let c = self.filt(
                    self.by_unit_name.get(&(fam, self.file_unit[file].clone(), o.to_string())),
                    |s| s.kind != "method",
                );
                if let Some(r) = self.pick(&c, true) {
                    return r;
                }
                let c = self.filt(self.by_fam_name.get(&(fam, o.to_string())), |s| s.kind != "method");
                return self.pick(&c, true).unwrap_or(Res::External);
            } else {
                // imported from an external package
                return Res::External;
            }
        }
        // Go: same package (dir) is lexical scope -> extracted
        self.ladder(file, name, |s| s.kind != "method", true, fam == Fam::Go)
    }

    fn resolve_type(&self, file: usize, name: &str) -> Res {
        let is_type = |s: &extract::Sym| s.is_type();
        if let Some((files, orig)) = self.name_maps[file].get(name) {
            if !files.is_empty() {
                let c = self.in_files(files, orig, is_type);
                if let Some(r) = self.pick(&c, true) {
                    return r;
                }
            }
        }
        let fam = self.fam(file);
        self.ladder(file, name, is_type, true, fam == Fam::Go || fam == Fam::Rust)
    }
}

#[derive(Serialize, Default)]
struct Dist {
    max: usize,
    p99: usize,
    p90: usize,
    median: usize,
    mean: f64,
}

fn dist(mut v: Vec<usize>) -> Dist {
    if v.is_empty() {
        return Dist::default();
    }
    v.sort_unstable();
    let q = |p: f64| v[((v.len() - 1) as f64 * p).round() as usize];
    Dist {
        max: *v.last().unwrap(),
        p99: q(0.99),
        p90: q(0.90),
        median: q(0.5),
        mean: v.iter().sum::<usize>() as f64 / v.len() as f64,
    }
}

/// BFS over reversed edges; returns (reach<=3, reach<=10, reach unbounded).
fn blast(rev: &[Vec<u32>], target: usize, depth: &mut [u32], q: &mut VecDeque<u32>, touched: &mut Vec<u32>) -> (usize, usize, usize) {
    let (mut d3, mut d10, mut all) = (0, 0, 0);
    depth[target] = 0;
    touched.push(target as u32);
    q.push_back(target as u32);
    while let Some(y) = q.pop_front() {
        let d = depth[y as usize];
        for &x in &rev[y as usize] {
            if depth[x as usize] == u32::MAX {
                let nd = d + 1;
                depth[x as usize] = nd;
                touched.push(x);
                all += 1;
                if nd <= 3 {
                    d3 += 1;
                }
                if nd <= 10 {
                    d10 += 1;
                }
                q.push_back(x);
            }
        }
    }
    for &t in touched.iter() {
        depth[t as usize] = u32::MAX;
    }
    touched.clear();
    (d3, d10, all)
}

fn tarjan(adj: &[Vec<u32>]) -> Vec<u32> {
    let n = adj.len();
    let mut index = vec![u32::MAX; n];
    let mut low = vec![0u32; n];
    let mut on = vec![false; n];
    let mut comp = vec![u32::MAX; n];
    let mut stack = Vec::new();
    let mut idx = 0u32;
    let mut ncomp = 0u32;
    let mut call: Vec<(u32, usize)> = Vec::new();
    for s in 0..n {
        if index[s] != u32::MAX {
            continue;
        }
        call.push((s as u32, 0));
        while let Some(&(v, i)) = call.last() {
            let v_ = v as usize;
            if i == 0 {
                index[v_] = idx;
                low[v_] = idx;
                idx += 1;
                stack.push(v);
                on[v_] = true;
            }
            if i < adj[v_].len() {
                call.last_mut().unwrap().1 += 1;
                let w = adj[v_][i] as usize;
                if index[w] == u32::MAX {
                    call.push((w as u32, 0));
                } else if on[w] {
                    low[v_] = low[v_].min(index[w]);
                }
            } else {
                call.pop();
                if let Some(&(p, _)) = call.last() {
                    low[p as usize] = low[p as usize].min(low[v_]);
                }
                if low[v_] == index[v_] {
                    loop {
                        let w = stack.pop().unwrap();
                        on[w as usize] = false;
                        comp[w as usize] = ncomp;
                        if w == v {
                            break;
                        }
                    }
                    ncomp += 1;
                }
            }
        }
    }
    comp
}

#[derive(Serialize)]
struct SccStats {
    nontrivial_sccs: usize,
    largest_scc: usize,
    symbols_in_cycles: usize,
}

fn scc_stats(adj: &[Vec<u32>]) -> SccStats {
    let comp = tarjan(adj);
    let mut sizes: HashMap<u32, usize> = HashMap::new();
    for &c in &comp {
        *sizes.entry(c).or_default() += 1;
    }
    let nontriv: Vec<usize> = sizes.values().copied().filter(|&s| s > 1).collect();
    SccStats {
        nontrivial_sccs: nontriv.len(),
        largest_scc: nontriv.iter().copied().max().unwrap_or(0),
        symbols_in_cycles: nontriv.iter().sum(),
    }
}

#[derive(Serialize)]
struct Hub {
    name: String,
    kind: String,
    file: String,
    line: u32,
    fan_in: usize,
    blast_d10_calls: usize,
    blast_d10_all: usize,
}

#[derive(Serialize)]
struct BlastStats {
    edge_set: &'static str,
    targets_measured: usize,
    sampled: bool,
    d3: Dist,
    d10: Dist,
    unbounded: Dist,
    top5_hub_pct_d10: Vec<f64>,
    top5_hub_pct_unbounded: Vec<f64>,
    max_pct_d10: f64,
    targets_over_10pct_d10: usize,
    targets_over_50pct_d10: usize,
}

#[derive(Serialize, Default)]
struct GroupStats {
    group: String,
    files: usize,
    symbols: usize,
    edges_out: usize,
    edges_internal: usize,
    top_hub: String,
    top_hub_fan_in: usize,
    max_blast_d10_calls: usize,
    max_blast_d10_all: usize,
}

#[derive(Serialize)]
struct Report {
    label: String,
    mode: &'static str,
    root: String,
    files: usize,
    files_by_lang: HashMap<String, usize>,
    parse_errors: usize,
    generated_files_skipped: usize,
    nodes: usize,
    symbols_non_module: usize,
    symbols_by_kind: HashMap<String, usize>,
    edges: usize,
    edges_by_kind: HashMap<String, usize>,
    calls_extracted: usize,
    calls_inferred: usize,
    calls_dropped_ambiguous: usize,
    calls_external_unresolved: usize,
    implements_resolved: usize,
    implements_unresolved: usize,
    fan_in_calls: Dist,
    top20_hubs: Vec<Hub>,
    blast_calls: BlastStats,
    blast_all: BlastStats,
    scc_calls: SccStats,
    scc_all: SccStats,
    groups: Vec<GroupStats>,
}

fn group_of(path: &str, rules: &[(String, usize)]) -> String {
    let segs: Vec<&str> = path.split('/').collect();
    let dirs = &segs[..segs.len().saturating_sub(1)];
    if dirs.is_empty() {
        return "(root)".into();
    }
    for (prefix, depth) in rules {
        if dirs[0] == prefix {
            let d = (*depth).min(dirs.len());
            return dirs[..d].join("/");
        }
    }
    dirs[0].to_string()
}

fn main() -> anyhow::Result<()> {
    let child = std::thread::Builder::new()
        .stack_size(512 * 1024 * 1024)
        .spawn(run)?;
    child.join().unwrap()
}

fn run() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: real-graph <repo-root> <label> [prefix=depth ...]");
        std::process::exit(2);
    }
    let root = PathBuf::from(&args[1]).canonicalize()?;
    let label = args[2].clone();
    let rules: Vec<(String, usize)> = args[3..]
        .iter()
        .filter_map(|a| a.split_once('=').map(|(p, d)| (p.to_string(), d.parse().unwrap_or(1))))
        .collect();

    let t0 = std::time::Instant::now();
    let x = extract::extract_repo(&root, SKIP_DIRS)?;
    eprintln!("[{label}] extracted {} files, {} syms in {:?}", x.files.len(), x.syms.len(), t0.elapsed());
    let ctx = Ctx::new(&x, &root);

    // ---- build edges
    let mut edges: HashMap<(u32, u32, Kind), Prov> = HashMap::new();
    let mut add = |e: &mut HashMap<(u32, u32, Kind), Prov>, s: usize, d: usize, k: Kind, p: Prov| {
        if s == d {
            return;
        }
        let key = (s as u32, d as u32, k);
        let cur = e.get(&key).copied();
        if cur != Some(Prov::Extracted) {
            e.insert(key, p);
        }
    };
    let (mut c_ex, mut c_inf, mut c_amb, mut c_ext) = (0, 0, 0, 0);
    for call in &x.calls {
        match ctx.resolve_call(call) {
            Res::Ex(d) => {
                c_ex += 1;
                add(&mut edges, call.caller, d, Kind::Calls, Prov::Extracted)
            }
            Res::Inf(d) => {
                c_inf += 1;
                add(&mut edges, call.caller, d, Kind::Calls, Prov::Inferred)
            }
            Res::Ambiguous => c_amb += 1,
            Res::External => c_ext += 1,
        }
    }
    let (mut i_ok, mut i_bad) = (0, 0);
    for im in &x.impls {
        let src = match (im.src_sym, &im.src_type) {
            (Some(s), _) => Some(s),
            (None, Some(t)) => match ctx.resolve_type(im.file, t) {
                Res::Ex(s) | Res::Inf(s) => Some(s),
                _ => None,
            },
            _ => None,
        };
        let dst = ctx.resolve_type(im.file, &im.base);
        match (src, dst) {
            (Some(s), Res::Ex(d)) => {
                i_ok += 1;
                add(&mut edges, s, d, Kind::Implements, Prov::Extracted)
            }
            (Some(s), Res::Inf(d)) => {
                i_ok += 1;
                add(&mut edges, s, d, Kind::Implements, Prov::Inferred)
            }
            _ => i_bad += 1,
        }
    }
    for &(c, m) in &x.contains {
        add(&mut edges, c, m, Kind::Contains, Prov::Extracted);
    }
    for (m, t) in &x.contains_by_type {
        let file = x.syms[*m].file;
        let c = match ctx.resolve_type(file, t) {
            Res::Ex(s) | Res::Inf(s) => s,
            _ => x.files[file].module_sym,
        };
        add(&mut edges, c, *m, Kind::Contains, Prov::Extracted);
    }
    for (i, f) in x.files.iter().enumerate() {
        for spec in &f.imports.specs {
            for tf in ctx.resolve_spec(i, spec) {
                add(&mut edges, f.module_sym, x.files[tf].module_sym, Kind::Imports, Prov::Extracted);
            }
        }
    }
    let mut edge_list: Vec<(u32, u32, Kind, Prov)> = edges.into_iter().map(|((s, d, k), p)| (s, d, k, p)).collect();
    edge_list.sort_by_key(|e| (e.0, e.1, e.2 as u8));

    // ---- deterministic IDs + export
    let n = x.syms.len();
    let mut ids: Vec<String> = Vec::with_capacity(n);
    let mut seen: HashMap<String, usize> = HashMap::new();
    for s in &x.syms {
        let f = &x.files[s.file].path;
        let base = match (&s.parent_type, s.kind) {
            (_, "module") => format!("{f}"),
            (Some(p), _) => format!("{f}#{p}.{}:{}", s.name, s.kind),
            (None, _) => format!("{f}#{}:{}", s.name, s.kind),
        };
        let k = seen.entry(base.clone()).or_default();
        *k += 1;
        ids.push(if *k == 1 { base } else { format!("{base}~{k}") });
    }
    let data_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data").join(&label);
    std::fs::create_dir_all(&data_dir)?;
    {
        let mut w = std::io::BufWriter::new(std::fs::File::create(data_dir.join("symbols.jsonl"))?);
        for (i, s) in x.syms.iter().enumerate() {
            let f = &x.files[s.file];
            serde_json::to_writer(
                &mut w,
                &serde_json::json!({
                    "i": i, "id": ids[i], "name": s.name, "kind": s.kind,
                    "parent": s.parent_type, "file": f.path, "line": s.line,
                    "lang": f.lang.fam().name(),
                }),
            )?;
            w.write_all(b"\n")?;
        }
        let mut w = std::io::BufWriter::new(std::fs::File::create(data_dir.join("edges.jsonl"))?);
        for (s, d, k, p) in &edge_list {
            serde_json::to_writer(
                &mut w,
                &serde_json::json!({
                    "src": s, "dst": d, "kind": k.name(),
                    "prov": if *p == Prov::Extracted { "extracted" } else { "inferred" },
                }),
            )?;
            w.write_all(b"\n")?;
        }
    }

    // ---- adjacency
    let mut rev_calls = vec![Vec::new(); n];
    let mut rev_all = vec![Vec::new(); n];
    let mut fwd_calls = vec![Vec::new(); n];
    let mut fwd_all = vec![Vec::new(); n];
    for &(s, d, k, _) in &edge_list {
        rev_all[d as usize].push(s);
        fwd_all[s as usize].push(d);
        if matches!(k, Kind::Calls | Kind::Implements) {
            rev_calls[d as usize].push(s);
            fwd_calls[s as usize].push(d);
        }
    }
    let fan_in: Vec<usize> = rev_calls.iter().map(|v| v.len()).collect();
    let non_module: Vec<usize> = (0..n).filter(|&i| x.syms[i].kind != "module").collect();

    // ---- targets (all if small, else hubs + deterministic sample)
    let mut by_fan: Vec<usize> = non_module.clone();
    by_fan.sort_by(|&a, &b| fan_in[b].cmp(&fan_in[a]).then(a.cmp(&b)));
    const FULL_LIMIT: usize = 40_000;
    let sampled = non_module.len() > FULL_LIMIT;
    let targets: Vec<usize> = if !sampled {
        non_module.clone()
    } else {
        let mut t: Vec<usize> = by_fan.iter().take(1000).copied().collect();
        let mut rng = 0x9E3779B97F4A7C15u64;
        let set: HashSet<usize> = t.iter().copied().collect();
        let mut extra = Vec::new();
        while extra.len() < 10_000 {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            let c = non_module[(rng % non_module.len() as u64) as usize];
            if !set.contains(&c) {
                extra.push(c);
            }
        }
        t.extend(extra);
        t
    };

    let mut depth = vec![u32::MAX; n];
    let mut q = VecDeque::new();
    let mut touched = Vec::new();
    let mut bc: HashMap<usize, (usize, usize, usize)> = HashMap::new();
    let mut ba: HashMap<usize, (usize, usize, usize)> = HashMap::new();
    let t1 = std::time::Instant::now();
    for &t in &targets {
        bc.insert(t, blast(&rev_calls, t, &mut depth, &mut q, &mut touched));
        ba.insert(t, blast(&rev_all, t, &mut depth, &mut q, &mut touched));
    }
    eprintln!("[{label}] blast over {} targets in {:?}", targets.len(), t1.elapsed());

    let pct = |v: usize| 100.0 * v as f64 / n as f64;
    let mk = |set: &'static str, m: &HashMap<usize, (usize, usize, usize)>| -> BlastStats {
        let d3 = dist(targets.iter().map(|t| m[t].0).collect());
        let d10v: Vec<usize> = targets.iter().map(|t| m[t].1).collect();
        let over10 = d10v.iter().filter(|&&v| pct(v) > 10.0).count();
        let over50 = d10v.iter().filter(|&&v| pct(v) > 50.0).count();
        let d10 = dist(d10v);
        let unb = dist(targets.iter().map(|t| m[t].2).collect());
        BlastStats {
            edge_set: set,
            targets_measured: targets.len(),
            sampled,
            max_pct_d10: pct(d10.max),
            d3,
            d10,
            unbounded: unb,
            top5_hub_pct_d10: by_fan.iter().take(5).map(|t| (pct(m[t].1) * 100.0).round() / 100.0).collect(),
            top5_hub_pct_unbounded: by_fan.iter().take(5).map(|t| (pct(m[t].2) * 100.0).round() / 100.0).collect(),
            targets_over_10pct_d10: over10,
            targets_over_50pct_d10: over50,
        }
    };
    let blast_calls = mk("calls+implements (symbol-level dependents)", &bc);
    let blast_all = mk("all kinds incl. contains+imports (synthetic benchmark semantics)", &ba);

    let top20_hubs: Vec<Hub> = by_fan
        .iter()
        .take(20)
        .map(|&i| Hub {
            name: match &x.syms[i].parent_type {
                Some(p) => format!("{p}.{}", x.syms[i].name),
                None => x.syms[i].name.clone(),
            },
            kind: x.syms[i].kind.into(),
            file: x.files[x.syms[i].file].path.clone(),
            line: x.syms[i].line,
            fan_in: fan_in[i],
            blast_d10_calls: bc[&i].1,
            blast_d10_all: ba[&i].1,
        })
        .collect();

    // ---- groups
    let mut groups: HashMap<String, GroupStats> = HashMap::new();
    let file_group: Vec<String> = x.files.iter().map(|f| group_of(&f.path, &rules)).collect();
    for g in &file_group {
        groups.entry(g.clone()).or_insert_with(|| GroupStats { group: g.clone(), ..Default::default() }).files += 1;
    }
    for &i in &non_module {
        let g = groups.get_mut(&file_group[x.syms[i].file]).unwrap();
        g.symbols += 1;
        if fan_in[i] > g.top_hub_fan_in {
            g.top_hub_fan_in = fan_in[i];
            g.top_hub = format!("{} ({})", x.syms[i].name, x.files[x.syms[i].file].path);
        }
        if let Some(v) = bc.get(&i) {
            g.max_blast_d10_calls = g.max_blast_d10_calls.max(v.1);
        }
        if let Some(v) = ba.get(&i) {
            g.max_blast_d10_all = g.max_blast_d10_all.max(v.1);
        }
    }
    for &(s, d, _, _) in &edge_list {
        let gs = &file_group[x.syms[s as usize].file];
        let gd = &file_group[x.syms[d as usize].file];
        let g = groups.get_mut(gs).unwrap();
        g.edges_out += 1;
        if gs == gd {
            g.edges_internal += 1;
        }
    }
    let mut groups: Vec<GroupStats> = groups.into_values().collect();
    groups.sort_by(|a, b| b.symbols.cmp(&a.symbols));

    let mut files_by_lang = HashMap::new();
    for f in &x.files {
        *files_by_lang.entry(f.lang.fam().name().to_string()).or_insert(0) += 1;
    }
    let mut symbols_by_kind = HashMap::new();
    for s in &x.syms {
        *symbols_by_kind.entry(s.kind.to_string()).or_insert(0) += 1;
    }
    let mut edges_by_kind = HashMap::new();
    for e in &edge_list {
        let key = format!("{}:{}", e.2.name(), if e.3 == Prov::Extracted { "extracted" } else { "inferred" });
        *edges_by_kind.entry(key).or_insert(0) += 1;
    }

    let report = Report {
        mode: if permissive() { "permissive" } else { "strict" },
        label: label.clone(),
        root: root.display().to_string(),
        files: x.files.len(),
        files_by_lang,
        parse_errors: x.parse_errors,
        generated_files_skipped: x.generated_skipped,
        nodes: n,
        symbols_non_module: non_module.len(),
        symbols_by_kind,
        edges: edge_list.len(),
        edges_by_kind,
        calls_extracted: c_ex,
        calls_inferred: c_inf,
        calls_dropped_ambiguous: c_amb,
        calls_external_unresolved: c_ext,
        implements_resolved: i_ok,
        implements_unresolved: i_bad,
        fan_in_calls: dist(non_module.iter().map(|&i| fan_in[i]).collect()),
        top20_hubs,
        blast_calls,
        blast_all,
        scc_calls: scc_stats(&fwd_calls),
        scc_all: scc_stats(&fwd_all),
        groups,
    };
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("results").join(format!("{label}.json"));
    std::fs::write(&out, serde_json::to_string_pretty(&report)?)?;
    eprintln!("[{label}] wrote {} in {:?}", out.display(), t0.elapsed());
    Ok(())
}
