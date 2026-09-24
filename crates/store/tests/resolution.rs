//! Resolution tiers against real extractor output on small Python fixtures.

use graphite_extract_python::extract;
use graphite_model::{EdgeKind, Provenance, Symbol, Target};
use graphite_store::{Adjacency, CozoStore, GraphStore, Outcome, Resolution};

fn store_with(files: &[(&str, &str)]) -> (tempfile::TempDir, CozoStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = CozoStore::open(&dir.path().join("db")).unwrap();
    for (path, src) in files {
        SOURCES.with(|m| m.borrow_mut().insert(path.to_string(), src.to_string()));
        store.replace_file(&extract(path, src.as_bytes())).unwrap();
    }
    (dir, store)
}

fn sym(store: &CozoStore, qualified: &str) -> Symbol {
    let name = qualified.rsplit('.').next().unwrap();
    store
        .symbols_by_name(name)
        .unwrap()
        .into_iter()
        .find(|s| s.qualified == qualified)
        .unwrap_or_else(|| panic!("no symbol {qualified}"))
}

/// Outcome of the call named `name` made directly from `caller`, located through the extracted facts.
fn call(store: &CozoStore, caller: &str, name: &str) -> Outcome {
    let caller = sym(store, caller);
    let path = caller.path.clone();
    let source = SOURCES.with(|m| m.borrow()[&path].clone());
    let facts = extract(&path, source.as_bytes());
    let idx = facts
        .edges
        .iter()
        .position(|e| {
            e.src == caller.id
                && e.kind == EdgeKind::Calls
                && match &e.dst {
                    Target::Unresolved { name: n, .. } => n == name,
                    Target::Symbol(d) => facts.symbols.iter().any(|s| s.id == *d && s.name == name),
                }
        })
        .unwrap_or_else(|| panic!("no call to {name} from {}", caller.qualified))
        as u32;
    store
        .callees(caller.id)
        .unwrap()
        .into_iter()
        .find(|r: &Resolution| r.key.path == path && r.key.idx == idx)
        .expect("resolution for the call")
        .outcome
}

thread_local! {
    static SOURCES: std::cell::RefCell<std::collections::HashMap<String, String>> = Default::default();
}

#[test]
fn builtin_method_on_untyped_receiver_is_ambiguous_not_guessed() {
    let (_d, store) = store_with(&[
        (
            "app/core.py",
            "class Config:\n    def get(self, key):\n        return key\n",
        ),
        ("app/api.py", "def unrelated(x):\n    return x.get(\"k\")\n"),
    ]);
    match call(&store, "app.api.unrelated", "get") {
        Outcome::Ambiguous { candidates } => assert_eq!(candidates, 2, "repo Config.get + builtin"),
        other => panic!("x.get must be a disclosed gap, got {other:?}"),
    }
    let gaps = store.name_gaps("get").unwrap();
    assert_eq!(
        gaps.len(),
        1,
        "the x.get call must show up as a gap for Config.get"
    );
}

#[test]
fn untyped_receiver_with_two_same_name_functions_is_ambiguous() {
    let (_d, store) = store_with(&[
        ("app/jobs.py", "def run():\n    return 1\n"),
        ("app/tasks.py", "def run():\n    return 2\n"),
        (
            "app/dynamic.py",
            "def dispatch(worker):\n    return worker.run()\n",
        ),
    ]);
    assert_eq!(
        call(&store, "app.dynamic.dispatch", "run"),
        Outcome::Ambiguous { candidates: 2 }
    );
}

#[test]
fn untyped_receiver_with_unique_repo_method_is_name_guess() {
    let (_d, store) = store_with(&[
        (
            "app/core.py",
            "class Registry:\n    def lookup_meeting(self, key):\n        return key\n",
        ),
        (
            "app/api.py",
            "def handler(reg):\n    return reg.lookup_meeting(1)\n",
        ),
    ]);
    let target = sym(&store, "app.core.Registry.lookup_meeting");
    assert_eq!(
        call(&store, "app.api.handler", "lookup_meeting"),
        Outcome::Resolved {
            dst: target.id,
            provenance: Provenance::NameGuess
        }
    );
}

#[test]
fn import_resolves_across_differing_import_root() {
    // Path-derived module is tools.orch.runner.runner.core.x, but code imports it as runner.core.x.
    let (_d, store) = store_with(&[
        (
            "tools/orch/runner/runner/core/x.py",
            "def helper():\n    return 1\n",
        ),
        (
            "tools/orch/runner/runner/cli.py",
            "from runner.core.x import helper\n\ndef main():\n    return helper()\n",
        ),
    ]);
    let target = sym(&store, "tools.orch.runner.runner.core.x.helper");
    assert_eq!(
        call(&store, "tools.orch.runner.runner.cli.main", "helper"),
        Outcome::Resolved {
            dst: target.id,
            provenance: Provenance::Resolved
        }
    );
}

#[test]
fn inherited_self_method_resolves_through_base_in_other_file() {
    let (_d, store) = store_with(&[
        (
            "app/base.py",
            "class Base:\n    def save(self):\n        return 1\n",
        ),
        (
            "app/child.py",
            "from app.base import Base\n\nclass Child(Base):\n    def run(self):\n        return self.save()\n",
        ),
    ]);
    let target = sym(&store, "app.base.Base.save");
    assert_eq!(
        call(&store, "app.child.Child.run", "save"),
        Outcome::Resolved {
            dst: target.id,
            provenance: Provenance::Inferred
        }
    );
}

#[test]
fn super_init_with_external_base_stays_unresolved() {
    let (_d, store) = store_with(&[
        (
            "app/errors.py",
            "class AppError(Exception):\n    def __init__(self, msg):\n        super().__init__(msg)\n",
        ),
        ("app/other.py", "class Thing:\n    def __init__(self):\n        self.x = 1\n"),
    ]);
    assert_eq!(
        call(&store, "app.errors.AppError.__init__", "__init__"),
        Outcome::Unresolved
    );
}

#[test]
fn third_party_import_never_name_guesses_into_repo() {
    let (_d, store) = store_with(&[
        (
            "app/core.py",
            "class Client:\n    def fetch_remote(self):\n        return 1\n",
        ),
        (
            "app/api.py",
            "import somevendor\n\ndef handler():\n    return somevendor.fetch_remote()\n",
        ),
    ]);
    assert_eq!(
        call(&store, "app.api.handler", "fetch_remote"),
        Outcome::Unresolved
    );
}

#[test]
fn batch_symbols_and_file_hashes() {
    let src = "def a():\n    return 1\n\ndef b():\n    return a()\n";
    let (_d, store) = store_with(&[("app/m.py", src)]);
    let a = sym(&store, "app.m.a");
    let b = sym(&store, "app.m.b");
    let mut got: Vec<String> = store
        .symbols(&[a.id, b.id, a.id])
        .unwrap()
        .into_iter()
        .map(|s| s.qualified)
        .collect();
    got.sort();
    assert_eq!(got, vec!["app.m.a", "app.m.b"]);
    assert!(store.symbols(&[]).unwrap().is_empty());

    let hashes = store.file_hashes().unwrap();
    assert_eq!(hashes.len(), 1);
    assert_eq!(hashes["app/m.py"], *blake3::hash(src.as_bytes()).as_bytes());
}

#[test]
fn deleting_the_module_named_by_an_import_head_reresolves_its_edges() {
    // `regent.core.helper` falls back to name guessing only while a repo module named `regent` exists.
    let files = [
        ("regent/__init__.py", ""),
        ("regent/core/other.py", "def helper():\n    return 1\n"),
        (
            "app/use.py",
            "from regent.core import missing\n\ndef go():\n    return missing.helper()\n",
        ),
    ];
    let (_d, store) = store_with(&files);
    let mut adj = Adjacency::rebuild(&store).unwrap();
    let delta = store.remove_file("regent/__init__.py").unwrap();
    adj.apply(&store, &delta).unwrap();
    assert_eq!(
        adj.snapshot(),
        Adjacency::rebuild(&store).unwrap().snapshot()
    );
}
