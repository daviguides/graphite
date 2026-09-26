use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use graphite_daemon::{client, server, GraphQueries, Op, RepoPaths};
use graphite_hooks::parse::{plan, Action};
use graphite_hooks::shell::split;
use graphite_hooks::{install, pre};
use serde_json::{json, Value};

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

fn repo() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    std::fs::create_dir(root.join(".git")).unwrap();
    write(&root, "pkg/core.py", "def resolve_owner():\n    return 1\n");
    write(
        &root,
        "pkg/use.py",
        "from pkg.core import resolve_owner\n\ndef a():\n    return resolve_owner()\n",
    );
    write(&root, "README.md", "resolve_owner docs\n");
    (d, root)
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
    client::request(paths, Op::Shutdown).unwrap();
    h.join().unwrap().unwrap();
}

fn kind(cmd: &str, root: &Path) -> Option<Vec<&'static str>> {
    let segs = split(cmd)?;
    let acts = plan(&segs, root, root)?;
    Some(
        acts.iter()
            .map(|a| match a {
                Action::Search { .. } => "search",
                Action::Read { .. } => "read",
                Action::List { .. } => "list",
                Action::Cd(_) => "cd",
                Action::Plain => "plain",
            })
            .collect(),
    )
}

#[test]
fn command_table() {
    let (_d, root) = repo();
    let yes: &[(&str, &[&str])] = &[
        ("grep -rn resolve_owner --include=*.py .", &["search"]),
        ("grep -rn resolve_owner --include='*.py' . | grep -v tests | head -50", &["search"]),
        ("rg -n resolve_owner -g '*.py'", &["search"]),
        ("rg resolve_owner pkg -t py 2>/dev/null", &["search"]),
        ("grep -rnw -E 'resolve_(owner|x)' pkg", &["search"]),
        ("grep -n resolve_owner pkg/use.py", &["search"]),
        ("rtk grep -rn resolve_owner .", &["search"]),
        ("cat pkg/use.py", &["read"]),
        ("head -n 20 pkg/use.py", &["read"]),
        ("sed -n '1,4p' pkg/use.py", &["read"]),
        ("ls pkg", &["list"]),
        ("find . -name '*.py'", &["list"]),
        ("cd pkg && grep -n resolve_owner use.py", &["cd", "search"]),
        (
            "graphite blast resolve_owner --depth 1 --json | head -c 6000; echo; grep -rn resolve_owner --include=*.py .",
            &["plain", "plain", "search"],
        ),
    ];
    for (cmd, want) in yes {
        assert_eq!(kind(cmd, &root).as_deref(), Some(*want), "{cmd}");
    }
    let no = [
        "grep resolve_owner",                 // stdin filter
        "grep -c resolve_owner -r .",         // count output not reproduced
        "grep -v resolve_owner -r .",         // inverted
        "grep -rn resolve_owner . > out.txt", // file redirect
        "grep -rn x . | xargs rm",            // unsafe downstream
        "find . -name '*.py' -delete",        // mutating
        "sed -i 's/a/b/' pkg/use.py",         // mutating
        "cat README.md",                      // nothing graph-relevant to add
        "grep -rn x /etc",                    // outside the repo
        "grep -rn x missing_dir",             // grep's error is preserved
        "python -c 'print(1)'; grep -rn x .", // unknown segment
        "echo $(grep -rn x .)",               // substitution
    ];
    for cmd in no {
        let k = kind(cmd, &root);
        assert!(
            k.is_none() || !k.as_ref().unwrap().iter().any(|s| *s != "plain"),
            "{cmd} → {k:?}"
        );
    }
}

#[test]
fn rg_hidden_flags_reach_the_search_spec() {
    let (_d, root) = repo();
    let hidden = |cmd: &str| {
        let acts = plan(&split(cmd).unwrap(), &root, &root).unwrap();
        match &acts[0] {
            Action::Search { spec, .. } => spec.hidden,
            other => panic!("{cmd} → {other:?}"),
        }
    };
    assert!(!hidden("rg -n resolve_owner"));
    assert!(hidden("rg --hidden -n resolve_owner"));
    assert!(hidden("rg -. resolve_owner"));
    assert!(!hidden("grep -rn resolve_owner ."));
}

fn payload(cmd: &str, cwd: &Path) -> Value {
    json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "tool_input": {"command": cmd, "description": "d"},
        "cwd": cwd,
    })
}

fn hook_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_graphite-hook"))
}

fn run_bin(args: &[&str]) -> (String, i32) {
    let o = Command::new(hook_bin()).args(args).output().unwrap();
    (
        String::from_utf8_lossy(&o.stdout).into_owned(),
        o.status.code().unwrap_or(-1),
    )
}

#[test]
fn fail_open_without_daemon() {
    let (_d, root) = repo();
    let exe = hook_bin();
    assert!(pre::handle(&payload("grep -rn resolve_owner .", &root), &exe).is_none());
    // A routed command whose daemon is gone runs the original.
    let cwd = root.to_string_lossy().into_owned();
    let (out, code) = run_bin(&[
        "run",
        "--cwd",
        &cwd,
        "--",
        "grep -rn resolve_owner pkg/use.py",
    ]);
    assert_eq!(code, 0);
    assert!(
        out.contains("pkg/use.py:1:from pkg.core import resolve_owner"),
        "{out}"
    );
    assert!(!out.contains("[graphite]"), "{out}");
}

#[test]
fn end_to_end_rewrite_and_answer() {
    let (_d, root) = repo();
    let (paths, h) = start(&root);
    let exe = hook_bin();
    let out = pre::handle(
        &payload("grep -rn resolve_owner --include=*.py . | head -40", &root),
        &exe,
    )
    .expect("rewrite");
    let hs = &out["hookSpecificOutput"];
    assert_eq!(hs["hookEventName"], "PreToolUse");
    assert!(hs.get("permissionDecision").is_none());
    assert_eq!(hs["updatedInput"]["description"], "d");
    let new = hs["updatedInput"]["command"].as_str().unwrap().to_string();
    assert!(
        new.contains("graphite-hook") && new.contains(" run --cwd "),
        "{new}"
    );

    // Execute exactly what Claude Code would run.
    let o = Command::new("/bin/sh")
        .arg("-c")
        .arg(&new)
        .current_dir(&root)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(0), "{text}");
    assert!(
        text.starts_with(
            "[graphite] grep -rn resolve_owner --include=*.py . → 3 matches in 2 files"
        ),
        "{text}"
    );
    assert!(
        text.contains("pkg/use.py:4:    return resolve_owner()"),
        "{text}"
    );
    assert!(text.contains("definition:"), "{text}");

    // Reads get a header, then the real file.
    let (read, _) = run_bin(&[
        "run",
        "--cwd",
        &root.to_string_lossy(),
        "--",
        "cat pkg/core.py",
    ]);
    assert!(
        read.starts_with(
            "[graphite] pkg/core.py — 1 symbols; most used: resolve_owner L1 (2 callers)"
        ),
        "{read}"
    );
    assert!(
        read.ends_with("def resolve_owner():\n    return 1\n"),
        "{read}"
    );

    // No matches keeps grep's exit status.
    let (_, code) = run_bin(&[
        "run",
        "--cwd",
        &root.to_string_lossy(),
        "--",
        "grep -rn zzz_nothing .",
    ]);
    assert_eq!(code, 1);

    let log = std::fs::read_to_string(paths.dir.join("hooks.jsonl")).unwrap();
    let evs: Vec<Value> = log
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert!(evs
        .iter()
        .any(|e| e["event"] == "pre" && e["action"] == "rewrite"));
    let ans = evs
        .iter()
        .find(|e| e["event"] == "exec" && e["kind"] == "search")
        .unwrap();
    assert_eq!(ans["action"], "answer");
    assert_eq!(ans["graph_verdict"], "complete");
    assert!(ans["answer"].as_str().unwrap().contains("resolve_owner"));
    assert_eq!(ans["residue"]["reference"], 1);
    assert_eq!(ans["residue"]["import"], 1);
    stop(&paths, h);
}

#[test]
fn install_is_idempotent_and_keeps_foreign_hooks() {
    let (_d, root) = repo();
    write(
        &root,
        ".claude/settings.json",
        r#"{"permissions":{"allow":["Bash(ls:*)"]},"hooks":{"PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"rtk hook claude"}]}]}}"#,
    );
    let exe = PathBuf::from("/opt/bin/graphite-hook");
    install::install(&root, &exe).unwrap();
    install::install(&root, &exe).unwrap();
    let s: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(".claude/settings.json")).unwrap())
            .unwrap();
    let pre = s["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre.len(), 2, "{s}");
    assert_eq!(pre[0]["hooks"][0]["command"], "rtk hook claude");
    assert_eq!(pre[1]["hooks"][0]["command"], "/opt/bin/graphite-hook pre");
    assert_eq!(s["hooks"]["PostToolUse"].as_array().unwrap().len(), 1);
    assert_eq!(s["permissions"]["allow"][0], "Bash(ls:*)");
    assert!(root.join(".claude/settings.json.graphite-bak").exists());

    install::uninstall(&root).unwrap();
    let s: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(".claude/settings.json")).unwrap())
            .unwrap();
    assert_eq!(s["hooks"]["PreToolUse"].as_array().unwrap().len(), 1);
    assert!(s["hooks"].get("PostToolUse").is_none());
}
