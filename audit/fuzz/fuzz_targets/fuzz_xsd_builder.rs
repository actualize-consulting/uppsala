#![no_main]
use libfuzzer_sys::fuzz_target;
use uppsala::{parse, XsdValidator};

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(schema_doc) = parse(s) {
        // Schema-poisoning surface. Composition is intentionally NOT
        // enabled — from_schema() does no I/O. A separate harness
        // should feed paths when fuzzing from_schema_with_base_path.
        let _ = XsdValidator::from_schema(&schema_doc);
    }
});
