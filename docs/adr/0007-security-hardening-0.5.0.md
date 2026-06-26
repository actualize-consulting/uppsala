# Security Hardening Defaults for 0.5.0

## Status

Accepted.

## Date

2026-06-26

## Context

Uppsala is commonly used on XML that may cross trust boundaries before being
serialized, queried, or validated. A security review found several places where
the library preserved or accepted ambiguous XML state:

- DTD declarations were preserved by default during serialization, which could
  hand an XXE-capable `DOCTYPE` to downstream XML processors.
- Programmatic element and attribute names could be serialized without XML name
  validation.
- Namespace-sensitive parser, XPath, and XSD paths sometimes compared only local
  names or fell back to no-namespace declarations.
- XSD named type references could fail open when resolution failed.
- XPath axis traversal and XPath 2.0 range construction lacked allocation
  limits.
- DOM tree mutation accepted invalid or cyclic `NodeId` relationships.

## Decision

Version 0.5.0 makes the secure behavior the default:

- Parsed `Document::doctype` is still preserved for inspection, but serializers
  omit it by default. Trusted callers can opt in with
  `XmlWriteOptions::with_doctype(true)`.
- DOM and `XmlWriter` serialization replace invalid structural QNames with `_`.
- Namespace-expanded attributes are checked for duplicates after namespace
  resolution.
- XPath 1.0 name tests are namespace-aware and require explicit prefix bindings.
- XPath 1.0 axis/predicate traversal and XPath 2.0 eager range construction are
  bounded by configurable defaults.
- XSD root lookup, type resolution, identity constraints, wildcard merging, and
  complex type derivation now fail closed on namespace mismatch, unresolved
  types, malformed temporal values, and derivation cycles.
- Public DOM mutation APIs no-op on invalid handles or cycle-forming moves.

## Consequences

This is a behavior change for callers that depended on automatic DTD
round-tripping, local-name-only XPath/XSD matching, or permissive invalid
programmatic names. These changes are included in the 0.5.0 release because
they close validation and serialization bypasses in security-sensitive
embeddings. Callers that intentionally round-trip trusted DTDs must opt in
explicitly at serialization time.
