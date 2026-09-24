#!/usr/bin/env python3
"""Render results/*.jsonl as markdown tables (stdout)."""
import glob
import json
import os
import sys
from collections import defaultdict

HERE = os.path.dirname(os.path.abspath(__file__))
sizes = sys.argv[1:] or ["small", "medium", "large"]

data = defaultdict(dict)  # (engine, size) -> metric -> rec
for path in glob.glob(os.path.join(HERE, "results", "*.jsonl")):
    for line in open(path):
        r = json.loads(line)
        data[(r["engine"], r["size"])][r["metric"]] = r

ORDER = ["cozo-mnestic-mem", "cozo-mnestic-sqlite", "cozo-mnestic-newrocksdb", "lbug-disk", "lbug-mem", "diy-ascent-redb"]


def engines(size):
    present = [e for (e, s) in data if s == size]
    return [e for e in ORDER if e in present] + sorted(e for e in present if e not in ORDER)


def ms(us):
    if us is None:
        return "—"
    if us >= 1000:
        return f"{us / 1000:.1f} ms"
    return f"{us:.0f} µs" if us >= 10 else f"{us:.1f} µs"


def get(m, key, field="p50_us"):
    r = m.get(key)
    return None if r is None else r[field]


def variants(m):
    return sorted({k[k.index("[") + 1:-1] for k in m if k.startswith("blast_d10_median[")})


def best_variant(m):
    vs = variants(m)
    ok = [v for v in vs if m.get(f"correct_vs_reference_initial[{v}]", {}).get("value") == 1]
    return min(ok or vs, key=lambda v: get(m, f"blast_d10_hub[{v}]") or 1e18)


def flag(m, key):
    r = m.get(key)
    return "—" if r is None else ("✅" if r["value"] == 1 else "❌")


for size in sizes:
    es = engines(size)
    if not es:
        continue
    any_m = data[(es[0], size)]
    print(f"\n### {size}: {int(any_m['dataset_symbols']['value']):,} symbols / {int(any_m['dataset_edges']['value']):,} edges")
    print(f"\nTargets: hub blast(d10) = {int(any_m['target_hub_blast10_size']['value']):,} nodes; "
          f"median blast(d10) = {int(any_m['target_median_blast10_size']['value']):,} nodes.\n")

    print("#### Blast radius by traversal strategy (p50 / p95)\n")
    print("| engine | strategy | d3 median | d10 median | d10 hub | d10 hub trusted | correct |")
    print("|---|---|---|---|---|---|---|")
    for e in es:
        m = data[(e, size)]
        for v in variants(m):
            cells = []
            for key in (f"blast_d3_median[{v}]", f"blast_d10_median[{v}]", f"blast_d10_hub[{v}]", f"blast_d10_hub_trusted[{v}]"):
                cells.append(f"{ms(get(m, key))} / {ms(get(m, key, 'p95_us'))}")
            ok = flag(m, f"correct_vs_reference_initial[{v}]") + flag(m, f"correct_incremental_eq_full[{v}]")
            print(f"| {e} | {v} | " + " | ".join(cells) + f" | {ok} |")

    print("\n#### Everything else (best correct strategy for blast)\n")
    cols = [
        "best strategy", "build", "disk", "reopen", "cold 1st query", "shortest path", "SCC", "Louvain", "PageRank",
        "update p50 / p95", "read while writing p50 / p99 / max", "write while reading p50 / p99", "peak RSS",
    ]
    print("| engine | " + " | ".join(cols) + " |")
    print("|---|" + "---|" * len(cols))
    for e in es:
        m = data[(e, size)]
        bv = best_variant(m)

        def algo(key):
            r = m.get(key)
            if r is None:
                return "—"
            return "unsupported" if r["value"] == -1 and r["n"] == 0 else ms(r["p50_us"])

        wr = "watcher_continuous_read_blast10_median"
        ww = "watcher_continuous_write"
        row = [
            bv,
            f"{m['build_ms']['value']:.0f} ms" if "build_ms" in m else "—",
            f"{m['disk_mb']['value']:.1f} MB" if "disk_mb" in m else "in-mem",
            f"{m['reopen_ms']['value']:.0f} ms" if "reopen_ms" in m else "—",
            ms(get(m, "cold_first_blast10_median")),
            ms(get(m, "shortest_path")),
            algo("scc"),
            algo("communities_louvain"),
            algo("pagerank"),
            f"{ms(get(m, 'incremental_replace_file'))} / {ms(get(m, 'incremental_replace_file', 'p95_us'))}",
            f"{ms(get(m, wr))} / {ms(get(m, wr, 'p99_us'))} / {ms(get(m, wr, 'max_us'))}",
            f"{ms(get(m, ww))} / {ms(get(m, ww, 'p99_us'))}",
            f"{m['peak_rss_mb']['value']:.0f} MB" if "peak_rss_mb" in m else "—",
        ]
        print(f"| {e} | " + " | ".join(row) + " |")

    notes = []
    for e in es:
        m = data[(e, size)]
        for key in ("shortest_path", "scc"):
            if key in m and m[key]["note"]:
                notes.append(f"- {e} {key}: {m[key]['note']}")
        for k, r in sorted(m.items()):
            if k.startswith("correct_") and r["value"] != 1:
                notes.append(f"- {e} {k}: {r['note']}")
    if notes:
        print("\nNotes:\n" + "\n".join(notes))

exit_log = os.path.join(HERE, "results", "exit-codes.log")
if os.path.exists(exit_log):
    print("\n#### Process exit codes\n\n```")
    print(open(exit_log).read().rstrip())
    print("```")
