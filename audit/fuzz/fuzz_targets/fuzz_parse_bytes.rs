#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Exercises decode_xml_bytes + parser. Focused on BOM /
    // UTF-16-without-BOM / odd-byte paths.
    let _ = uppsala::parse_bytes(data);
});
