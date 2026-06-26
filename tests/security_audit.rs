//! Security-audit test harness.
//!
//! Each test reproduces a finding in SECURITY_AUDIT.md. Tests that exercise
//! reliably-crashing bugs (stack overflow, billion-laughs OOM) are marked
//! `#[ignore]` so the default `cargo test` run remains safe; invoke them
//! explicitly with `cargo test --test security_audit -- --ignored`.
//!
//! Tests that time-bound a DoS use a worker thread plus a wall-clock
//! timeout — if the worker doesn't finish within `TIMEOUT`, the test fails
//! with "DoS: took > TIMEOUT".

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use uppsala::{parse, XPathEvaluator, XmlWriter, XsdRegex, XsdValidator};

// Wall-clock cap for DoS tests. Well under a typical cargo-test default.
const TIMEOUT: Duration = Duration::from_secs(5);

fn run_with_timeout<F, R>(label: &'static str, f: F) -> R
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let r = f();
        let _ = tx.send(r);
    });
    match rx.recv_timeout(TIMEOUT) {
        Ok(v) => v,
        Err(_) => panic!(
            "{}: exceeded {}s timeout (DoS confirmed)",
            label,
            TIMEOUT.as_secs()
        ),
    }
}

// ─── Finding F-01 — Billion Laughs (entity expansion) ──────────────────

/// Canonical Billion Laughs. `&lol9;` expands to 10^9 characters.
/// Must either be rejected *before* expansion (size cap / depth cap)
/// or complete in bounded time. Current code fully expands it.
#[test]
#[ignore = "OOMs / hangs the test runner — run explicitly to confirm"]
fn billion_laughs_full() {
    let xml = include_str!("../audit/pocs/billion_laughs.xml");
    let start = Instant::now();
    let res = parse(xml);
    let elapsed = start.elapsed();
    assert!(
        res.is_err() && elapsed < Duration::from_secs(1),
        "expected billion-laughs rejection/limit; elapsed={:?}, err={:?}",
        elapsed,
        res.err()
    );
}

/// CI-safe variant: 6-level nesting (~10^6 chars). Parser still fully
/// expands but it fits in memory, so the failure mode is "completes
/// silently" rather than OOM. Asserts the parser accepted the blow-up
/// — which itself is the bug.
#[test]
fn billion_laughs_small_expands_unchecked() {
    let xml = r#"<?xml version="1.0"?>
<!DOCTYPE lolz [
  <!ENTITY lol "lol">
  <!ENTITY lol1 "&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;&lol;">
  <!ENTITY lol2 "&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;&lol1;">
  <!ENTITY lol3 "&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;&lol2;">
  <!ENTITY lol4 "&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;&lol3;">
  <!ENTITY lol5 "&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;&lol4;">
]>
<lolz>&lol5;</lolz>"#;
    let doc = parse(xml).expect("small billion-laughs still parses today");
    let root = doc.document_element().unwrap();
    let text = doc.text_content_deep(root);
    // 3 * 10^5 chars — no entity-expansion cap was applied.
    assert_eq!(
        text.len(),
        3 * 100_000,
        "parser should have capped expansion; instead produced {} chars",
        text.len()
    );
    // This test currently PASSES, which means the vulnerability is real.
    // A fix should make this test FAIL (parser should reject / truncate).
}

// ─── Finding F-02 — Quadratic entity expansion ──────────────────────────

#[test]
fn quadratic_entity_expansion_unchecked() {
    let xml = include_str!("../audit/pocs/quadratic_blowup.xml");
    let doc = parse(xml).expect("quadratic blow-up still parses today");
    let root = doc.document_element().unwrap();
    let text_len = doc.text_content_deep(root).len();
    // 20 chars × 10 (inner) × 10 (outer) = 2000 — small enough to run,
    // but enforcement would reject or cap before expansion.
    assert!(text_len >= 2000);
}

// ─── Finding F-03 — Parser stack overflow on deep nesting ───────────────

/// Building a 1,000,000-deep element chain and feeding it to the parser
/// aborts the process on Linux with an 8 MiB stack. We run it in a
/// spawned thread with a small stack so the failure manifests as a
/// thread-panic rather than crashing the test binary — but Rust still
/// `abort()`s on stack overflow regardless of thread, so the whole
/// binary will die. Marked `#[ignore]`.
#[test]
#[ignore = "stack overflow aborts the test binary; run explicitly"]
fn deep_nesting_parser_stack_overflow() {
    let depth = 1_000_000;
    let mut xml = String::with_capacity(depth * 8);
    for _ in 0..depth {
        xml.push_str("<a>");
    }
    xml.push('x');
    for _ in 0..depth {
        xml.push_str("</a>");
    }
    let _ = parse(&xml);
}

/// Smaller, survivable variant — demonstrates that the parser accepts
/// very deep nesting *without* any configured cap. The author can make
/// this test fail by introducing a depth limit.
#[test]
#[ignore = "5k-deep nesting aborts the test binary on the default stack"]
fn deep_nesting_accepted_without_cap() {
    let depth = 5_000; // confirmed to stack-overflow on default 8 MiB stack
    let mut xml = String::with_capacity(depth * 8);
    for _ in 0..depth {
        xml.push_str("<a>");
    }
    xml.push('x');
    for _ in 0..depth {
        xml.push_str("</a>");
    }
    let res = parse(&xml);
    assert!(
        res.is_ok(),
        "expected current build to accept 5k-deep nesting (no cap enforced)"
    );
}

// ─── Finding F-04 — XSD regex parser stack overflow on nested parens ────

#[test]
#[ignore = "stack overflow aborts the test binary; run explicitly"]
fn xsd_regex_deep_paren_stack_overflow() {
    let n = 50_000;
    let mut pat = String::with_capacity(n * 2 + 1);
    for _ in 0..n {
        pat.push('(');
    }
    pat.push('a');
    for _ in 0..n {
        pat.push(')');
    }
    let _ = XsdRegex::compile(&pat);
}

// ─── Finding F-05 — Polynomial ReDoS in XSD regex ───────────────────────

#[test]
fn xsd_regex_polynomial_redos() {
    // (a*)*b against N 'a's. Matcher is O(n^3)/O(n^4); at N=200
    // it should finish quickly. If the dedup guard regresses, this
    // will blow past TIMEOUT.
    let re = XsdRegex::compile("(a*)*b").expect("compile");
    let input: String = "a".repeat(200);
    run_with_timeout("polynomial_redos", move || {
        // Intentionally no 'b' => matcher explores every way to partition
        // the 'a's before failing. This is the worst case.
        assert!(!re.is_match(&input));
    });
}

#[test]
#[ignore = "demonstrates O(n^3+) blow-up; seconds of CPU at N=1000"]
fn xsd_regex_polynomial_redos_heavy() {
    let re = XsdRegex::compile("(a*)*b").unwrap();
    let input: String = "a".repeat(1000);
    let start = Instant::now();
    let _ = re.is_match(&input);
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(1),
        "polynomial ReDoS: 1000-byte input took {:?}",
        elapsed
    );
}

// ─── Finding F-06 — XSD regex accepts unbounded {n,m} ───────────────────

#[test]
fn xsd_regex_unbounded_brace_accepted() {
    // A schema may legitimately specify a large upper bound such as
    // a{0,4_000_000_000}. Compile intentionally accepts it: the upper
    // bound never drives allocation or a fixed iteration count. Matching
    // is bounded instead by input saturation and the per-match step
    // budget (ADR 0004, finding F-05), so a huge `m` cannot cause a
    // time/memory blow-up. We therefore assert acceptance, and separately
    // assert that matching such a pattern stays fast.
    let re = XsdRegex::compile("a{0,4000000000}").expect("large bounds accepted");
    let input: String = "a".repeat(10_000);
    let start = Instant::now();
    assert!(re.is_match(&input), "linear match should succeed");
    assert!(
        start.elapsed() < Duration::from_secs(1),
        "huge upper bound must not slow matching (step budget bounds it)"
    );
}

// ─── Finding F-07 — \p{unknown} silently matches nothing;
//                   \P{unknown} matches everything (bypass) — FIXED ──────

#[test]
fn xsd_regex_unknown_prop_rejected() {
    // Regression: an unknown Unicode property must be rejected at compile
    // time (fail-closed). Previously `\P{unknown}` compiled and matched
    // every character, silently widening an "only-letters" pattern into one
    // that admits arbitrary markup. Both `\p{...}` and `\P{...}` forms, and
    // the in-character-class form, must error.
    assert!(
        XsdRegex::compile("\\P{unknowncategory}+").is_err(),
        "\\P{{unknown}} must fail to compile, not match everything"
    );
    assert!(
        XsdRegex::compile("\\p{unknowncategory}+").is_err(),
        "\\p{{unknown}} must fail to compile"
    );
    assert!(
        XsdRegex::compile("[\\p{unknowncategory}]").is_err(),
        "\\p{{unknown}} inside a character class must fail to compile"
    );
    // Known categories and blocks still compile.
    assert!(XsdRegex::compile("\\p{Lu}+").is_ok());
    assert!(XsdRegex::compile("\\p{IsBasicLatin}+").is_ok());
}

// ─── Finding F-08 — XPath parser stack overflow ─────────────────────────

#[test]
#[ignore = "stack overflow aborts the test binary; run explicitly"]
fn xpath_parser_deep_paren_stack_overflow() {
    let n = 20_000;
    let mut expr = String::with_capacity(n * 2 + 1);
    for _ in 0..n {
        expr.push('(');
    }
    expr.push('1');
    for _ in 0..n {
        expr.push(')');
    }
    let mut doc = parse("<r/>").unwrap();
    doc.prepare_xpath();
    let eval = XPathEvaluator::new();
    let root = doc.root();
    let _ = eval.evaluate(&doc, root, &expr);
}

// ─── Finding F-09 — XPath substring() overflow in debug builds ──────────

#[test]
fn xpath_substring_overflow_debug() {
    // substring(s, 1, +inf) — NaN/inf length coerces to usize::MAX via
    // `f64::round() as usize`, then `start + len` overflows.
    // In debug builds this panics; in release it wraps. We use
    // catch_unwind so the test works either way, but assert that a
    // panic IS observed under debug_assertions.
    use std::panic;
    let mut doc = parse("<r>hello</r>").unwrap();
    doc.prepare_xpath();
    let eval = XPathEvaluator::new();
    let root = doc.root();
    let result = panic::catch_unwind(panic::AssertUnwindSafe(|| {
        // start=5 (nonzero), len=+inf → (start + usize::MAX) overflows.
        eval.evaluate(&doc, root, "substring('hello', 5, 1 div 0)")
    }));
    #[cfg(debug_assertions)]
    {
        assert!(
            result.is_err(),
            "expected panic on `start + len` overflow under debug_assertions"
        );
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = result; // in release, wraps silently; don't assert
    }
}

// ─── Finding F-10 — XSD xs:include arbitrary local file read ────────────

#[test]
fn xsd_include_reads_absolute_paths() {
    // Regression test for F-10. Pre-fix the validator would
    // `fs::read_to_string` any attacker-supplied absolute
    // `schemaLocation` and merge its declarations in. Post-fix
    // `resolve_include_path` rejects any resolved path that escapes the
    // schema's base directory. We assert the validator build fails
    // with the containment error.
    let tmp_dir = std::env::temp_dir().join("uppsala-audit-canary");
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let canary_path = tmp_dir.join("canary.xsd");
    let canary_contents = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="leaked" type="xs:string"/>
</xs:schema>"#;
    std::fs::write(&canary_path, canary_contents).unwrap();

    // The "untrusted" schema. schemaLocation is absolute — points
    // outside the schema's own base directory.
    let schema_dir = tmp_dir.join("schema-base");
    std::fs::create_dir_all(&schema_dir).unwrap();
    let schema_path = schema_dir.join("evil.xsd");
    let schema_xml = format!(
        r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:include schemaLocation="{}"/>
  <xs:element name="x" type="xs:string"/>
</xs:schema>"#,
        canary_path.display()
    );
    std::fs::write(&schema_path, &schema_xml).unwrap();

    let schema_doc = parse(&schema_xml).unwrap();
    // Post-fix: the validator rejects the absolute schemaLocation
    // because it escapes the schema's base directory. Any success
    // here is a regression.
    let result = XsdValidator::from_schema_with_base_path(&schema_doc, Some(&schema_path));
    match result {
        Ok(_) => panic!("absolute schemaLocation must be rejected (F-10 regression)"),
        Err(e) => {
            let msg = format!("{:?}", e);
            assert!(
                msg.contains("escapes the schema's base directory"),
                "expected containment error, got: {}",
                msg
            );
        }
    }

    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
#[ignore = "stack overflow aborts the test binary; run explicitly"]
fn xsd_include_circular_stack_overflow() {
    let tmp = std::env::temp_dir().join("uppsala-audit-circular");
    std::fs::create_dir_all(&tmp).unwrap();
    let a = tmp.join("a.xsd");
    let b = tmp.join("b.xsd");
    std::fs::write(
        &a,
        r#"<?xml version="1.0"?><xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:include schemaLocation="b.xsd"/></xs:schema>"#,
    )
    .unwrap();
    std::fs::write(
        &b,
        r#"<?xml version="1.0"?><xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:include schemaLocation="a.xsd"/></xs:schema>"#,
    )
    .unwrap();
    let schema_src = std::fs::read_to_string(&a).unwrap();
    let schema_doc = parse(&schema_src).unwrap();
    let _ = XsdValidator::from_schema_with_base_path(&schema_doc, Some(&a));
    let _ = std::fs::remove_dir_all(&tmp);
}

// ─── Finding F-11 — Round-trip injection via programmatic comment ──────

#[test]
fn roundtrip_comment_smuggle() {
    // Regression test for F-13. Pre-fix, a `-->` sequence inside a
    // comment body closed the comment early and let a subsequent
    // `<injected/>` become a real sibling element. Post-fix,
    // `sanitize_comment_content` pads consecutive dashes with a space
    // so the terminator never appears in the output.
    let mut w = XmlWriter::new();
    w.start_element("r", &[]);
    w.comment("safe --> <injected/> <!--trailing");
    w.end_element("r");
    let out = w.into_string();
    let reparsed = parse(&out).expect("sanitized output must reparse");
    let root = reparsed.document_element().unwrap();
    let children = reparsed.children(root);
    let injected_elements: Vec<_> = children
        .iter()
        .filter(|c| matches!(reparsed.node_kind(**c), Some(uppsala::NodeKind::Element(_))))
        .collect();
    assert!(
        injected_elements.is_empty(),
        "comment smuggle leaked {} element child(ren) — sanitizer regressed: {:?}",
        injected_elements.len(),
        out
    );
}

#[test]
fn roundtrip_pi_smuggle() {
    // Regression test for F-14. Pre-fix, a `?>` sequence inside PI
    // data terminated the PI early. Post-fix, `sanitize_pi_data`
    // inserts a space between `?` and `>` so the terminator scan
    // cannot match.
    let mut w = XmlWriter::new();
    w.start_element("r", &[]);
    w.processing_instruction("x", Some("?><injected/>"));
    w.end_element("r");
    let out = w.into_string();
    let reparsed = parse(&out).expect("sanitized output must reparse");
    let root = reparsed.document_element().unwrap();
    let children = reparsed.children(root);
    let element_children: Vec<_> = children
        .iter()
        .filter(|c| matches!(reparsed.node_kind(**c), Some(uppsala::NodeKind::Element(_))))
        .collect();
    assert!(
        element_children.is_empty(),
        "PI smuggle leaked {} element child(ren) — sanitizer regressed: {:?}",
        element_children.len(),
        out
    );
}

#[test]
fn roundtrip_cdata_smuggle() {
    // Regression test for F-15. Pre-fix, `]]>` inside CDATA content
    // closed the section early and let `<injected/>` become a real
    // sibling element. Post-fix, `split_cdata_content` replaces each
    // `]]>` with the canonical `]]]]><![CDATA[>` split so the emitted
    // output is two adjacent CDATA sections whose concatenation equals
    // the original text.
    let original = "safe]]><injected/>";
    let mut w = XmlWriter::new();
    w.start_element("r", &[]);
    w.cdata(original);
    w.end_element("r");
    let out = w.into_string();

    let reparsed = parse(&out).expect("split CDATA must reparse");
    let root = reparsed.document_element().unwrap();
    let children = reparsed.children(root);
    let element_children: Vec<_> = children
        .iter()
        .filter(|c| matches!(reparsed.node_kind(**c), Some(uppsala::NodeKind::Element(_))))
        .collect();
    assert!(
        element_children.is_empty(),
        "CDATA smuggle leaked {} element child(ren) — split regressed: {:?}",
        element_children.len(),
        out
    );
    // Text semantics must round-trip byte-exact (the CDATA split is
    // byte-equivalent per XML 1.0, unlike comment/PI sanitization).
    assert_eq!(
        reparsed.text_content_deep(root),
        original,
        "CDATA split must preserve original text semantically"
    );
}

// ─── Finding F-12 — UTF-16 decoder silently drops odd trailing byte — FIXED ─

#[test]
fn utf16_odd_byte_rejected() {
    // Regression: a UTF-16 byte stream with an odd length has an incomplete
    // trailing code unit. Previously the decoder's `chunks(2)` silently
    // dropped the stray byte, hiding mid-code-unit truncation. The decoder
    // now rejects odd-length input instead of fabricating a valid document.
    let mut bytes = vec![0xFFu8, 0xFE]; // BOM LE
    for c in "<r/>".chars() {
        let cu = c as u16;
        bytes.push(cu as u8);
        bytes.push((cu >> 8) as u8);
    }
    bytes.push(0x41); // stray trailing byte → odd total length
    let res = uppsala::parse_bytes(&bytes);
    assert!(
        res.is_err(),
        "parser must error on incomplete UTF-16 tail, not silently drop it"
    );
}

// ─── Finding F-13 — Pattern compile accepts arbitrary recursion in class subtraction ─

#[test]
#[ignore = "stack overflow on class-subtraction recursion"]
fn xsd_regex_class_subtraction_stack() {
    let n = 5_000;
    let mut pat = String::new();
    for _ in 0..n {
        pat.push_str("[a-z-");
    }
    pat.push_str("[a-z]");
    for _ in 0..n {
        pat.push(']');
    }
    let _ = XsdRegex::compile(&pat);
}

// ─── Finding F-14 — XPath `$var` unsupported (low; documented as XPath 1.0) ──

#[test]
fn xpath_variable_reference_unsupported() {
    let mut doc = parse("<r><a>1</a></r>").unwrap();
    doc.prepare_xpath();
    let eval = XPathEvaluator::new();
    let root = doc.root();
    let res = eval.evaluate(&doc, root, "$x");
    assert!(
        res.is_err(),
        "XPath 1.0 variable reference is not implemented — library documents support for XPath 1.0"
    );
}

// ─── Finding F-15 — Namespace resolver lets `xml:` be rebound ───────────

#[test]
fn namespace_resolver_accepts_xml_rebinding() {
    use std::borrow::Cow;
    use uppsala::NamespaceResolver;
    let mut r = NamespaceResolver::new();
    r.declare(Cow::Borrowed("xml"), Cow::Borrowed("urn:evil"));
    let uri = r.resolve("xml").map(|c| c.as_ref().to_string());
    // Per XML Namespaces §3 rebinding "xml" should be refused.
    assert_ne!(
        uri.as_deref(),
        Some("urn:evil"),
        "resolver permitted rebinding the reserved prefix `xml`"
    );
}

// ─── Finding F-16 — Control characters silently emitted on serialize ───

#[test]
fn control_char_emitted_on_attribute_write() {
    // Construct an attribute value containing U+0001 (an illegal Char
    // in XML 1.0) and serialize. Writer must either reject or
    // numeric-escape; current behavior emits raw.
    let mut w = XmlWriter::new();
    w.start_element("r", &[("a", "x\u{0001}y")]);
    w.end_element("r");
    let out = w.into_string();
    // The resulting XML should NOT reparse — the 0x01 byte is invalid.
    let reparsed = parse(&out);
    assert!(
        reparsed.is_err(),
        "serializer allowed U+0001 to reach output, producing invalid XML"
    );
}

// ─── Finding F-17 — XPath //a//b//c cross-product blow-up ──────────────

#[test]
fn xpath_double_slash_blowup() {
    // Build a modestly large tree and time a predicate-in-predicate
    // query. 50 levels × 20 siblings.
    let mut xml = String::from("<r>");
    for i in 0..50 {
        xml.push_str(&format!("<l{}>", i));
    }
    for _ in 0..20 {
        xml.push_str("<leaf/>");
    }
    for i in (0..50).rev() {
        xml.push_str(&format!("</l{}>", i));
    }
    xml.push_str("</r>");
    // Move the owned String into the worker so &doc stays alive for 'static.
    run_with_timeout("xpath_double_slash", move || {
        let mut doc = parse(&xml).unwrap();
        doc.prepare_xpath();
        let eval = XPathEvaluator::new();
        let root = doc.root();
        let _ = eval.evaluate(&doc, root, "//*[//leaf]");
    });
}
