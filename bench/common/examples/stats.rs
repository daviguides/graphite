use common::*;
fn main() {
    let names: Vec<String> = std::env::args().skip(1).collect();
    let names = if names.is_empty() { vec!["small".into(), "medium".into(), "large".into()] } else { names };
    for name in names {
        let ds = Dataset::by_name(&name);
        let r = Reference::new(&ds);
        let tg = pick_targets(&ds, &r);
        let inf = ds.edges().filter(|e| e.prov == PROV_INFERRED).count();
        println!(
            "{name}: files={} syms={} edges={} inferred={:.1}% blast10 hub={} p99={} median={} nontrivial_scc={}",
            ds.n_files(), ds.n_syms(), ds.n_edges(), 100.0 * inf as f64 / ds.n_edges() as f64,
            r.blast(tg.hub, 10, false).len(), r.blast(tg.p99, 10, false).len(), r.blast(tg.median, 10, false).len(),
            r.nontrivial_sccs()
        );
    }
}
