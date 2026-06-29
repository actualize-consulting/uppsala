# Uppsala Performance Harness

This is a local comparison harness for Uppsala's XML parser against a sibling
checkout of `roxmltree`.

It is intentionally checked into the repo so the same commands can be run later
on an x86_64 machine. That matters because Uppsala's hand-written SSE2 scanner
only runs on x86_64; Apple Silicon and other non-x86_64 targets use the scalar
fallback.

Expected layout:

```text
code/
  uppsala/
    performance-harness/
  roxmltree/
```

The harness measures parse-only time. It reports medians in microseconds after a
small warmup. The `Ratio` column is `roxmltree / Uppsala`; values above `1.0`
mean Uppsala parsed faster.

## Run One File

```bash
cargo run --release --manifest-path performance-harness/Cargo.toml -- \
  file ../roxmltree/benches/large.plist 101
```

The final argument is the sample count. If omitted, it defaults to `31`.

## Run roxmltree's Benchmark Inputs

```bash
cargo run --release --manifest-path performance-harness/Cargo.toml -- \
  suite ../roxmltree/benches 101
```

This runs:

- `fonts.conf`
- `medium.svg`
- `large.plist`
- `huge.xml`
- `gigantic.svg`
- `cdata.xml`
- `text.xml`
- `attributes.xml`

## Run SAML-Shaped Inputs

```bash
cargo run --release --manifest-path performance-harness/Cargo.toml -- \
  saml 101
```

This generates three namespace-heavy SAML-like responses in memory:

- `saml-small`, about 3.5 KB
- `saml-medium`, about 9 KB
- `saml-large`, about 28 KB

These are synthetic parser fixtures, not security or interoperability test
vectors.

## Notes

- Uppsala is benchmarked twice: namespace-aware default mode and
  namespace-disabled mode.
- SAML users should usually care about the namespace-aware column.
- On x86_64, this harness exercises Uppsala's SSE2 delimiter scanner. On
  aarch64/Apple Silicon, it exercises the scalar fallback.
- The harness intentionally lives outside the main crate's `[[bench]]` targets
  so normal library builds and tests do not depend on `roxmltree`.
