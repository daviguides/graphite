//! Text rendering of daemon responses: query answers use graphite-query's agent format; daemon status
//! gets its own short lines.

use graphite_daemon::Response;
use graphite_query::{render_text, TextOptions};

pub fn human(cmd: &str, r: &Response, all: bool) {
    if !r.ok {
        eprintln!("error: {}", r.error.as_deref().unwrap_or("unknown"));
        return;
    }
    match cmd {
        "status" => status(r),
        "stop" => println!("daemon stopping (rev {})", r.graph_rev),
        "lookup" | "blast" | "diff-impact" => {
            let mut data = r.data.clone();
            if r.stale {
                data["stale"] = true.into();
            }
            print!("{}", render_text(&data, TextOptions { all }));
        }
        _ => println!(
            "{}",
            serde_json::to_string_pretty(&r.data).unwrap_or_default()
        ),
    }
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
