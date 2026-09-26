use std::path::Path;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use graphite_daemon::{client, server, DaemonError, GraphQueries, Op, RepoPaths, Response};
use graphite_store::GraphStore;
use serde_json::Value;

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn start(root: &Path) -> (RepoPaths, JoinHandle<graphite_daemon::Result<()>>) {
    let paths = RepoPaths::new(root);
    let p = paths.clone();
    let h = std::thread::spawn(move || server::run(p, Box::new(GraphQueries)));
    let deadline = Instant::now() + Duration::from_secs(30);
    while client::connect(&paths).is_none() {
        assert!(Instant::now() < deadline, "daemon did not start");
        std::thread::sleep(Duration::from_millis(20));
    }
    (paths, h)
}

fn stop(paths: &RepoPaths, h: JoinHandle<graphite_daemon::Result<()>>) {
    let r = client::request(paths, Op::Shutdown).unwrap();
    assert!(r.ok);
    h.join().unwrap().unwrap();
}

fn blast(paths: &RepoPaths, symbol: &str) -> Response {
    client::request(
        paths,
        Op::Blast {
            symbol: symbol.into(),
            depth: Some(5),
            budget: Some(1_000_000),
            compact: Some(true),
        },
    )
    .unwrap()
}

fn dependents(r: &Response) -> Vec<String> {
    let mut v: Vec<String> = r.data["result"]["dependents"]["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| &i["symbol"])
        .filter(|s| s["kind"] != "module")
        .map(|s| s["qualified"].as_str().unwrap().to_string())
        .collect();
    v.sort();
    v
}

fn seed(root: &Path) {
    write(root, "a.py", "def f():\n    return 1\n");
    write(
        root,
        "b.py",
        "from a import f\n\ndef g():\n    return f()\n",
    );
    write(root, "node_modules/x.py", "def ignored():\n    pass\n");
}

#[test]
fn watcher_reflects_edits_and_bumps_rev() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path());
    let (paths, h) = start(tmp.path());

    let r = blast(&paths, "a.f");
    assert!(r.ok, "{r:?}");
    assert_eq!(dependents(&r), vec!["b.g"]);
    let rev0 = r.graph_rev;

    write(
        tmp.path(),
        "b.py",
        "from a import f\n\ndef g():\n    return f()\n\ndef h():\n    return f()\n",
    );
    let t = Instant::now();
    let seen = loop {
        let r = blast(&paths, "a.f");
        if dependents(&r) == vec!["b.g", "b.h"] {
            break r;
        }
        assert!(t.elapsed() < Duration::from_secs(10), "edit never visible");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(seen.graph_rev > rev0);
    eprintln!("edit visible via watcher after {:?}", t.elapsed());

    std::fs::remove_file(tmp.path().join("b.py")).unwrap();
    let t = Instant::now();
    while !dependents(&blast(&paths, "a.f")).is_empty() {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "delete never visible"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    stop(&paths, h);
}

#[test]
fn nudge_makes_edit_visible_immediately() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path());
    let (paths, h) = start(tmp.path());
    write(
        tmp.path(),
        "c.py",
        "from a import f\n\ndef k():\n    return f()\n",
    );
    let r = client::request(
        &paths,
        Op::Nudge {
            paths: vec!["c.py".into()],
        },
    )
    .unwrap();
    assert!(r.ok, "{r:?}");
    assert!(dependents(&blast(&paths, "a.f")).contains(&"c.k".to_string()));
    stop(&paths, h);
}

#[test]
fn burst_is_never_silently_stale() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path());
    let (paths, h) = start(tmp.path());
    let n = 100;
    for i in 0..n {
        write(
            tmp.path(),
            &format!("burst/m{i}.py"),
            &format!("from a import f\n\ndef u{i}():\n    return f()\n"),
        );
    }
    // Give the OS watcher a moment to report, then query: either complete or flagged stale.
    std::thread::sleep(Duration::from_millis(50));
    let mut saw_stale = false;
    let t = Instant::now();
    loop {
        let r = blast(&paths, "a.f");
        // Each burst module and its function depend on a.f, plus b and b.g.
        let total = r.data["result"]["dependents"]["summary"]["total"]
            .as_u64()
            .unwrap();
        if r.stale {
            saw_stale = true;
        } else if total == 2 * n as u64 + 2 {
            break;
        }
        assert!(
            t.elapsed() < Duration::from_secs(20),
            "burst never fully indexed"
        );
    }
    eprintln!(
        "burst of {n}: stale flag observed = {saw_stale}, complete after {:?}",
        t.elapsed()
    );
    stop(&paths, h);
}

#[test]
fn protocol_roundtrip_and_errors_are_success_shaped() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path());
    let (paths, h) = start(tmp.path());

    let st = client::request(&paths, Op::Status).unwrap();
    assert!(st.ok);
    assert_eq!(
        st.data["files"],
        Value::from(2),
        "node_modules must be skipped"
    );

    let amb_src = "def f():\n    pass\n";
    write(tmp.path(), "z.py", amb_src);
    client::request(
        &paths,
        Op::Nudge {
            paths: vec!["z.py".into()],
        },
    )
    .unwrap();
    let r = client::request(&paths, Op::Lookup { symbol: "f".into() }).unwrap();
    assert!(r.ok);
    assert_eq!(r.data["result"]["status"], "ambiguous");
    let r = blast(&paths, "nope.nothing");
    assert!(r.ok);
    assert_eq!(r.data["result"]["target"]["status"], "not_found");

    use std::io::{BufRead, BufReader, Write};
    let mut s = client::connect(&paths).unwrap();
    s.write_all(b"{not json}\n").unwrap();
    let mut line = String::new();
    BufReader::new(s.try_clone().unwrap())
        .read_line(&mut line)
        .unwrap();
    let resp: Response = serde_json::from_str(&line).unwrap();
    assert!(!resp.ok);
    stop(&paths, h);
}

#[test]
fn single_instance_per_repo() {
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path());
    let (paths, h) = start(tmp.path());
    let second = server::run(paths.clone(), Box::new(GraphQueries));
    assert!(matches!(second, Err(DaemonError::AlreadyRunning(_))));
    stop(&paths, h);
}

#[test]
fn restart_rewrites_only_changed_files() {
    use graphite_daemon::Engine;
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path());
    write(tmp.path(), "c.py", "def c():\n    pass\n");
    let paths = RepoPaths::new(tmp.path());

    let first = Engine::open(paths.clone()).unwrap().index_all().unwrap();
    assert_eq!(first.files_written, 3);

    let again = Engine::open(paths.clone()).unwrap().index_all().unwrap();
    assert_eq!((again.files_written, again.files_removed), (0, 0));

    write(
        tmp.path(),
        "b.py",
        "from a import f\n\ndef g2():\n    return f()\n",
    );
    std::fs::remove_file(tmp.path().join("c.py")).unwrap();
    let engine = Engine::open(paths.clone()).unwrap();
    let edited = engine.index_all().unwrap();
    assert_eq!((edited.files_written, edited.files_removed), (1, 1));
    let callers: Vec<_> = engine
        .store
        .symbols_by_name("g2")
        .unwrap()
        .into_iter()
        .map(|s| s.qualified)
        .collect();
    assert_eq!(callers, vec!["b.g2"]);
}

#[test]
fn db_layout_mismatch_forces_full_reindex() {
    use graphite_daemon::engine::DB_VERSION;
    use graphite_daemon::Engine;
    let tmp = tempfile::tempdir().unwrap();
    seed(tmp.path());
    let paths = RepoPaths::new(tmp.path());
    Engine::open(paths.clone()).unwrap().index_all().unwrap();
    assert_eq!(
        std::fs::read_to_string(&paths.db_version).unwrap(),
        DB_VERSION.to_string()
    );

    let same = Engine::open(paths.clone()).unwrap().index_all().unwrap();
    assert_eq!(same.files_written, 0);

    // A DB from another layout (or one predating the version file) is rebuilt, not trusted.
    for stale in [Some("1"), None] {
        match stale {
            Some(v) => std::fs::write(&paths.db_version, v).unwrap(),
            None => std::fs::remove_file(&paths.db_version).unwrap(),
        }
        let rebuilt = Engine::open(paths.clone()).unwrap().index_all().unwrap();
        assert_eq!(
            rebuilt.files_written, 2,
            "stale layout {stale:?} must reindex everything"
        );
    }
}

fn git(root: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?} failed");
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn state_dir_stays_out_of_git() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    git(root, &["init", "-q"]);
    seed(root);

    for _ in 0..2 {
        let (paths, h) = start(root);
        stop(&paths, h);
        let status = git(root, &["status", "--porcelain", "--untracked-files=all"]);
        assert!(
            !status.contains(".graphite"),
            "status shows state dir:\n{status}"
        );
    }

    git(root, &["add", "-A"]);
    let staged = git(root, &["diff", "--cached", "--name-only"]);
    assert!(
        !staged.contains(".graphite"),
        "add -A staged state dir:\n{staged}"
    );
    assert!(staged.contains("a.py"), "sanity: real files are staged");
    assert!(
        !root.join(".gitignore").exists(),
        "user .gitignore must not be created"
    );
}
