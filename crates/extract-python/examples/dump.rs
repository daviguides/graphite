//! Print the facts of one file in readable form: `cargo run --example dump -- <root> <relpath>`.

use std::collections::HashMap;

use graphite_model::Target;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (root, rel) = (&args[1], &args[2]);
    let source = std::fs::read(format!("{root}/{rel}")).expect("read file");
    let facts = graphite_extract_python::extract(rel, &source);
    let names: HashMap<_, _> = facts
        .symbols
        .iter()
        .map(|s| (s.id, s.qualified.clone()))
        .collect();
    println!("parse_ok={}", facts.parse_ok);
    for s in &facts.symbols {
        println!(
            "SYM {:?} {} L{}-{} exported={} test={} sig={:?}",
            s.kind, s.qualified, s.start_line, s.end_line, s.exported, s.is_test, s.signature
        );
    }
    for e in &facts.edges {
        let dst = match &e.dst {
            Target::Symbol(id) => names[id].clone(),
            Target::Unresolved {
                name,
                qualifier,
                import_path,
            } => {
                format!("?{name} q={qualifier:?} ip={import_path:?}")
            }
        };
        println!(
            "EDGE {:?} {} -> {} L{} {:?}",
            e.kind, names[&e.src], dst, e.site_line, e.provenance
        );
    }
}
