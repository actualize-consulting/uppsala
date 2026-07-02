#![no_main]
//! Differential harness for the `unsafe` SSE2 scanners in `src/simd.rs`
//! (`scan_content_sse2` / `scan_attr_sse2`). The SSE2 fast path and the scalar
//! reference MUST return identical `(pos, needs_validation)` for every input.
//!
//! This is the highest-value target: it compares the two paths *directly*
//! (through the `fuzzing` feature's `fuzz_exports`) instead of burying the
//! signal behind the parser, so a divergence surfaces as an assertion failure
//! rather than a subtle downstream mis-parse. It also asserts path-independent
//! invariants on the scalar reference (`pos <= len`; if it stopped, it stopped
//! on a genuine delimiter). Keep AddressSanitizer ON — the unaligned
//! `_mm_loadu_si128` loads at the `len % 16` tail boundary are what it watches.
//!
//! Historical note: the `needs_validation` flag used to diverge when a
//! delimiter was followed by an invalid byte in the same 16-byte chunk
//! (`"<" + 0xC3 + "a"*14` → sse2 `(0,true)` vs scalar `(0,false)`). That was
//! fixed by masking the validation lanes to the returned run; this harness is
//! the permanent regression guard.
use arbitrary::{Arbitrary, Unstructured};
use libfuzzer_sys::fuzz_target;
use uppsala::fuzz_exports as sx;

#[derive(Arbitrary, Debug)]
struct Input {
    quote: u8,
    data: Vec<u8>,
}

fn stop_byte_content(b: u8) -> bool {
    matches!(b, b'<' | b'&' | b'\r' | b']')
}
fn stop_byte_attr(b: u8, quote: u8) -> bool {
    b == quote || b == b'&' || b == b'<'
}

fn check_content(data: &[u8]) {
    let scalar = sx::scan_content_scalar(data);
    let (pos, _) = scalar;
    assert!(
        pos <= data.len(),
        "content: pos {} > len {}",
        pos,
        data.len()
    );
    if pos < data.len() {
        assert!(
            stop_byte_content(data[pos]),
            "content: stopped at non-delimiter {:#04x} at {}",
            data[pos],
            pos
        );
    }
    #[cfg(target_arch = "x86_64")]
    {
        let sse2 = sx::scan_content_sse2(data);
        assert_eq!(
            sse2, scalar,
            "scan_content divergence: sse2={:?} scalar={:?} data={:?}",
            sse2, scalar, data
        );
    }
}

fn check_attr(data: &[u8], quote: u8) {
    let scalar = sx::scan_attr_scalar(data, quote);
    let (pos, _) = scalar;
    assert!(pos <= data.len(), "attr: pos {} > len {}", pos, data.len());
    if pos < data.len() {
        assert!(
            stop_byte_attr(data[pos], quote),
            "attr: stopped at non-delimiter {:#04x} at {} (quote={:#04x})",
            data[pos],
            pos,
            quote
        );
    }
    #[cfg(target_arch = "x86_64")]
    {
        let sse2 = sx::scan_attr_sse2(data, quote);
        assert_eq!(
            sse2, scalar,
            "scan_attr divergence: sse2={:?} scalar={:?} quote={:#04x} data={:?}",
            sse2, scalar, quote, data
        );
    }
}

fuzz_target!(|bytes: &[u8]| {
    let mut u = Unstructured::new(bytes);
    let Ok(input) = Input::arbitrary(&mut u) else {
        return;
    };
    check_content(&input.data);
    // Fuzz the quote byte across all 256 values, not just `"` and `'`.
    check_attr(&input.data, input.quote);

    // NCName continuation scanner: SSE2 must equal scalar, the returned length
    // must be in bounds, and the byte at the stop offset (if any) must genuinely
    // be a non-continuation byte.
    let ncn_scalar = sx::scan_ncname_continuation_scalar(&input.data);
    assert!(ncn_scalar <= input.data.len());
    if ncn_scalar < input.data.len() {
        let b = input.data[ncn_scalar];
        let is_cont = b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.');
        assert!(
            !is_cont,
            "ncname scan stopped on a continuation byte {b:#04x}"
        );
    }
    #[cfg(target_arch = "x86_64")]
    {
        let ncn_sse2 = sx::scan_ncname_continuation_sse2(&input.data);
        assert_eq!(
            ncn_sse2, ncn_scalar,
            "scan_ncname_continuation divergence: sse2={ncn_sse2} scalar={ncn_scalar} data={:?}",
            input.data
        );
    }

    // Drive the arch-dispatched entry points too, so the SSE2 tail path is
    // exercised even when the checks above short-circuit early.
    let _ = sx::scan_content_delimiters(&input.data);
    let _ = sx::scan_attr_delimiters(&input.data, input.quote);
    let _ = sx::scan_ncname_continuation(&input.data);
});
