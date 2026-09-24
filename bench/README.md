# Graphite storage benchmark

Compares embedded storage/query engines for Graphite's code graph on a
synthetic, seeded, code-shaped dataset. Results and analysis:
[`references/studies/storage-benchmark.md`](../references/studies/storage-benchmark.md).

## Candidates

| crate | engine | backends | blast-radius strategies |
|---|---|---|---|
| `cozo-bench` | CozoDB via the `mnestic` fork (`=0.18.0`, crate imported as `cozo`) | `mem`, `sqlite`, `newrocksdb` | `recursive` (min-depth lattice rule), `unrolled` (layer_1..layer_N rules), `rust_bfs` (one indexed query per level) |
| `lbug-bench` | LadybugDB (`lbug =0.20.4`, maintained Kuzu fork) | `disk`, `mem` | `cypher_shortest` (`*SHORTEST 1..N`), `rust_bfs` (one `UNWIND` lookup per level), `cypher_varlen` (`*1..N`, 5 s timeout) |
| `diy-bench` | per-file facts in redb (`=4.3.0`) + in-memory adjacency; Ascent (`=0.8.1`) | `redb` | `rust_bfs`, `ascent_per_query` |

`common` holds the dataset generator, a plain-Rust reference implementation
(ground truth for every correctness check) and the measurement harness.

## Run

```bash
./run.sh                 # small + medium, every engine/backend in its own process
./run.sh large           # 200K symbols / ~1M edges
python3 report.py medium # markdown tables from results/*.jsonl
```

Single engine:

```bash
LBUG_VERSION=0.20.4 cargo build --release --workspace
./target/release/cozo-bench --size medium --backend newrocksdb
./target/release/lbug-bench --size medium --backend disk
./target/release/diy-bench  --size medium
```

`--skip-algos` skips SCC / Louvain / PageRank.

- Raw results: `results/<engine>-<size>.jsonl` (one JSON object per metric).
- Crash detection: `results/exit-codes.log` (non-zero exit = the engine crashed or aborted).
- Scratch databases and logs: `.data/` (gitignored).

## Dataset

`Dataset::generate` (seeded, deterministic): 10 symbols per file, 50 files per
module, ~5 outgoing edges per symbol. Edge kinds: calls 60%, imports 20%,
implements 5%, contains 15%. 15% of edges are INFERRED, and half of those are
resolved. Shape:

- Layered: files depend on nearby earlier files in their module (99.8%) and on
  the public API of nearby lower modules (99.9%).
- Sparse back-edges produce realistic small SCCs.
- The lowest 1% of symbols are utility hubs, reached with a power law.

| size | symbols | edges | hub blast(d10) | median blast(d10) |
|---|---|---|---|---|
| small | 5,000 | ~24K | ~all | ~250 |
| medium | 50,000 | ~240K | ~all | ~230 |
| large | 200,000 | ~960K | ~all | ~220 |

The confidence rule applied at query time is: an edge is *trusted* if it is
EXTRACTED, or INFERRED with a resolved target.

## Metrics

- **Blast radius:** depth 3 and depth 10, shallowest depth per node, from the
  hub and from the median symbol, plus a trusted-only variant.
- **Other queries:** shortest path; SCC / Louvain / PageRank.
- **Incremental update:** replace one file's facts (~10 symbols / ~50 edges) in
  one atomic transaction.
- **Correctness:** every strategy is checked against the reference on the
  initial state, then again after 200 incremental updates, against both a full
  rebuild and the reference.
- **Watcher scenario:** read latency while a writer thread applies per-file
  updates continuously, and at 20 updates/s.
- **Cost:** build time, on-disk size, reopen time, peak RSS.

## Environment notes

- LadybugDB downloads a prebuilt static `liblbug` at build time. `run.sh` sets
  `LBUG_VERSION=0.20.4` so the library matches the crate.
- LadybugDB's graph algorithms live in the ALGO extension. On macOS arm64 the
  prebuilt extension links against vendored `libnetworkit` / `libarrow` /
  `libomp` dylibs that are not shipped. Homebrew's `networkit` 11.2.2 is
  ABI-incompatible (`Symbol not found: __ZTVN9NetworKit5GraphE`), so SCC,
  Louvain and PageRank report `unsupported` for LadybugDB here.
