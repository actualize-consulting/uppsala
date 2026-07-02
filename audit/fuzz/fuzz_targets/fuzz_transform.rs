#![no_main]
//! XSLT transform surface: `uppsala::transform(xslt, xml)`. Drives the XSLT
//! engine, the compiled-XPath evaluator underneath it, and the result-tree
//! serializer in one shot -- a large amount of code, including the unsafe SIMD
//! escaper on the output side.
//!
//! Input is split on a NUL byte into (stylesheet, source); NUL never appears in
//! XML text, so it is an unambiguous separator that libFuzzer can discover and
//! preserve. Any panic (as opposed to a returned `XmlError`) is a finding; the
//! bounded XSLT recursion depth means legitimate deep templates return an error
//! rather than overflowing the stack.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let (xslt, xml) = match s.split_once('\0') {
        Some((a, b)) => (a, b),
        None => return,
    };
    if let Ok(out) = uppsala::transform(xslt, xml) {
        // The transform output is XML (for method="xml"); reparsing exercises
        // the parser on machine-generated markup.
        let _ = uppsala::parse(&out);
    }
});
