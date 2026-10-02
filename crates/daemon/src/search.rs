//! Embedded text search (ripgrep crates, in-process).
//!
//! Exclusion is by reason, not by leading dot: noise (VCS/Graphite state, dependency envs,
//! caches, build output, duplicate checkouts) is never entered; secrets are never opened, not
//! even to count, unless the command names that exact file; every other hidden file or dir
//! (`.github/`, tool configs) is searched like any other, respecting `.gitignore`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::sinks::Lossy;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::paths::{is_noise_dir, is_secret_name, NOISE_DIRS};

/// Hard stop so a pathological pattern can't exhaust memory; always disclosed.
pub const MAX_MATCHES: usize = 20_000;

/// A grep/rg/ack/ag invocation normalized by the hook; patterns are Rust-regex syntax unless `fixed`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchSpec {
    pub patterns: Vec<String>,
    #[serde(default)]
    pub fixed: bool,
    #[serde(default)]
    pub ignore_case: bool,
    #[serde(default)]
    pub smart_case: bool,
    #[serde(default)]
    pub word: bool,
    #[serde(default)]
    pub files_only: bool,
    #[serde(default)]
    pub before: u32,
    #[serde(default)]
    pub after: u32,
    /// Override globs in ripgrep syntax; a leading `!` excludes.
    #[serde(default)]
    pub globs: Vec<String>,
    /// Absolute files or directories to search.
    pub paths: Vec<String>,
    /// Directory the command ran in; display paths are relative to it.
    pub cwd: String,
    /// Disable .gitignore and noise excludes (rg -u / --no-ignore). Secrets stay unread.
    #[serde(default)]
    pub no_ignore: bool,
    /// rg --hidden / -. — accepted for compatibility; non-noise hidden paths are always searched.
    #[serde(default)]
    pub hidden: bool,
    /// grep would not name files (one file operand without -r/-H, or -h): lines are `N:text`.
    #[serde(default)]
    pub no_filename: bool,
    /// Print everything: no per-section caps.
    #[serde(default)]
    pub all: bool,
    /// The original command, echoed in the answer header and in the "see everything" hint.
    #[serde(default)]
    pub label: String,
    /// Which view of the answer to return.
    #[serde(default)]
    pub format: crate::answer::OutFormat,
    /// The agent's `| head …`, honored as an answer budget instead of a byte cut (`tail` runs raw).
    #[serde(default)]
    pub budget: Option<crate::answer::Budget>,
    /// The agent filtered test lines out (`| grep -v test`): drop test matches, disclose counts.
    #[serde(default)]
    pub drop_tests: Option<String>,
    /// Other grep-style filters from the agent's pipeline, applied to match lines only.
    #[serde(default)]
    pub line_filters: Vec<crate::answer::LineFilter>,
}

/// One matching line.
#[derive(Debug, Clone)]
pub struct Hit {
    pub abs: PathBuf,
    /// Path as the agent would see it (relative to the command's cwd when possible).
    pub display: String,
    pub line: u32,
    pub text: String,
    /// Byte column of the first match on the line.
    pub col: usize,
}

#[derive(Debug, Default)]
pub struct SearchOutcome {
    pub hits: Vec<Hit>,
    /// Absolute paths of every file searched.
    pub searched: HashSet<PathBuf>,
    /// Secret-looking files/dirs in scope that were skipped without being opened.
    pub secrets_skipped: usize,
    pub truncated: bool,
}

pub fn build_matcher(spec: &SearchSpec) -> Result<RegexMatcher, String> {
    if spec.patterns.is_empty() {
        return Err("no pattern".into());
    }
    let mut b = RegexMatcherBuilder::new();
    b.case_insensitive(spec.ignore_case)
        .case_smart(spec.smart_case && !spec.ignore_case)
        .word(spec.word)
        .fixed_strings(spec.fixed);
    if spec.patterns.len() == 1 {
        b.build(&spec.patterns[0])
    } else {
        b.build_many(&spec.patterns)
    }
    .map_err(|e| format!("pattern: {e}"))
}

/// Matches only; context lines are rendered from the file text by the caller.
fn searcher() -> Searcher {
    SearcherBuilder::new()
        .binary_detection(BinaryDetection::quit(b'\x00'))
        .line_number(true)
        .build()
}

/// Path as the agent would see it: relative to the command's cwd when inside it.
pub fn display_path(abs: &Path, cwd: &Path) -> String {
    match abs.strip_prefix(cwd) {
        Ok(rel) if !rel.as_os_str().is_empty() => rel.to_string_lossy().into_owned(),
        _ => abs.to_string_lossy().into_owned(),
    }
}

/// True if `rel` (repo-relative) lies inside a noise location — the command named it on purpose.
pub fn under_noise(rel: &Path) -> bool {
    let comps: Vec<String> = rel
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(n) => Some(n.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    comps.iter().any(|c| NOISE_DIRS.contains(&c.as_str()))
        || comps
            .windows(2)
            .any(|w| w[0] == ".claude" && w[1] == "worktrees")
}

/// Matching lines of one file, in line order.
fn search_file(matcher: &RegexMatcher, abs: &Path, cwd: &Path) -> Vec<Hit> {
    let display = display_path(abs, cwd);
    let mut hits = Vec::new();
    let _ = searcher().search_path(
        matcher,
        abs,
        Lossy(|lnum, line| {
            let text = line.trim_end_matches(['\n', '\r']).to_string();
            let col = matcher
                .find(text.as_bytes())
                .ok()
                .flatten()
                .map(|m| m.start())
                .unwrap_or(0);
            hits.push(Hit {
                abs: abs.to_path_buf(),
                display: display.clone(),
                line: lnum as u32,
                text,
                col,
            });
            Ok(hits.len() < MAX_MATCHES)
        }),
    );
    hits
}

/// Walk `root`. `gitignore` applies .gitignore/.ignore; `skip_noise` skips noise dirs.
/// Secret-looking entries are always skipped unopened and counted.
fn walker(
    root: &Path,
    spec: &SearchSpec,
    gitignore: bool,
    skip_noise: bool,
    secrets: Arc<AtomicUsize>,
) -> ignore::Walk {
    let mut b = WalkBuilder::new(root);
    b.standard_filters(gitignore)
        .hidden(false)
        .require_git(false)
        .sort_by_file_name(|a, b| a.cmp(b));
    if !spec.globs.is_empty() {
        let mut ob = OverrideBuilder::new(root);
        for g in &spec.globs {
            let _ = ob.add(g);
        }
        if let Ok(ov) = ob.build() {
            b.overrides(ov);
        }
    }
    b.filter_entry(move |e| {
        if e.depth() == 0 {
            return true;
        }
        let name = e.file_name().to_string_lossy();
        if name == ".git" {
            return false; // VCS dir, or a worktree's `.git` pointer file
        }
        if is_secret_name(&name) {
            secrets.fetch_add(1, Ordering::Relaxed);
            return false;
        }
        let is_dir = e.file_type().is_some_and(|t| t.is_dir());
        !(skip_noise && is_dir && is_noise_dir(e.path(), &name, e.depth()))
    });
    b.build()
}

/// Run the search under `repo_root`. Paths the command names inside noise dirs are searched.
pub fn run(spec: &SearchSpec, repo_root: &Path) -> Result<SearchOutcome, String> {
    let matcher = build_matcher(spec)?;
    let cwd = PathBuf::from(&spec.cwd);
    let mut out = SearchOutcome::default();
    let secrets = Arc::new(AtomicUsize::new(0));
    let mut files: Vec<PathBuf> = Vec::new();
    for p in &spec.paths {
        let root = PathBuf::from(p);
        if root.is_file() {
            files.push(root);
            continue;
        }
        if !root.is_dir() {
            continue;
        }
        let rel = root.strip_prefix(repo_root).unwrap_or(&root);
        // A path the command names inside a noise location is searched on purpose.
        let skip_noise = !spec.no_ignore && !under_noise(rel);
        files.extend(
            walker(&root, spec, !spec.no_ignore, skip_noise, secrets.clone())
                .flatten()
                .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
                .map(|e| e.into_path()),
        );
    }
    // Search in parallel, keep walk order for output.
    let per_file: Vec<Vec<Hit>> = files
        .par_iter()
        .map(|f| search_file(&matcher, f, &cwd))
        .collect();
    out.searched = files.into_iter().collect();
    for hits in per_file {
        let room = MAX_MATCHES.saturating_sub(out.hits.len());
        if hits.len() > room {
            out.hits.extend(hits.into_iter().take(room));
            out.truncated = true;
            break;
        }
        out.hits.extend(hits);
    }
    out.secrets_skipped = secrets.load(Ordering::Relaxed);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(root: &Path, pat: &str) -> SearchSpec {
        SearchSpec {
            patterns: vec![pat.into()],
            paths: vec![root.to_string_lossy().into()],
            cwd: root.to_string_lossy().into(),
            ..Default::default()
        }
    }

    #[test]
    fn skips_noise_dirs_unless_targeted() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".venv/lib")).unwrap();
        std::fs::create_dir_all(root.join("pkg/__pycache__")).unwrap();
        std::fs::write(root.join(".venv/lib/x.py"), "needle\n").unwrap();
        std::fs::write(root.join("pkg/__pycache__/a.py"), "needle\n").unwrap();
        std::fs::write(root.join("pkg/a.py"), "x = 1\nneedle()\n").unwrap();
        let out = run(&spec(&root, "needle"), &root).unwrap();
        let shown: Vec<&str> = out.hits.iter().map(|h| h.display.as_str()).collect();
        assert_eq!(shown, vec!["pkg/a.py"]);
        assert_eq!(out.hits[0].line, 2);

        // A path the command names inside a noise dir is searched.
        let mut s = spec(&root, "needle");
        s.paths = vec![root.join(".venv").to_string_lossy().into()];
        assert_eq!(run(&s, &root).unwrap().hits.len(), 1);
    }

    #[test]
    fn duplicate_checkouts_are_noise_other_hidden_paths_are_searched() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        // .claude/worktrees copies and any git worktree (a dir with a `.git` FILE).
        std::fs::create_dir_all(root.join(".claude/worktrees/a/pkg")).unwrap();
        std::fs::write(root.join(".claude/worktrees/a/pkg/a.py"), "needle()\n").unwrap();
        std::fs::create_dir_all(root.join("wt/pkg")).unwrap();
        std::fs::write(root.join("wt/.git"), "gitdir: /elsewhere\n").unwrap();
        std::fs::write(root.join("wt/pkg/a.py"), "needle()\n").unwrap();
        // Relevant hidden paths: CI, tool config, hidden agent dirs.
        std::fs::create_dir_all(root.join(".github/workflows")).unwrap();
        std::fs::write(root.join(".github/workflows/ci.yml"), "run: needle\n").unwrap();
        std::fs::write(root.join(".pre-commit-config.yaml"), "id: needle\n").unwrap();
        std::fs::create_dir_all(root.join("pkg")).unwrap();
        std::fs::write(root.join("pkg/a.py"), "needle()\n").unwrap();

        let out = run(&spec(&root, "needle"), &root).unwrap();
        let shown: Vec<&str> = out.hits.iter().map(|h| h.display.as_str()).collect();
        assert_eq!(
            shown,
            vec![
                ".github/workflows/ci.yml",
                ".pre-commit-config.yaml",
                "pkg/a.py"
            ],
            "checkout copies skipped; CI/config searched"
        );

        let mut s = spec(&root, "needle");
        s.paths = vec![root.join(".claude/worktrees").to_string_lossy().into()];
        assert_eq!(
            run(&s, &root).unwrap().hits.len(),
            1,
            "named copy is searched"
        );
    }

    /// A FIFO blocks forever when opened for reading: the search finishing proves it was not opened.
    fn fifo(p: &Path) {
        let ok = std::process::Command::new("mkfifo")
            .arg(p)
            .status()
            .unwrap();
        assert!(ok.success());
    }

    #[test]
    fn secrets_are_never_opened_even_with_no_ignore() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("cfg/.envs")).unwrap();
        std::fs::create_dir_all(root.join("pkg")).unwrap();
        fifo(&root.join(".env"));
        fifo(&root.join("cfg/.envs/prod"));
        fifo(&root.join("pkg/server.pem"));
        fifo(&root.join("pkg/credentials.json"));
        std::fs::write(root.join("pkg/a.py"), "needle()\n").unwrap();
        for no_ignore in [false, true] {
            let mut s = spec(&root, "needle");
            s.no_ignore = no_ignore;
            let r2 = root.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || tx.send(run(&s, &r2).unwrap()).unwrap());
            let out = rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .expect("search opened a secret file (blocked on a FIFO)");
            assert_eq!(out.hits.len(), 1);
            assert_eq!(out.secrets_skipped, 4, "no_ignore={no_ignore}");
        }
        // Named exactly by the command: read.
        std::fs::remove_file(root.join(".env")).unwrap();
        std::fs::write(root.join(".env"), "needle=1\n").unwrap();
        let mut s = spec(&root, "needle");
        s.paths = vec![root.join(".env").to_string_lossy().into()];
        assert_eq!(run(&s, &root).unwrap().hits.len(), 1);
    }

    #[test]
    fn same_query_twice_is_identical() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        for i in 0..40 {
            std::fs::create_dir_all(root.join(format!("m{i}"))).unwrap();
            std::fs::write(root.join(format!("m{i}/a.py")), "needle()\nneedle\n").unwrap();
        }
        let a = run(&spec(&root, "needle"), &root).unwrap();
        let b = run(&spec(&root, "needle"), &root).unwrap();
        let key = |o: &SearchOutcome| {
            o.hits
                .iter()
                .map(|h| format!("{}:{}:{}", h.display, h.line, h.text))
                .collect::<Vec<_>>()
        };
        assert_eq!(key(&a), key(&b));
    }

    #[test]
    fn include_globs_filter_files() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        std::fs::write(root.join("a.py"), "needle\n").unwrap();
        std::fs::write(root.join("b.md"), "needle\n").unwrap();
        let mut s = spec(&root, "needle");
        s.globs = vec!["*.py".into()];
        let out = run(&s, &root).unwrap();
        assert_eq!(out.hits.len(), 1);
        assert_eq!(out.hits[0].display, "a.py");
    }
}
