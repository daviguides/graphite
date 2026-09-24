//! Python extractor: one file in, per-file facts out.
//!
//! Resolution stops at the file boundary so that incremental re-extraction of a
//! file always equals a full rebuild. Anything bound outside the file becomes a
//! [`Target::Unresolved`] carrying enough (`name`, `qualifier`, `import_path`)
//! for the store to resolve it later.
//!
//! Target conventions for unresolved references:
//! - `import_path` starting with [`EXTERNAL_PREFIX`]: stdlib or builtin, never in the repo.
//! - `name == MODULE_TARGET`: the reference is to a whole module (`import a.b`).
//! - `name == WILDCARD_TARGET`: `from x import *`.
//! - `name == DYNAMIC_TARGET`: callee is not a name (`f()()`, `x[0]()`); `qualifier` holds its text.
//! - `qualifier`: the receiver text as written (`self`, `obj`, `mod.sub`, `super()`).
//!
//! Provenance: `Extracted` for syntactic bindings (names bound in this file, and every
//! unresolved reference), `Resolved` when an attribute was matched to a class member
//! (`self.m()`, `Class.m()`), which assumes the receiver is what it looks like.

mod known;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use graphite_model::{
    EdgeKind, FileFacts, Lang, Provenance, RawEdge, Symbol, SymbolId, SymbolKind, Target,
};
use streaming_iterator::StreamingIterator;
use tree_sitter::{Node, Parser, Point, Query, QueryCursor};

pub const EXTERNAL_PREFIX: &str = "<external>:";
pub const MODULE_TARGET: &str = "<module>";
pub const WILDCARD_TARGET: &str = "*";
pub const DYNAMIC_TARGET: &str = "<dynamic>";

const QUERY_SOURCE: &str = include_str!("../queries/python.scm");
const QUALIFIER_MAX: usize = 80;

struct Captures {
    def_class: u32,
    def_function: u32,
    name: u32,
    bases: u32,
    call: u32,
    callee: u32,
    decorator: u32,
    import: u32,
    import_module: u32,
    import_alias: u32,
    import_from: u32,
    import_name: u32,
    import_wildcard: u32,
    export_list: u32,
}

struct Compiled {
    query: Query,
    cap: Captures,
}

fn compiled() -> &'static Compiled {
    static COMPILED: OnceLock<Compiled> = OnceLock::new();
    COMPILED.get_or_init(|| {
        let language = tree_sitter_python::LANGUAGE.into();
        let query = Query::new(&language, QUERY_SOURCE).expect("python.scm must compile");
        let idx = |n: &str| {
            query
                .capture_index_for_name(n)
                .unwrap_or_else(|| panic!("capture @{n} missing from python.scm"))
        };
        let cap = Captures {
            def_class: idx("definition.class"),
            def_function: idx("definition.function"),
            name: idx("name"),
            bases: idx("definition.bases"),
            call: idx("reference.call"),
            callee: idx("reference.callee"),
            decorator: idx("reference.decorator"),
            import: idx("import"),
            import_module: idx("import.module"),
            import_alias: idx("import.alias"),
            import_from: idx("import.from"),
            import_name: idx("import.name"),
            import_wildcard: idx("import.wildcard"),
            export_list: idx("export.list"),
        };
        Compiled { query, cap }
    })
}

thread_local! {
    static PARSER: RefCell<Parser> = RefCell::new({
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("tree-sitter-python grammar version must match tree-sitter");
        parser
    });
}

/// Extract the facts of one Python file. `path` is repo-relative with `/` separators.
pub fn extract(path: &str, source: &[u8]) -> FileFacts {
    let content_hash = *blake3::hash(source).as_bytes();
    let module = ModulePath::from_path(path);
    let file_is_test = is_test_path(path);

    let tree = PARSER.with(|p| p.borrow_mut().parse(source, None));
    let Some(tree) = tree else {
        let module_sym = module_symbol(
            path,
            &module,
            source.len(),
            line_count(source),
            file_is_test,
        );
        return FileFacts {
            path: path.to_string(),
            lang: Lang::Python,
            content_hash,
            symbols: vec![module_sym],
            edges: Vec::new(),
            parse_ok: false,
        };
    };
    let root = tree.root_node();

    let sites = collect_sites(root, source);
    let mut ctx = Ctx::new(path, source, &module, file_is_test, root);
    ctx.build_definitions(&sites.defs, sites.exports.as_ref());
    ctx.build_imports(&sites.imports);
    ctx.emit_contains();
    ctx.emit_inherits(&sites.defs);
    ctx.emit_decorators(&sites.decorators);
    ctx.emit_calls(&sites.calls);

    let mut symbols = ctx.symbols;
    symbols.sort_by(|a, b| {
        (a.start_byte, a.kind, &a.qualified).cmp(&(b.start_byte, b.kind, &b.qualified))
    });
    let mut edges = ctx.edges;
    edges.sort_by_cached_key(edge_sort_key);
    edges.dedup();

    FileFacts {
        path: path.to_string(),
        lang: Lang::Python,
        content_hash,
        symbols,
        edges,
        parse_ok: !root.has_error(),
    }
}

// ---------------------------------------------------------------- query pass

struct DefSite<'t> {
    node: Node<'t>,
    name: Node<'t>,
    is_class: bool,
    bases: Option<Node<'t>>,
}

#[derive(Default)]
struct ImportSite<'t> {
    stmt: Option<Node<'t>>,
    module: Option<Node<'t>>,
    alias: Option<Node<'t>>,
    from: Option<Node<'t>>,
    name: Option<Node<'t>>,
    wildcard: bool,
}

struct Sites<'t> {
    defs: Vec<DefSite<'t>>,
    calls: Vec<(Node<'t>, Node<'t>)>,
    decorators: Vec<Node<'t>>,
    imports: Vec<ImportSite<'t>>,
    exports: Option<HashSet<String>>,
}

fn collect_sites<'t>(root: Node<'t>, source: &[u8]) -> Sites<'t> {
    let Compiled { query, cap } = compiled();
    let mut sites = Sites {
        defs: Vec::new(),
        calls: Vec::new(),
        decorators: Vec::new(),
        imports: Vec::new(),
        exports: None,
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, root, source);
    while let Some(m) = matches.next() {
        let get = |idx: u32| m.captures.iter().find(|c| c.index == idx).map(|c| c.node);
        if let Some(node) = get(cap.def_class).or_else(|| get(cap.def_function)) {
            if let Some(name) = get(cap.name) {
                sites.defs.push(DefSite {
                    node,
                    name,
                    is_class: node.kind() == "class_definition",
                    bases: get(cap.bases),
                });
            }
        } else if let Some(call) = get(cap.call) {
            if let Some(callee) = get(cap.callee) {
                sites.calls.push((call, callee));
            }
        } else if let Some(expr) = get(cap.decorator) {
            sites.decorators.push(expr);
        } else if let Some(stmt) = get(cap.import) {
            sites.imports.push(ImportSite {
                stmt: Some(stmt),
                module: get(cap.import_module),
                alias: get(cap.import_alias),
                from: get(cap.import_from),
                name: get(cap.import_name),
                wildcard: get(cap.import_wildcard).is_some(),
            });
        } else if let Some(list) = get(cap.export_list) {
            sites.exports = Some(string_items(list, source));
        }
    }
    sites.defs.sort_by_key(|d| d.node.start_byte());
    sites
}

fn string_items(list: Node, source: &[u8]) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut walker = list.walk();
    for item in list.named_children(&mut walker) {
        if item.kind() != "string" {
            continue;
        }
        let mut w = item.walk();
        let content: String = item
            .named_children(&mut w)
            .filter(|c| c.kind() == "string_content")
            .map(|c| text(c, source))
            .collect();
        if !content.is_empty() {
            out.insert(content);
        }
    }
    out
}

// ---------------------------------------------------------------- resolution

struct ModulePath {
    qualified: String,
    name: String,
    package: Vec<String>,
}

impl ModulePath {
    fn from_path(path: &str) -> Self {
        let p = path.trim_start_matches("./");
        let stem = p
            .strip_suffix(".pyi")
            .or_else(|| p.strip_suffix(".py"))
            .unwrap_or(p);
        let mut segs: Vec<String> = stem
            .split('/')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        let is_init = segs.last().is_some_and(|s| s == "__init__");
        if is_init && segs.len() > 1 {
            segs.pop();
        }
        let package = if is_init {
            segs.clone()
        } else {
            segs[..segs.len().saturating_sub(1)].to_vec()
        };
        let name = segs.last().cloned().unwrap_or_default();
        ModulePath {
            qualified: segs.join("."),
            name,
            package,
        }
    }

    /// Absolute dotted path for a `from X import` source, relative imports anchored at this module.
    fn resolve_from(&self, from_text: &str) -> (String, bool) {
        let dots = from_text.chars().take_while(|c| *c == '.').count();
        if dots == 0 {
            return (from_text.to_string(), false);
        }
        let rest = &from_text[dots..];
        let up = dots - 1;
        let keep = self.package.len().saturating_sub(up);
        let mut parts: Vec<&str> = self.package[..keep].iter().map(String::as_str).collect();
        if !rest.is_empty() {
            parts.push(rest);
        }
        (parts.join("."), true)
    }
}

#[derive(Clone)]
struct Binding {
    path: String,
    is_module: bool,
    external: bool,
}

impl Binding {
    fn import_path(&self, suffix: &[&str]) -> String {
        let mut path = self.path.clone();
        for s in suffix {
            path.push('.');
            path.push_str(s);
        }
        if self.external {
            format!("{EXTERNAL_PREFIX}{path}")
        } else {
            path
        }
    }
}

struct Def {
    sym: SymbolId,
    kind: SymbolKind,
    start: usize,
    end: usize,
    parent: Option<usize>,
    members: HashMap<String, usize>,
    imports: HashMap<String, Binding>,
}

enum Lookup<'a> {
    Local(usize),
    Import(&'a Binding),
    Builtin,
    Unknown,
}

struct Ctx<'s> {
    path: &'s str,
    source: &'s [u8],
    module: &'s ModulePath,
    file_is_test: bool,
    module_sym: SymbolId,
    module_members: HashMap<String, usize>,
    module_imports: HashMap<String, Binding>,
    defs: Vec<Def>,
    def_by_start: HashMap<usize, usize>,
    symbols: Vec<Symbol>,
    edges: Vec<RawEdge>,
}

impl<'s> Ctx<'s> {
    fn new(
        path: &'s str,
        source: &'s [u8],
        module: &'s ModulePath,
        file_is_test: bool,
        root: Node,
    ) -> Self {
        let module_symbol = module_symbol(path, module, source.len(), end_line(root), file_is_test);
        Ctx {
            path,
            source,
            module,
            file_is_test,
            module_sym: module_symbol.id,
            module_members: HashMap::new(),
            module_imports: HashMap::new(),
            defs: Vec::new(),
            def_by_start: HashMap::new(),
            symbols: vec![module_symbol],
            edges: Vec::new(),
        }
    }

    fn build_definitions(&mut self, sites: &[DefSite], exports: Option<&HashSet<String>>) {
        let mut stack: Vec<usize> = Vec::new();
        let mut overloads: HashMap<(String, SymbolKind), u32> = HashMap::new();
        let mut info: Vec<(bool, bool)> = Vec::with_capacity(sites.len()); // (exported, is_test)
        for site in sites {
            let start = site.node.start_byte();
            while stack.last().is_some_and(|&top| self.defs[top].end <= start) {
                stack.pop();
            }
            let parent = stack.last().copied();
            let name = text(site.name, self.source);
            let kind = if site.is_class {
                SymbolKind::Class
            } else if parent.is_some_and(|p| self.defs[p].kind == SymbolKind::Class) {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };
            let parent_qualified = match parent {
                Some(p) => &self.symbols[p + 1].qualified,
                None => &self.module.qualified,
            };
            let qualified = if parent_qualified.is_empty() {
                name.clone()
            } else {
                format!("{parent_qualified}.{name}")
            };
            let slot = overloads.entry((qualified.clone(), kind)).or_insert(0);
            let overload = *slot;
            *slot += 1;
            let id = SymbolId::new(Lang::Python, self.path, &qualified, kind, overload);

            let (parent_exported, parent_test) = match parent {
                Some(p) => info[p],
                None => (true, self.file_is_test),
            };
            let exported = match (parent, exports) {
                (None, Some(all)) => all.contains(&name),
                _ => parent_exported && is_public(&name),
            };
            let is_test = parent_test
                || match kind {
                    SymbolKind::Class => name.starts_with("Test"),
                    _ => name.starts_with("test"),
                };
            info.push((exported, is_test));

            let outer = decorated_outer(site.node);
            let index = self.defs.len();
            self.defs.push(Def {
                sym: id,
                kind,
                start,
                end: site.node.end_byte(),
                parent,
                members: HashMap::new(),
                imports: HashMap::new(),
            });
            self.def_by_start.insert(start, index);
            match parent {
                Some(p) => self.defs[p].members.insert(name.clone(), index),
                None => self.module_members.insert(name.clone(), index),
            };
            self.symbols.push(Symbol {
                id,
                lang: Lang::Python,
                path: self.path.to_string(),
                name,
                qualified,
                kind,
                start_line: outer.start_position().row as u32 + 1,
                end_line: end_line(outer),
                start_byte: outer.start_byte() as u32,
                end_byte: outer.end_byte() as u32,
                exported,
                signature: signature(site.node, self.source),
                parent: Some(parent.map_or(self.module_sym, |p| self.defs[p].sym)),
                is_test,
            });
            stack.push(index);
        }
    }

    fn build_imports(&mut self, sites: &[ImportSite]) {
        for site in sites {
            let Some(stmt) = site.stmt else { continue };
            let scope = self.enclosing(stmt.start_byte());
            let src = self.scope_sym(scope);
            let line = stmt.start_position().row as u32 + 1;

            let (binding_name, binding, target_name, import_path) = if let Some(from) = site.from {
                let (base, relative) = self.module.resolve_from(&text(from, self.source));
                let external = !relative && is_external_path(&base);
                if site.wildcard {
                    let b = Binding {
                        path: base,
                        is_module: true,
                        external,
                    };
                    self.push_edge(
                        src,
                        unresolved(WILDCARD_TARGET, None, Some(b.import_path(&[]))),
                        EdgeKind::Imports,
                        line,
                        Provenance::Extracted,
                    );
                    continue;
                }
                let Some(name) = site.name else { continue };
                let imported = text(name, self.source);
                let path = if base.is_empty() {
                    imported.clone()
                } else {
                    format!("{base}.{imported}")
                };
                let last = imported.rsplit('.').next().unwrap_or(&imported).to_string();
                let local = site
                    .alias
                    .map_or_else(|| last.clone(), |a| text(a, self.source));
                let b = Binding {
                    path,
                    is_module: false,
                    external,
                };
                let ip = b.import_path(&[]);
                (local, b, last, ip)
            } else {
                let Some(module) = site.module else { continue };
                let dotted = text(module, self.source);
                let external = is_external_path(&dotted);
                let target = Binding {
                    path: dotted.clone(),
                    is_module: true,
                    external,
                };
                let ip = target.import_path(&[]);
                match site.alias {
                    Some(alias) => (
                        text(alias, self.source),
                        target,
                        MODULE_TARGET.to_string(),
                        ip,
                    ),
                    None => {
                        // `import a.b.c` binds only `a`.
                        let head = dotted.split('.').next().unwrap_or(&dotted).to_string();
                        let b = Binding {
                            path: head.clone(),
                            is_module: true,
                            external,
                        };
                        (head, b, MODULE_TARGET.to_string(), ip)
                    }
                }
            };
            self.push_edge(
                src,
                unresolved(&target_name, None, Some(import_path)),
                EdgeKind::Imports,
                line,
                Provenance::Extracted,
            );
            match scope {
                Some(d) => self.defs[d].imports.insert(binding_name, binding),
                None => self.module_imports.insert(binding_name, binding),
            };
        }
    }

    fn emit_contains(&mut self) {
        for i in 0..self.defs.len() {
            let src = self.scope_sym(self.defs[i].parent);
            let line = self.symbols[i + 1].start_line;
            self.push_edge(
                src,
                Target::Symbol(self.defs[i].sym),
                EdgeKind::Contains,
                line,
                Provenance::Extracted,
            );
        }
    }

    fn emit_inherits(&mut self, sites: &[DefSite]) {
        for site in sites.iter().filter(|s| s.is_class) {
            let Some(bases) = site.bases else { continue };
            let Some(&class) = self.def_by_start.get(&site.node.start_byte()) else {
                continue;
            };
            let src = self.defs[class].sym;
            // Base expressions are evaluated in the scope enclosing the class.
            let scope = self.defs[class].parent;
            let mut walker = bases.walk();
            for base in bases.named_children(&mut walker) {
                let expr = match base.kind() {
                    "keyword_argument" | "comment" | "list_splat" | "dictionary_splat" => continue,
                    "subscript" => base.child_by_field_name("value").unwrap_or(base),
                    _ => base,
                };
                let (dst, prov) = self.resolve_expr(expr, scope);
                let line = base.start_position().row as u32 + 1;
                self.push_edge(src, dst, EdgeKind::Inherits, line, prov);
            }
        }
    }

    fn emit_decorators(&mut self, decorators: &[Node]) {
        for &expr in decorators {
            let Some(decorated) = expr.parent().and_then(|d| d.parent()) else {
                continue;
            };
            let Some(def) = decorated.child_by_field_name("definition") else {
                continue;
            };
            let Some(&index) = self.def_by_start.get(&def.start_byte()) else {
                continue;
            };
            let target = if expr.kind() == "call" {
                expr.child_by_field_name("function").unwrap_or(expr)
            } else {
                expr
            };
            let scope = self.defs[index].parent;
            let (dst, prov) = self.resolve_expr(target, scope);
            let src = self.defs[index].sym;
            let line = expr.start_position().row as u32 + 1;
            self.push_edge(src, dst, EdgeKind::References, line, prov);
        }
    }

    fn emit_calls(&mut self, calls: &[(Node, Node)]) {
        for &(call, callee) in calls {
            if call.parent().is_some_and(|p| p.kind() == "decorator") {
                continue;
            }
            let scope = self.enclosing(call.start_byte());
            let (dst, prov) = self.resolve_expr(callee, scope);
            let src = self.scope_sym(scope);
            let line = call.start_position().row as u32 + 1;
            self.push_edge(src, dst, EdgeKind::Calls, line, prov);
        }
    }

    fn resolve_expr(&self, node: Node, scope: Option<usize>) -> (Target, Provenance) {
        match node.kind() {
            "identifier" => {
                let name = text(node, self.source);
                let target = match self.lookup(&name, scope) {
                    Lookup::Local(i) => {
                        return (Target::Symbol(self.defs[i].sym), Provenance::Extracted)
                    }
                    Lookup::Import(b) => {
                        let tail = if b.is_module {
                            MODULE_TARGET
                        } else {
                            last_segment(&b.path)
                        };
                        unresolved(tail, None, Some(b.import_path(&[])))
                    }
                    Lookup::Builtin => unresolved(
                        &name,
                        None,
                        Some(format!("{EXTERNAL_PREFIX}builtins.{name}")),
                    ),
                    Lookup::Unknown => unresolved(&name, None, None),
                };
                (target, Provenance::Extracted)
            }
            "attribute" => {
                let attr = node
                    .child_by_field_name("attribute")
                    .map(|a| text(a, self.source))
                    .unwrap_or_default();
                let Some(object) = node.child_by_field_name("object") else {
                    return (unresolved(&attr, None, None), Provenance::Extracted);
                };
                let receiver = truncate(&text(object, self.source));
                let Some(segs) = dotted(object, self.source) else {
                    return (
                        unresolved(&attr, Some(receiver), None),
                        Provenance::Extracted,
                    );
                };
                let head = segs[0].as_str();
                if segs.len() == 1 && (head == "self" || head == "cls") {
                    if let Some(member) = self
                        .enclosing_class(scope)
                        .and_then(|c| self.defs[c].members.get(&attr))
                    {
                        return (Target::Symbol(self.defs[*member].sym), Provenance::Resolved);
                    }
                    return (
                        unresolved(&attr, Some(receiver), None),
                        Provenance::Extracted,
                    );
                }
                match self.lookup(head, scope) {
                    Lookup::Local(i)
                        if segs.len() == 1 && self.defs[i].kind == SymbolKind::Class =>
                    {
                        match self.defs[i].members.get(&attr) {
                            Some(&m) => (Target::Symbol(self.defs[m].sym), Provenance::Resolved),
                            None => (
                                unresolved(&attr, Some(receiver), None),
                                Provenance::Extracted,
                            ),
                        }
                    }
                    Lookup::Import(b) => {
                        let mut suffix: Vec<&str> = segs[1..].iter().map(String::as_str).collect();
                        suffix.push(&attr);
                        (
                            unresolved(&attr, Some(receiver), Some(b.import_path(&suffix))),
                            Provenance::Extracted,
                        )
                    }
                    _ => (
                        unresolved(&attr, Some(receiver), None),
                        Provenance::Extracted,
                    ),
                }
            }
            _ => (
                unresolved(
                    DYNAMIC_TARGET,
                    Some(truncate(&text(node, self.source))),
                    None,
                ),
                Provenance::Extracted,
            ),
        }
    }

    /// LEGB lookup. Class scopes are visible only to code directly in the class body.
    fn lookup(&self, name: &str, scope: Option<usize>) -> Lookup<'_> {
        let mut current = scope;
        let mut innermost = true;
        while let Some(d) = current {
            let def = &self.defs[d];
            if innermost || def.kind != SymbolKind::Class {
                if let Some(&m) = def.members.get(name) {
                    return Lookup::Local(m);
                }
                if let Some(b) = def.imports.get(name) {
                    return Lookup::Import(b);
                }
            }
            innermost = false;
            current = def.parent;
        }
        if let Some(&m) = self.module_members.get(name) {
            return Lookup::Local(m);
        }
        if let Some(b) = self.module_imports.get(name) {
            return Lookup::Import(b);
        }
        if known::is_builtin(name) {
            return Lookup::Builtin;
        }
        Lookup::Unknown
    }

    fn enclosing_class(&self, scope: Option<usize>) -> Option<usize> {
        let mut current = scope;
        while let Some(d) = current {
            if self.defs[d].kind == SymbolKind::Method {
                return self.defs[d].parent;
            }
            current = self.defs[d].parent;
        }
        None
    }

    /// Innermost definition whose body contains `pos`.
    fn enclosing(&self, pos: usize) -> Option<usize> {
        let idx = self.defs.partition_point(|d| d.start <= pos);
        let mut current = idx.checked_sub(1);
        while let Some(d) = current {
            if pos < self.defs[d].end {
                return Some(d);
            }
            current = self.defs[d].parent;
        }
        None
    }

    fn scope_sym(&self, scope: Option<usize>) -> SymbolId {
        scope.map_or(self.module_sym, |d| self.defs[d].sym)
    }

    fn push_edge(
        &mut self,
        src: SymbolId,
        dst: Target,
        kind: EdgeKind,
        site_line: u32,
        provenance: Provenance,
    ) {
        self.edges.push(RawEdge {
            src,
            dst,
            kind,
            site_line,
            provenance,
        });
    }
}

// ---------------------------------------------------------------- helpers

fn module_symbol(
    path: &str,
    module: &ModulePath,
    len: usize,
    end_line: u32,
    is_test: bool,
) -> Symbol {
    let qualified = if module.qualified.is_empty() {
        path.to_string()
    } else {
        module.qualified.clone()
    };
    Symbol {
        id: SymbolId::new(Lang::Python, path, &qualified, SymbolKind::Module, 0),
        lang: Lang::Python,
        path: path.to_string(),
        name: if module.name.is_empty() {
            qualified.clone()
        } else {
            module.name.clone()
        },
        qualified,
        kind: SymbolKind::Module,
        start_line: 1,
        end_line,
        start_byte: 0,
        end_byte: len as u32,
        exported: true,
        signature: String::new(),
        parent: None,
        is_test,
    }
}

fn unresolved(name: &str, qualifier: Option<String>, import_path: Option<String>) -> Target {
    Target::Unresolved {
        name: name.to_string(),
        qualifier,
        import_path,
    }
}

fn text(node: Node, source: &[u8]) -> String {
    String::from_utf8_lossy(&source[node.byte_range()]).into_owned()
}

fn truncate(s: &str) -> String {
    let collapsed = collapse_ws(s);
    match collapsed.char_indices().nth(QUALIFIER_MAX) {
        Some((i, _)) => format!("{}…", &collapsed[..i]),
        None => collapsed,
    }
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn last_segment(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or(path)
}

fn dotted(node: Node, source: &[u8]) -> Option<Vec<String>> {
    match node.kind() {
        "identifier" => Some(vec![text(node, source)]),
        "attribute" => {
            let mut segs = dotted(node.child_by_field_name("object")?, source)?;
            segs.push(text(node.child_by_field_name("attribute")?, source));
            Some(segs)
        }
        _ => None,
    }
}

fn decorated_outer(def: Node) -> Node {
    match def.parent() {
        Some(p) if p.kind() == "decorated_definition" => p,
        _ => def,
    }
}

fn signature(def: Node, source: &[u8]) -> String {
    let end = def
        .child_by_field_name("body")
        .map_or(def.end_byte(), |b| b.start_byte());
    let head = String::from_utf8_lossy(&source[def.start_byte()..end]);
    collapse_ws(&head)
        .trim_end_matches(':')
        .trim_end()
        .to_string()
}

fn end_line(node: Node) -> u32 {
    point_end_line(node.start_position(), node.end_position())
}

fn point_end_line(start: Point, end: Point) -> u32 {
    if end.column == 0 && end.row > start.row {
        end.row as u32
    } else {
        end.row as u32 + 1
    }
}

fn line_count(source: &[u8]) -> u32 {
    let newlines = source.iter().filter(|&&b| b == b'\n').count() as u32;
    if source.last().is_some_and(|&b| b != b'\n') {
        newlines + 1
    } else {
        newlines.max(1)
    }
}

fn is_public(name: &str) -> bool {
    !name.starts_with('_') || (name.starts_with("__") && name.ends_with("__"))
}

fn is_external_path(dotted: &str) -> bool {
    dotted.split('.').next().is_some_and(known::is_stdlib_top)
}

/// Test files by pytest/unittest convention: `test_*.py`, `*_test.py`, `conftest.py`, or under `tests/`/`test/`.
pub fn is_test_path(path: &str) -> bool {
    let mut parts = path.split('/').filter(|s| !s.is_empty()).peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            let stem = part
                .strip_suffix(".py")
                .or_else(|| part.strip_suffix(".pyi"))
                .unwrap_or(part);
            return stem.starts_with("test_") || stem.ends_with("_test") || stem == "conftest";
        }
        if part == "tests" || part == "test" {
            return true;
        }
    }
    false
}

fn edge_sort_key(
    e: &RawEdge,
) -> (
    SymbolId,
    EdgeKind,
    u32,
    u8,
    String,
    String,
    String,
    Provenance,
) {
    let (tag, a, b, c) = match &e.dst {
        Target::Symbol(id) => (0, id.to_hex(), String::new(), String::new()),
        Target::Unresolved {
            name,
            qualifier,
            import_path,
        } => (
            1,
            name.clone(),
            qualifier.clone().unwrap_or_default(),
            import_path.clone().unwrap_or_default(),
        ),
    };
    (e.src, e.kind, e.site_line, tag, a, b, c, e.provenance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_paths() {
        let m = ModulePath::from_path("pkg/sub/mod.py");
        assert_eq!(m.qualified, "pkg.sub.mod");
        assert_eq!(m.package, vec!["pkg", "sub"]);
        let init = ModulePath::from_path("pkg/sub/__init__.py");
        assert_eq!(init.qualified, "pkg.sub");
        assert_eq!(init.package, vec!["pkg", "sub"]);
    }

    #[test]
    fn relative_imports_anchor_at_package() {
        let m = ModulePath::from_path("pkg/sub/mod.py");
        assert_eq!(m.resolve_from("."), ("pkg.sub".into(), true));
        assert_eq!(m.resolve_from(".x"), ("pkg.sub.x".into(), true));
        assert_eq!(m.resolve_from("..y.z"), ("pkg.y.z".into(), true));
        assert_eq!(m.resolve_from("os.path"), ("os.path".into(), false));
        let init = ModulePath::from_path("pkg/sub/__init__.py");
        assert_eq!(init.resolve_from(".x"), ("pkg.sub.x".into(), true));
    }

    #[test]
    fn test_paths() {
        assert!(is_test_path("tests/unit/foo.py"));
        assert!(is_test_path("pkg/test_foo.py"));
        assert!(is_test_path("pkg/foo_test.py"));
        assert!(is_test_path("conftest.py"));
        assert!(!is_test_path("pkg/testing.py"));
        assert!(!is_test_path("pkg/contest.py"));
    }
}
