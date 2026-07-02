#![no_main]
use libfuzzer_sys::fuzz_target;
use uppsala::XsdRegex;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let (pat, input) = match s.find('\n') {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    };
    if let Ok(re) = XsdRegex::compile(pat) {
        let _ = re.is_match(input);
    }
});
