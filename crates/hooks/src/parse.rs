//! Turn one shell segment into something Graphite can answer, or refuse.

use std::path::{Path, PathBuf};

use graphite_daemon::SearchSpec;

use crate::shell::{words, Segment};

/// What a segment becomes.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// grep/rg/ag/ack answered from embedded search + graph judgment; `filters` are downstream stages fed our text.
    Search {
        spec: SearchSpec,
        filters: Vec<String>,
    },
    /// cat/head/tail/sed -n/nl of files: real output, graph header first.
    Read { files: Vec<String> },
    /// ls/find: real output, graph summary after.
    List { dirs: Vec<String> },
    /// `cd DIR` inside a compound command.
    Cd(PathBuf),
    /// A read-only command run as-is (echo, graphite queries, ...).
    Plain,
}

/// Downstream pipeline stages that only read stdin and write stdout.
const SAFE_FILTERS: &[&str] = &[
    "head", "tail", "grep", "egrep", "fgrep", "rg", "sort", "uniq", "wc", "cut", "tr", "jq", "cat",
    "column", "nl",
];

/// Commands allowed to run untouched next to answered segments.
const PLAIN_OK: &[&str] = &["echo", "printf", "pwd", "true", "wc"];

/// `graphite` subcommands that only read.
const GRAPHITE_READ: &[&str] = &["lookup", "blast", "diff-impact", "status"];

fn strip_wrappers(mut w: Vec<String>) -> Vec<String> {
    while matches!(w.first().map(String::as_str), Some("rtk") | Some("command")) {
        w.remove(0);
    }
    w
}

fn abs_path(cwd: &Path, p: &str) -> PathBuf {
    let pb = if Path::new(p).is_absolute() {
        PathBuf::from(p)
    } else {
        cwd.join(p)
    };
    pb.canonicalize().unwrap_or(pb)
}

fn inside(root: &Path, p: &Path) -> bool {
    p.starts_with(root)
}

fn safe_filter(stage: &str) -> bool {
    let Some(w) = words(stage) else {
        return false;
    };
    let Some(cmd) = w.first() else { return false };
    if !SAFE_FILTERS.contains(&cmd.as_str()) {
        return cmd == "sed"
            && w.iter().any(|a| a == "-n")
            && !w.iter().any(|a| a.starts_with("-i"));
    }
    // A filter with a path operand reads files, not stdin: still read-only, fine.
    !w.iter().any(|a| a == "-exec" || a == "-delete")
}

/// Classify one segment. `root` is the repo the daemon serves.
pub fn classify(seg: &Segment, cwd: &Path, root: &Path) -> Option<Action> {
    let first = strip_wrappers(words(&seg.stages[0])?);
    let cmd = first.first()?.as_str();
    let rest = &seg.stages[1..];
    let filters_ok = rest.iter().all(|s| safe_filter(s));
    match cmd {
        "grep" | "egrep" | "fgrep" | "rg" | "ag" | "ack" => {
            let spec = search_spec(&first, cwd, root, &seg.stages[0])?;
            filters_ok.then(|| Action::Search {
                spec,
                filters: rest.to_vec(),
            })
        }
        "cat" | "head" | "tail" | "nl" | "sed" => {
            let files = reader_files(&first, cwd, root)?;
            filters_ok.then_some(Action::Read { files })
        }
        "ls" | "find" => {
            let dirs = list_dirs(&first, cwd, root)?;
            filters_ok.then_some(Action::List { dirs })
        }
        "cd" if seg.stages.len() == 1 && first.len() == 2 => {
            let d = abs_path(cwd, &first[1]);
            d.is_dir().then_some(Action::Cd(d))
        }
        "graphite"
            if first
                .get(1)
                .is_some_and(|s| GRAPHITE_READ.contains(&s.as_str())) =>
        {
            filters_ok.then_some(Action::Plain)
        }
        c if PLAIN_OK.contains(&c) => filters_ok.then_some(Action::Plain),
        _ => None,
    }
}

/// True if at least one segment is answered by Graphite (not just run as-is).
pub fn answers(actions: &[Action]) -> bool {
    actions.iter().any(|a| {
        matches!(
            a,
            Action::Search { .. } | Action::Read { .. } | Action::List { .. }
        )
    })
}

/// Classify every segment, following `cd`s; None if any segment is unsupported.
pub fn plan(segs: &[Segment], cwd: &Path, root: &Path) -> Option<Vec<Action>> {
    let mut dir = cwd.to_path_buf();
    let mut out = Vec::new();
    for s in segs {
        let a = classify(s, &dir, root)?;
        if let Action::Cd(d) = &a {
            dir = d.clone();
        }
        out.push(a);
    }
    Some(out)
}

/// Basic regular expression (grep default) to Rust regex syntax.
pub fn bre_to_ere(p: &str) -> String {
    let mut out = String::new();
    let mut it = p.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '\\' => match it.next() {
                Some(n @ ('|' | '(' | ')' | '{' | '}' | '+' | '?')) => out.push(n),
                Some(n) => {
                    out.push('\\');
                    out.push(n);
                }
                None => out.push_str("\\\\"),
            },
            '|' | '(' | ')' | '{' | '}' | '+' | '?' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

fn rg_type(t: &str) -> Option<Vec<&'static str>> {
    Some(match t {
        "py" | "python" => vec!["*.py", "*.pyi"],
        "rust" | "rs" => vec!["*.rs"],
        "js" => vec!["*.js", "*.jsx", "*.mjs", "*.cjs"],
        "ts" => vec!["*.ts", "*.tsx"],
        "md" | "markdown" => vec!["*.md"],
        "toml" => vec!["*.toml"],
        "yaml" => vec!["*.yml", "*.yaml"],
        "json" => vec!["*.json"],
        "go" => vec!["*.go"],
        "sh" => vec!["*.sh", "*.bash", "*.zsh"],
        _ => return None,
    })
}

fn num(s: &str) -> Option<u32> {
    s.parse().ok()
}

/// grep/rg/ag/ack argv → search spec; None for flags or shapes whose output we don't reproduce.
pub fn search_spec(w: &[String], cwd: &Path, root: &Path, raw: &str) -> Option<SearchSpec> {
    let tool = w[0].as_str();
    let is_grep = matches!(tool, "grep" | "egrep" | "fgrep");
    let mut spec = SearchSpec {
        fixed: tool == "fgrep",
        cwd: cwd.to_string_lossy().into(),
        label: raw.to_string(),
        ..Default::default()
    };
    let mut extended = tool == "egrep" || !is_grep;
    let mut recursive = !is_grep;
    let mut explicit_patterns = false;
    let mut positional: Vec<String> = Vec::new();
    let mut i = 1;
    let mut end_of_flags = false;
    while i < w.len() {
        let a = w[i].as_str();
        i += 1;
        if end_of_flags || !a.starts_with('-') || a == "-" {
            if a == "-" {
                return None;
            }
            positional.push(a.to_string());
            continue;
        }
        if a == "--" {
            end_of_flags = true;
            continue;
        }
        let take = |i: &mut usize| -> Option<String> {
            let v = w.get(*i)?.clone();
            *i += 1;
            Some(v)
        };
        if let Some(long) = a.strip_prefix("--") {
            let (k, v) = match long.split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (long, None),
            };
            match k {
                "recursive" | "dereference-recursive" => recursive = true,
                "line-number" | "with-filename" | "no-heading" | "heading" | "no-messages"
                | "no-filename" | "color" | "colour" => {}
                "hidden" if !is_grep => spec.hidden = true,
                "ignore-case" => spec.ignore_case = true,
                "smart-case" => spec.smart_case = true,
                "case-sensitive" => {}
                "word-regexp" => spec.word = true,
                "fixed-strings" | "literal" => spec.fixed = true,
                "extended-regexp" => extended = true,
                "files-with-matches" => spec.files_only = true,
                "no-ignore" | "unrestricted" => spec.no_ignore = true,
                "include" | "glob" | "iglob" => spec.globs.push(v.or_else(|| take(&mut i))?),
                "exclude" => spec.globs.push(format!("!{}", v.or_else(|| take(&mut i))?)),
                "exclude-dir" => spec.globs.push(format!("!{}", v.or_else(|| take(&mut i))?)),
                "type" => {
                    for g in rg_type(&v.or_else(|| take(&mut i))?)? {
                        spec.globs.push(g.into());
                    }
                }
                "regexp" => {
                    spec.patterns.push(v.or_else(|| take(&mut i))?);
                    explicit_patterns = true;
                }
                "after-context" => spec.after = num(&v.or_else(|| take(&mut i))?)?,
                "before-context" => spec.before = num(&v.or_else(|| take(&mut i))?)?,
                "context" => {
                    let n = num(&v.or_else(|| take(&mut i))?)?;
                    spec.before = n;
                    spec.after = n;
                }
                _ => return None,
            }
            continue;
        }
        let flags: Vec<char> = a[1..].chars().collect();
        let mut j = 0;
        while j < flags.len() {
            let f = flags[j];
            j += 1;
            let attached: String = flags[j..].iter().collect();
            let value = |i: &mut usize| -> Option<String> {
                if !attached.is_empty() {
                    Some(attached.clone())
                } else {
                    take(i)
                }
            };
            match f {
                'r' | 'R' if is_grep => recursive = true,
                'n' | 'H' | 'h' | 's' | 'I' => {}
                'i' => spec.ignore_case = true,
                'S' if !is_grep => spec.smart_case = true,
                'w' => spec.word = true,
                'F' | 'Q' => spec.fixed = true,
                'E' => extended = true,
                'l' => spec.files_only = true,
                'u' if !is_grep => spec.no_ignore = true,
                '.' if !is_grep => spec.hidden = true,
                'e' => {
                    spec.patterns.push(value(&mut i)?);
                    explicit_patterns = true;
                    break;
                }
                'g' if !is_grep => {
                    spec.globs.push(value(&mut i)?);
                    break;
                }
                't' if !is_grep => {
                    for g in rg_type(&value(&mut i)?)? {
                        spec.globs.push(g.into());
                    }
                    break;
                }
                'A' | 'B' | 'C' => {
                    let n = num(&value(&mut i)?)?;
                    match f {
                        'A' => spec.after = n,
                        'B' => spec.before = n,
                        _ => {
                            spec.before = n;
                            spec.after = n;
                        }
                    }
                    break;
                }
                c if c.is_ascii_digit() && is_grep => {
                    let n = num(&a[1..])?;
                    spec.before = n;
                    spec.after = n;
                    break;
                }
                _ => return None,
            }
        }
    }
    if !explicit_patterns {
        if positional.is_empty() {
            return None;
        }
        spec.patterns.push(positional.remove(0));
    }
    if is_grep && !extended && !spec.fixed {
        spec.patterns = spec.patterns.iter().map(|p| bre_to_ere(p)).collect();
    }
    if positional.is_empty() {
        if !recursive {
            return None; // grep reading stdin
        }
        positional.push(".".into());
    }
    for p in &positional {
        let a = abs_path(cwd, p);
        if !a.exists() || !inside(root, &a) || (a.is_dir() && !recursive) {
            return None;
        }
        spec.paths.push(a.to_string_lossy().into());
    }
    Some(spec)
}

fn reader_files(w: &[String], cwd: &Path, root: &Path) -> Option<Vec<String>> {
    let tool = w[0].as_str();
    let mut files = Vec::new();
    let mut i = 1;
    if tool == "sed" {
        // Only `sed -n 'A,Bp' FILE...` style printing.
        let mut saw_n = false;
        let mut script = false;
        for a in &w[1..] {
            if a == "-n" {
                saw_n = true;
            } else if a.starts_with('-') {
                return None;
            } else if !script {
                if !a
                    .chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, ',' | 'p' | '$' | ';'))
                {
                    return None;
                }
                script = true;
            } else {
                files.push(a.clone());
            }
        }
        if !saw_n || !script {
            return None;
        }
    } else {
        while i < w.len() {
            let a = w[i].as_str();
            i += 1;
            let numeric = |s: &str| s.chars().all(|c| c.is_ascii_digit());
            match (tool, a) {
                ("head" | "tail", "-n" | "-c") => i += 1,
                ("head" | "tail", _) if a.starts_with('-') && numeric(&a[1..]) => {}
                ("head" | "tail", _) if a.starts_with("-n") && numeric(&a[2..]) => {}
                ("cat" | "nl", "-n" | "-b") => {}
                _ if a.starts_with('-') => return None,
                _ => files.push(a.to_string()),
            }
        }
    }
    if files.is_empty() {
        return None;
    }
    let mut any_py = false;
    let mut out = Vec::new();
    for f in &files {
        let a = abs_path(cwd, f);
        if !a.is_file() || !inside(root, &a) {
            return None;
        }
        any_py |= a.extension().is_some_and(|e| e == "py");
        out.push(a.to_string_lossy().into_owned());
    }
    any_py.then_some(out)
}

fn list_dirs(w: &[String], cwd: &Path, root: &Path) -> Option<Vec<String>> {
    let tool = w[0].as_str();
    let mut dirs = Vec::new();
    if tool == "find" {
        const MUTATING: &[&str] = &[
            "-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprintf", "-fls",
            "-fprint0",
        ];
        if w.iter().any(|a| MUTATING.contains(&a.as_str())) {
            return None;
        }
        for a in &w[1..] {
            if a.starts_with('-') || a == "!" || a == "(" {
                break;
            }
            dirs.push(a.clone());
        }
        if dirs.is_empty() {
            dirs.push(".".into());
        }
    } else {
        for a in &w[1..] {
            if !a.starts_with('-') {
                dirs.push(a.clone());
            }
        }
        if dirs.is_empty() {
            dirs.push(".".into());
        }
    }
    let mut out = Vec::new();
    for d in dirs {
        let a = abs_path(cwd, &d);
        if !inside(root, &a) {
            return None;
        }
        if a.is_dir() {
            out.push(a.to_string_lossy().into_owned());
        }
    }
    (!out.is_empty()).then_some(out)
}
