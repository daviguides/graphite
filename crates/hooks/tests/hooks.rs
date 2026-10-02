use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use graphite_daemon::{client, server, GraphQueries, Op, RepoPaths};
use graphite_hooks::parse::{plan, stage, Action, Stage};
use graphite_hooks::shell::split;
use graphite_hooks::{install, logview, pre};
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
                Action::Assign { .. } => "assign",
                Action::Deferred { answerable: true } => "deferred",
                Action::Deferred { answerable: false } => "deferred-plain",
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
        ("grep -rn resolve_owner . | head -c 3000", &["search"]),
        ("grep -rn resolve_owner . | wc -l", &["plain"]),
        ("grep -rn resolve_owner . | sort | uniq", &["plain"]),
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
        // Pilot C pass-throughs (2026-09-26), now covered.
        (
            "f=$(find . -path '*pkg/core.py' -not -path '*/node_modules/*' | head -1); echo $f; wc -l $f; cat $f",
            &["assign", "deferred-plain", "deferred-plain", "deferred"],
        ),
        (
            "f=$(find . -path ./node_modules -prune -o -name core.py -print | head -1); cat -n \"$f\"",
            &["assign", "deferred"],
        ),
        (
            "sed -n 1,25p README.md; sed -n 1,4p pkg/use.py; grep -n resolve_owner pkg/use.py | head",
            &["plain", "read", "search"],
        ),
        ("cat README.md | sed -n 1,40p; cat pkg/use.py", &["plain", "read"]),
        (
            "git stash list >/dev/null; git status --short; grep -rn resolve_owner .",
            &["plain", "plain", "search"],
        ),
        ("grep -rn resolve_owner . >/dev/null", &["plain"]),
        ("git diff --stat | tail -1; git log --oneline -3", &["plain", "plain"]),
    ];
    for (cmd, want) in yes {
        assert_eq!(kind(cmd, &root).as_deref(), Some(*want), "{cmd}");
    }
    let no = [
        "grep resolve_owner",                          // stdin filter
        "grep -c resolve_owner -r .",                  // count output not reproduced
        "grep -v resolve_owner -r .",                  // inverted
        "grep -rn resolve_owner . > out.txt",          // file redirect
        "grep -rn x . | xargs rm",                     // unsafe downstream
        "find . -name '*.py' -delete",                 // mutating
        "sed -i 's/a/b/' pkg/use.py",                  // mutating
        "cat README.md",                               // nothing graph-relevant to add
        "grep -rn x /etc",                             // outside the repo
        "grep -rn x missing_dir",                      // grep's error is preserved
        "python -c 'print(1)'; grep -rn x .",          // unknown segment
        "echo $(grep -rn x .)",                        // substitution
        "f=$(rm -rf pkg); cat $f",                     // substitution that writes
        "f=$(find .); python $f",                      // variable fed to an unknown command
        "f=$(find .); $f",                             // variable as the command
        "cat $g",                                      // variable not set by this command
        "git push origin main",                        // git that writes
        "git branch -D old",                           // git that writes
        "git diff --output=x.patch",                   // git that writes a file
        "sed -n 1p README.md; sed -i '' 1d README.md", // one segment writes
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
        "session_id": "s1",
        "tool_use_id": "toolu_1",
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
        new.contains("graphite-hook")
            && new.contains(" run ")
            && new.contains(" --session s1 ")
            && new.contains(" --turn toolu_1 ")
            && new.contains(" --cwd "),
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
            "# graphite: grep -rn resolve_owner --include=*.py . → 3 matches in 2 files · graph COMPLETE"
        ),
        "{text}"
    );
    assert!(
        text.contains("pkg/use.py:4:    return resolve_owner()    ← a"),
        "{text}"
    );
    assert!(
        text.contains("pkg/core.py:1:def resolve_owner():    [definition]"),
        "{text}"
    );
    assert!(
        text.contains("# graphite: `head -40` applied as answer budget (40 lines)"),
        "{text}"
    );

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
    assert_eq!(ans["session_id"], "s1");
    assert_eq!(ans["turn_id"], "toolu_1");
    let pre_ev = evs
        .iter()
        .find(|e| e["event"] == "pre" && e["action"] == "rewrite")
        .unwrap();
    assert_eq!(pre_ev["call_id"], ans["call_id"]);
    let keys: Vec<&str> = ans["keys"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap())
        .collect();
    assert!(
        keys.contains(&"pkg/core.py:1") && keys.contains(&"pkg/use.py:4"),
        "{keys:?}"
    );
    assert!(ans["record"]["items"].as_array().unwrap().len() >= 2);

    // Debug views of what the agent got.
    let listing = logview::list(&paths.dir, 20);
    assert!(listing.contains("exec  answer"), "{listing}");
    let n = listing
        .lines()
        .find(|l| l.contains("exec  answer") && l.contains("search") && l.contains("resolve_owner"))
        .and_then(|l| l.trim_start_matches('#').split_whitespace().next())
        .and_then(|n| n.parse::<usize>().ok())
        .unwrap();
    let shown = logview::show(&paths.dir, n).unwrap();
    assert!(
        shown.contains("--- model answer (exactly what the agent received) ---"),
        "{shown}"
    );
    assert!(
        shown.contains("--- human view (same record) ---"),
        "{shown}"
    );
    assert!(shown.contains("class=definition"), "{shown}");

    // Formats on demand; model is the default.
    let cwd = root.to_string_lossy().into_owned();
    let (human, _) = run_bin(&[
        "run",
        "--human",
        "--cwd",
        &cwd,
        "--",
        "grep -rn resolve_owner .",
    ]);
    assert!(human.contains("pkg/use.py\n"), "{human}");
    let (explain, _) = run_bin(&[
        "run",
        "--explain",
        "--cwd",
        &cwd,
        "--",
        "grep -rn resolve_owner .",
    ]);
    assert!(explain.contains("why:"), "{explain}");
    let (js, _) = run_bin(&[
        "run",
        "--json",
        "--cwd",
        &cwd,
        "--",
        "grep -rn resolve_owner .",
    ]);
    let v: Value = serde_json::from_str(&js).unwrap();
    assert_eq!(v["mode"], "identifier");

    // Pipeline intent: budget and semantic test filter, counts pass through raw.
    write(&root, "tests/test_x.py", "from unittest.mock import patch\n\ndef test_a():\n    with patch(\"pkg.core.resolve_owner\"):\n        pass\n");
    std::thread::sleep(Duration::from_millis(400));
    let (b, _) = run_bin(&[
        "run",
        "--cwd",
        &cwd,
        "--",
        "grep -rn resolve_owner . | head -c 600",
    ]);
    assert!(b.len() <= 600, "{}: {b}", b.len());
    assert!(b.contains("applied as answer budget (600 bytes)"), "{b}");
    let (t, _) = run_bin(&[
        "run",
        "--cwd",
        &cwd,
        "--",
        "grep -rn resolve_owner . | grep -v test",
    ]);
    assert!(!t.contains("tests/test_x.py"), "{t}");
    assert!(t.contains("omitted (you filtered tests)"), "{t}");
    let (c, _) = run_bin(&[
        "run",
        "--cwd",
        &cwd,
        "--",
        "grep -rn resolve_owner --include=*.py . | wc -l",
    ]);
    assert_eq!(c.trim(), "4", "{c}");
    stop(&paths, h);
}

// kinhin: decision(ref="docs/foundation/interception.md#1-steering-by-transparent-interception")
#[test]
fn assigned_paths_and_repeated_reads_run_like_the_shell() {
    let (_d, root) = repo();
    let (paths, h) = start(&root);
    let exe = hook_bin();
    let cmd = "f=$(find pkg -name core.py | head -1); echo $f; wc -l $f; cat \"$f\"";
    assert!(
        pre::handle(&payload(cmd, &root), &exe).is_some(),
        "not rewritten"
    );
    let cwd = root.to_string_lossy().into_owned();
    let (out, code) = run_bin(&["run", "--cwd", &cwd, "--", cmd]);
    assert_eq!(code, 0, "{out}");
    let plain = Command::new("/bin/sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(&root)
        .output()
        .unwrap();
    let plain = String::from_utf8_lossy(&plain.stdout);
    // The agent's own output, untouched, plus one graph header before the file.
    let without_header: String = out
        .lines()
        .filter(|l| !l.starts_with("[graphite]"))
        .map(|l| format!("{l}\n"))
        .collect();
    assert_eq!(without_header, plain, "{out}");
    assert_eq!(out.matches("[graphite] pkg/core.py").count(), 1, "{out}");

    // Two reads of one file in one command: one header.
    let (two, _) = run_bin(&[
        "run",
        "--cwd",
        &cwd,
        "--",
        "sed -n 1,1p pkg/core.py; sed -n 2,2p pkg/core.py",
    ]);
    assert_eq!(two.matches("[graphite]").count(), 1, "{two}");
    assert!(
        two.ends_with("def resolve_owner():\n    return 1\n"),
        "{two}"
    );
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

#[test]
fn install_then_uninstall_is_byte_identical() {
    let (_d, root) = repo();
    let originals = [
        // pretty, non-alphabetical keys, foreign RTK hook, 2-space
        "{\n  \"permissions\": {\n    \"allow\": [\"Bash(ls:*)\"]\n  },\n  \"hooks\": {\n    \"PreToolUse\": [\n      {\n        \"matcher\": \"Bash\",\n        \"hooks\": [{\"type\": \"command\", \"command\": \"rtk hook claude\"}]\n      }\n    ]\n  },\n  \"env\": {\"A\": \"1\"}\n}\n",
        // compact one-liner without hooks
        r#"{"zeta":1,"alpha":{"b":2}}"#,
        // 4-space indent, empty hooks object
        "{\n    \"model\": \"opus\",\n    \"hooks\": {}\n}\n",
        // existing empty PreToolUse array, no trailing newline
        "{\n  \"hooks\": {\n    \"PreToolUse\": []\n  }\n}",
    ];
    let exe = PathBuf::from("/opt/bin/graphite-hook");
    for orig in originals {
        write(&root, ".claude/settings.json", orig);
        install::install(&root, &exe).unwrap();
        let during = std::fs::read_to_string(root.join(".claude/settings.json")).unwrap();
        let v: Value = serde_json::from_str(&during).unwrap_or_else(|e| panic!("{e}\n{during}"));
        assert!(during.contains("/opt/bin/graphite-hook pre"), "{during}");
        assert!(v["hooks"]["PostToolUse"].is_array(), "{during}");
        // user keys keep their order
        let first_key = |t: &str| t.split('"').nth(1).unwrap().to_string();
        assert_eq!(first_key(orig), first_key(&during));
        install::install(&root, &exe).unwrap(); // idempotent
        assert_eq!(
            std::fs::read_to_string(root.join(".claude/settings.json")).unwrap(),
            during
        );
        install::uninstall(&root).unwrap();
        let after = std::fs::read_to_string(root.join(".claude/settings.json")).unwrap();
        assert_eq!(after, orig, "not byte-identical after uninstall");
        assert!(!root.join(".claude/settings.json.graphite-install").exists());
        assert!(!root.join(".claude/settings.json.graphite-bak").exists());
    }
}

#[test]
fn uninstall_removes_what_install_created() {
    let exe = PathBuf::from("/opt/bin/graphite-hook");
    // No .claude/ at all: both dir and file go away.
    let (_d, root) = repo();
    install::install(&root, &exe).unwrap();
    assert!(root.join(".claude/settings.json").exists());
    install::uninstall(&root).unwrap();
    assert!(!root.join(".claude").exists(), "created dir left behind");

    // .claude/ exists (with another file) but no settings.json: only the file goes away.
    let (_d2, root2) = repo();
    write(&root2, ".claude/commands/x.md", "hi\n");
    install::install(&root2, &exe).unwrap();
    install::uninstall(&root2).unwrap();
    assert!(!root2.join(".claude/settings.json").exists());
    assert!(root2.join(".claude/commands/x.md").exists());
    assert!(!root2
        .join(".claude/settings.json.graphite-install")
        .exists());
}

#[test]
fn pipeline_stage_table() {
    let budget = |s: &str| match stage(s) {
        Stage::Budget(b) => (b.bytes, b.lines),
        other => panic!("{s} → {other:?}"),
    };
    assert_eq!(budget("head -c 3000"), (Some(3000), None));
    assert_eq!(budget("head -c3000"), (Some(3000), None));
    assert_eq!(budget("head --bytes=100"), (Some(100), None));
    assert_eq!(budget("head -n 50"), (None, Some(50)));
    assert_eq!(budget("head -50"), (None, Some(50)));
    assert_eq!(budget("head"), (None, Some(10)));
    // tail wants the end of grep's file order: the original command runs untouched.
    for t in ["tail -n 20", "tail -20", "tail"] {
        assert_eq!(stage(t), Stage::Transform, "{t}");
    }
    for t in [
        "grep -v test",
        "grep -v tests",
        "grep -v '/tests/'",
        "grep -vi test",
        "rg -v _test",
    ] {
        assert!(matches!(stage(t), Stage::DropTests(_)), "{t}");
    }
    match stage("grep -v foo") {
        Stage::Line(f) => assert!(f.invert && f.pattern == "foo"),
        other => panic!("{other:?}"),
    }
    match stage("grep -i 'a\\|b'") {
        Stage::Line(f) => assert!(f.ignore_case && f.pattern == "a|b", "{f:?}"),
        other => panic!("{other:?}"),
    }
    for t in ["wc -l", "sort", "uniq -c", "cut -d: -f1", "tr a b", "jq ."] {
        assert_eq!(stage(t), Stage::Transform, "{t}");
    }
    assert_eq!(stage("cat"), Stage::Noop);
    for t in [
        "grep -c x",
        "grep -o x",
        "head -n -5",
        "sed -n 1p",
        "grep x file.txt",
    ] {
        assert_eq!(stage(t), Stage::Real, "{t}");
    }
}
