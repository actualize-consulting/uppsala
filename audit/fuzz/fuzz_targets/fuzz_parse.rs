#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        // Any panic here is a finding. Resource limits belong in the
        // library; the harness just feeds input and lets libFuzzer
        // observe.
        let _ = uppsala::parse(s);
    }
});
