#![no_main]
//! Differential + semantic-safety harness for the `unsafe` escape scanner
//! (`scan_escape_sse2`, new on this branch) and the serializer escaper that
//! rides on it (`write_escaped_run_dyn`).
//!
//! 1. **Path equality:** `scan_escape_sse2(data, is_attr)` must equal
//!    `scan_escape_scalar(data, is_attr)`. Both directions matter — an
//!    *over-long* "safe run" makes the serializer copy a byte verbatim that
//!    should have been escaped, i.e. emit an unescaped `<` or `&`. In a SAML
//!    consumer that is a markup-injection / assertion-integrity primitive.
//! 2. **Semantic safety (reference-independent):** the escaped *fragment* must
//!    never contain a raw `<`, `>`, `\r`, or a bare `&` that does not begin an
//!    entity — and, in attribute context, no raw `"`, `\t`, or `\n`. This is
//!    the property that actually protects a downstream consumer, asserted on
//!    the escaper output directly (not a whole serialized document, which
//!    legitimately contains structural `<`).
use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use uppsala::fuzz_exports as sx;

#[derive(Arbitrary, Debug)]
struct Input {
    is_attr: bool,
    data: Vec<u8>,
}

/// True if `&` at byte `i` begins a plausible XML entity (`&name;`, `&#123;`,
/// `&#xAB;`). Conservative — only needs to avoid false positives on the
/// escaper's own legitimate output.
fn amp_starts_entity(s: &str, i: usize) -> bool {
    let rest = &s[i + 1..];
    let Some(semi) = rest.find(';') else {
        return false;
    };
    let body = &rest[..semi];
    if body.is_empty() || body.len() > 32 {
        return false;
    }
    if let Some(num) = body.strip_prefix('#') {
        let num = num.strip_prefix(['x', 'X']).unwrap_or(num);
        return !num.is_empty() && num.chars().all(|c| c.is_ascii_hexdigit());
    }
    body.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
}

fn assert_no_injectable_bytes(out: &str, is_attr: bool) {
    let b = out.as_bytes();
    for (i, &c) in b.iter().enumerate() {
        assert_ne!(c, b'<', "escaped output has raw '<' at {}: {:?}", i, out);
        assert_ne!(c, b'>', "escaped output has raw '>' at {}: {:?}", i, out);
        assert_ne!(c, b'\r', "escaped output has raw CR at {}: {:?}", i, out);
        if is_attr {
            assert_ne!(c, b'"', "escaped attr has raw '\"' at {}: {:?}", i, out);
            assert_ne!(c, b'\t', "escaped attr has raw TAB at {}: {:?}", i, out);
            assert_ne!(c, b'\n', "escaped attr has raw LF at {}: {:?}", i, out);
        }
        if c == b'&' && !amp_starts_entity(out, i) {
            panic!("escaped output has bare '&' at {}: {:?}", i, out);
        }
    }
}

fuzz_target!(|bytes: &[u8]| {
    let mut u = Unstructured::new(bytes);
    let Ok(input) = Input::arbitrary(&mut u) else {
        return;
    };

    // (1) SSE2 vs scalar equality on raw bytes.
    let scalar = sx::scan_escape_scalar(&input.data, input.is_attr);
    assert!(scalar <= input.data.len());
    #[cfg(target_arch = "x86_64")]
    {
        let sse2 = sx::scan_escape_sse2(&input.data, input.is_attr);
        assert_eq!(
            sse2, scalar,
            "scan_escape divergence: sse2={} scalar={} is_attr={} data={:?}",
            sse2, scalar, input.is_attr, input.data
        );
    }

    // (2) Semantic safety on the real escaper, over the exact fragment. The
    // serializer takes &str, so this only applies to valid UTF-8 input.
    if let Ok(s) = std::str::from_utf8(&input.data) {
        let escaped = sx::escape_to_string(s, input.is_attr);
        assert_no_injectable_bytes(&escaped, input.is_attr);
    }
});
