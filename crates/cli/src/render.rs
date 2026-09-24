//! Compact human-readable rendering of daemon responses.

use graphite_daemon::Response;
use serde_json::Value;

pub fn human(cmd: &str, r: &Response) {
    if !r.ok {
        eprintln!("error: {}", r.error.as_deref().unwrap_or("unknown"));
        return;
    }
    let stale = if r.stale {
        " (stale: recent edits still indexing)"
    } else {
        ""
    };
    let d = &r.data;
    match cmd {
        "status" => {
            println!(
                "root {}  files {}  edges {}  rev {}  pending {}{}",
                d["root"].as_str().unwrap_or("?"),
                d["files"],
                d["adjacency_edges"],
                r.graph_rev,
                d["pending_events"],
                stale
            );
            let i = &d["init"];
            println!(
                "initial index: {} files, {} symbols, {} ms (extract {} / write {} / adjacency {})",
                i["files_seen"],
                i["symbols"],
                i["total_ms"],
                i["extract_ms"],
                i["write_ms"],
                i["adjacency_ms"]
            );
        }
        "lookup" => lookup(d),
        "blast" => {
            if d["status"] != "found" {
                return lookup(d);
            }
            println!("{}  (rev {}{})", sym(&d["target"]), r.graph_rev, stale);
            dependents(&d["dependents"]);
        }
        "diff-impact" => {
            println!(
                "vs {}  (rev {}{})",
                d["base"].as_str().unwrap_or("HEAD"),
                r.graph_rev,
                stale
            );
            for s in d["changed_symbols"].as_array().into_iter().flatten() {
                println!("  changed  {}", sym(s));
            }
            dependents(&d["impacted"]);
            if let Some(u) = d["unindexed_files"].as_array().filter(|u| !u.is_empty()) {
                println!("  not indexed (no graph info): {}", u.len());
            }
        }
        _ => println!("{}", serde_json::to_string_pretty(d).unwrap_or_default()),
    }
}

fn sym(s: &Value) -> String {
    format!(
        "{} [{}] {}:{}",
        s["qualified"].as_str().unwrap_or("?"),
        s["kind"].as_str().unwrap_or("?"),
        s["path"].as_str().unwrap_or("?"),
        s["line"]
    )
}

fn lookup(d: &Value) {
    match d["status"].as_str() {
        Some("found") => println!("{}", sym(&d["symbol"])),
        Some("ambiguous") => {
            println!("ambiguous — candidates:");
            for c in d["candidates"].as_array().into_iter().flatten() {
                println!("  {}", sym(c));
            }
        }
        _ => println!("not found: {}", d["hint"].as_str().unwrap_or("")),
    }
}

fn dependents(d: &Value) {
    println!(
        "  {} dependents ({} prod, {} test) by depth {}",
        d["total"], d["prod"], d["test"], d["by_depth"]
    );
    for i in d["items"].as_array().into_iter().flatten() {
        let via = i["via"]
            .as_str()
            .map(|v| format!("  via {v}"))
            .unwrap_or_default();
        println!("  d{}  {}{}", i["depth"], sym(i), via);
    }
    if d["truncated"] == true {
        println!(
            "  … {} more not listed",
            d["total"].as_u64().unwrap_or(0) - d["listed"].as_u64().unwrap_or(0)
        );
    }
}
