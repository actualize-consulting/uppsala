#![no_main]
use libfuzzer_sys::fuzz_target;
use uppsala::{parse, XsdValidator};

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let (schema_xml, instance_xml) = s
        .split_once('\0')
        .or_else(|| s.split_once("\n---INSTANCE---\n"))
        .unwrap_or((s, "<r/>"));
    if let Ok(schema_doc) = parse(schema_xml) {
        // Schema-poisoning surface. Composition is intentionally NOT
        // enabled — from_schema() does no I/O. A separate harness
        // should feed paths when fuzzing from_schema_with_base_path.
        if let Ok(validator) = XsdValidator::from_schema(&schema_doc) {
            if let Ok(instance_doc) = parse(instance_xml) {
                let _ = validator.validate(&instance_doc);
            }
        }
    }
});
