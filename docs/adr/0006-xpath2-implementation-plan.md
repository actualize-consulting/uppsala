# Implement XPath 2.0 With SIMD and Zero-Copy Parsing

## Summary

Implement a first-class `xpath2` module alongside the existing XPath 1.0 engine. Target full XPath 2.0 language behavior using the local spec at `specs/xpath2.0.html`, with zero runtime dependencies, borrowed expression tokens, SIMD-assisted lexing, XDM sequences, typed atomic values, schema-aware evaluation, resolver hooks, and vendored upstream conformance tests from W3C `qt3tests`.

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
authoritative spec at `specs/xpath2.0.html`.

- [x] Public module and API:
  - [x] Add `src/xpath2/` beside the existing XPath 1.0 engine.
  - [x] Re-export `XPath2Evaluator`, `XPath2Value`, `XPath2Item`, `XPath2AtomicValue`, `XPath2Options`, and `XPath2Resolver`.
  - [x] Keep `XPathEvaluator` and `XPathValue` unchanged for XPath 1.0.
  - [x] Add `XPath2Evaluator::with_xpath1_compatibility(bool)` option storage.
  - [ ] Implement actual XPath 1.0 compatibility-mode behavior.
- [ ] Lexer:
  - [x] Tokenize from `&str` into `Token<'expr>` with borrowed lexemes where possible.
  - [x] Use `Cow<'expr, str>` only where string literals require unescaping doubled quotes.
  - [x] Handle nested XPath comments `(: :)`.
  - [x] Tokenize numeric literals, punctuation, and current multi-character operators.
  - [x] Add SIMD/scalar whitespace scanning parity.
  - [ ] Add full XPath lexical disambiguation rules, including reserved function names and operator/name boundary edge cases.
  - [x] Add complete QName/wildcard token coverage, including `*:local` and `prefix:*`.
  - [ ] Add expression-size/token-count resource limits.
- [ ] Parser:
  - [x] Parse expression lists, empty sequence, literals, variables, function calls, and parenthesized expressions.
  - [x] Parse `for`, `if`, and quantified `some`/`every` expressions.
  - [x] Parse boolean, comparison, range, arithmetic, unary, union, and basic path expressions.
  - [x] Preserve configurable parse-depth limit.
  - [x] Implement `intersect` and `except`.
  - [ ] Implement `instance of`, `treat as`, `castable as`, and `cast as`.
  - [ ] Implement `SequenceType`, `ItemType`, `SingleType`, occurrence indicators, and sequence type matching syntax.
  - [ ] Implement complete `KindTest` grammar: `document-node`, `element`, `attribute`, `schema-element`, and `schema-attribute`.
  - [x] Implement complete wildcard grammar: `*`, `prefix:*`, and `*:local`.
  - [ ] Enforce static syntax constraints for reserved function names and ambiguous grammar cases.
- [ ] XDM and evaluation:
  - [x] Represent results as ordered sequences of `XPath2Item`.
  - [x] Represent DOM nodes and typed atomic values distinctly.
  - [x] Implement basic atomization and effective boolean value.
  - [x] Implement document-order duplicate elimination for current node operators.
  - [ ] Implement full sequence type matching and dynamic type errors.
  - [ ] Implement stable cross-document ordering and node identity for resolver-returned documents.
  - [ ] Implement full typed node values and type annotations.
- [ ] Path semantics:
  - [x] Evaluate child, descendant, attribute, self, descendant-or-self, and parent axes.
  - [x] Evaluate predicates and basic kind tests: `node`, `text`, `comment`, and `processing-instruction`.
  - [x] Implement ancestor, ancestor-or-self, following, following-sibling, preceding, and preceding-sibling axes.
  - [ ] Implement namespace axis nodes.
  - [ ] Implement reverse-axis ordering and predicate position semantics exactly.
  - [ ] Implement namespace URI resolution for prefixed names and default element/type namespace.
  - [ ] Implement full wildcard matching with namespaces.
- [ ] Atomic type system:
  - [x] Add initial string, boolean, integer, decimal, double, and untypedAtomic values.
  - [x] Store integer/decimal lexical values without external dependencies.
  - [ ] Replace numeric operations with dependency-free arbitrary-precision integer/decimal semantics where required.
  - [ ] Add built-in `xs:*` atomic hierarchy used by XPath 2.0.
  - [ ] Add date, time, dateTime, dayTimeDuration, yearMonthDuration, QName, anyURI, and related conversions.
  - [ ] Implement `cast`, `castable`, constructors, and type promotion rules.
- [ ] Schema-aware behavior:
  - [ ] Add schema overlay built from `XsdValidator` without mutating `Document`.
  - [ ] Resolve typed values and type annotations from schema validation.
  - [ ] Implement `schema-element()` and `schema-attribute()` tests.
  - [ ] Implement schema import behavior and unsupported-feature diagnostics.
- [ ] Functions and operators:
  - [x] Implement initial built-ins: `true`, `false`, `position`, `last`, `string`, `boolean`, `not`, `empty`, `exists`, `count`, `number`, `concat`, `doc`, `doc-available`, and `collection`.
  - [x] Implement initial arithmetic, range, general/value/node comparisons, union, intersect, and except.
  - [ ] Implement full XPath 2.0 Functions and Operators coverage for string, sequence, node, QName, numeric, date/time, duration, regex, and error functions.
  - [ ] Implement default Unicode codepoint collation.
  - [ ] Add collation registry and collation-aware comparisons/functions.
  - [ ] Return explicit `XmlError::xpath(...)` for unsupported implementation-defined behavior.
- [ ] Static and dynamic context:
  - [x] Track context item, position, size, and local variables for implemented expressions.
  - [x] Add resolver hooks for `doc()`, `doc-available()`, and `collection()`.
  - [ ] Add namespace resolver configuration.
  - [ ] Add variable resolver and external function resolver hooks.
  - [ ] Add schema, collation, base URI, default namespaces, current date/time, and implicit timezone configuration.
  - [ ] Make all context settings available through builder methods.
- [ ] Error semantics:
  - [ ] Introduce structured XPath 2.0 error codes or equivalent internal classification.
  - [ ] Distinguish static, dynamic, and type errors.
  - [ ] Map parser/evaluator failures to spec-accurate errors where practical.
- [ ] Resolver-backed documents:
  - [x] Ensure no filesystem or network access occurs without a caller-supplied resolver.
  - [ ] Define owned document lifetime/storage model for `doc()` and `collection()`.
  - [ ] Support path evaluation across resolver-returned document nodes.
  - [ ] Define document cache and `doc-available()` consistency behavior.
- [ ] Conformance, tests, and benchmarks:
  - [x] Add `tests/xpath2_conformance.rs` focused coverage for the first slice.
  - [x] Add `just test-xpath2`.
  - [x] Expand focused tests for `intersect`, `except`, additional axes, wildcard forms, and per-context predicates.
  - [ ] Expand focused tests for casts, types, schema-aware values, compatibility mode, collations, resolver documents, and namespace axes.
  - [ ] Vendor a pinned W3C `qt3tests` snapshot under `test-data/qt3tests/` with a source commit note.
  - [ ] Add a QT3 XPath 2.0 runner with metadata-aware skips for optional/environment-dependent tests.
  - [ ] Add `just test-qt3-xpath2`.
  - [ ] Add zero-dependency lexer/parser/evaluator benchmark harnesses.
  - [ ] Add `just bench-xpath2`.

## Conformance, Tests, and Benchmarks

- Add `tests/xpath2_conformance.rs` for focused curated coverage:
  - sequence construction and empty sequence
  - zero-copy lexer token cases
  - SIMD/scalar lexer parity
  - path navigation and predicates
  - comparisons: general, value, and node
  - `for`, `if`, `some/every`
  - atomization and EBV
  - casts and castable checks
  - schema-aware typed values
  - resolver-backed `doc()` and `collection()`
  - XPath 1.0 compatibility-mode differences
- Vendor a pinned W3C `qt3tests` snapshot under `test-data/qt3tests/` with a source commit note.
- Add a QT3 runner that filters XPath 2.0-applicable tests and skips tests whose metadata requires unsupported host features only when the spec marks them optional or environment-dependent.
- Add `just` recipes:
  - `just test-xpath2`
  - `just test-qt3-xpath2`
  - `just bench-xpath2`
- Add zero-dependency benchmark harnesses using `std::time` for lexer, parser, and evaluator hot paths, including long ASCII expressions that exercise SIMD.

## Assumptions

- XPath 2.0 is exposed by default, not feature-gated.
- The implementation aims for full XPath 2.0 dynamic conformance, but does not implement the optional Static Typing Feature unless later requested.
- The authoritative local spec is `specs/xpath2.0.html`; referenced W3C Data Model and Functions/Operators behavior must be used where XPath 2.0 delegates semantics.
- Existing XPath 1.0 behavior, tests, and public API remain backward-compatible.
