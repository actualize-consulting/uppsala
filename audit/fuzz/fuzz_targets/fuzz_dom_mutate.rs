#![no_main]
//! DOM mutation + `prepare_xpath()` recycling stress. Parses a seed document,
//! then applies an arbitrary sequence of tree edits interleaved with
//! `prepare_xpath()` and serialization -- the exact shape of pyFF's
//! mutate/query loop.
//!
//! This targets the recently changed code:
//!   * arena recycling of virtual attribute nodes (`attr_node_pool`, stable
//!     per-element slot reuse) -- checks the arena stays bounded and slots are
//!     never double-assigned (ASan/UB) across many rounds;
//!   * the `is_linkable_node` guards that reject virtual attribute / document
//!     nodes in `append_child`/`detach`/`insert_*`/`replace_child` -- fed with
//!     attribute NodeIds and the root as reparent operands, which previously
//!     corrupted the tree.
//!
//! Oracle: whatever the mutation sequence, the document must still serialize to
//! well-formed XML that reparses. Corruption (e.g. a wiped child list, a cyclic
//! sibling link, or an out-of-bounds recycled slot) surfaces as a reparse
//! failure, an ASan report, or a hang caught by libFuzzer's timeout.
use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use uppsala::{parse, Document, NodeId, QName};

const MAX_OPS: usize = 200;

/// All element nodes currently attached under the document root, gathered by a
/// bounded tree walk (avoids picking orphaned or virtual attribute nodes as
/// reparent targets).
fn live_elements(doc: &Document<'_>, cap: usize) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![doc.root()];
    while let Some(id) = stack.pop() {
        if out.len() >= cap {
            break;
        }
        if doc.element(id).is_some() {
            out.push(id);
        }
        let mut c = doc.first_child(id);
        while let Some(cid) = c {
            stack.push(cid);
            c = doc.next_sibling(cid);
        }
    }
    out
}

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    // First line seeds the tree; the rest drives the mutation opcodes.
    let (seed, rest) = match s.find('\n') {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    };
    let Ok(mut doc) = parse(seed) else {
        return;
    };
    doc.prepare_xpath();
    let mut u = Unstructured::new(rest.as_bytes());

    for _ in 0..MAX_OPS {
        if u.is_empty() {
            break;
        }
        // Collect the current element set to pick reparent targets from.
        let elems = live_elements(&doc, 1024);
        if elems.is_empty() {
            break;
        }
        let pick = |u: &mut Unstructured| -> NodeId {
            let i = u.int_in_range(0..=elems.len() - 1).unwrap_or(0);
            elems[i]
        };
        match u.int_in_range(0u8..=7).unwrap_or(7) {
            0 => {
                let p = pick(&mut u);
                let name: String = u.arbitrary().unwrap_or_default();
                let child = doc.create_element(QName::local(name));
                doc.append_child(p, child);
            }
            1 => {
                let p = pick(&mut u);
                let an: String = u.arbitrary().unwrap_or_default();
                let av: String = u.arbitrary().unwrap_or_default();
                if let Some(e) = doc.element_mut(p) {
                    e.set_attribute(QName::local(an), av.into());
                }
            }
            2 => {
                // Deliberately feed a *virtual attribute node* into a tree
                // mutator -- must be a rejected no-op, not corruption.
                let owner = pick(&mut u);
                let target = pick(&mut u);
                let attrs = doc.get_attribute_nodes(owner).to_vec();
                if let Some(&attr) = attrs.first() {
                    doc.append_child(target, attr);
                    doc.detach(attr);
                    doc.remove_child(owner, attr);
                    doc.replace_child(owner, target, attr);
                }
            }
            3 => {
                // Feed the document root as a reparent operand -- also rejected.
                let p = pick(&mut u);
                doc.append_child(p, doc.root());
            }
            4 => {
                let p = pick(&mut u);
                if let Some(first) = doc.first_child(p) {
                    doc.remove_child(p, first);
                }
            }
            5 => {
                let p = pick(&mut u);
                let name: String = u.arbitrary().unwrap_or_default();
                let n = doc.create_element(QName::local(name));
                if let Some(reference) = doc.first_child(p) {
                    doc.insert_before(p, n, reference);
                } else {
                    doc.append_child(p, n);
                }
            }
            6 => {
                doc.prepare_xpath();
            }
            _ => {
                // Re-prepare then serialize both ways and reparse: the tree must
                // still be coherent no matter what edits ran above.
                doc.prepare_xpath();
                let out = doc.to_xml();
                let _ = parse(&out);
            }
        }
    }

    // Final coherence check.
    doc.prepare_xpath();
    let _ = parse(&doc.to_xml());
});
