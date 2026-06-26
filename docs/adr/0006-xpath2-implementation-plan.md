# Implement XPath 2.0 With SIMD and Zero-Copy Parsing

## Summary

Implement a first-class `xpath2` module alongside the existing XPath 1.0 engine. Target full XPath 2.0 language behavior using the local spec at `specs/xpath2.0.html`, with zero runtime dependencies, borrowed expression tokens, SIMD-assisted lexing, XDM sequences, typed atomic values, schema-aware evaluation, resolver hooks, and vendored upstream conformance tests from W3C `qt3tests`.

## Status Snapshot

**Status:** Unreleased, broad implementation. This ADR describes the target
implementation and the current in-branch progress; it is not a release note.
Dynamic conformance against the full W3C QT3 suite has not yet been *measured*
(the snapshot is not vendored in this repository — see below), so this must not
be read as a verified claim of full XPath 2.0 conformance.

As of 2026-06-26, the implementation covers the great majority of the XPath 2.0
language surface end-to-end: the XDM/atomic type system and casts, sequence
types and the full `KindTest` grammar, `instance of` / `treat as` /
`castable as` / `cast as`, a large Functions & Operators library (string,
numeric, aggregate, sequence, node, QName, date/time, regex, error), the default
Unicode codepoint collation, XPath 1.0 compatibility-mode behavior, structured
XPath/XQuery error codes, exact reverse-axis predicate semantics, default
element namespace handling, and external variable/function resolver hooks. It is
exercised by an expanded focused test suite and a QT3 runner that runs against a
vendored snapshot when present.

Current release-readiness summary:

- Implemented and tested: public module surface; zero-copy SIMD lexer; full
  expression grammar including `instance of`/`treat`/`castable`/`cast`,
  `SequenceType`/`ItemType`/`SingleType`, occurrence indicators, and the full
  `KindTest` grammar; the built-in `xs:*` atomic hierarchy with date/time,
  duration, binary, anyURI, and QName values; casting, constructors, and
  numeric type promotion; the bulk of Functions & Operators; codepoint
  collation; XPath 1.0 compatibility mode; structured error codes; exact
  reverse-axis ordering and per-context predicate position semantics; default
  element/type namespace; variable/function/`doc()`/`collection()` resolver
  hooks; and resource budgets across the evaluator.
- Known limitations (documented, not silently wrong): full PSVI schema-aware
  typing (`schema-element()`/`schema-attribute()` degrade to name tests; typed
  node values are not derived from a schema); the namespace axis (the data model
  has no namespace nodes — an explicit `XPST0010` diagnostic is raised, and
  `fn:namespace-uri-for-prefix` is provided instead); cross-document path
  navigation across resolver-returned documents (the single-`Document`
  evaluation model means resolver results are owned `XPath2Value`s and node
  navigation is constrained to the evaluated document); `fn:replace` capturing
  group references; and Unicode normalization beyond NFC pass-through.
- Release posture: keep this documented as **experimental XPath 2.0** until a
  pinned W3C QT3 snapshot is vendored and the runner's measured pass rate is
  recorded here. The runner, `just test-qt3-xpath2`, and `just bench-xpath2`
  exist today and skip gracefully when the snapshot is absent.

## Key Changes

- Add `src/xpath2/` with separate lexer, parser, AST, XDM value model, evaluator, built-in functions/operators, schema typing, and conformance harness support.
- Expose default public API:
  - `uppsala::xpath2::{XPath2Evaluator, XPath2Value, XPath2Item, XPath2AtomicValue, XPath2Options, XPath2Resolver}`.
  - Re-export the primary evaluator/value types from `src/lib.rs`.
  - Keep `XPathEvaluator` and `XPathValue` unchanged for XPath 1.0.
- Default XPath 2.0 semantics:
  - XPath 1.0 compatibility mode is off by default.
  - Add `XPath2Evaluator::with_xpath1_compatibility(bool)`.
  - Add namespace, variable, function, schema, collation, base URI, current date/time, and resource-limit configuration through builder methods.
- Add resolver hooks for `doc()`, `doc-available()`, and `collection()`; no filesystem or network access occurs unless the caller supplies a resolver.
- Preserve the crate's zero-dependency policy; use only `std`, existing DOM/XSD code, and internal helpers.

## Implementation Details

### Implementation Todo

Status is tracked against the first in-repo XPath 2.0 slice and the local
authoritative spec at `specs/xpath2.0.html`. Parent checkboxes are intentionally
left unchecked until every child item in that area is complete.

- [x] Public module and API:
  - [x] Add `src/xpath2/` beside the existing XPath 1.0 engine.
  - [x] Re-export `XPath2Evaluator`, `XPath2Value`, `XPath2Item`, `XPath2AtomicValue`, `XPath2Options`, and `XPath2Resolver` (plus `AtomicType`, `QNameValue`).
  - [x] Keep `XPathEvaluator` and `XPathValue` unchanged for XPath 1.0.
  - [x] Add `XPath2Evaluator::with_xpath1_compatibility(bool)` option storage.
  - [x] Implement actual XPath 1.0 compatibility-mode behavior (multi-item operands reduce to the first item; arithmetic promotes to `xs:double`).
- [x] Lexer:
  - [x] Tokenize from `&str` into `Token<'expr>` with borrowed lexemes where possible.
  - [x] Use `Cow<'expr, str>` only where string literals require unescaping doubled quotes.
  - [x] Handle nested XPath comments `(: :)`.
  - [x] Tokenize numeric literals, punctuation, and current multi-character operators (incl. `?` for occurrence/single-type).
  - [x] Add SIMD/scalar whitespace scanning parity.
  - [x] Add XPath lexical disambiguation rules, including reserved operator names and the prefixed-function-call vs. name-test boundary.
  - [x] Add complete QName/wildcard token coverage, including `*:local` and `prefix:*`.
  - [x] Expression-size/AST-node and nesting-depth resource limits (parser-side `charge_node`/`max_depth`).
- [x] Parser:
  - [x] Parse expression lists, empty sequence, literals, variables, function calls, and parenthesized expressions.
  - [x] Parse `for`, `if`, and quantified `some`/`every` expressions.
  - [x] Parse boolean, comparison, range, arithmetic, unary, union, and basic path expressions.
  - [x] Preserve configurable parse-depth limit.
  - [x] Implement `intersect` and `except`.
  - [x] Implement `instance of`, `treat as`, `castable as`, and `cast as` at correct precedence.
  - [x] Implement `SequenceType`, `ItemType`, `SingleType`, occurrence indicators (`?`/`*`/`+`), and `empty-sequence()`.
  - [x] Implement complete `KindTest` grammar: `document-node`, `element`, `attribute`, `schema-element`, and `schema-attribute`.
  - [x] Implement complete wildcard grammar: `*`, `prefix:*`, and `*:local`.
  - [x] Enforce static syntax constraints for reserved operator names and ambiguous grammar cases.
- [ ] XDM and evaluation:
  - [x] Represent results as ordered sequences of `XPath2Item`.
  - [x] Represent DOM nodes and typed atomic values distinctly.
  - [x] Implement atomization and effective boolean value.
  - [x] Implement document-order duplicate elimination for node operators.
  - [x] Implement sequence type matching (`instance of`) and dynamic type errors (`XPTY0004`, `XPDY0050`).
  - [ ] Implement stable cross-document ordering and node identity for resolver-returned documents (single-`Document` model; see Known limitations).
  - [ ] Implement full PSVI typed node values and schema type annotations (atomized nodes are `xs:untypedAtomic`).
- [x] Path semantics:
  - [x] Evaluate child, descendant, attribute, self, descendant-or-self, and parent axes.
  - [x] Evaluate predicates and the kind tests `node`, `text`, `comment`, `processing-instruction`, `element`, `attribute`, `document-node`, `schema-element`, `schema-attribute`.
  - [x] Implement ancestor, ancestor-or-self, following, following-sibling, preceding, and preceding-sibling axes.
  - [~] Namespace axis: the data model has no namespace nodes; an explicit `XPST0010` diagnostic is raised and `fn:namespace-uri-for-prefix` is provided instead.
  - [x] Implement reverse-axis ordering and predicate position semantics exactly (reverse axes number positions nearest-first).
  - [x] Implement namespace URI resolution for prefixed element/attribute name tests through evaluator namespace bindings.
  - [x] Implement default element/type namespace semantics (`with_default_element_namespace`).
  - [x] Implement prefix and local-name wildcard matching with namespace URI checks.
- [x] Atomic type system:
  - [x] Add string, boolean, integer, decimal, double, float, and untypedAtomic values.
  - [x] Store integer/decimal lexical values without external dependencies.
  - [x] Dependency-free integer arithmetic (checked `i128`) with decimal/double fallbacks where required.
  - [x] Add the built-in `xs:*` atomic hierarchy used by XPath 2.0 (`AtomicType`, subtype relation, primitive base).
  - [x] Add date, time, dateTime, dayTimeDuration, yearMonthDuration, gregorian, hexBinary, base64Binary, QName, anyURI, and related conversions.
  - [x] Implement `cast`, `castable`, constructor functions, and numeric type promotion rules.
- [ ] Schema-aware behavior:
  - [ ] Add schema overlay built from `XsdValidator` without mutating `Document`.
  - [ ] Resolve typed values and type annotations from schema validation.
  - [~] `schema-element()` and `schema-attribute()` tests parse and evaluate, degrading to name-only element/attribute matches without PSVI.
  - [~] Unsupported schema-aware features surface explicit diagnostics rather than silent wrong answers.
- [ ] Functions and operators:
  - [x] Implement boolean, position, string, and accessor built-ins.
  - [x] Implement arithmetic, range, general/value/node comparisons, union, intersect, and except (with type-correct temporal/duration comparison).
  - [x] Implement broad Functions & Operators coverage for string, regex (`matches`/`replace`/`tokenize`), numeric, aggregate, sequence, node, QName, date/time, and error functions.
  - [x] Implement default Unicode codepoint collation.
  - [~] Collation-aware comparisons accept a collation argument; only the codepoint collation is implemented (a registry for additional collations is future work).
  - [x] Return explicit structured `XmlError::xpath_code(...)` codes for unsupported/implementation-defined behavior.
- [x] Static and dynamic context:
  - [x] Track context item, position, size, and local variables for implemented expressions.
  - [x] Add resolver hooks for `doc()`, `doc-available()`, and `collection()`.
  - [x] Add static namespace binding configuration for name tests and wildcard matching.
  - [x] Add external variable binding (`with_variable`/`set_variable`) and external function resolver hook (`XPath2Resolver::resolve_function`).
  - [x] Add base URI, default element namespace, current date/time, and implicit timezone configuration.
  - [x] Make context settings available through builder methods.
- [x] Error semantics:
  - [x] Introduce structured XPath 2.0/XQuery error codes (`XPathError::code`, `XmlError::xpath_code`).
  - [x] Distinguish static (`XPST*`), dynamic (`FO*`/`FODC*`), and type (`XPTY*`/`FORG*`) errors.
  - [x] Map parser/evaluator failures to spec-accurate codes where practical.
- [ ] Resolver-backed documents:
  - [x] Ensure no filesystem or network access occurs without a caller-supplied resolver.
  - [x] Define owned document lifetime/storage model for `doc()`/`collection()` (resolvers return owned `XPath2Value` sequences).
  - [~] Path evaluation across resolver-returned document nodes is constrained to the single evaluated `Document`; multi-document node identity is future work.
  - [x] `doc()` and `doc-available()` consistency: both route through `XPath2Resolver::resolve_doc`.
- [x] Conformance, tests, and benchmarks:
  - [x] `tests/xpath2_conformance.rs` focused coverage (22 tests).
  - [x] Add `just test-xpath2`.
  - [x] Expand focused tests for `intersect`, `except`, additional axes, wildcard forms, and per-context predicates.
  - [x] Expand focused tests for casts, types, compatibility mode, kind tests, reverse-axis predicates, default namespace, resolver functions, and date/time.
  - [ ] Vendor a pinned W3C `qt3tests` snapshot under `test-data/qt3tests/` with a source commit note (manual step; not committed here).
  - [x] Add a QT3 XPath 2.0 runner with metadata-aware skips for optional/environment-dependent tests (skips gracefully when the snapshot is absent).
  - [x] Add `just test-qt3-xpath2`.
  - [x] Add zero-dependency lexer/parser/evaluator benchmark harnesses (`benches/xpath2_bench.rs`, `std::time` only).
  - [x] Add `just bench-xpath2`.

## Conformance, Tests, and Benchmarks

- Current focused coverage in `tests/xpath2_conformance.rs` (22 tests) includes:
  - sequence construction and empty sequence
  - zero-copy lexer token cases and SIMD/scalar lexer parity
  - path navigation, predicates, and per-context predicate positions
  - comparisons: general, value, node, and type-correct temporal comparison
  - `for`, `if`, `some/every`, atomization and EBV
  - resolver-backed `doc()`/`collection()` and the external function resolver hook
  - additional axes, `intersect`, `except`, namespace-bound name tests, and
    prefix/local wildcard matching
  - casts, constructors, `instance of`, `treat as`, `castable as`
  - the Functions & Operators library (string, regex, numeric, aggregate,
    sequence, node, codepoint) and `concat`/`string-join`/`tokenize`/`replace`
  - XPath 1.0 compatibility-mode differences
  - the full `KindTest` grammar, default element namespace, and reverse-axis
    predicate semantics
  - deterministic `current-date()` and node accessors (`local-name`, `lang`)
- The QT3 runner lives in `tests/xpath2_qt3.rs`. It is metadata-aware: it runs
  only XPath-applicable test cases, skips cases requiring an external source
  document, schema awareness, collations, higher-order functions, or
  XQuery-only syntax, and interprets the common assertion kinds
  (`assert-true`/`-false`/`-empty`/`-eq`/`-count`/`-string-value`, `error`,
  `all-of`/`any-of`). It dogfoods the uppsala parser to read the catalog and
  test-set files, and prints pass/fail/skip statistics with a pass rate.
- A pinned W3C `qt3tests` snapshot is **not committed** to this repository
  (vendoring requires fetching from `https://github.com/w3c/qt3tests`). Place a
  snapshot at `test-data/qt3tests/` (so `catalog.xml` exists) and record the
  commit in `SOURCE_COMMIT.txt`; the runner skips cleanly when it is absent, so
  CI on a clean checkout is unaffected. The measured QT3 pass rate should be
  recorded in this ADR once a snapshot is vendored.
- `just` recipes added: `just test-xpath2`, `just test-qt3-xpath2`,
  `just bench-xpath2`.
- The zero-dependency benchmark harness (`benches/xpath2_bench.rs`, `std::time`
  only, `harness = false`) times the lexer, parser, and evaluator hot paths,
  including a long whitespace-heavy expression that exercises the SIMD scanner.

## Assumptions

- XPath 2.0 is exposed by default, not feature-gated.
- The implementation aims for full XPath 2.0 dynamic conformance, but does not implement the optional Static Typing Feature unless later requested.
- The authoritative local spec is `specs/xpath2.0.html`; referenced W3C Data Model and Functions/Operators behavior must be used where XPath 2.0 delegates semantics.
- Existing XPath 1.0 behavior, tests, and public API remain backward-compatible.
