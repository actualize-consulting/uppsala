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
//! rather than overflowing the stack. The harness also applies result-tree and
//! serialized-output caps: output-amplification is legal XSLT
//! behavior, but fuzzing should spend cycles on semantic bugs instead of
//! materializing or serializing tens of megabytes from a kilobyte-sized seed.
use libfuzzer_sys::fuzz_target;
use uppsala::{Parser, Stylesheet};

// These are harness limits, not production defaults. `Stylesheet` remains
// unbounded unless the caller opts in with the setter APIs.
const FUZZ_MAX_XSLT_DEPTH: u32 = 64;
const FUZZ_MAX_RESULT_TREE_BYTES: usize = 1 << 20;
const FUZZ_MAX_OUTPUT_BYTES: usize = 1 << 20;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let (xslt, xml) = match s
        .split_once('\0')
        .or_else(|| s.split_once("\n---XML---\n"))
    {
        Some((a, b)) => (a, b),
        None => return,
    };
    let Ok(style_doc) = Parser::new().parse(xslt) else {
        return;
    };
    let Ok(stylesheet) = Stylesheet::compile(&style_doc) else {
        return;
    };
    let Ok(mut source) = Parser::new().parse(xml) else {
        return;
    };
    source.prepare_xpath();
    if let Ok(out) = stylesheet
        .set_max_depth(FUZZ_MAX_XSLT_DEPTH)
        .set_max_result_tree_bytes(FUZZ_MAX_RESULT_TREE_BYTES)
        .set_max_output_bytes(FUZZ_MAX_OUTPUT_BYTES)
        .transform(&source)
    {
        // The transform output is XML (for method="xml"); reparsing exercises
        // the parser on machine-generated markup.
        let _ = uppsala::parse(&out);
    }
});
