//! Compact human-readable rendering of daemon responses (the `--json` output is the full envelope).

use graphite_daemon::Response;
use serde_json::Value;

pub fn human(cmd: &str, r: &Response) {
    if !r.ok {
        eprintln!("error: {}", r.error.as_deref().unwrap_or("unknown"));
        return;
    }
    let d = &r.data;
    match cmd {
        "status" => status(r),
        "stop" => println!("daemon stopping (rev {})", r.graph_rev),
        "lookup" => {
            header(r);
            lookup(&d["result"]);
        }
        "blast" => {
            header(r);
            let res = &d["result"];
            if res["target"]["status"] != "found" {
                return lookup(&res["target"]);
            }
            println!("{}", sym(&res["target"]["symbol"]));
            risk(&res["risk"]);
            dependents(&res["dependents"]);
        }
        "diff-impact" => {
            header(r);
            let res = &d["result"];
            for c in res["changed"].as_array().into_iter().flatten() {
                println!(
                    "  changed  {}  risk {}  callers {} prod / {} test",
                    sym(&c["symbol"]),
                    c["risk"].as_str().unwrap_or("?"),
                    c["direct_prod_callers"],
                    c["direct_test_callers"]
                );
            }
            let unmapped = res["unmapped"].as_array().map_or(0, Vec::len);
            if unmapped > 0 {
                println!("  {unmapped} changed hunks not mapped to a symbol");
            }
            risk(&res["risk"]);
            dependents(&res["dependents"]);
            let tests = res["covering_tests"].as_array().map_or(0, Vec::len);
            println!(
                "  {tests} covering tests ({})",
                res["tests_basis"].as_str().unwrap_or("")
            );
        }
        _ => println!("{}", serde_json::to_string_pretty(d).unwrap_or_default()),
    }
    disclosures(d);
}

fn header(r: &Response) {
    let d = &r.data;
    let stale = if r.stale || d["stale"] == true {
        "  STALE: recent edits still indexing"
    } else {
        ""
    };
    println!(
        "rev {}  tier {}  completeness {}{}",
        r.graph_rev,
        d["tier"].as_str().unwrap_or("-"),
        d["completeness"]["status"].as_str().unwrap_or("-"),
        stale
    );
}

fn status(r: &Response) {
    let d = &r.data;
    println!(
        "root {}  files {}  edges {}  rev {}  pending {}",
        d["root"].as_str().unwrap_or("?"),
        d["files"],
        d["adjacency_edges"],
        r.graph_rev,
        d["pending_events"],
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

fn sym(s: &Value) -> String {
    format!(
        "{} [{}] {}:{}",
        s["qualified"].as_str().unwrap_or("?"),
        s["kind"].as_str().unwrap_or("?"),
        s["path"].as_str().unwrap_or("?"),
        s["start_line"]
    )
}

fn lookup(l: &Value) {
    match l["status"].as_str() {
        Some("found") => println!("{}", sym(&l["symbol"])),
        Some("ambiguous") => {
            println!("ambiguous — candidates:");
            for c in l["candidates"].as_array().into_iter().flatten() {
                println!("  {}", sym(c));
            }
        }
        _ => println!("not found: {}", l["query"].as_str().unwrap_or("")),
    }
}

fn risk(r: &Value) {
    if r.is_null() {
        return;
    }
    println!(
        "  risk {}: {}",
        r["level"].as_str().unwrap_or("?"),
        r["reason"].as_str().unwrap_or("")
    );
}

fn dependents(d: &Value) {
    let s = &d["summary"];
    println!(
        "  {} dependents ({} prod, {} test) by depth {}",
        s["total"], s["prod"], s["test"], s["by_depth"]
    );
    for i in d["items"].as_array().into_iter().flatten() {
        println!(
            "  d{}  {}  {} {}",
            i["depth"],
            sym(&i["symbol"]),
            i["edge"].as_str().unwrap_or(""),
            i["confidence"].as_str().unwrap_or("")
        );
    }
    for f in d["by_file"].as_array().into_iter().flatten() {
        println!("  {} ({})", f["path"].as_str().unwrap_or("?"), f["count"]);
    }
    for f in d["by_directory"].as_array().into_iter().flatten() {
        println!(
            "  {}/ ({} in {} files)",
            f["dir"].as_str().unwrap_or("?"),
            f["count"],
            f["files"]
        );
    }
}

fn disclosures(d: &Value) {
    for x in d["disclosures"].as_array().into_iter().flatten() {
        println!(
            "  note: {} shown {} omitted {} — {}",
            x["what"].as_str().unwrap_or(""),
            x["shown"],
            x["omitted"],
            x["reason"].as_str().unwrap_or("")
        );
    }
}
