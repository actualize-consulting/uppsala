# Fuzzing uppsala

A [cargo-fuzz](https://rust-fuzz.github.io/book/cargo-fuzz.html) / libFuzzer
harness suite for uppsala, targeting the untrusted-input surfaces — the parser,
the serializer (including the **`unsafe` SSE2 SIMD scanners** in `src/simd.rs`),
the DOM mutation + `prepare_xpath()` machinery, XPath, XSLT, and the XSD builder.

This crate is **not** part of uppsala's build. Its `[workspace]` table detaches
it, so `cargo build`/`cargo test` at the repo root never pull in
`libfuzzer-sys`/`arbitrary` and uppsala keeps its zero-dependency guarantee.
Only the fuzzer compiles this crate.

## Why these harnesses

uppsala's only `unsafe` code is the three SSE2 routines in `src/simd.rs`:
`scan_content_sse2` and `scan_attr_sse2` (parser hot loops) and the newer
`scan_escape_sse2` (serializer escape scanner). A wrong length, a misread lane
mask, or a bad alignment there is a memory-safety bug that ASan will catch. The
harnesses are chosen to drive every one of those routines with adversarial input.

| Target | Surface exercised | `unsafe` / recently-changed code it stresses |
|---|---|---|
| `fuzz_parse` | `parse(&str)` | `scan_content_sse2`, `scan_attr_sse2` |
| `fuzz_parse_bytes` | `parse_bytes(&[u8])` | UTF-16/BOM decode + parser |
| `fuzz_roundtrip` | parse → serialize → reparse (fixpoint oracle) | `scan_escape_sse2`, sibling-walk serializer |
| `fuzz_serialize` | **builds an arbitrary DOM** → serialize 3 ways | `scan_escape_sse2` at all alignments; name sanitizers |
| `fuzz_dom_mutate` | arbitrary edit sequence + `prepare_xpath()` | attribute-node arena recycling; `is_linkable_node` guards |
| `fuzz_xpath` | XPath lex/parse/eval | evaluator, doc-order index |
| `fuzz_transform` | `transform(xslt, xml)` | XSLT engine + XPath + serializer |
| `fuzz_xsd_regex` | `XsdRegex::compile` + `is_match` | backtracking NFA matcher |
| `fuzz_xsd_builder` | `XsdValidator::from_schema` | schema builder |

`fuzz_serialize` and `fuzz_dom_mutate` are **structure-aware** (they use the
`arbitrary` crate to build DOMs / edit sequences from the raw bytes), so they
reach code the byte-oriented parser harnesses can't — in particular the
serializer fed with control characters, invalid-XML scalars, `]]>`, `?>` and
multibyte sequences at arbitrary offsets, and the tree mutators fed with virtual
attribute nodes and the document root (the operands that used to corrupt the
tree). Both bound tree depth and node count so a harness-side stack overflow
can't masquerade as a library finding.

### Oracles

Beyond "don't crash / don't trip ASan", two harnesses assert semantic
invariants:

- **`fuzz_roundtrip`** — serialization is a fixpoint: `parse(s).to_xml()` must
  equal `parse(parse(s).to_xml()).to_xml()`. The assert only fires when both
  parses succeed, so parser resource limits never cause false positives.
- **`fuzz_dom_mutate`** — after any edit sequence the document must still
  serialize to XML that reparses; a wiped child list or a cyclic sibling link
  shows up as a reparse failure, an ASan report, or a libFuzzer timeout.

Input splitting for the multi-part harnesses: `fuzz_xpath` and `fuzz_xsd_regex`
split on the first newline (`expr\nxml`, `pattern\ninput`); `fuzz_dom_mutate`
uses the first line as seed XML and the rest as edit opcodes; `fuzz_transform`
splits on a NUL byte (`stylesheet\0source`) since NUL never appears in XML.

## Quick start

```bash
# One-time, per machine:
just fuzz-setup            # nightly toolchain + llvm-tools + cargo-fuzz

# Fuzz everything on a remote box, then walk away:
just fuzz                  # detached tmux session 'uppsala-fuzz', runs forever
just fuzz-attach           # reattach to watch
just fuzz-crashes          # list any crash artifacts found
just fuzz-stop             # stop the whole session
```

`just fuzz-setup` installs cargo-fuzz through `sfw` (Socket Firewall), matching
this repo's package-install policy.

## Running on a remote multicore server (tmux)

`just fuzz` is built for exactly this. It:

1. builds all targets once (so the windows don't fight over the build lock),
2. opens a **detached** tmux session `uppsala-fuzz` with one window per target,
3. splits cores evenly across targets — each target runs libFuzzer **fork mode**
   (`-fork=N`), which keeps fuzzing across crashes and saves each one.

Because the session is detached and owned by the tmux server, it **survives SSH
logout**. Typical remote workflow:

```bash
ssh bigbox
cd uppsala
just fuzz-setup            # first time only
just fuzz                  # launches detached session, returns immediately
# ... log out; fuzzing continues ...

# later:
ssh bigbox
cd uppsala
just fuzz-attach           # Ctrl-b n / Ctrl-b p to switch targets, Ctrl-b d to detach
just fuzz-crashes          # anything found?
just fuzz-stop             # done
```

Run for a fixed budget instead of forever:

```bash
just fuzz 3600             # 1 hour per target, then each stops on its own
```

Focus a single target in the foreground (all cores, fork mode):

```bash
just fuzz-one fuzz_roundtrip 3600
```

Tuning knobs (env vars, honored by the scripts):

| Var | Default | Meaning |
|---|---|---|
| `JOBS` | `nproc` (or cores/targets under `just fuzz`) | fork workers for a target |
| `MAX_LEN` | `16384` | max input length in bytes |
| `SESSION` | `uppsala-fuzz` | tmux session name |

## Keep the SIMD sanitizer ON

Because the SIMD scanners are `unsafe`, **do not** pass `--sanitizer none`.
AddressSanitizer is on by default here and is the whole point of fuzzing this
code — it's what turns a mis-indexed SSE2 lane into a reported crash instead of
silent corruption. (The `--sanitizer none` 2× speedup advice only applies to
100% safe-Rust projects.)

## Triage

A crash is written under `audit/fuzz/artifacts/<target>/`. Reproduce it with a
stack trace:

```bash
just fuzz-crashes                                  # list them
just fuzz-repro fuzz_parse audit/fuzz/artifacts/fuzz_parse/crash-<hash>
```

The failing input is just bytes; for the split harnesses, remember the
separator (newline / NUL) when reading it.

## Corpus and coverage

- **Seeds** (curated, tracked): `audit/fuzz/seeds/<target>/`. `run.sh` copies
  them into the working corpus on start.
- **Working corpus** (grows as the fuzzer finds coverage; git-ignored):
  `audit/fuzz/corpus/<target>/`.
- **Dictionaries**: `audit/fuzz/dict/` (`xml.dict`, `xpath.dict`,
  `xsd_regex.dict`) — auto-selected per target by `run.sh`.

Coverage report and corpus minimization:

```bash
just fuzz-coverage fuzz_roundtrip   # HTML report under audit/fuzz/coverage/<target>/html
just fuzz-cmin fuzz_roundtrip       # drop redundant corpus entries
```

`fuzz-coverage` needs `llvm-tools-preview` (installed by `just fuzz-setup`) plus
`cargo-binutils` and `rustfilt` (`sfw cargo install cargo-binutils rustfilt`).

## Manual invocation (no just)

Everything routes through `cargo +nightly fuzz ... --fuzz-dir audit/fuzz`:

```bash
cargo +nightly fuzz build --fuzz-dir audit/fuzz
cargo +nightly fuzz run   --fuzz-dir audit/fuzz fuzz_roundtrip -- -max_total_time=300
cargo +nightly fuzz run   --fuzz-dir audit/fuzz fuzz_roundtrip -- -fork=$(nproc) -ignore_crashes=1
```

## Layout

```
audit/fuzz/
├── Cargo.toml              # detached fuzz crate (uppsala + libfuzzer-sys + arbitrary)
├── fuzz_targets/*.rs       # the 9 harnesses
├── seeds/<target>/         # curated seed inputs (tracked)
├── dict/*.dict             # libFuzzer dictionaries
└── scripts/
    ├── common.sh           # shared paths, target list, dict/tool checks
    ├── build.sh            # build all targets once
    ├── run.sh              # run one target, fork mode, auto seed+dict
    ├── fuzz-all.sh         # tmux orchestrator (one window per target)
    ├── repro.sh            # reproduce a crash artifact
    ├── minimize.sh         # cmin a corpus
    └── coverage.sh         # HTML coverage report
```
