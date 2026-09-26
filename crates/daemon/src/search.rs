//! Embedded text search (ripgrep crates, in-process) with the indexer's excludes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::sinks::Lossy;
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};
use ignore::overrides::OverrideBuilder;
use ignore::WalkBuilder;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

use crate::paths::DEFAULT_EXCLUDES;

/// Hard stop so a pathological pattern can't exhaust memory; always disclosed.
pub const MAX_MATCHES: usize = 20_000;

/// Time spent counting matches inside excluded dirs, for the "omitted" disclosure.
const EXCLUDED_SCAN_BUDGET: Duration = Duration::from_millis(100);

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
    /// Disable .gitignore and default excludes (rg -u / --no-ignore).
    #[serde(default)]
    pub no_ignore: bool,
    /// Print everything: no per-section caps.
    #[serde(default)]
    pub all: bool,
    /// The original command, echoed in the answer header and in the "see everything" hint.
    #[serde(default)]
    pub label: String,
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
    /// Excluded directories encountered under the search roots.
    pub excluded_dirs: Vec<PathBuf>,
    /// Matches found inside `excluded_dirs` within the scan budget.
    pub excluded_matches: u64,
    /// False if the budget ran out before every excluded dir was scanned.
    pub excluded_scan_complete: bool,
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

/// True if any component of `p` is a default-excluded directory name.
pub fn under_excluded(p: &Path) -> bool {
    p.components()
        .any(|c| DEFAULT_EXCLUDES.contains(&c.as_os_str().to_string_lossy().as_ref()))
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

fn walker(
    root: &Path,
    spec: &SearchSpec,
    filtered: bool,
    skipped: Arc<Mutex<Vec<PathBuf>>>,
) -> ignore::Walk {
    let mut b = WalkBuilder::new(root);
    b.standard_filters(filtered)
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
    if filtered {
        b.filter_entry(move |e| {
            let is_dir = e.file_type().is_some_and(|t| t.is_dir());
            let name = e.file_name().to_string_lossy();
            if is_dir && e.depth() > 0 && DEFAULT_EXCLUDES.contains(&name.as_ref()) {
                // VCS and Graphite state never hold matches worth disclosing.
                if name != ".git" && name != crate::paths::STATE_DIR {
                    skipped.lock().unwrap().push(e.path().to_path_buf());
                }
                return false;
            }
            true
        });
    }
    b.build()
}

/// Run the search under `repo_root`. Explicit paths into excluded locations are searched unfiltered.
pub fn run(spec: &SearchSpec, repo_root: &Path) -> Result<SearchOutcome, String> {
    let matcher = build_matcher(spec)?;
    let cwd = PathBuf::from(&spec.cwd);
    let mut out = SearchOutcome::default();
    let skipped = Arc::new(Mutex::new(Vec::new()));
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
        let filtered = !spec.no_ignore && !under_excluded(rel);
        files.extend(
            walker(&root, spec, filtered, skipped.clone())
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
    out.excluded_dirs = std::mem::take(&mut *skipped.lock().unwrap());
    count_excluded(spec, &matcher, &mut out);
    Ok(out)
}

/// Count matches the default excludes hid, within a small time budget.
fn count_excluded(spec: &SearchSpec, matcher: &RegexMatcher, out: &mut SearchOutcome) {
    out.excluded_scan_complete = true;
    if out.excluded_dirs.is_empty() {
        return;
    }
    let deadline = Instant::now() + EXCLUDED_SCAN_BUDGET;
    let mut s = searcher();
    let none = Arc::new(Mutex::new(Vec::new()));
    'dirs: for d in &out.excluded_dirs {
        for e in walker(d, spec, false, none.clone()).flatten() {
            if Instant::now() > deadline {
                out.excluded_scan_complete = false;
                break 'dirs;
            }
            if !e.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let mut n = 0u64;
            let _ = s.search_path(
                matcher,
                e.path(),
                Lossy(|_, _| {
                    n += 1;
                    Ok(true)
                }),
            );
            out.excluded_matches += n;
        }
    }
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
    fn skips_default_excludes_but_counts_them() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".venv/lib")).unwrap();
        std::fs::create_dir_all(root.join("pkg")).unwrap();
        std::fs::write(root.join(".venv/lib/x.py"), "needle\n").unwrap();
        std::fs::write(root.join("pkg/a.py"), "x = 1\nneedle()\n").unwrap();
        let out = run(&spec(&root, "needle"), &root).unwrap();
        assert_eq!(out.hits.len(), 1);
        assert_eq!(out.hits[0].display, "pkg/a.py");
        assert_eq!(out.hits[0].line, 2);
        assert_eq!(out.excluded_matches, 1);

        // An explicit path into an excluded dir is searched.
        let mut s = spec(&root, "needle");
        s.paths = vec![root.join(".venv").to_string_lossy().into()];
        assert_eq!(run(&s, &root).unwrap().hits.len(), 1);
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
