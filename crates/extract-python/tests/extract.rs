use std::collections::HashMap;
use std::path::Path;

use graphite_extract_python::{extract, EXTERNAL_PREFIX, MODULE_TARGET, WILDCARD_TARGET};
use graphite_model::{EdgeKind, FileFacts, Provenance, SymbolKind, Target};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn load(rel: &str) -> FileFacts {
    let source = std::fs::read(Path::new(FIXTURES).join(rel)).expect("fixture");
    extract(rel, &source)
}

/// Readable projection used for assertions and the golden file.
fn render(facts: &FileFacts) -> Vec<String> {
    let names: HashMap<_, _> = facts
        .symbols
        .iter()
        .map(|s| (s.id, s.qualified.as_str()))
        .collect();
    let mut out = vec![format!("parse_ok={}", facts.parse_ok)];
    for s in &facts.symbols {
        let parent = s.parent.map_or("-", |p| names[&p]);
        out.push(format!(
            "SYM {} {} L{}-{} parent={} exported={} test={} sig={}",
            s.kind.as_str(),
            s.qualified,
            s.start_line,
            s.end_line,
            parent,
            s.exported,
            s.is_test,
            s.signature
        ));
    }
    for e in &facts.edges {
        let dst = match &e.dst {
            Target::Symbol(id) => names[id].to_string(),
            Target::Unresolved {
                name,
                qualifier,
                import_path,
            } => format!(
                "?{name} q={} ip={}",
                qualifier.as_deref().unwrap_or("-"),
                import_path.as_deref().unwrap_or("-")
            ),
        };
        out.push(format!(
            "EDGE {} {} -> {} L{} {}",
            e.kind.as_str(),
            names[&e.src],
            dst,
            e.site_line,
            e.provenance.as_str()
        ));
    }
    out
}

fn has(lines: &[String], needle: &str) -> bool {
    lines.iter().any(|l| l == needle)
}

fn assert_has(lines: &[String], needle: &str) {
    assert!(
        has(lines, needle),
        "missing line:\n  {needle}\nin:\n{}",
        lines.join("\n")
    );
}

#[test]
fn definitions_have_qualified_names_kinds_and_parents() {
    let r = render(&load("pkg/sub/service.py"));
    assert_has(
        &r,
        "SYM module pkg.sub.service L1-64 parent=- exported=true test=false sig=",
    );
    assert_has(&r, "SYM function pkg.sub.service.build L14-19 parent=pkg.sub.service exported=true test=false sig=def build(name: str) -> \"Service\"");
    assert_has(&r, "SYM method pkg.sub.service.Service.start L39-43 parent=pkg.sub.service.Service exported=true test=false sig=async def start(self)");
    assert_has(&r, "SYM function pkg.sub.service.Service._boot.inner L46-47 parent=pkg.sub.service.Service._boot exported=false test=false sig=def inner()");
    assert_has(&r, "SYM class pkg.sub.service.Nested.Config L58-59 parent=pkg.sub.service.Nested exported=false test=false sig=class Config");
}

#[test]
fn decorated_definitions_span_their_decorators() {
    let r = render(&load("pkg/sub/service.py"));
    assert_has(&r, "SYM method pkg.sub.service.Service.label L31-33 parent=pkg.sub.service.Service exported=true test=false sig=def label(self) -> str");
    assert_has(&r, "SYM function pkg.sub.service.handler L62-64 parent=pkg.sub.service exported=false test=false sig=def handler()");
}

#[test]
fn redefinitions_get_distinct_ids() {
    let facts = load("pkg/sub/service.py");
    let labels: Vec<_> = facts
        .symbols
        .iter()
        .filter(|s| s.qualified == "pkg.sub.service.Service.label")
        .collect();
    assert_eq!(labels.len(), 2);
    assert_ne!(labels[0].id, labels[1].id);
}

#[test]
fn dunder_all_controls_top_level_export() {
    let facts = load("pkg/sub/service.py");
    let exported = |q: &str| {
        facts
            .symbols
            .iter()
            .find(|s| s.qualified == q)
            .unwrap()
            .exported
    };
    assert!(exported("pkg.sub.service.build"));
    assert!(exported("pkg.sub.service.Service"));
    assert!(!exported("pkg.sub.service.Nested"), "not in __all__");
    assert!(!exported("pkg.sub.service._private"));
    assert!(
        exported("pkg.sub.service.Service.__init__"),
        "dunder stays public"
    );
}

#[test]
fn imports_keep_absolute_paths_and_flag_externals() {
    let r = render(&load("pkg/sub/service.py"));
    assert_has(&r, &format!("EDGE imports pkg.sub.service -> ?{MODULE_TARGET} q=- ip={EXTERNAL_PREFIX}os.path L3 extracted"));
    assert_has(&r, &format!("EDGE imports pkg.sub.service -> ?{MODULE_TARGET} q=- ip={EXTERNAL_PREFIX}json L4 extracted"));
    assert_has(
        &r,
        "EDGE imports pkg.sub.service -> ?helpers q=- ip=pkg.sub.helpers L6 extracted",
    );
    assert_has(
        &r,
        "EDGE imports pkg.sub.service -> ?User q=- ip=pkg.sub.models.User L7 extracted",
    );
    assert_has(&r, "EDGE imports pkg.sub.service -> ?BaseService q=- ip=pkg.core.base.BaseService L8 extracted");
    assert_has(
        &r,
        &format!(
            "EDGE imports pkg.sub.service -> ?{WILDCARD_TARGET} q=- ip=pkg.sub.star L9 extracted"
        ),
    );
}

#[test]
fn package_init_anchors_relative_imports_at_itself() {
    let r = render(&load("pkg/__init__.py"));
    assert_has(
        &r,
        "EDGE imports pkg -> ?service q=- ip=pkg.sub.service L1 extracted",
    );
    assert_has(
        &r,
        "EDGE calls pkg.init -> ?build q=subpkg.service ip=pkg.sub.service.build L7 extracted",
    );
}

#[test]
fn calls_resolve_in_file_and_leave_the_rest_unresolved() {
    let r = render(&load("pkg/sub/service.py"));
    // constructor call on a same-file class
    assert_has(
        &r,
        "EDGE calls pkg.sub.service.build -> pkg.sub.service.Service L15 extracted",
    );
    // module-level name
    assert_has(
        &r,
        "EDGE calls pkg.sub.service._private -> pkg.sub.service.build L23 extracted",
    );
    // nested function in enclosing scope
    assert_has(&r, "EDGE calls pkg.sub.service.Service._boot -> pkg.sub.service.Service._boot.inner L49 extracted");
    // self.member and Class.member matched against the class
    assert_has(
        &r,
        "EDGE calls pkg.sub.service.Service.start -> pkg.sub.service.Service._boot L40 resolved",
    );
    assert_has(
        &r,
        "EDGE calls pkg.sub.service.Service.start -> pkg.sub.service.Service.stop L42 resolved",
    );
    // self.member not defined here (maybe inherited): kept with its receiver
    assert_has(
        &r,
        "EDGE calls pkg.sub.service.Service.start -> ?missing q=self ip=- L41 extracted",
    );
    // through an import alias
    assert_has(&r, "EDGE calls pkg.sub.service.Service.start -> ?load q=U ip=pkg.sub.models.User.load L43 extracted");
    assert_has(
        &r,
        "EDGE calls pkg.sub.service.build -> ?log q=helpers ip=pkg.sub.helpers.log L17 extracted",
    );
    // stdlib and builtins are external
    assert_has(&r, &format!("EDGE calls pkg.sub.service.build -> ?dumps q=j ip={EXTERNAL_PREFIX}json.dumps L18 extracted"));
    assert_has(&r, &format!("EDGE calls pkg.sub.service.Service.stop -> ?join q=os.path ip={EXTERNAL_PREFIX}os.path.join L53 extracted"));
    assert_has(&r, &format!("EDGE calls pkg.sub.service.build -> ?print q=- ip={EXTERNAL_PREFIX}builtins.print L18 extracted"));
    // unknown receiver, super(), dynamic callee, unknown name
    assert_has(
        &r,
        "EDGE calls pkg.sub.service.build -> ?start q=svc ip=- L16 extracted",
    );
    assert_has(
        &r,
        "EDGE calls pkg.sub.service.Service.__init__ -> ?__init__ q=super() ip=- L28 extracted",
    );
    assert_has(
        &r,
        "EDGE calls pkg.sub.service.Service.stop -> ?<dynamic> q=factory() ip=- L54 extracted",
    );
    assert_has(
        &r,
        "EDGE calls pkg.sub.service.handler -> ?undefined_thing q=- ip=- L64 extracted",
    );
}

#[test]
fn methods_do_not_see_class_scope_names() {
    let source = b"class C:\n    def a(self):\n        pass\n\n    def b(self):\n        a()\n";
    let r = render(&extract("m.py", source));
    assert_has(&r, "EDGE calls m.C.b -> ?a q=- ip=- L6 extracted");
}

#[test]
fn inherits_and_decorators() {
    let r = render(&load("pkg/sub/service.py"));
    assert_has(
        &r,
        "EDGE inherits pkg.sub.service.Nested -> pkg.sub.service.Service L57 extracted",
    );
    assert_has(&r, "EDGE inherits pkg.sub.service.Service -> ?BaseService q=- ip=pkg.core.base.BaseService L26 extracted");
    assert_has(&r, &format!("EDGE references pkg.sub.service.Service.stop -> ?staticmethod q=- ip={EXTERNAL_PREFIX}builtins.staticmethod L51 extracted"));
    assert_has(
        &r,
        "EDGE references pkg.sub.service.handler -> ?route q=app ip=- L62 extracted",
    );
    // the decorator call is a reference, not also a call
    assert!(!r
        .iter()
        .any(|l| l.starts_with("EDGE calls") && l.contains("?route")));
}

#[test]
fn contains_edges_mirror_parents() {
    let facts = load("pkg/sub/service.py");
    for sym in facts
        .symbols
        .iter()
        .filter(|s| s.kind != SymbolKind::Module)
    {
        let parent = sym.parent.expect("every non-module symbol has a parent");
        assert!(facts.edges.iter().any(|e| e.kind == EdgeKind::Contains
            && e.src == parent
            && e.dst == Target::Symbol(sym.id)
            && e.provenance == Provenance::Extracted));
    }
}

#[test]
fn test_code_is_flagged() {
    let facts = load("pkg/sub/test_service.py");
    assert!(facts.symbols.iter().all(|s| s.is_test));
    let src = b"def test_x():\n    pass\n\nclass TestY:\n    def helper(self):\n        pass\n\ndef util():\n    pass\n";
    let facts = extract("pkg/mod.py", src);
    let test = |q: &str| {
        facts
            .symbols
            .iter()
            .find(|s| s.qualified == q)
            .unwrap()
            .is_test
    };
    assert!(test("pkg.mod.test_x"));
    assert!(test("pkg.mod.TestY.helper"));
    assert!(!test("pkg.mod.util"));
}

#[test]
fn syntax_errors_still_yield_facts() {
    let r = render(&load("pkg/sub/broken.py"));
    assert_has(&r, "parse_ok=false");
    assert_has(
        &r,
        "EDGE calls pkg.sub.broken.after -> pkg.sub.broken.ok L10 extracted",
    );
}

#[test]
fn output_is_deterministic_and_line_independent_ids() {
    let a = load("pkg/sub/service.py");
    let b = load("pkg/sub/service.py");
    assert_eq!(a, b);
    let shifted = format!(
        "\n\n{}",
        std::fs::read_to_string(Path::new(FIXTURES).join("pkg/sub/service.py")).unwrap()
    );
    let c = extract("pkg/sub/service.py", shifted.as_bytes());
    let ids = |f: &FileFacts| f.symbols.iter().map(|s| s.id).collect::<Vec<_>>();
    assert_eq!(ids(&a), ids(&c), "ids must not depend on line numbers");
    assert_ne!(a.content_hash, c.content_hash);
}

#[test]
fn golden() {
    let mut rendered = Vec::new();
    for rel in [
        "pkg/__init__.py",
        "pkg/sub/service.py",
        "pkg/sub/test_service.py",
        "pkg/sub/broken.py",
    ] {
        rendered.push(format!("### {rel}"));
        rendered.extend(render(&load(rel)));
    }
    let actual = rendered.join("\n") + "\n";
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/fixtures.txt");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(&golden).expect("run with UPDATE_GOLDEN=1 to create");
    assert_eq!(
        actual, expected,
        "golden mismatch; rerun with UPDATE_GOLDEN=1 after reviewing the diff"
    );
}
