#![no_main]
//! Parse -> serialize -> reparse -> serialize. Exercises the whole serializer,
//! including the SSE2 `scan_escape_sse2` unsafe run-scanner and the sibling-walk
//! `write_node_to`, over content that a real parser accepted (so names, text and
//! attribute values are already well-formed byte sequences).
//!
//! Oracle: serialization is a fixpoint. Once a document has been serialized, a
//! reparse-then-serialize must reproduce it byte-for-byte. A mismatch is a real
//! serializer/parser round-trip bug. The assert only fires when *both* parses
//! succeed, so parser resource limits (depth/entities) never cause false
//! positives. Memory-safety bugs in the unsafe SIMD are caught by ASan on every
//! iteration regardless of the assert.
use libfuzzer_sys::fuzz_target;
use uppsala::parse;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(doc1) = parse(s) else {
        return;
    };
    // Compact.
    let out1 = doc1.to_xml();
    if let Ok(doc2) = parse(&out1) {
        let out2 = doc2.to_xml();
        assert_eq!(out1, out2, "compact serialization is not idempotent");
    }
    // Pretty-printed path (different serializer branch: element-only probe +
    // indentation) must be well-formed enough to reparse and also be a fixpoint.
    let pretty = doc1.to_xml_with_options(&uppsala::XmlWriteOptions::pretty("  "));
    if let Ok(docp) = parse(&pretty) {
        let pretty2 = docp.to_xml_with_options(&uppsala::XmlWriteOptions::pretty("  "));
        assert_eq!(pretty, pretty2, "pretty serialization is not idempotent");
    }
});
