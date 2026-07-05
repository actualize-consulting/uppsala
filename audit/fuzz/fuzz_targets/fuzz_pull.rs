#![no_main]
use libfuzzer_sys::fuzz_target;
use uppsala::{PullEvent, PullParser};

// Drives the scan-only pull surface (no DOM), asserting the event-stream
// invariants of ADR 0018, then checks the differential oracle: the raw event
// stream must accept/reject the input exactly like the DOM parser. The
// empty-entity end-of-document bug (W3C valid-sa-023) is the kind of finding
// this oracle exists for.
fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    let mut parser = PullParser::new(s);
    let mut open: Vec<(String, u32)> = Vec::new();
    let mut ns_starts = 0usize;
    let mut ns_ends = 0usize;
    let mut scan_failed = false;

    let check_range = |byte_start: usize, byte_end: usize| {
        assert!(
            byte_start <= byte_end && byte_end <= s.len(),
            "event byte range {byte_start}..{byte_end} out of bounds (len {})",
            s.len()
        );
    };

    while let Some(item) = parser.next() {
        let event = match item {
            Ok(event) => event,
            Err(_) => {
                scan_failed = true;
                // The iterator must fuse after an error.
                assert!(parser.next().is_none(), "iterator not fused after error");
                break;
            }
        };
        match event {
            PullEvent::StartElement {
                name,
                byte_start,
                byte_end,
                depth,
                ..
            } => {
                check_range(byte_start, byte_end);
                assert_eq!(depth as usize, open.len(), "StartElement depth mismatch");
                open.push((name.to_string(), depth));
            }
            PullEvent::EndElement {
                name,
                byte_start,
                byte_end,
                depth,
            } => {
                check_range(byte_start, byte_end);
                let (open_name, open_depth) =
                    open.pop().expect("EndElement without matching start");
                assert_eq!(name.to_string(), open_name, "EndElement name mismatch");
                assert_eq!(depth, open_depth, "EndElement depth mismatch");
            }
            PullEvent::StartNamespace { .. } => ns_starts += 1,
            PullEvent::EndNamespace => {
                ns_ends += 1;
                assert!(ns_ends <= ns_starts, "EndNamespace without matching start");
            }
            PullEvent::Text {
                content,
                byte_start,
                byte_end,
            } => {
                check_range(byte_start, byte_end);
                assert!(!content.is_empty(), "empty Text event");
            }
            PullEvent::CData {
                byte_start,
                byte_end,
                ..
            }
            | PullEvent::Comment {
                byte_start,
                byte_end,
                ..
            }
            | PullEvent::ProcessingInstruction {
                byte_start,
                byte_end,
                ..
            } => check_range(byte_start, byte_end),
            PullEvent::XmlDeclaration(_) | PullEvent::Doctype(_) => {}
        }
    }

    if !scan_failed {
        assert!(open.is_empty(), "elements left open after clean exhaustion");
        assert_eq!(ns_starts, ns_ends, "unbalanced namespace events");
    }

    // Differential oracle: both public surfaces must agree on accept/reject.
    let dom_ok = uppsala::parse(s).is_ok();
    assert_eq!(
        dom_ok, !scan_failed,
        "pull scan and DOM parser disagree (dom_ok={dom_ok})"
    );
});
