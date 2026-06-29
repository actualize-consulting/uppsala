# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- In-repo `performance-harness/` crate for comparing Uppsala against a sibling
  `roxmltree` checkout, with `suite`, `saml`, and `file` run modes. Kept outside
  the main crate's targets so library builds and tests do not depend on
  `roxmltree`.
- `docs/performance.md`: benchmark tables and reproducible commands for running
  the performance harness and profiling the parser with `perf`.
- README "Performance" section with measured comparison tables (roxmltree
  benchmark inputs and SAML-shaped inputs) on x86_64.
- ADR 0009: decision to keep parser byte-scanning single-pass and
  dependency-free (no `memchr`).
- `justfile` targets for focused test suites and performance-harness runs.

### Changed

- Faster default-namespace resolution: `NamespaceResolver` caches default
  namespace state and restores it via a stack on scope push/pop instead of
  re-scanning scopes.
- Parser hot-path optimizations: improved attribute-value scanning, reduced
  per-reference allocations on the predefined-entity and numeric-character
  -reference paths, and denser arena pre-allocation heuristics for large inputs.
- Added a dependency-free `find_byte` helper in `src/simd.rs` used by the parser.

### Fixed

- Reject empty numeric character references (`&#;` and `&#x;`) with a clear
  "Invalid decimal/hex character reference" error instead of a misleading
  "Character reference U+0000 is not a valid XML character" report.
- Numeric character-reference overflow now reports the hex/decimal-specific
  message, matching the invalid-digit error wording.

### Security

- Node-arena pre-allocation now uses `Vec::try_reserve` instead of `reserve`, so
  a hostile or very large input cannot trigger an aborting upfront allocation via
  the global allocation-error handler. The reservation is a best-effort hint:
  on failure it falls back to a sparser estimate, then to no pre-reservation,
  and parsing still succeeds by regrowing the arena on demand.
