"""Print a compact summary of results/*.json (real-graph shape reports)."""
import json
import sys

names = sys.argv[1:] or ["continuum-strict", "continuum-permissive", "sensemesh-strict", "sensemesh-permissive"]
for r in names:
    d = json.load(open(f"results/{r}.json"))
    fi = d["fan_in_calls"]
    print(f'{r:22} nodes={d["nodes"]} edges={d["edges"]} calls ex/inf/amb/ext='
          f'{d["calls_extracted"]}/{d["calls_inferred"]}/{d["calls_dropped_ambiguous"]}/{d["calls_external_unresolved"]} '
          f'fan-in max/p99/p90/med={fi["max"]}/{fi["p99"]}/{fi["p90"]}/{fi["median"]}')
    for n in ("blast_calls", "blast_all"):
        b = d[n]
        f = lambda k: "/".join(str(b[k][q]) for q in ("max", "p99", "p90", "median"))
        print(f'   {n:11} d3={f("d3"):18} d10={f("d10"):18} max%={b["max_pct_d10"]:.1f} '
              f'top5%={b["top5_hub_pct_d10"]} >10%={b["targets_over_10pct_d10"]} sampled={b["sampled"]}')
    print("   scc calls", d["scc_calls"], "all", d["scc_all"])
    if r.endswith("strict"):
        for g in d["groups"][:12]:
            internal = 100 * g["edges_internal"] / max(1, g["edges_out"])
            print(f'     {g["group"]:36} files={g["files"]:4} syms={g["symbols"]:5} internal%={internal:3.0f} '
                  f'maxblast10 calls/all={g["max_blast_d10_calls"]}/{g["max_blast_d10_all"]}')
