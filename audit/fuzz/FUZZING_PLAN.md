# Fuzzing Plan — `uppsala` (`fix/pyff_part2`)

> **Integration status (what actually shipped in `audit/fuzz/`).** This external
> plan drove the harness set; here is how it maps to the implemented targets:
>
> | Plan harness | Shipped as | Notes |
> |---|---|---|
> | A `simd_scan_differential` | `fuzz_simd_differential` | implemented; **found & fixed** the §0 `needs_validation` divergence |
> | B `writer_escape_differential` | `fuzz_escape_differential` | implemented; semantic check runs on the escaped *fragment* (via `fuzz_exports::escape_to_string`), not a whole document |
> | C `parse_roundtrip` | `fuzz_roundtrip` | already present (fixpoint oracle) |
> | D `parse_bytes` | `fuzz_parse_bytes` | already present |
> | E `dom_mutations` | `fuzz_dom_mutate` | already present, more complete (real tree-walk id enumeration + attribute-node/root misuse ops) |
> | F `xpath_eval` | `fuzz_xpath` | already present |
> | G `xsd_validate` | `fuzz_xsd_builder` (+ `fuzz_xsd_regex`) | present; schema build + regex engine |
>
> The internals are exposed through the crate's `fuzzing` Cargo feature
> (`uppsala::fuzz_exports`), integrated into `src/simd.rs`/`src/lib.rs` — the
> intent of the plan's `simd_fuzz_exports.patch`, applied directly rather than
> as a separate patch. **The §0 finding is real and now fixed** (see
> `README.md` → "Differential harnesses & the `needs_validation` finding");
> analysis showed it was a benign over-report (never an under-report), and the
> masking fix makes SSE2 byte-identical to the scalar reference.
>
> Run paths differ from the plan's examples: harnesses live in `audit/fuzz/`,
> driven via `just fuzz` / `cargo +nightly fuzz run --fuzz-dir audit/fuzz`.

**Target:** `github.com/kushaldas/uppsala` @ `fix/pyff_part2`
**Library:** zero-dependency pure-Rust XML stack — parser, arena DOM, XPath 1.0, XSD 1.1 validation, XSD regex, serializer.
**Why this matters:** the library is slated to back SAML/federation (pyFF-style) identity services. XML parsers are one of the most attacked surfaces in existence (XXE, entity/expansion DoS, namespace confusion, serializer injection, parser-differential signature bypass). This branch additionally introduces **hand-written `unsafe` SSE2 SIMD** in `src/simd.rs`, which removes Rust's memory-safety guarantee for the parser/serializer hot loops. That combination — untrusted input + `unsafe` + a security-critical consumer — is precisely the profile Trail of Bits fuzzes hardest.

This plan follows the Trail of Bits Testing Handbook fuzzing methodology and their published fuzzing skills (`cargo-fuzz`, `harness-writing`, `fuzzing-dictionary`, `coverage-analysis`, `address-sanitizer`). It is deliberately **oracle-driven**: for `unsafe` SIMD, crashes are not the only signal — the strongest bugs are *behavioral divergences* between the SIMD path and its scalar reference, which are silent unless you assert them.

---

## 0. Headline finding (already reproduced)

Before writing a line of fuzzer, a differential model of the two code paths in `src/simd.rs` surfaced a real, reproducible divergence — so the first harness below is not speculative.

`scan_content_sse2` computes `needs_validation` over the **entire 16-byte chunk**, including bytes *after* the first delimiter, then returns. `scan_content_scalar` walks byte-by-byte and **stops at the delimiter**. Result: whenever a delimiter (`<`, `&`, `\r`, `]`) is followed by a non-ASCII (`>= 0x80`) or control byte within the same 16-byte window, the two paths return the same `pos` but a **different `needs_validation` flag**.

- Minimal witness: `b"<" + b"\xc3" + b"a"*14` → SSE2 `(0, true)` vs scalar `(0, false)`.
- ~2,900 divergent inputs appeared in a 200k random sample of length ≤ 40.
- `scan_attr_sse2` has the same structure and the same expected divergence; it must additionally be fuzzed **over all 256 possible `quote` bytes**, not just `"` and `'`.

**Direction matters for triage.** Here the SIMD path *over-reports* `needs_validation` (safe-but-wrong: caller does extra validation). The dangerous direction is the opposite — SIMD returning `needs_validation = false` where scalar says `true`, which would let invalid XML chars through unvalidated. The differential harness asserts exact tuple equality, so it catches **both** directions; triage classifies each. This same over/under logic applies with higher stakes to `scan_escape_run` (see Harness B), where an over-long "safe run" means the serializer emits an **unescaped `<` or `&`** — an XML/markup-injection primitive in a SAML context.

---

## 1. Target inventory and priority

Priority is by (attacker reachability × `unsafe`/memory-safety exposure × blast radius for identity services).

| # | Harness | Entry point | `unsafe`? | Why it's ranked here |
|---|---------|-------------|-----------|----------------------|
| **A** | `simd_scan_differential` | `simd::scan_content_*` / `scan_attr_*` | **yes (SSE2)** | Direct `unsafe`; already-found divergence; per-byte reachable from any document. Highest value. |
| **B** | `writer_escape_differential` | `simd::scan_escape_run` + `writer::write_escaped_run_dyn` | **yes (SSE2)** | `unsafe`; over-report ⇒ unescaped `<`/`&` in output ⇒ injection. New on this branch. |
| **C** | `parse_roundtrip` | `uppsala::parse` → `Document::to_xml` → re-parse | reaches A & B | End-to-end; catches SIMD bugs *in situ* + serializer round-trip fidelity. |
| **D** | `parse_bytes` | `uppsala::parse_bytes` | reaches A & B | Raw-byte entry incl. UTF-16 LE/BE + BOM auto-detection (encoding path, common in SAML POSTs). |
| **E** | `dom_mutations` | `Document` mutators + `prepare_xpath` | arena/`NodeId` | The three branch commits are UAF-adjacent (arena growth, NodeId stability, virtual-attr rejection). Op-sequence fuzzing. |
| **F** | `xpath_eval` | `XPathEvaluator::evaluate` | recursion | Recursive-descent evaluator → stack-exhaustion/complexity DoS. |
| **G** | `xsd_validate` | `XsdValidator::from_schema` + `validate` | recursion | Schema is also attacker-controlled in federation; identity-constraint + regex engine are complexity-DoS prone. |

The parser exposes hardening knobs (`with_max_depth`, `with_max_entity_expansion`, `with_forbid_dtd`, `with_forbid_entities`). **Fuzz both hardened and unhardened configurations** — unhardened to find the raw crash, hardened to prove the mitigation actually bounds it.

---

## 2. The harnesses (what each asserts)

Full runnable sources are in `fuzz/fuzz_targets/`. Summary of the **oracle** (the property that turns a silent bug into a crash) for each:

### A. `simd_scan_differential` — the crown jewel
Splits fuzz bytes with `arbitrary` into `(quote: u8, is_attr: bool, data: &[u8])` and asserts:
1. `scan_content_sse2(data) == scan_content_scalar(data)` (exact `(usize, bool)`).
2. `scan_attr_sse2(data, quote) == scan_attr_scalar(data, quote)` for the fuzzed `quote`.
3. Sanity invariants that must hold for *either* path: returned `pos <= data.len()`; and if `pos < data.len()`, `data[pos]` is genuinely one of the stop bytes for that mode. (Catches an OOB/late-stop even if both paths agree with each other but are both wrong.)
Run with **ASan on** — the unaligned `_mm_loadu_si128` loads are the thing ASan is here to watch at the tail/`len % 16` boundary. Exercise lengths 0,1,15,16,17,31,32,33 explicitly via the seed corpus.

### B. `writer_escape_differential`
Asserts `scan_escape_sse2(data, is_attr) == scan_escape_scalar(data, is_attr)`, **plus** a semantic safety oracle independent of the reference: feed the same string through `write_escaped_run_dyn` and assert the output contains no raw `<`, no raw `&` that doesn't begin a valid entity, and no raw `\r` — i.e. the escaper never emits an injectable byte regardless of what `scan_escape_run` claimed. This is the property that actually protects a SAML consumer.

### C. `parse_roundtrip`
`parse(s)` → if `Ok(doc)`, `let out = doc.to_xml(); let doc2 = parse(&out).expect(...)`; assert `doc2.to_xml() == out` (idempotent round-trip). Property: **parsing is a fixed point after one serialization**. Divergence means either a parser bug or a serializer that emits something it won't re-accept (a classic parser-differential / mutation vector). Also a pure no-panic/no-ASan-crash harness on the whole pipeline.

### D. `parse_bytes`
`uppsala::parse_bytes(data)` — never panics, never trips ASan, on arbitrary bytes. Specifically drives UTF-16 LE/BE ± BOM detection. Seed with UTF-16-encoded XML and BOM-prefixed samples.

### E. `dom_mutations`
Uses `arbitrary` to derive a *sequence* of operations (`append_child`, `insert_before/after`, `remove_child`, `replace_child`, `set_attribute`, `remove_attribute`, `prepare_xpath`) over IDs drawn from the live arena, interleaving `prepare_xpath()` (the function the branch fixes). Oracle: no panic/ASan; after any sequence, `to_xml()` still succeeds and re-parses. Targets the exact regressions the branch addresses — unbounded arena growth, NodeId stability across re-prepare, virtual-attribute-node rejection. Bound sequence length so hangs are distinguishable from DoS.

### F. `xpath_eval`
Split input into `(xml, expr)`; parse the doc; `XPathEvaluator::new().with_max_depth(64).with_max_node_visits(...).evaluate(&doc, doc.root(), expr)`. Oracle: no panic/crash. The `max_depth`/`max_node_visits` budgets are themselves under test — run one config with generous budgets to hunt stack overflow, one with tight budgets to prove they bound recursion (this mirrors Trail of Bits' repeated "uncontrolled recursion" DoS disclosures against Elastic, Wire, and the protobuf crates).

### G. `xsd_validate`
Split into `(schema_xml, instance_xml)`; `XsdValidator::from_schema(&schema_doc)`; if `Ok`, `validator.validate(&instance_doc)`. Oracle: no panic/crash/hang. Treat **the schema as attacker-controlled** — in federation the metadata *is* the input. The XSD regex NFA engine (`xsd_regex.rs`, custom matcher) is a prime catastrophic-backtracking / ReDoS candidate; consider a dedicated sub-harness feeding `(pattern, subject)` straight into it with a wall-clock timeout oracle.

---

## 3. Making the `unsafe` internals reachable

`scan_*` and their scalar/SSE2 halves are `pub(crate)`. Rather than fuzz only through the public parser (which would bury Harness A's signal), expose them behind a fuzz-only feature — the Testing Handbook's "if the SUT blocks the harness, patch the SUT" guidance. Apply `simd_fuzz_exports.patch` (included), which adds:

- a `fuzzing` Cargo feature (`[features] fuzzing = []`), and
- a `#[cfg(feature = "fuzzing")] pub mod fuzz_exports` in `src/simd.rs` re-exporting `scan_content_scalar/_sse2`, `scan_attr_scalar/_sse2`, `scan_escape_scalar/_sse2`, plus a crate-root `pub use`.

The `fuzz/Cargo.toml` enables it via `uppsala = { path = "..", features = ["fuzzing"] }`. This keeps the export out of normal release builds entirely.

---

## 4. Inputs: corpus + dictionary (the part that decides success)

A coverage-guided fuzzer is only as good as its seeds and dictionary. For a structured format like XML this is the difference between scratching the lexer and reaching the XSD identity-constraint evaluator.

**Seed corpus** (`fuzz/corpus_seeds/`, copy into `fuzz/corpus/<target>/` per target):
- Minimal well-formed docs; deeply nested elements; long text runs (to force the SIMD 16-byte loop + tail).
- **Boundary-length** payloads: text/attribute runs of exactly 15/16/17/31/32/33 bytes, and the same straddling a delimiter (this is where Harness A/B live).
- UTF-8 multibyte content adjacent to delimiters (`café<`, `<ö…`); UTF-16 LE/BE with and without BOM (Harness D).
- Adversarial XML: entity-expansion / "billion laughs" nests, DTDs, `xmlns` redefinitions, CDATA with `]]>` edge cases, `]` runs, bare `&`, `<` in odd positions.
- **Real SAML fixtures**: a handful of real (sanitized) SAML AuthnRequests/Responses and SAML metadata (`EntityDescriptor`/`EntitiesDescriptor`) — this is the actual production distribution and pulls the fuzzer straight into the code paths that matter for you.
- Reuse the repo's own W3C conformance corpus (`test-data/`) as seeds — thousands of pre-shaped valid/invalid docs are an enormous head start; point corpus minimization at them.
- For XSD (Harness G): pairs of schema + instance, including recursive/mutually-recursive type definitions and pathological regex patterns for the NFA engine.

**Dictionary** (`fuzz/xml.dict`, provided): XML/namespace/DTD/XSD/SAML tokens (`<![CDATA[`, `]]>`, `<!DOCTYPE`, `<!ENTITY`, `xmlns:`, `xsi:type`, `SYSTEM`, `PUBLIC`, `&#x`, `saml:`, `samlp:`, `ds:Signature`, `EntityDescriptor`, …). Pass with `-dict=xml.dict`. Dictionaries let libFuzzer synthesize keywords it would essentially never discover by random mutation.

---

## 5. Tooling & runtime configuration

Trail of Bits' de-facto Rust stack is **cargo-fuzz (libFuzzer backend)** on **nightly**.

```bash
rustup toolchain install nightly --component llvm-tools-preview
cargo install cargo-fuzz cargo-binutils rustfilt
# from repo root, after applying simd_fuzz_exports.patch and dropping in fuzz/
cargo +nightly fuzz run simd_scan_differential -- -dict=fuzz/xml.dict -max_len=4096
```

Key knobs:
- **AddressSanitizer: keep it ON.** cargo-fuzz enables ASan by default. The Handbook notes ASan is ~2× overhead and can be disabled for *pure-safe* Rust — **that carve-out does not apply here.** You have `unsafe` SSE2 with raw-pointer loads, so ASan is exactly the detector you want for Harnesses A–D. (Optionally keep a second, ASan-off run of the pure-logic differential Harness A for throughput, since the differential assertion itself catches logic bugs without ASan — but always keep at least one ASan-on run over the raw loads.)
- `-max_len` sized per harness (small for A/B, larger for C/G).
- `-timeout=10` and `-rss_limit_mb` to turn hangs/OOM (entity expansion, ReDoS) into reportable findings rather than silent stalls.
- **Coverage-guided iteration:** after an initial run, `cargo +nightly fuzz coverage <target>` → render HTML → confirm the SIMD tail loop, the UTF-16 branch, the XSD identity/regex paths are actually hit; grow seeds/dictionary for gaps. Coverage is the feedback loop, not a one-time check.
- **Scale-out:** once harnesses are stable, move the long-running campaign to multi-core (LibAFL shim under cargo-fuzz, or AFL++), and wire the whole set into **OSS-Fuzz** for continuous fuzzing + automated regression corpus — appropriate given the "critical identity services" bar.

---

## 6. Triage rubric

- **ASan report** (heap/stack overflow, use-after-free, bad free) → memory-safety bug in `unsafe` SIMD or the arena. Highest severity; reproduce with `cargo +nightly fuzz run <target> <artifact>`.
- **Differential assertion fail (A/B)** → classify by direction. *SIMD stricter/over-reports* = correctness + parser-differential (still a cross-arch divergence bug). *SIMD looser/under-reports, or escaper emits injectable byte* = **security bug** (validation bypass / markup injection). Both get fixed; the latter is release-blocking for identity use.
- **Round-trip assertion fail (C)** → serializer emits something the parser won't re-accept, or vice-versa: a mutation/canonicalization vector — directly relevant to SAML signature integrity.
- **Panic** → availability bug (DoS) in a library that should return `Err`, not unwind. Prioritize by reachability from `parse`/`parse_bytes`.
- **Timeout/OOM** → algorithmic-complexity DoS (entity expansion, quadratic parsing, ReDoS). Verify the `with_max_*` budgets actually bound it; if a documented mitigation doesn't, that's a finding about the mitigation.

Regression: commit every crashing artifact (post-`cargo fuzz tmin` minimization) into the seed corpus so fixes stay fixed. Keep corpora in **access-controlled** storage, not a public repo branch — a corpus is a map to your soft spots.

---

## 7. Suggested rollout

1. **Week 1 — land the `unsafe` differential.** Apply the patch, stand up Harnesses **A** and **B**, reproduce the known `needs_validation` divergence, decide the intended contract (should the flag describe only `[0, pos)`? if so the SSE2 masks must be cleared past the delimiter), fix, and keep the harness green in CI. This is the highest-risk, lowest-effort win.
2. **Week 1–2 — end-to-end.** Harnesses **C** and **D** with the SAML + W3C seed corpus and the dictionary; get coverage HTML and close obvious gaps.
3. **Week 2–3 — stateful + recursive.** Harnesses **E**, **F**, **G**; validate the depth/visit/expansion budgets under adversarial input; add the dedicated XSD-regex ReDoS sub-harness with a timeout oracle.
4. **Ongoing — continuous.** Multi-core campaign + OSS-Fuzz integration; treat any new `unsafe` block or SIMD change as requiring a matching differential harness before merge.

---

## References (Trail of Bits, consulted for methodology)

- Testing Handbook — Fuzzing (cargo-fuzz): https://appsec.guide/docs/fuzzing/rust/cargo-fuzz/
- Testing Handbook — Writing harnesses: https://appsec.guide/docs/fuzzing/rust/techniques/writing-harnesses/
- Testing Handbook launch (scope: libFuzzer / AFL++ / cargo-fuzz, ASan, dictionaries, harness quality): ToB blog, "Master fuzzing with our new Testing Handbook chapter"
- ToB skills: `cargo-fuzz`, `harness-writing`, `fuzzing-dictionary`, `coverage-analysis`, `address-sanitizer`, `libfuzzer`, `libafl`, `aflpp`, `property-based-testing`, `differential-review` (trailofbits.com/skills/…)
- Invariant/oracle-driven fuzzing (beyond crashes): ToB blog, "Finding mispriced opcodes with fuzzing" (LibAFL shim, multi-core)
- Parser bug-class precedent: ToB disclosures — uncontrolled-recursion DoS (Elastic WKT, Wire, rust-protobuf), DoS in protobuf-python/-java & XStream, "Stranger Strings" (SQLite), memory corruption in GnuTLS X.509; paper "Input-Driven Recursion: Ongoing Security Risks."

*Note: harness sources compile against the `fix/pyff_part2` API as read on 2025-… ; they were authored without a Rust toolchain in the authoring environment, so do a `cargo +nightly fuzz build` pass and adjust any type/constructor name the branch has since renamed. The logic and oracles are the substance; the API bindings are mechanical.*
