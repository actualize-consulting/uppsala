#![no_main]
//! Structure-aware serializer fuzzing. Builds an arbitrary DOM directly from the
//! fuzz bytes -- element names, attribute names/values, and text/CDATA/comment/PI
//! payloads all come from `arbitrary` `String`s, so they include control
//! characters, invalid-XML scalars, `]]>`, `--`, `?>`, `<`, `&`, `"` and
//! multi-byte sequences at every buffer offset. This is the strongest driver for
//! the unsafe `scan_escape_sse2` run-scanner (all 16-byte alignments and tail
//! paths) and for the name-sanitization helpers (`safe_xml_qname`,
//! `unique_safe_xml_qname`, `write_qname_sanitized`).
//!
//! The tree is depth- and node-bounded so a builder-side stack overflow or
//! runaway allocation cannot masquerade as a library finding.
use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use uppsala::{Document, NodeId, QName, XmlWriteOptions};

const MAX_DEPTH: u32 = 8;
const MAX_NODES: u32 = 400;

fn build(
    doc: &mut Document<'static>,
    parent: NodeId,
    u: &mut Unstructured,
    budget: &mut u32,
    depth: u32,
) {
    while *budget > 0 && !u.is_empty() {
        *budget -= 1;
        match u.int_in_range(0u8..=5).unwrap_or(5) {
            0 if depth < MAX_DEPTH => {
                let name: String = u.arbitrary().unwrap_or_default();
                let el = doc.create_element(QName::local(name));
                let nattr = u.int_in_range(0u8..=3).unwrap_or(0);
                for _ in 0..nattr {
                    let an: String = u.arbitrary().unwrap_or_default();
                    let av: String = u.arbitrary().unwrap_or_default();
                    if let Some(e) = doc.element_mut(el) {
                        e.set_attribute(QName::local(an), av.into());
                    }
                }
                // Optional programmatic namespace declaration.
                if u.arbitrary().unwrap_or(false) {
                    let px: String = u.arbitrary().unwrap_or_default();
                    let uri: String = u.arbitrary().unwrap_or_default();
                    let prefix = if px.is_empty() { None } else { Some(px) };
                    doc.declare_namespace(el, prefix.as_deref(), uri);
                }
                doc.append_child(parent, el);
                build(doc, el, u, budget, depth + 1);
            }
            1 => {
                let t: String = u.arbitrary().unwrap_or_default();
                let n = doc.create_text(t);
                doc.append_child(parent, n);
            }
            2 => {
                let t: String = u.arbitrary().unwrap_or_default();
                let n = doc.create_cdata(t);
                doc.append_child(parent, n);
            }
            3 => {
                let t: String = u.arbitrary().unwrap_or_default();
                let n = doc.create_comment(t);
                doc.append_child(parent, n);
            }
            4 => {
                let tgt: String = u.arbitrary().unwrap_or_default();
                let d: Option<String> = u.arbitrary().unwrap_or(None);
                let n = doc.create_processing_instruction(tgt, d.map(Into::into));
                doc.append_child(parent, n);
            }
            _ => break,
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let mut u = Unstructured::new(data);
    let mut doc = Document::new();
    let root = doc.root();
    let mut budget = MAX_NODES;
    build(&mut doc, root, &mut u, &mut budget, 0);

    // Three serializer configurations; ASan validates the unsafe SIMD on each.
    let compact = doc.to_xml();
    let _ = uppsala::parse(&compact);
    let pretty = doc.to_xml_with_options(&XmlWriteOptions::pretty("  "));
    let _ = uppsala::parse(&pretty);
    let expanded =
        doc.to_xml_with_options(&XmlWriteOptions::compact().with_expand_empty_elements(true));
    let _ = uppsala::parse(&expanded);
});
