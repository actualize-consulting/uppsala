#![no_main]
use libfuzzer_sys::fuzz_target;
use uppsala::{parse, XPathEvaluator};

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    // Split input: first line = XPath expression, rest = XML doc.
    let (expr, xml) = match s.find('\n') {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, "<r/>"),
    };
    if let Ok(mut doc) = parse(xml) {
        doc.prepare_xpath();
        let eval = XPathEvaluator::new();
        let root = doc.root();
        let _ = eval.evaluate(&doc, root, expr);
    }
});
