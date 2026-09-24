use common::*;
fn main() {
    for size in ["small", "medium", "large"] {
        let cfg = Config::by_name(size);
        let t = std::time::Instant::now();
        let ds = Dataset::generate(&cfg);
        let r = Reference::new(&ds);
        let n = ds.n_syms();
        let hub = (0..n).max_by_key(|&s| r.in_degree(s)).unwrap();
        let mut rng = Rng::new(7);
        let mut sizes: Vec<usize> = (0..201).map(|_| r.blast(rng.below(n as u64) as u32, 10, false).len()).collect();
        sizes.sort();
        let inf = ds.edges().filter(|e| e.prov == PROV_INFERRED).count();
        println!("{size}: syms={n} edges={} inferred={:.1}% hub_in={} hub_blast10={} blast10 p25={} p50={} p75={} p90={} nontrivial_scc={} ({:?})",
            ds.n_edges(), 100.0*inf as f64/ds.n_edges() as f64, r.in_degree(hub), r.blast(hub,10,false).len(),
            sizes[50], sizes[100], sizes[150], sizes[180], r.nontrivial_sccs(), t.elapsed());
    }
}
