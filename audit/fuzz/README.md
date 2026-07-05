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
| `fuzz_simd_differential` | **SSE2 vs scalar**, `scan_content`/`scan_attr` | direct equality check on the `unsafe` scanners (highest value) |
| `fuzz_escape_differential` | **SSE2 vs scalar**, `scan_escape` + escaper safety | `scan_escape_sse2`; asserts no injectable byte survives escaping |
| `fuzz_parse` | `parse(&str)` | `scan_content_sse2`, `scan_attr_sse2` |
| `fuzz_parse_bytes` | `parse_bytes(&[u8])` | UTF-16/BOM decode + parser |
| `fuzz_pull` | **pull vs DOM**, `PullParser` event stream (scan-only) | event-stream invariants (ADR 0018) + accept/reject agreement with `parse` |
| `fuzz_roundtrip` | parse → serialize → reparse (fixpoint oracle) | `scan_escape_sse2`, sibling-walk serializer |
| `fuzz_serialize` | **builds an arbitrary DOM** → serialize 3 ways | `scan_escape_sse2` at all alignments; name sanitizers |
| `fuzz_dom_mutate` | arbitrary edit sequence + `prepare_xpath()` | attribute-node arena recycling; `is_linkable_node` guards |
| `fuzz_xpath` | XPath lex/parse/eval | evaluator, doc-order index |
| `fuzz_transform` | `transform(xslt, xml)` | XSLT engine + XPath + serializer |
| `fuzz_xsd_regex` | `XsdRegex::compile` + `is_match` | backtracking NFA matcher |
| `fuzz_xsd_builder` | `XsdValidator::from_schema` plus optional instance validation | schema builder + identity constraints |

### Differential harnesses & the `needs_validation` finding

The two `*_differential` targets compare each `unsafe` SSE2 scanner against its
scalar reference *directly* (via the crate's `fuzzing` feature, which exposes
`uppsala::fuzz_exports`). This is the strongest way to test SIMD: many SIMD bugs
don't crash, they just compute a different answer — invisible unless you assert
the two paths agree. Because it forces the SSE2 tail/`len % 16` path over
unaligned `_mm_loadu_si128` loads, it is also the best ASan target.

Building these targets found a **real (benign) divergence** and it has been
fixed on this branch:

> `scan_content_sse2` accumulated `needs_validation` over the whole 16-byte
> chunk, including bytes *after* the first delimiter, while `scan_content_scalar`
> stops at the delimiter. So `"<" + 0xC3 + "a"*14` gave SSE2 `(0, true)` vs
> scalar `(0, false)`. Direction analysis (3M random trials): the position
> always matched and the SSE2 path only ever *over-*reported the flag — the
> parser (`parser.rs`) uses it solely to decide whether to validate the returned
> run `data[..pos]`, so an over-report meant redundant validation of a clean
> range, never a skipped validation of a dirty one. Not exploitable, but a genuine
> cross-path inconsistency. **Fix:** mask the validation lanes to bytes before
> the delimiter (`src/simd.rs`), making SSE2 byte-identical to scalar (verified
> over 3M random trials + an exhaustive boundary sweep). `fuzz_simd_differential`
> is now the permanent regression guard; a unit test
> (`content_flag_matches_scalar_when_delim_precedes_invalid`) pins the witness.

`fuzz_escape_differential` additionally asserts a **reference-independent safety
property**: the real escaper's output on any fragment never contains a raw `<`,
`>`, `\r`, or a bare `&` (nor a raw `"`/`\t`/`\n` in attribute context) — the
markup-injection guarantee that matters to a SAML consumer.

`fuzz_serialize` and `fuzz_dom_mutate` are **structure-aware** (they use the
`arbitrary` crate to build DOMs / edit sequences from the raw bytes), so they
reach code the byte-oriented parser harnesses can't — in particular the
serializer fed with control characters, invalid-XML scalars, `]]>`, `?>` and
multibyte sequences at arbitrary offsets, and the tree mutators fed with virtual
attribute nodes and the document root (the operands that used to corrupt the
tree). Both bound tree depth and node count so a harness-side stack overflow
can't masquerade as a library finding.

### Oracles

Beyond "don't crash / don't trip ASan", three harnesses assert semantic
invariants:

- **`fuzz_pull`** — the scan-only `PullParser` event stream must satisfy the
  ADR 0018 stream invariants (start/end element balance with matching names
  and depths, namespace-event balance, in-bounds byte ranges, fused iterator
  after an error), direct `next_event()` must fuse after any returned error,
  and the pull stream must accept/reject the input exactly like the DOM parser.
  This oracle class is what caught the empty-entity end-of-document regression
  (W3C valid-sa-023) during the pull-parser bring-up.
- **`fuzz_roundtrip`** — serialization is a fixpoint: `parse(s).to_xml()` must
  equal `parse(parse(s).to_xml()).to_xml()`. The assert only fires when both
  parses succeed, so parser resource limits never cause false positives.
- **`fuzz_dom_mutate`** — after any edit sequence the document must still
  serialize to XML that reparses; a wiped child list or a cyclic sibling link
  shows up as a reparse failure, an ASan report, or a libFuzzer timeout.

Input splitting for the multi-part harnesses: `fuzz_xpath` and `fuzz_xsd_regex`
split on the first newline (`expr\nxml`, `pattern\ninput`); `fuzz_dom_mutate`
uses the first line as seed XML and the rest as edit opcodes; `fuzz_transform`
splits on a NUL byte (`stylesheet\0source`) or the ASCII seed marker
`\n---XML---\n`; `fuzz_xsd_builder` splits on NUL or
`\n---INSTANCE---\n` when the input also carries an instance document.

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

### Extended corpus (from test-data/corpus)

`scripts/fetch_corpus.sh` assembles a broader corpus under
`test-data/corpus/` (libxml2 `test/recurse`, `test/schemas`, `test/XPath`,
and fuzzer dictionaries at a pinned commit; dvyukov/go-fuzz-corpus if online;
generated real-world dialect samples and XXE/round-trip payloads). Import it
into the working set with:

```bash
just fetch-corpus        # assemble test-data/corpus/ (idempotent)
just fuzz-seed-import    # copy into audit/fuzz/corpus/<target>/ + merge dicts
```

`fuzz-seed-import` (`scripts/seed-import.sh`) lands the large external inputs
in the **git-ignored working corpus** (`corpus/<target>/`, not the tracked
`seeds/`), so git stays lean, and folds the libxml2 `xml`/`xpath`/`schema`/
`regexp` dictionary tokens into the tracked `dict/*.dict` (deduplicated). It
is a no-op when the corpus has not been fetched.

## Mapping to SECURITY_AUDIT findings

The fuzz targets provide continuous coverage of the same surfaces the
`SECURITY_AUDIT.md` findings (and `tests/security_audit.rs`,
`tests/hardening_regressions.rs`, `tests/security_regressions.rs`,
`tests/security_corpus.rs`) pin as regressions:

| Finding area | Regression tests | Fuzz target(s) |
|---|---|---|
| Entity-expansion DoS (billion laughs, quadratic, parameter laughs) | `security_corpus::recurse_*`, `security_audit` F-01/F-02 | `fuzz_parse`, `fuzz_parse_bytes`, `fuzz_pull` (seeded from `security/recurse`) |
| Pull/DOM parser agreement (`tests/pull_differential.rs`, `w3c_xmlconf` sweep) | `pull_differential::*`, `w3c_pull_event_stream_agrees_with_dom_parser` | `fuzz_pull` (stream invariants + accept/reject agreement) |
| Deep-nesting stack safety (F-03) | `security_audit::deep_nesting_*` | `fuzz_parse` |
| XXE / external-entity non-resolution | `security_corpus::xxe_*`, `security_corpus::recurse_external_*` | `fuzz_parse` (seeded from `security/xxe`) |
| Round-trip smuggling (F-13/14/15) | `security_corpus::roundtrip_*`, `security_regressions` | `fuzz_roundtrip` (fixpoint oracle) |
| UTF-16 / encoding decode | `encoding_matrix::*`, `security_audit` UTF-16 | `fuzz_parse_bytes` |
| SIMD scalar/SSE2 divergence | `simd::tests::content_flag_matches_scalar_*` | `fuzz_simd_differential`, `fuzz_escape_differential` |
| XSD regex ReDoS (F-04) | `hardening_regressions::xsd_regex_polynomial_redos` | `fuzz_xsd_regex` |
| XPath axis budget (F-05) | `hardening_regressions::xpath_axis_expansion_is_budgeted` | `fuzz_xpath` |
| Pull direct error fusion | `security_regressions::pull_next_event_fuses_after_direct_error` | `fuzz_pull` |
| XSLT computed-name injection | `security_regressions::xslt_computed_*_rejects_markup_injection` | `fuzz_transform` |
| XPath trailing tokens and flat-chain depth | `security_regressions::xpath_public_evaluate_rejects_trailing_tokens`, `security_regressions::xpath_flat_operator_chains_observe_depth_limit` | `fuzz_xpath` |
| XSD identity tuple lookup complexity | `security_regressions::xsd_identity_tuple_index_preserves_decimal_duplicate_detection`, `security_regressions::xsd_keyref_tuple_index_reports_missing_references` | `fuzz_xsd_builder` |

`fuzz-coverage` needs `llvm-tools-preview` (installed by `just fuzz-setup`); it
calls `llvm-cov` directly rather than the `cargo cov` wrapper, which panics on
some `cargo-binutils`/clap combinations. `rustfilt` is optional — install it
(`sfw cargo install rustfilt`) for demangled function names; without it the
report still renders with mangled symbols.

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
├── fuzz_targets/*.rs       # the 12 harnesses (3 differential + 9 end-to-end)
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
