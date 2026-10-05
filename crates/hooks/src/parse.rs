//! Turn one shell segment into something Graphite can answer, or refuse.

use std::path::{Path, PathBuf};

use graphite_daemon::answer::{Budget, LineFilter};
use graphite_daemon::SearchSpec;

use crate::shell::{references, split, words, Env, Segment};

/// What a segment becomes.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// grep/rg/ag/ack answered from embedded search + graph judgment. Budget, test and grep
    /// filters from the pipeline are folded into `spec`; `filters` are the remaining stages,
    /// run for real over our text.
    Search {
        spec: Box<SearchSpec>,
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
    /// `NAME=$(inner)`: the read-only `inner` runs for real and its output is stored.
    Assign { name: String, inner: String },
    /// A read-only command that uses a variable assigned earlier in the command. Its paths are
    /// only known at run time, so it is classified again once the variable is set.
    Deferred { answerable: bool },
}

/// Downstream pipeline stages that only read stdin and write stdout.
const SAFE_FILTERS: &[&str] = &[
    "head", "tail", "grep", "egrep", "fgrep", "rg", "sort", "uniq", "wc", "cut", "tr", "jq", "cat",
    "column", "nl",
];

/// Downstream stages that compute over raw matches; the whole segment then runs verbatim.
const TRANSFORMS: &[&str] = &["wc", "sort", "uniq", "cut", "tr", "column", "nl", "jq"];

/// What a pipeline stage after a search means for the answer.
#[derive(Debug, Clone, PartialEq)]
pub enum Stage {
    /// `head -c N`, `head -n N`, `head -N`: an answer budget, not a cut.
    Budget(Budget),
    /// `grep -v test` and friends: drop test matches semantically.
    DropTests(String),
    /// A plain grep filter we apply to match lines ourselves (header/footer kept).
    Line(LineFilter),
    /// Computation over the raw output: run the original command.
    Transform,
    /// `cat`: a no-op.
    Noop,
    /// Anything else: run for real over our text.
    Real,
}

const TESTISH: &[&str] = &[
    "test", "tests", "/tests/", "tests/", "/test/", "test/", "_test", "test_", "/tests", "/test",
];

fn budget_of(w: &[String]) -> Option<Budget> {
    let tool = w[0].as_str();
    let source = w.join(" ");
    let n = |s: &str| s.parse::<usize>().ok().filter(|v| *v > 0);
    let mut out = Budget {
        source,
        ..Default::default()
    };
    match w.len() {
        1 => out.lines = Some(10),
        2 => {
            let a = w[1].as_str();
            if let Some(v) = a.strip_prefix("--bytes=") {
                out.bytes = Some(n(v)?);
            } else if let Some(v) = a.strip_prefix("--lines=") {
                out.lines = Some(n(v)?);
            } else if let Some(v) = a.strip_prefix("-c") {
                out.bytes = Some(n(v)?);
            } else if let Some(v) = a.strip_prefix("-n") {
                out.lines = Some(n(v)?);
            } else if let Some(v) = a.strip_prefix('-') {
                out.lines = Some(n(v)?);
            } else {
                return None;
            }
        }
        3 => match w[1].as_str() {
            "-c" | "--bytes" => out.bytes = Some(n(&w[2])?),
            "-n" | "--lines" => out.lines = Some(n(&w[2])?),
            _ => return None,
        },
        _ => return None,
    }
    let _ = tool;
    Some(out)
}

fn grep_filter(w: &[String], raw: &str) -> Option<LineFilter> {
    let tool = w[0].as_str();
    let mut f = LineFilter {
        source: raw.trim().to_string(),
        fixed: tool == "fgrep",
        ..Default::default()
    };
    let mut extended = tool != "grep";
    let mut pat: Option<String> = None;
    let mut i = 1;
    while i < w.len() {
        let a = w[i].as_str();
        i += 1;
        if a == "-e" {
            pat = Some(w.get(i)?.clone());
            i += 1;
            continue;
        }
        if let Some(flags) = a
            .strip_prefix('-')
            .filter(|f| !f.is_empty() && !a.starts_with("--"))
        {
            for c in flags.chars() {
                match c {
                    'v' => f.invert = true,
                    'i' => f.ignore_case = true,
                    'E' => extended = true,
                    'F' => f.fixed = true,
                    'w' => f.word = true,
                    _ => return None,
                }
            }
            continue;
        }
        if a.starts_with('-') || pat.is_some() {
            return None; // long flags or file operands: not a plain stdin filter
        }
        pat = Some(a.to_string());
    }
    let p = pat?;
    f.pattern = if tool == "grep" && !extended && !f.fixed {
        bre_to_ere(&p)
    } else {
        p
    };
    Some(f)
}

/// Classify a pipeline stage that reads a search's output.
pub fn stage(st: &str) -> Stage {
    let Some(w) = words(st) else {
        return Stage::Real;
    };
    let Some(cmd) = w.first().map(String::as_str) else {
        return Stage::Real;
    };
    match cmd {
        "head" => budget_of(&w).map_or(Stage::Real, Stage::Budget),
        // `tail` asks for the END of grep's file-ordered output; a relevance-ordered answer has no
        // meaningful end, so the original command runs untouched (no enrichment, no surprise).
        "tail" => Stage::Transform,
        "cat" if w.len() == 1 => Stage::Noop,
        "grep" | "egrep" | "fgrep" | "rg" => match grep_filter(&w, st) {
            Some(f)
                if f.invert
                    && TESTISH.iter().any(|t| {
                        f.pattern
                            .trim_matches(|c| c == '\'')
                            .eq_ignore_ascii_case(t)
                    }) =>
            {
                Stage::DropTests(st.trim().to_string())
            }
            Some(f) => Stage::Line(f),
            None => Stage::Real,
        },
        c if TRANSFORMS.contains(&c) => Stage::Transform,
        _ => Stage::Real,
    }
}

/// Commands allowed to run untouched next to answered segments.
const PLAIN_OK: &[&str] = &["echo", "printf", "pwd", "true", "wc"];

/// `git` subcommands that only read the repository (no flag of theirs writes a file).
const GIT_READ: &[&str] = &[
    "status",
    "diff",
    "log",
    "show",
    "rev-parse",
    "ls-files",
    "blame",
    "grep",
];

/// Read-only commands whose operands may come from a variable assigned earlier in the command.
const DEFERRABLE: &[&str] = &[
    "cat", "head", "tail", "nl", "sed", "grep", "egrep", "fgrep", "rg", "ls", "find", "wc", "echo",
    "printf", "file", "stat",
];

/// Deferred commands that become a Graphite answer once their paths are known.
const ANSWERABLE: &[&str] = &[
    "cat", "head", "tail", "nl", "sed", "grep", "egrep", "fgrep", "rg", "ls", "find",
];

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

/// A path operand as the shell sees it: globs (`*`, `?`, `[`) expanded against `cwd`, sorted.
/// None when a glob matches nothing (zsh refuses to run it; sh passes it literally to a command
/// that then fails) — the caller then leaves the segment to the shell.
fn operands(cwd: &Path, p: &str) -> Option<Vec<String>> {
    if !p.contains(['*', '?', '[']) {
        return Some(vec![p.to_string()]);
    }
    let pattern = if Path::new(p).is_absolute() {
        p.to_string()
    } else {
        cwd.join(p).to_string_lossy().into_owned()
    };
    let mut out: Vec<String> = glob::glob(&pattern)
        .ok()?
        .filter_map(Result::ok)
        .map(|m| m.to_string_lossy().into_owned())
        .collect();
    out.sort();
    (!out.is_empty()).then_some(out)
}

/// Nearest existing directory above a missing path, strictly inside the repo (never the root
/// itself: that would turn a narrow search into a repo-wide one).
fn existing_parent(missing: &Path, root: &Path) -> Option<PathBuf> {
    let mut cur = missing.parent()?;
    loop {
        if cur.is_dir() {
            let c = cur.canonicalize().ok()?;
            return (c != root && inside(root, &c)).then_some(c);
        }
        cur = cur.parent()?;
    }
}

/// `p` relative to `cwd` when below it, else as is.
fn display(p: &Path, cwd: &Path) -> String {
    let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    match p.strip_prefix(&cwd) {
        Ok(r) if r.as_os_str().is_empty() => ".".into(),
        Ok(r) => r.to_string_lossy().into_owned(),
        Err(_) => p.to_string_lossy().into_owned(),
    }
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

/// `git` invocations that cannot change the repository or write files.
fn git_read_only(w: &[String]) -> bool {
    let Some(sub) = w.get(1).map(String::as_str) else {
        return false;
    };
    let writes = |a: &String| a.starts_with("--output") || a == "-o" || a.starts_with("--ext-diff");
    if w.iter().any(writes) {
        return false;
    }
    match sub {
        "stash" => w.get(2).is_some_and(|a| a == "list"),
        "branch" => w[2..].iter().all(|a| {
            matches!(
                a.as_str(),
                "-a" | "-r" | "-v" | "-vv" | "--list" | "--show-current" | "--all" | "--remotes"
            )
        }),
        s => GIT_READ.contains(&s),
    }
}

/// A segment that uses an earlier variable: acceptable now if its command is read-only whatever
/// the variable holds; classified for real at run time.
fn deferred(seg: &Segment) -> Option<Action> {
    let first = strip_wrappers(words(&seg.stages[0])?);
    let cmd = first.first()?.as_str();
    if cmd.contains('$') || !DEFERRABLE.contains(&cmd) {
        return None;
    }
    let read_only = match cmd {
        "sed" => first.iter().any(|a| a == "-n") && !first.iter().any(|a| a.starts_with("-i")),
        "find" => !first.iter().any(|a| FIND_MUTATING.contains(&a.as_str())),
        _ => true,
    };
    let filters_ok = seg.stages[1..].iter().all(|s| safe_filter(s));
    (read_only && filters_ok).then(|| Action::Deferred {
        answerable: ANSWERABLE.contains(&cmd) && !seg.discard_stdout,
    })
}

/// Classify one segment. `root` is the repo the daemon serves.
pub fn classify(seg: &Segment, cwd: &Path, root: &Path) -> Option<Action> {
    let action = classify_command(seg, cwd, root)?;
    // Output thrown away: nothing reaches the agent, so there is nothing to answer.
    Some(match action {
        Action::Search { .. } | Action::Read { .. } | Action::List { .. } if seg.discard_stdout => {
            Action::Plain
        }
        a => a,
    })
}

fn classify_command(seg: &Segment, cwd: &Path, root: &Path) -> Option<Action> {
    let first = strip_wrappers(words(&seg.stages[0])?);
    let rest = &seg.stages[1..];
    let filters_ok = rest.iter().all(|s| safe_filter(s));
    answerable(seg, &first, cwd, root, filters_ok)
        // A read-only command Graphite cannot answer (a path that does not exist, a glob with no
        // match, a flag whose output we don't reproduce) still runs as written next to the
        // answered segments instead of sending the whole command around Graphite.
        .or_else(|| (filters_ok && read_only(&first)).then_some(Action::Plain))
}

/// Commands that only read, whatever their operands: no flag of theirs writes a file or runs a
/// program.
pub fn read_only(w: &[String]) -> bool {
    let Some(cmd) = w.first().map(String::as_str) else {
        return false;
    };
    match cmd {
        "grep" | "egrep" | "fgrep" | "cat" | "head" | "tail" | "nl" | "wc" | "ls" | "file"
        | "stat" => true,
        // `--pre` runs a program per file.
        "rg" => !w.iter().any(|a| a.starts_with("--pre")),
        "find" => !w.iter().any(|a| FIND_MUTATING.contains(&a.as_str())),
        "sed" => sed_prints_only(w),
        _ => false,
    }
}

/// `sed -n` with no in-place flag and no script command that writes (`w`/`W`) or executes (`e`).
fn sed_prints_only(w: &[String]) -> bool {
    let mut saw_n = false;
    let mut script_seen = false;
    for a in &w[1..] {
        if a == "-n" || a == "--quiet" || a == "--silent" {
            saw_n = true;
        } else if a.starts_with("-i") || a.starts_with("--in-place") || a == "-e" || a == "-f" {
            return false;
        } else if a.starts_with('-') {
            if a != "-E" && a != "-r" {
                return false;
            }
        } else if !script_seen {
            script_seen = true;
            // Outside a /regex/, these letters are the commands that write or execute.
            let mut in_re = false;
            for c in a.chars() {
                match c {
                    '/' => in_re = !in_re,
                    'w' | 'W' | 'e' | 'r' | 'R' if !in_re => return false,
                    _ => {}
                }
            }
        }
    }
    saw_n && script_seen
}

/// A segment Graphite answers (or a known read-only companion of one).
fn answerable(
    seg: &Segment,
    first: &[String],
    cwd: &Path,
    root: &Path,
    filters_ok: bool,
) -> Option<Action> {
    let first = first.to_vec();
    let cmd = first.first()?.as_str();
    let rest = &seg.stages[1..];
    match cmd {
        "grep" | "egrep" | "fgrep" | "rg" | "ag" | "ack" => {
            let mut spec = search_spec(&first, cwd, root, &seg.stages[0])?;
            if !filters_ok {
                return None;
            }
            let mut filters = Vec::new();
            for st in rest {
                match stage(st) {
                    // A computation over the raw matches (count, sort, cut…): run it verbatim.
                    Stage::Transform => return Some(Action::Plain),
                    Stage::Noop => {}
                    Stage::Budget(b) => spec.budget = Some(b),
                    Stage::DropTests(src) if filters.is_empty() => spec.drop_tests = Some(src),
                    Stage::Line(f) if filters.is_empty() => spec.line_filters.push(f),
                    _ => filters.push(st.clone()),
                }
            }
            Some(Action::Search {
                spec: Box::new(spec),
                filters,
            })
        }
        "cat" | "head" | "tail" | "nl" | "sed" => {
            let (files, any_py) = reader_files(&first, cwd, root)?;
            // A read of non-Python files (pyproject.toml, docs) has no graph header to add, but
            // it is still read-only: run it as written next to the answered segments.
            filters_ok.then_some(if any_py {
                Action::Read { files }
            } else {
                Action::Plain
            })
        }
        "ls" | "find" => {
            let dirs = list_dirs(&first, cwd, root)?;
            filters_ok.then_some(Action::List { dirs })
        }
        "cd" if seg.stages.len() == 1 && first.len() == 2 => {
            let d = abs_path(cwd, &first[1]);
            // A `cd` that fails changes nothing: the shell prints its error, the directory stays
            // and a following `&&` is skipped (the segment's exit status says so).
            Some(if d.is_dir() { Action::Cd(d) } else { Action::Plain })
        }
        "graphite"
            if first
                .get(1)
                .is_some_and(|s| GRAPHITE_READ.contains(&s.as_str())) =>
        {
            filters_ok.then_some(Action::Plain)
        }
        "git" if git_read_only(&first) => filters_ok.then_some(Action::Plain),
        c if PLAIN_OK.contains(&c) => filters_ok.then_some(Action::Plain),
        _ => None,
    }
}

/// True if at least one segment is (or may become, once its variable is set) a Graphite answer.
pub fn answers(actions: &[Action]) -> bool {
    actions.iter().any(|a| {
        matches!(
            a,
            Action::Search { .. }
                | Action::Read { .. }
                | Action::List { .. }
                | Action::Deferred { answerable: true }
        )
    })
}

/// `NAME=$(inner)` is supported when `inner` is one read-only segment with no variables.
fn assignment(seg: &Segment, cwd: &Path, root: &Path, env: &Env) -> Option<Action> {
    let a = seg.assign.as_ref()?;
    let inner = split(&a.inner)?;
    let [one] = inner.as_slice() else {
        return None;
    };
    if one.assign.is_some() || !references(&a.inner, env).is_empty() {
        return None;
    }
    match classify(one, cwd, root)? {
        Action::Cd(_) | Action::Assign { .. } | Action::Deferred { .. } => None,
        _ => Some(Action::Assign {
            name: a.name.clone(),
            inner: a.inner.clone(),
        }),
    }
}

/// Classify every segment, following `cd`s and `NAME=$(…)` assignments; None if any segment is
/// unsupported. Values of assigned variables are unknown here: segments using them are deferred.
pub fn plan(segs: &[Segment], cwd: &Path, root: &Path) -> Option<Vec<Action>> {
    let mut dir = cwd.to_path_buf();
    let mut env = Env::new();
    let mut out = Vec::new();
    for s in segs {
        let a = if s.assign.is_some() {
            let a = assignment(s, &dir, root, &env)?;
            if let Action::Assign { name, .. } = &a {
                env.insert(name.clone(), String::new());
            }
            a
        } else if !references(&s.raw, &env).is_empty() {
            deferred(s)?
        } else {
            classify(s, &dir, root)?
        };
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
    let (mut force_names, mut hide_names) = (false, false);
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
                "with-filename" => force_names = true,
                "no-filename" => hide_names = true,
                "line-number" | "no-heading" | "heading" | "no-messages" | "color" | "colour" => {}
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
                'H' => force_names = true,
                'h' => hide_names = true,
                'n' | 's' | 'I' => {}
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
    let globbed = positional.iter().any(|p| p.contains(['*', '?', '[']));
    for p in &positional {
        for o in operands(cwd, p)? {
            let a = abs_path(cwd, &o);
            if !a.exists() && recursive {
                // A recursive search of a directory that isn't there (`refiner/src` when the
                // package is `refiner/refiner`): search its nearest existing parent and say so,
                // instead of an error that costs the agent a turn to recover from.
                let anc = existing_parent(&a, root)?;
                spec.path_notes.push(format!(
                    "`{o}` does not exist; searched `{}/`, its nearest existing parent",
                    display(&anc, cwd)
                ));
                spec.paths.push(anc.to_string_lossy().into());
                continue;
            }
            if !a.exists() || !inside(root, &a) || (a.is_dir() && !recursive) {
                return None;
            }
            spec.paths.push(a.to_string_lossy().into());
        }
    }
    // grep names files for several operands, a directory, or -r; rg for anything but one file.
    let one_file = spec.paths.len() == 1 && !globbed && Path::new(&spec.paths[0]).is_file();
    let grep_recursive = is_grep && recursive;
    spec.no_filename = hide_names || (!force_names && one_file && !grep_recursive);
    Some(spec)
}

/// Files a reader prints, and whether any of them is Python (the only files with a graph header).
fn reader_files(w: &[String], cwd: &Path, root: &Path) -> Option<(Vec<String>, bool)> {
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
        for o in operands(cwd, f)? {
            let a = abs_path(cwd, &o);
            if !a.is_file() || !inside(root, &a) {
                return None;
            }
            any_py |= a.extension().is_some_and(|e| e == "py");
            out.push(a.to_string_lossy().into_owned());
        }
    }
    Some((out, any_py))
}

/// `find` primaries that run commands or write files.
const FIND_MUTATING: &[&str] = &[
    "-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprintf", "-fls", "-fprint0",
];

fn list_dirs(w: &[String], cwd: &Path, root: &Path) -> Option<Vec<String>> {
    let tool = w[0].as_str();
    let mut dirs = Vec::new();
    if tool == "find" {
        if w.iter().any(|a| FIND_MUTATING.contains(&a.as_str())) {
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
        for o in operands(cwd, &d)? {
            let a = abs_path(cwd, &o);
            if !inside(root, &a) {
                return None;
            }
            if a.is_dir() {
                out.push(a.to_string_lossy().into_owned());
            }
        }
    }
    (!out.is_empty()).then_some(out)
}
