# Performance

Uppsala uses accelerated byte scanning for text content and attribute values:
SSE2 SIMD on x86_64 (16 bytes per iteration) and a one-pass scalar delimiter
scanner elsewhere. Parsing throughput depends heavily on document shape: long
plain-text spans and large attribute values are favorable, while very small
documents are dominated by fixed parser overhead.

## Results

The tables below compare release builds (`cargo run --release`, no extra
profile overrides) of Uppsala 0.5.2 against a local checkout of roxmltree
0.21.1. Results are median parse times from 101 samples on x86_64 (the SSE2
scanner path). The `Ratio` column is
`roxmltree / Uppsala`; values above 1.0 mean Uppsala parsed faster.

### roxmltree benchmark inputs

| File | Size | Uppsala | roxmltree | Ratio |
|------|------|---------|-----------|-------|
| fonts.conf | 429 B | 2.9 us | 4.0 us | 1.38x |
| medium.svg | 155 KB | 306 us | 489 us | 1.60x |
| large.plist | 321 KB | 1.72 ms | 2.39 ms | 1.39x |
| huge.xml | 835 KB | 3.69 ms | 4.80 ms | 1.30x |
| gigantic.svg | 1.34 MB | 411 us | 1.94 ms | 4.73x |
| cdata.xml | 102 KB | 215 us | 252 us | 1.17x |
| text.xml | 129 KB | 650 us | 5.96 ms | 9.17x |
| attributes.xml | 271 KB | 1.48 ms | 5.24 ms | 3.55x |

### SAML-shaped inputs

The main production target is SAML: namespace-heavy documents in the 3-30 KB
range with signed assertions. On generated SAML-shaped inputs, default
namespace-aware parsing is consistently faster than roxmltree.

| File | Size | Uppsala | roxmltree | Ratio |
|------|------|---------|-----------|-------|
| SAML small | 3.5 KB | 7.7 us | 13.3 us | 1.74x |
| SAML medium | 9.1 KB | 25.1 us | 29.0 us | 1.16x |
| SAML large | 27.8 KB | 62.7 us | 92.1 us | 1.47x |

Disabling namespace resolution improves some ordinary XML inputs further, but
SAML users should usually keep namespace-aware parsing enabled.

## Running the performance test

The comparison harness is checked into the repo under `performance-harness/`.
It is kept outside the main crate's targets so normal library builds and tests
do not depend on roxmltree.

### Prerequisites

The harness expects a sibling checkout of roxmltree next to the Uppsala repo:

```text
code/
  uppsala/
    performance-harness/
  roxmltree/
```

Because Uppsala's hand-written SSE2 scanner only runs on x86_64, run the harness
on an x86_64 machine to reproduce the SIMD numbers above. On aarch64 / Apple
Silicon it exercises the scalar fallback instead.

### Run roxmltree's benchmark inputs

```bash
cargo run --release --manifest-path performance-harness/Cargo.toml -- \
  suite ../roxmltree/benches 101
```

### Run the SAML-shaped inputs

```bash
cargo run --release --manifest-path performance-harness/Cargo.toml -- \
  saml 101
```

### Run a single file

```bash
cargo run --release --manifest-path performance-harness/Cargo.toml -- \
  file ../roxmltree/benches/large.plist 101
```

The trailing number is the sample count (default `31`). Each run does a short
warmup, then reports medians in microseconds. Output is tab-separated with
columns for both namespace-aware and namespace-disabled Uppsala timings:

```text
file  bytes  uppsala_ns_us  uppsala_no_ns_us  roxmltree_us  ratio_ns  ratio_no_ns
```

The `ratio_ns` column corresponds to the default namespace-aware mode used in
the tables above; `ratio_no_ns` is Uppsala with namespace resolution disabled.

See [`performance-harness/README.md`](../performance-harness/README.md) for
additional notes.

## Profiling with `perf`

To find hot functions when optimizing the parser, build the harness and record
a profile (Linux, `perf` installed):

```bash
cargo build --release --manifest-path performance-harness/Cargo.toml
perf record -g --call-graph dwarf \
  ./target/release/uppsala-performance-harness suite ../roxmltree/benches 101
perf report --no-children --sort=dso,symbol
```

Focus on self-time (`--no-children`) to identify the real bottleneck rather than
its callers, and use `perf annotate --symbol=<fn>` to drill into a hot loop's
per-instruction sample counts.
