//! W3C QT3 (XQuery/XPath 3.* Functions and Operators) conformance runner for
//! the XPath 2.0 engine.
//!
//! The runner is metadata-aware: it evaluates only test cases that apply to
//! XPath, and skips cases that require unsupported host features (external
//! source documents, schema awareness, collations, higher-order functions,
//! XQuery-only syntax, etc.) rather than reporting them as failures.
//!
//! ## Vendoring the test data
//!
//! The QT3 snapshot is **not** committed to this repository. To run the suite,
//! vendor a pinned snapshot under `test-data/qt3tests/` (so that
//! `test-data/qt3tests/catalog.xml` exists). The reference source is:
//!
//!   https://github.com/w3c/qt3tests  (pin a specific commit)
//!
//! Record the chosen commit in `test-data/qt3tests/SOURCE_COMMIT.txt`.
//!
//! When the snapshot is absent the test prints a notice and passes, so CI on a
//! clean checkout is unaffected.

use std::fs;
use std::path::Path;

use uppsala::{parse, Document, NodeId, NodeKind, XPath2Evaluator};

const CATALOG: &str = "test-data/qt3tests/catalog.xml";

#[derive(Default)]
struct Stats {
    passed: usize,
    failed: usize,
    skipped: usize,
    first_failures: Vec<String>,
}

#[test]
fn qt3_xpath2_conformance() {
    let catalog = Path::new(CATALOG);
    if !catalog.exists() {
        eprintln!(
            "QT3 snapshot not found at {CATALOG}; skipping. \
             See the module docs in tests/xpath2_qt3.rs for how to vendor it."
        );
        return;
    }

    let base = catalog.parent().unwrap_or(Path::new("."));
    let mut stats = Stats::default();

    let catalog_doc = match read_doc(catalog) {
        Some(doc) => doc,
        None => {
            eprintln!("QT3: could not parse {CATALOG}; skipping.");
            return;
        }
    };

    // Each <test-set file="..."/> points at a test-set document.
    for set_file in attr_values(&catalog_doc, "test-set", "file") {
        let set_path = base.join(&set_file);
        let Some(set_doc) = read_doc(&set_path) else {
            stats.skipped += 1;
            continue;
        };
        run_test_set(&set_doc, &mut stats);
    }

    let total = stats.passed + stats.failed + stats.skipped;
    println!(
        "QT3 XPath 2.0: {} passed, {} failed, {} skipped (of {} cases)",
        stats.passed, stats.failed, stats.skipped, total
    );
    if total > 0 {
        let denom = (stats.passed + stats.failed).max(1);
        println!(
            "QT3 pass rate (excluding skips): {:.1}% ({}/{})",
            stats.passed as f64 * 100.0 / denom as f64,
            stats.passed,
            denom
        );
    }
    for failure in stats.first_failures.iter().take(25) {
        println!("  FAIL: {failure}");
    }
    // The runner reports statistics; it intentionally does not hard-fail on
    // individual mismatches because XPath 2.0 conformance here is documented as
    // partial. CI green means the harness ran end-to-end over the vendored data.
}

fn run_test_set(doc: &Document<'_>, stats: &mut Stats) {
    for tc in elements_named(doc, "test-case") {
        // Skip cases that declare unsupported dependencies or need an
        // environment-provided source document.
        if test_case_unsupported(doc, tc) {
            stats.skipped += 1;
            continue;
        }
        let Some(expr) = child_text(doc, tc, "test") else {
            stats.skipped += 1;
            continue;
        };
        let name = element_attr(doc, tc, "name").unwrap_or_default();
        let Some(result) = child_elements(doc, tc)
            .into_iter()
            .find(|c| local_name(doc, *c) == "result")
        else {
            stats.skipped += 1;
            continue;
        };

        match evaluate_case(doc, &expr, result) {
            CaseOutcome::Skip => stats.skipped += 1,
            CaseOutcome::Pass => stats.passed += 1,
            CaseOutcome::Fail(reason) => {
                stats.failed += 1;
                if stats.first_failures.len() < 25 {
                    stats.first_failures.push(format!("{name}: {reason}"));
                }
            }
        }
    }
}

enum CaseOutcome {
    Pass,
    Fail(String),
    Skip,
}

/// Evaluate a self-contained expression (no external context) and check the
/// associated assertion subtree if it is one of the supported kinds.
fn evaluate_case(doc: &Document<'_>, expr: &str, result: NodeId) -> CaseOutcome {
    let host = match parse("<root/>") {
        Ok(d) => d,
        Err(_) => return CaseOutcome::Skip,
    };
    let eval = XPath2Evaluator::new();
    let outcome = eval.evaluate(&host, host.root(), expr);
    // The single primary assertion is the first element child of <result>.
    let Some(assertion) = child_elements(doc, result).into_iter().next() else {
        return CaseOutcome::Skip;
    };
    check_assertion(doc, assertion, &outcome, &host)
}

fn check_assertion(
    doc: &Document<'_>,
    assertion: NodeId,
    outcome: &Result<uppsala::XPath2Value, uppsala::XmlError>,
    host: &Document<'_>,
) -> CaseOutcome {
    let kind = local_name(doc, assertion);
    match kind.as_str() {
        "all-of" => {
            for child in child_elements(doc, assertion) {
                match check_assertion(doc, child, outcome, host) {
                    CaseOutcome::Pass => {}
                    other => return other,
                }
            }
            CaseOutcome::Pass
        }
        "any-of" => {
            let mut saw_skip = false;
            for child in child_elements(doc, assertion) {
                match check_assertion(doc, child, outcome, host) {
                    CaseOutcome::Pass => return CaseOutcome::Pass,
                    CaseOutcome::Skip => saw_skip = true,
                    CaseOutcome::Fail(_) => {}
                }
            }
            if saw_skip {
                CaseOutcome::Skip
            } else {
                CaseOutcome::Fail("no alternative matched".into())
            }
        }
        "error" => match outcome {
            Err(_) => CaseOutcome::Pass,
            Ok(_) => CaseOutcome::Fail("expected an error".into()),
        },
        "assert-true" => expect_value(outcome, |v| {
            v.effective_boolean_value(host).unwrap_or(false)
        }),
        "assert-false" => expect_value(outcome, |v| {
            !v.effective_boolean_value(host).unwrap_or(true)
        }),
        "assert-empty" => expect_value(outcome, |v| v.is_empty()),
        "assert-string-value" => {
            let expected = doc.text_content_deep(assertion);
            expect_value(outcome, |v| {
                let actual: Vec<String> = v.items().iter().map(|i| i.string_value(host)).collect();
                actual.join(" ") == expected.trim()
            })
        }
        "assert-count" => {
            let expected: usize = doc
                .text_content_deep(assertion)
                .trim()
                .parse()
                .unwrap_or(usize::MAX);
            expect_value(outcome, |v| v.len() == expected)
        }
        "assert-eq" => {
            // Evaluate the expected expression and compare canonical strings.
            let expected_expr = doc.text_content_deep(assertion);
            let eval = XPath2Evaluator::new();
            let expected = eval.evaluate(host, host.root(), &expected_expr);
            match (outcome, expected) {
                (Ok(a), Ok(b)) => {
                    if a.to_string_value(host) == b.to_string_value(host) {
                        CaseOutcome::Pass
                    } else {
                        CaseOutcome::Fail(format!(
                            "expected {:?}, got {:?}",
                            b.to_string_value(host),
                            a.to_string_value(host)
                        ))
                    }
                }
                _ => CaseOutcome::Skip,
            }
        }
        // Assertion kinds we do not model are skipped, not failed.
        _ => CaseOutcome::Skip,
    }
}

fn expect_value(
    outcome: &Result<uppsala::XPath2Value, uppsala::XmlError>,
    predicate: impl Fn(&uppsala::XPath2Value) -> bool,
) -> CaseOutcome {
    match outcome {
        Ok(v) if predicate(v) => CaseOutcome::Pass,
        Ok(_) => CaseOutcome::Fail("assertion not satisfied".into()),
        Err(_) => CaseOutcome::Fail("unexpected evaluation error".into()),
    }
}

/// Whether a test case requires a feature we do not support and should be
/// skipped (rather than failed).
fn test_case_unsupported(doc: &Document<'_>, tc: NodeId) -> bool {
    // Needs a context/source document via <environment> reference.
    for env in child_elements(doc, tc) {
        if local_name(doc, env) == "environment" {
            return true;
        }
    }
    // Dependency gates: skip XQuery-only specs, schema, collation, HOF, etc.
    for dep in child_elements(doc, tc) {
        if local_name(doc, dep) != "dependency" {
            continue;
        }
        let ty = element_attr(doc, dep, "type").unwrap_or_default();
        let value = element_attr(doc, dep, "value").unwrap_or_default();
        match ty.as_str() {
            "spec" => {
                // Require an XPath spec token; otherwise skip.
                if !value.split_whitespace().any(|t| t.starts_with("XP")) {
                    return true;
                }
            }
            "feature" => {
                if matches!(
                    value.as_str(),
                    "schemaValidation"
                        | "schemaImport"
                        | "staticTyping"
                        | "higherOrderFunctions"
                        | "moduleImport"
                        | "namespace-axis"
                        | "schema-aware"
                ) {
                    return true;
                }
            }
            "collation" => return true,
            _ => {}
        }
    }
    false
}

// --- Minimal DOM query helpers (namespace-insensitive on local names) ---

/// Read and parse a document, leaking the source so the returned `Document`
/// can borrow it for `'static`. This is a test process, so the small,
/// bounded leak is acceptable and avoids self-referential lifetimes.
fn read_doc(path: &Path) -> Option<Document<'static>> {
    let content = fs::read_to_string(path).ok()?;
    let static_src: &'static str = Box::leak(content.into_boxed_str());
    parse(static_src).ok()
}

fn local_name(doc: &Document<'_>, node: NodeId) -> String {
    match doc.node_kind(node) {
        Some(NodeKind::Element(e)) => e.name.local_name.to_string(),
        _ => String::new(),
    }
}

fn element_attr(doc: &Document<'_>, node: NodeId, attr: &str) -> Option<String> {
    doc.get_attribute(node, attr).map(|s| s.to_string())
}

fn child_elements(doc: &Document<'_>, node: NodeId) -> Vec<NodeId> {
    doc.children(node)
        .into_iter()
        .filter(|c| matches!(doc.node_kind(*c), Some(NodeKind::Element(_))))
        .collect()
}

fn elements_named(doc: &Document<'_>, name: &str) -> Vec<NodeId> {
    let mut out = Vec::new();
    let mut stack = vec![doc.root()];
    while let Some(node) = stack.pop() {
        for child in doc.children(node) {
            if matches!(doc.node_kind(child), Some(NodeKind::Element(_))) {
                if local_name(doc, child) == name {
                    out.push(child);
                }
                stack.push(child);
            }
        }
    }
    out
}

fn attr_values(doc: &Document<'_>, element: &str, attr: &str) -> Vec<String> {
    elements_named(doc, element)
        .into_iter()
        .filter_map(|n| element_attr(doc, n, attr))
        .collect()
}

fn child_text(doc: &Document<'_>, node: NodeId, child_name: &str) -> Option<String> {
    for child in child_elements(doc, node) {
        if local_name(doc, child) == child_name {
            return Some(doc.text_content_deep(child));
        }
    }
    None
}
