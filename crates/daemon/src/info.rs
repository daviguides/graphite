//! Short graph headers the hooks attach to file reads, directory listings and native searches.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use graphite_model::{SymbolId, SymbolKind};
use graphite_store::GraphStore;

use crate::engine::Engine;
use crate::judge::{name_facts, short_label};
use crate::search::display_path;

const TOP_SYMBOLS: usize = 8;
const DIR_BUDGET: Duration = Duration::from_millis(150);
const DIR_MAX_FILES: usize = 80;

fn rel_of(engine: &Engine, cwd: &Path, p: &str) -> Option<(PathBuf, String)> {
    let abs = if Path::new(p).is_absolute() {
        PathBuf::from(p)
    } else {
        cwd.join(p)
    };
    let abs = abs.canonicalize().unwrap_or(abs);
    let rel = engine.paths.relative(&abs)?;
    Some((abs, rel))
}

fn callers_count(engine: &Engine, id: SymbolId) -> usize {
    let adj = engine.adjacency();
    adj.callers_of(id)
        .iter()
        .filter(|(_, kind, _)| *kind != graphite_model::EdgeKind::Contains)
        .map(|(src, _, _)| *src)
        .collect::<HashSet<_>>()
        .len()
}

/// One symbol of a file header: richest form first.
struct HeaderPart {
    with_tests: String,
    bare: String,
}

/// `[graphite] path — N symbols; most used: …` fitted to `max` bytes: test names go first, then
/// the least-used symbols (counted in `+N more`); the busiest symbol always stays.
fn fit_header(prefix: &str, parts: &[HeaderPart], total: usize, max: Option<usize>) -> String {
    let render = |shown: usize, tests: usize| -> String {
        let listed: Vec<&str> = parts
            .iter()
            .take(shown)
            .enumerate()
            .map(|(i, p)| {
                if i < tests {
                    p.with_tests.as_str()
                } else {
                    p.bare.as_str()
                }
            })
            .collect();
        let more = total - shown;
        format!(
            "{prefix}{}{}",
            if listed.is_empty() {
                "none".to_string()
            } else {
                listed.join(" · ")
            },
            if more > 0 {
                format!(" · +{more} more")
            } else {
                String::new()
            }
        )
    };
    let Some(max) = max else {
        return render(parts.len(), parts.len());
    };
    let mut shown = parts.len();
    let mut tests = parts.len();
    loop {
        let line = render(shown, tests);
        if line.len() <= max || shown <= 1 && tests == 0 {
            return line;
        }
        if tests > 0 {
            tests -= 1;
        } else {
            shown -= 1;
        }
    }
}

/// One line per indexed Python file: its most-used symbols, their callers and covering tests.
/// `budget` caps the total bytes (split evenly across files); details degrade to fit it.
pub fn file_header(engine: &Engine, cwd: &Path, paths: &[String], budget: Option<usize>) -> String {
    let per_file = budget.map(|b| b / paths.len().max(1));
    let mut out = String::new();
    for p in paths {
        let Some((abs, rel)) = rel_of(engine, cwd, p) else {
            continue;
        };
        if !engine.is_indexable(&rel) {
            continue;
        }
        let Ok(syms) = engine.store.symbols_in_file(&rel) else {
            continue;
        };
        let mut ranked: Vec<(usize, &graphite_model::Symbol)> = syms
            .iter()
            .filter(|s| s.kind != SymbolKind::Module)
            .map(|s| (callers_count(engine, s.id), s))
            .collect();
        ranked.sort_by_key(|(n, s)| (std::cmp::Reverse(*n), s.start_line));
        let tests_for = |id: SymbolId| -> Vec<String> {
            let ids: Vec<SymbolId> = engine
                .adjacency()
                .covering_tests(id, 3)
                .into_iter()
                .map(|(t, _)| t)
                .collect();
            let n = ids.len();
            let mut names: Vec<String> = engine
                .store
                .symbols(&ids[..n.min(2)])
                .unwrap_or_default()
                .iter()
                .map(|s| s.name.clone())
                .collect();
            if n > 2 {
                names.push(format!("+{}", n - 2));
            }
            names
        };
        let parts: Vec<HeaderPart> = ranked
            .iter()
            .take(TOP_SYMBOLS)
            .map(|(n, s)| {
                let tests = tests_for(s.id);
                let bare = format!("{} L{} ({n} callers)", short_label(s), s.start_line);
                let with_tests = if tests.is_empty() {
                    bare.clone()
                } else {
                    format!(
                        "{} L{} ({n} callers; tests: {})",
                        short_label(s),
                        s.start_line,
                        tests.join(", ")
                    )
                };
                HeaderPart { with_tests, bare }
            })
            .collect();
        let prefix = format!(
            "[graphite] {} — {} symbols; most used: ",
            display_path(&abs, cwd),
            ranked.len()
        );
        let _ = writeln!(
            out,
            "{}",
            fit_header(
                &prefix,
                &parts,
                ranked.len(),
                per_file.map(|b| b.saturating_sub(1))
            )
        );
    }
    out
}

/// One line per directory: indexed Python files under it and its most-called symbols.
pub fn dir_summary(engine: &Engine, cwd: &Path, paths: &[String]) -> String {
    let deadline = Instant::now() + DIR_BUDGET;
    let indexed = engine.indexed_paths();
    let mut out = String::new();
    for p in paths {
        let Some((abs, rel)) = rel_of(engine, cwd, p) else {
            continue;
        };
        if !abs.is_dir() {
            continue;
        }
        let prefix = if rel.is_empty() {
            String::new()
        } else {
            format!("{rel}/")
        };
        let mut files: Vec<&String> = indexed.iter().filter(|f| f.starts_with(&prefix)).collect();
        if files.is_empty() {
            continue;
        }
        files.sort();
        let total = files.len();
        let mut top: Vec<(usize, String)> = Vec::new();
        let mut scanned = 0;
        for f in files.iter().take(DIR_MAX_FILES) {
            if Instant::now() > deadline {
                break;
            }
            scanned += 1;
            for s in engine.store.symbols_in_file(f).unwrap_or_default() {
                if s.kind == SymbolKind::Module {
                    continue;
                }
                let n = callers_count(engine, s.id);
                if n > 0 {
                    top.push((n, short_label(&s)));
                }
            }
        }
        top.sort_by_key(|(n, l)| (std::cmp::Reverse(*n), l.clone()));
        let names: Vec<String> = top
            .iter()
            .take(5)
            .map(|(n, l)| format!("{l} ({n})"))
            .collect();
        let partial = if scanned < total {
            format!(" (ranked over {scanned} of {total} files)")
        } else {
            String::new()
        };
        let shown = if rel.is_empty() {
            ".".to_string()
        } else {
            display_path(&abs, cwd)
        };
        let _ = writeln!(
            out,
            "[graphite] {shown}/ — {total} indexed Python files; most called: {}{partial}",
            if names.is_empty() {
                "none".into()
            } else {
                names.join(", ")
            }
        );
    }
    out
}

/// Graph facts about a name, appended after a native search tool ran.
pub fn name_summary(engine: &Engine, cwd: &Path, name: &str) -> Result<String, String> {
    let f = name_facts(engine, name, name)?;
    if f.syms.is_empty() {
        return Ok(String::new());
    }
    let mut out = String::new();
    for s in f.syms.iter().take(5) {
        let abs = engine.paths.root.join(&s.path);
        let _ = writeln!(
            out,
            "[graphite] `{name}` defined at {}:{} — {}",
            display_path(&abs, cwd),
            s.start_line,
            s.signature
        );
    }
    let files: HashSet<&str> = f.ref_paths().collect();
    let verdict = if f.gap_count() == 0 {
        "complete: no unresolved or ambiguous calls with this name".to_string()
    } else {
        format!(
            "lower bound: {} calls with this name could not be resolved to one target",
            f.gap_count()
        )
    };
    let _ = writeln!(
        out,
        "[graphite] graph references: {} sites in {} files — {verdict}. `graphite blast {name} --depth 1` lists them with call lines.",
        f.ref_count(),
        files.len()
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(n: usize) -> Vec<HeaderPart> {
        (0..n)
            .map(|i| HeaderPart {
                with_tests: format!("sym{i} L{i} (9 callers; tests: test_a_long_name, test_b, +7)"),
                bare: format!("sym{i} L{i} (9 callers)"),
            })
            .collect()
    }

    // kinhin: decision(ref="docs/foundation/interception.md#4-size-budget-never-byte-cut")
    #[test]
    fn header_degrades_tests_then_symbols_and_counts_what_it_drops() {
        let prefix = "[graphite] a.py — 12 symbols; most used: ";
        let full = fit_header(prefix, &parts(8), 12, None);
        assert!(
            full.contains("tests: test_a_long_name") && full.ends_with("+4 more"),
            "{full}"
        );

        let fitted = fit_header(prefix, &parts(8), 12, Some(160));
        assert!(fitted.len() <= 160, "{}: {fitted}", fitted.len());
        assert!(!fitted.contains("tests:"), "{fitted}");
        assert!(fitted.starts_with(&format!("{prefix}sym0 L0")), "{fitted}");
        let listed = fitted.matches(" callers)").count();
        assert!(
            fitted.ends_with(&format!("+{} more", 12 - listed)),
            "{fitted}"
        );

        // Never below the busiest symbol, even when the budget is smaller than that.
        let tiny = fit_header(prefix, &parts(8), 12, Some(10));
        assert!(tiny.contains("sym0 L0 (9 callers) · +11 more"), "{tiny}");
    }
}
