//! Agent-facing text rendering of an envelope: grep-shaped lines, graph facts inside, verdict first so
//! any truncation keeps what matters.

use serde_json::Value;

use crate::sites::local_name;

/// Knobs for text rendering.
#[derive(Debug, Clone, Copy, Default)]
pub struct TextOptions {
    /// List every call site present in the envelope instead of the top few.
    pub all: bool,
}

/// Callers listed per target before "… N more".
const TOP_CALLERS: usize = 12;
/// Callers listed per changed symbol in a diff.
const TOP_CHANGED_CALLERS: usize = 5;
const LINES_PER_CALLER: usize = 4;
const TOP_TESTS: usize = 8;
const TOP_GROUPS: usize = 6;
const TOP_UNMAPPED: usize = 5;

/// Render a serialized `Envelope` (lookup, blast_radius or diff_impact) as compact text.
pub fn render_text(env: &Value, o: TextOptions) -> String {
    let mut out = Out::default();
    if env["stale"] == true {
        out.line(format!(
            "STALE: index is behind the working tree; answer reflects rev {}",
            env["graph_rev"]
        ));
    }
    let res = &env["result"];
    match env["query"].as_str() {
        Some("lookup") => lookup(&mut out, res),
        Some("blast_radius") => blast(&mut out, env, res, o),
        Some("diff_impact") => diff(&mut out, env, res, o),
        _ => out.line(serde_json::to_string_pretty(env).unwrap_or_default()),
    }
    notes(&mut out, env);
    out.0
}

#[derive(Default)]
struct Out(String);

impl Out {
    fn line(&mut self, s: impl AsRef<str>) {
        self.0.push_str(s.as_ref());
        self.0.push('\n');
    }
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

fn n(v: &Value, k: &str) -> u64 {
    v[k].as_u64().unwrap_or(0)
}

fn arr(v: &Value) -> &[Value] {
    v.as_array().map_or(&[], Vec::as_slice)
}

fn local(sym: &Value) -> &str {
    local_name(s(sym, "qualified"), s(sym, "path"))
}

fn plural(count: u64, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// `signature  path:line  [kind]`, falling back to the local name when the signature is empty.
fn head(sym: &Value) -> String {
    let sig = match s(sym, "signature") {
        "" => local(sym).to_string(),
        sig => sig.to_string(),
    };
    format!(
        "{sig}  {}:{}  [{}]",
        s(sym, "path"),
        n(sym, "start_line"),
        s(sym, "kind")
    )
}

fn lookup(out: &mut Out, l: &Value) {
    match s(l, "status") {
        "found" => {
            let sym = &l["symbol"];
            out.line(head(sym));
            out.line(format!("qualified: {}", s(sym, "qualified")));
        }
        "ambiguous" => {
            let c = arr(&l["candidates"]);
            out.line(format!(
                "AMBIGUOUS: {} match '{}' — pass a qualified name:",
                plural(c.len() as u64, "symbol", "symbols"),
                s(l, "query")
            ));
            for sym in c {
                out.line(format!(
                    "  {}  {}:{}  [{}]",
                    s(sym, "qualified"),
                    s(sym, "path"),
                    n(sym, "start_line"),
                    s(sym, "kind")
                ));
            }
        }
        _ => out.line(format!(
            "NOT FOUND: no indexed symbol matches '{}'",
            s(l, "query")
        )),
    }
}

/// `COMPLETE` or `LOWER-BOUND (why)` for the whole answer.
fn completeness(env: &Value, name: &str) -> String {
    let c = &env["completeness"];
    if s(c, "status") == "complete" {
        return "COMPLETE".to_string();
    }
    let k = &c["causes"];
    let mut why = Vec::new();
    let (amb, unres) = (n(k, "ambiguous_refs"), n(k, "unresolved_refs"));
    if amb + unres > 0 {
        let parts: Vec<String> = [(amb, "ambiguous"), (unres, "unresolved")]
            .into_iter()
            .filter(|(x, _)| *x > 0)
            .map(|(x, w)| format!("{x} {w}"))
            .collect();
        let named = if name.is_empty() {
            String::new()
        } else {
            format!(" named `{name}`")
        };
        why.push(format!(
            "{} refs{named} could be hidden callers",
            parts.join(" + ")
        ));
    }
    if n(k, "parse_failures") > 0 {
        why.push(plural(
            n(k, "parse_failures"),
            "file failed to parse",
            "files failed to parse",
        ));
    }
    if n(k, "source_changed") > 0 {
        why.push(format!(
            "{} changed since indexing",
            plural(n(k, "source_changed"), "symbol", "symbols")
        ));
    }
    format!("LOWER-BOUND ({})", why.join("; "))
}

fn risk(r: &Value) -> String {
    if r.is_null() {
        return String::new();
    }
    let level = s(r, "level").to_uppercase();
    if level == "UNKNOWN" {
        return format!("risk UNKNOWN: {}", s(r, "reason"));
    }
    format!(
        "risk {level} ({} prod direct; {} prod / {} test total)",
        n(r, "prod_direct"),
        n(r, "prod_total"),
        n(r, "test_total")
    )
}

fn direct_counts(d: &Value) -> String {
    let callers = n(d, "callers");
    let mut out = if callers == 0 {
        "no direct callers".to_string()
    } else {
        format!(
            "{} in {} across {}",
            plural(n(d, "sites"), "call site", "call sites"),
            plural(callers, "caller", "callers"),
            plural(n(d, "files"), "file", "files")
        )
    };
    let imports = n(d, "imported_by_files");
    if imports > 0 {
        out.push_str(&format!(
            " (imported in {})",
            plural(imports, "file", "files")
        ));
    }
    out
}

fn lines(v: &Value) -> String {
    let all: Vec<u64> = arr(v).iter().filter_map(Value::as_u64).collect();
    let mut shown: Vec<String> = all
        .iter()
        .take(LINES_PER_CALLER)
        .map(u64::to_string)
        .collect();
    if all.len() > LINES_PER_CALLER {
        shown.push(format!("+{}", all.len() - LINES_PER_CALLER));
    }
    shown.join(",")
}

fn site_line(c: &Value, indent: &str) -> String {
    let caller = &c["caller"];
    let conf = match s(c, "confidence") {
        "extracted" => "",
        "inferred" => " (inferred)",
        _ => " (name guess)",
    };
    let edge = match s(c, "edge") {
        "calls" => String::new(),
        e => format!(" [{e}]"),
    };
    let test = if s(caller, "role") == "test" {
        " [test]"
    } else {
        ""
    };
    let mut line = format!(
        "{indent}{}:{}  in {}{edge}{test}{conf}",
        s(caller, "path"),
        lines(&c["lines"]),
        local(caller)
    );
    let by = arr(&c["called_by"]);
    if !by.is_empty() {
        let names: Vec<&str> = by.iter().filter_map(Value::as_str).collect();
        line.push_str(&format!("  ← called by {}", names.join(", ")));
        let more = n(c, "called_by_total").saturating_sub(by.len() as u64);
        if more > 0 {
            line.push_str(&format!(" (+{more})"));
        }
    }
    line
}

fn call_sites(out: &mut Out, sites: &Value, total: u64, cap: usize, indent: &str) {
    let list = arr(sites);
    let shown = list.len().min(cap);
    for c in &list[..shown] {
        out.line(site_line(c, indent));
    }
    let more = total.saturating_sub(shown as u64);
    if more > 0 {
        let noun = if more == 1 { "caller" } else { "callers" };
        out.line(format!(
            "{indent}… {more} more {noun} — `--all` lists every call site"
        ));
    }
}

fn indirect(out: &mut Out, ind: &Value, depth: u64) {
    let total = n(ind, "total");
    if total == 0 {
        return;
    }
    let range = if depth <= 2 {
        "depth 2".to_string()
    } else {
        format!("depth 2–{depth}")
    };
    out.line(format!(
        "indirect ({range}): {} ({} prod, {} test) in {}",
        plural(total, "symbol", "symbols"),
        n(ind, "prod"),
        n(ind, "test"),
        plural(n(ind, "modules"), "dir", "dirs")
    ));
    let groups = arr(&ind["groups"]);
    for g in groups.iter().take(TOP_GROUPS) {
        let top: Vec<&str> = arr(&g["top"]).iter().filter_map(Value::as_str).collect();
        out.line(format!(
            "  {}/  {}  e.g. {}",
            s(g, "module"),
            n(g, "count"),
            top.join(", ")
        ));
    }
    let rest = n(ind, "modules").saturating_sub(TOP_GROUPS.min(groups.len()) as u64);
    if rest > 0 {
        out.line(format!("  … {} more", plural(rest, "dir", "dirs")));
    }
}

fn tests(out: &mut Out, t: &Value, name: &str) {
    let items = arr(&t["items"]);
    let total = n(t, "total");
    if total == 0 {
        out.line(format!(
            "tests: none reach {name} within depth {} ⚠",
            n(t, "depth_limit")
        ));
        return;
    }
    out.line(format!("tests reaching {name}: {total} (nearest first)"));
    for x in items.iter().take(TOP_TESTS) {
        out.line(format!("  {}  (depth {})", s(x, "node_id"), n(x, "depth")));
    }
    let more = total.saturating_sub(TOP_TESTS.min(items.len()) as u64);
    if more > 0 {
        out.line(format!("  … {more} more"));
    }
}

fn overrides(out: &mut Out, ov: &Value, indent: &str) {
    for o in arr(ov) {
        let sym = &o["symbol"];
        let label = match s(o, "relation") {
            "overridden_by" => "overridden in subclass",
            _ => "overrides base",
        };
        out.line(format!(
            "{indent}{label}: {}  {}:{}  — keep signatures compatible",
            local(sym),
            s(sym, "path"),
            n(sym, "start_line")
        ));
    }
}

fn blast(out: &mut Out, env: &Value, res: &Value, o: TextOptions) {
    let target = &res["target"];
    if s(target, "status") != "found" {
        return lookup(out, target);
    }
    let sym = &target["symbol"];
    let name = s(sym, "qualified").rsplit('.').next().unwrap_or("");
    out.line(head(sym));
    let direct = &res["direct"];
    let mut verdict = format!("{} · {}", completeness(env, name), direct_counts(direct));
    let r = risk(&res["risk"]);
    if !r.is_empty() {
        verdict.push_str(" · ");
        verdict.push_str(&r);
    }
    out.line(verdict);
    let total = n(direct, "callers");
    if total > 0 {
        out.line("direct:");
        let cap = if o.all { usize::MAX } else { TOP_CALLERS };
        call_sites(out, &res["call_sites"], total, cap, "  ");
    }
    indirect(
        out,
        &res["indirect"],
        n(&res["dependents"]["summary"], "depth_limit"),
    );
    tests(out, &res["tests"], local(sym));
    overrides(out, &res["overrides"], "");
}

fn diff(out: &mut Out, env: &Value, res: &Value, o: TextOptions) {
    let changed = arr(&res["changed"]);
    let unmapped = arr(&res["unmapped"]);
    let files: std::collections::BTreeSet<&str> =
        changed.iter().map(|c| s(&c["symbol"], "path")).collect();
    let mut head_line = format!(
        "diff: {} in {}",
        plural(changed.len() as u64, "changed symbol", "changed symbols"),
        plural(files.len() as u64, "file", "files")
    );
    if !unmapped.is_empty() {
        head_line.push_str(&format!(
            " · {} not in any indexed symbol",
            plural(unmapped.len() as u64, "hunk", "hunks")
        ));
    }
    out.line(head_line);
    let mut verdict = completeness(env, "");
    let r = risk(&res["risk"]);
    if !r.is_empty() {
        verdict.push_str(" · ");
        verdict.push_str(&r);
    }
    out.line(verdict);
    for c in changed {
        let sym = &c["symbol"];
        let ranges: Vec<String> = arr(&c["hunks"])
            .iter()
            .map(|h| {
                let (a, b) = (h[0].as_u64().unwrap_or(0), h[1].as_u64().unwrap_or(0));
                if a == b {
                    a.to_string()
                } else {
                    format!("{a}-{b}")
                }
            })
            .collect();
        let gaps = n(c, "reference_gaps");
        let gap_note = if gaps > 0 {
            format!(" · {gaps} unresolved refs with this name")
        } else {
            String::new()
        };
        out.line(format!(
            "changed {}:{}  {}  [{}] · {} · risk {}{gap_note}",
            s(sym, "path"),
            ranges.join(","),
            local(sym),
            s(sym, "kind"),
            direct_counts(&c["direct"]),
            s(c, "risk").to_uppercase()
        ));
        let cap = if o.all {
            usize::MAX
        } else {
            TOP_CHANGED_CALLERS
        };
        call_sites(
            out,
            &c["call_sites"],
            n(&c["direct"], "callers"),
            cap,
            "    ",
        );
        overrides(out, &c["overrides"], "    ");
    }
    indirect(
        out,
        &res["indirect"],
        n(&res["dependents"]["summary"], "depth_limit"),
    );
    let t = &res["tests"];
    if n(t, "total") == 0 {
        if !changed.is_empty() {
            out.line(format!(
                "tests: none reach the changed symbols within depth {} ⚠",
                n(t, "depth_limit")
            ));
        }
    } else {
        out.line(format!("tests to run: {} (nearest first)", n(t, "total")));
        let items = arr(&t["items"]);
        for x in items.iter().take(TOP_TESTS) {
            out.line(format!("  {}", s(x, "node_id")));
        }
        let more = n(t, "total").saturating_sub(TOP_TESTS.min(items.len()) as u64);
        if more > 0 {
            out.line(format!("  … {more} more"));
        }
    }
    for h in unmapped.iter().take(TOP_UNMAPPED) {
        out.line(format!(
            "unmapped {}:{}+{}  ({})",
            s(h, "path"),
            n(h, "start_line"),
            n(h, "line_count"),
            s(h, "reason")
        ));
    }
    if unmapped.len() > TOP_UNMAPPED {
        out.line(format!(
            "… {} more unmapped",
            plural((unmapped.len() - TOP_UNMAPPED) as u64, "hunk", "hunks")
        ));
    }
}

/// Cuts that change what an agent should conclude; tier regrouping of the JSON detail is not one.
fn notes(out: &mut Out, env: &Value) {
    let mut more = Vec::new();
    for d in arr(&env["disclosures"]) {
        let what = s(d, "what");
        let reason = s(d, "reason");
        match what {
            "depth" => more.push(format!("{reason} (--depth N)")),
            "call_sites" | "changed_call_sites" | "candidates" | "covering_tests" => {
                more.push(reason.to_string())
            }
            "dependents" if !reason.starts_with("tier ") => more.push(reason.to_string()),
            _ => {}
        }
    }
    for m in more {
        out.line(format!("note: {m}"));
    }
    if matches!(env["query"].as_str(), Some("blast_radius" | "diff_impact")) {
        out.line("more: --depth N · --all · --json");
    }
}
