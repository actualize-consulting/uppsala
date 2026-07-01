# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.7.0] - 2026-07-01

### Added

- XSLT 1.0 transform engine (`uppsala::transform`, plus `Stylesheet::compile` /
  `Stylesheet::transform`), layered on the existing XPath 1.0 evaluator with no
  second XPath implementation. Implements the "Tier A" subset that pyFF's
  stylesheets use (see ADR 0010 and the crate docs). Adds the XPath
  `$variable`/extension-function resolution seams, a compiled-expression entry
  point, XSLT match patterns, and the `lang()`/`current()` core functions.
- Opt-in EXSLT extension-function library (`src/exslt.rs`): `math:` (abs, sqrt,
  power, log, exp, sin, cos, tan, constant, min, max, highest, lowest), `str:`
  (concat, padding, align), `set:` (distinct, difference, intersection,
  has-same-node), and `exsl:object-type`. Enabled per stylesheet via
  `Stylesheet::with_exslt(true)`; matched by the conventional EXSLT prefix.
  `date:date-time()` remains available unconditionally.
- `Stylesheet::set_max_depth` and `DEFAULT_MAX_XSLT_DEPTH` (default 500): a bound
  on XSLT template-activation recursion.
- Opt-in libxml2-compatible lenient XSD datatype validation via
  `XsdValidator::set_lenient(bool)` (default off, strict/spec-faithful). The one
  relaxed rule is `xs:anyURI`: a value containing a space is accepted, matching
  libxml2/lxml/pyFF (strict mode still rejects it per RFC 3987). Applies to
  `anyURI` in element content and attribute values alike; no other datatype,
  facet, or structural check is weakened. See ADR 0012.
- `Document::import_subtree(&mut self, src, src_id) -> Option<NodeId>`: deep-copy
  a node and its entire subtree from another document into this one in a single
  native pass, returning the new detached root node id (used by the pyuppsala DOM
  wrapper for cross-tree element moves).

### Changed

- XSLT transforms of large, wide documents are linear instead of quadratic.
  Several O(width²) hot spots were removed: the XSLT match-pattern tester no
  longer materializes every sibling to check membership (it tests the node
  locally and only falls back to the full set for positional predicates);
  `prepare_xpath` now precomputes a per-node document-order index so node-set
  deduplication (`dedup_document_order`) sorts by an O(1) key instead of walking
  each node to the document root and re-indexing wide ancestor sibling lists on
  every call (previously quadratic for relative paths like `name/text()`
  evaluated once per sibling); and a per-node template-dispatch pre-filter skips
  patterns that cannot match a node's kind/name. Net effect: a 91 MB eduGAIN
  aggregate that previously did not complete now transforms in a few seconds
  with every pyFF stylesheet (1–5 s each).
- `xs:import/@schemaLocation` is now treated as a hint (XSD 1.0 Part 1 §4.2.3):
  an import whose location cannot be resolved (unsupported scheme, missing file,
  or a path outside the base directory) is skipped instead of aborting the schema
  build, matching libxml2/Xerces. `xs:include`/`xs:redefine` are unchanged (they
  still treat base-directory escapes and unsupported absolute-URI schemes as hard
  errors). This lets composite schemas such as pyFF's `schema.xsd` build. See
  ADR 0011.
- XML serialization escaping (`XmlWriter` text/attribute output and the DOM
  serializer) is now run-based: safe byte runs are bulk-copied with one
  `push_str` (SIMD `memcpy`) instead of pushing one character at a time, which is
  faster on ASCII-heavy payloads like SAML metadata. Output is byte-identical to
  the previous per-character version.

### Security

- XSLT recursion is now bounded by `DEFAULT_MAX_XSLT_DEPTH` (default 500,
  overridable via `Stylesheet::set_max_depth`). A self-recursive
  `xsl:call-template`, mutually-recursive named templates, or an
  `xsl:apply-templates select="."` cycle previously recursed unbounded and
  aborted the process with an uncatchable stack-overflow `SIGABRT`; they now
  return a graceful `XmlError`.

## [0.6.0] - 2026-06-30

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
