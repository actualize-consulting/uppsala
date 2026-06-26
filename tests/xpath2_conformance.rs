//! Focused integration tests for the initial XPath 2.0 implementation.
//!
//! The evaluator intentionally implements a small XPath 2.0 slice. These tests
//! document that slice at the API boundary: lexer ownership behavior, sequence
//! evaluation, path navigation, namespace matching, and resolver isolation.

use std::borrow::Cow;

use uppsala::xpath2::lexer::{tokenize, TokenKind};
use uppsala::{parse, XPath2AtomicValue, XPath2Evaluator, XPath2Resolver, XPath2Value, XmlResult};

fn eval(expression: &str) -> XPath2Value {
    // Most expression tests do not need a real document. A minimal root keeps
    // the assertions focused on XPath 2.0 expression semantics.
    let doc = parse("<root/>").unwrap();
    XPath2Evaluator::new()
        .evaluate(&doc, doc.root(), expression)
        .unwrap()
}

fn atom_strings(value: &XPath2Value) -> Vec<String> {
    // Convert mixed XDM values to stable strings so tests can compare compact
    // sequences without matching every enum variant inline.
    value
        .items()
        .iter()
        .map(|item| match item {
            uppsala::XPath2Item::Atomic(value) => value.to_xpath_string(),
            uppsala::XPath2Item::Node(node) => format!("node:{}", node.index()),
        })
        .collect()
}

#[test]
fn lexer_borrows_unescaped_string_literals() {
    // Unescaped string literals should borrow from the input to preserve the
    // zero-copy lexer path for the common case.
    let tokens = tokenize("\"alpha\"").unwrap();
    match &tokens[0].kind {
        TokenKind::StringLiteral(Cow::Borrowed(value)) => assert_eq!(*value, "alpha"),
        other => panic!("expected borrowed string literal, got {other:?}"),
    }
}

#[test]
fn lexer_unescapes_doubled_quotes() {
    // XPath doubles quotes inside a string literal. That transform requires an
    // owned token because the source bytes are not the final value.
    let tokens = tokenize("\"a \"\"quote\"\"\"").unwrap();
    match &tokens[0].kind {
        TokenKind::StringLiteral(Cow::Owned(value)) => assert_eq!(value, "a \"quote\""),
        other => panic!("expected owned string literal, got {other:?}"),
    }
}

#[test]
fn evaluates_empty_sequence_and_sequence_constructor() {
    // Empty and comma-separated sequence constructors are the basis for later
    // expression forms such as function arguments and quantified bindings.
    assert!(eval("()").is_empty());
    assert_eq!(atom_strings(&eval("(1, 2, 3)")), ["1", "2", "3"]);
}

#[test]
fn evaluates_range_and_arithmetic() {
    // Arithmetic and range construction share numeric coercion paths. This
    // covers precedence, integer division, and eager `to` sequence creation.
    assert_eq!(atom_strings(&eval("1 to 4")), ["1", "2", "3", "4"]);
    assert_eq!(atom_strings(&eval("1 + 2 * 3")), ["7"]);
    assert_eq!(atom_strings(&eval("7 idiv 2")), ["3"]);
}

#[test]
fn evaluates_for_if_and_quantified_expressions() {
    // These forms all rely on dynamic context state: variable binding, boolean
    // branch selection, and short-circuiting quantified evaluation.
    assert_eq!(
        atom_strings(&eval("for $x in 1 to 3 return $x * 2")),
        ["2", "4", "6"]
    );
    assert_eq!(atom_strings(&eval("if (false()) then 1 else 2")), ["2"]);
    assert_eq!(
        atom_strings(&eval("some $x in 1 to 3 satisfies $x eq 2")),
        ["true"]
    );
    assert_eq!(
        atom_strings(&eval("every $x in 1 to 3 satisfies $x lt 4")),
        ["true"]
    );
}

#[test]
fn evaluates_path_navigation_and_predicates() {
    // Basic path evaluation should combine descendant traversal, attribute
    // predicates, and child selection in one document-backed query.
    let xml = r#"
        <library>
            <book category="fiction"><title>Dune</title></book>
            <book category="science"><title>Cosmos</title></book>
        </library>
    "#;
    let mut doc = parse(xml).unwrap();
    doc.prepare_xpath();
    let eval = XPath2Evaluator::new();
    let nodes = eval
        .select_nodes(&doc, doc.root(), "//book[@category = 'fiction']/title")
        .unwrap();

    assert_eq!(nodes.len(), 1);
    assert_eq!(doc.text_content_deep(nodes[0]), "Dune");
}

#[test]
fn evaluates_predicates_per_path_context_node() {
    // Numeric predicates are evaluated relative to each current path context.
    // `//section/item[1]` should pick the first item under every section, not
    // only the first item in document order.
    let xml = r#"
        <root>
            <section><item>A1</item><item>A2</item></section>
            <section><item>B1</item><item>B2</item></section>
        </root>
    "#;
    let doc = parse(xml).unwrap();
    let eval = XPath2Evaluator::new();
    let nodes = eval
        .select_nodes(&doc, doc.root(), "//section/item[1]")
        .unwrap();
    let text: Vec<String> = nodes
        .iter()
        .map(|node| doc.text_content_deep(*node))
        .collect();

    assert_eq!(text, ["A1", "B1"]);
}

#[test]
fn evaluates_node_set_intersect_and_except() {
    // XPath 2.0 node-set operators must de-duplicate in document order while
    // preserving set semantics for intersect and except.
    let xml = r#"
        <root>
            <item id="a"/>
            <item id="b" skip="yes"/>
            <item id="c"/>
        </root>
    "#;
    let mut doc = parse(xml).unwrap();
    doc.prepare_xpath();
    let eval = XPath2Evaluator::new();

    let intersect = eval
        .select_nodes(&doc, doc.root(), "//item intersect //*[@id = 'b']")
        .unwrap();
    assert_eq!(intersect.len(), 1);
    assert_eq!(doc.get_attribute(intersect[0], "id"), Some("b"));

    let except = eval
        .select_nodes(&doc, doc.root(), "//item except //item[@skip = 'yes']")
        .unwrap();
    let ids: Vec<&str> = except
        .iter()
        .map(|node| doc.get_attribute(*node, "id").unwrap())
        .collect();
    assert_eq!(ids, ["a", "c"]);
}

#[test]
fn allows_operator_words_as_path_step_names_after_separators() {
    // Operator keywords are still legal element names in path-step positions.
    // The parser should classify them by context instead of reserving them
    // globally.
    let doc = parse("<root><intersect/><except/><union/></root>").unwrap();
    let eval = XPath2Evaluator::new();

    assert_eq!(
        eval.select_nodes(&doc, doc.root(), "/root/intersect")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        eval.select_nodes(&doc, doc.root(), "/root/except")
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        eval.select_nodes(&doc, doc.root(), "/root/union")
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn evaluates_additional_axes() {
    // Covers axes that are easy to regress because they require sibling,
    // ancestor, and document-order traversal rather than direct children.
    let xml = r#"
        <root>
            <a/>
            <b><c/><d/></b>
            <e/>
        </root>
    "#;
    let doc = parse(xml).unwrap();
    let eval = XPath2Evaluator::new();
    let d = eval.select_nodes(&doc, doc.root(), "//d").unwrap()[0];
    let c = eval.select_nodes(&doc, doc.root(), "//c").unwrap()[0];

    assert_eq!(
        eval.select_nodes(&doc, d, "preceding-sibling::*")
            .unwrap()
            .len(),
        1
    );
    let following_sibling = eval.select_nodes(&doc, c, "following-sibling::*").unwrap()[0];
    assert_eq!(
        doc.element(following_sibling)
            .unwrap()
            .name
            .local_name
            .as_ref(),
        "d"
    );
    assert_eq!(
        eval.select_nodes(&doc, d, "ancestor::root").unwrap().len(),
        1
    );
    assert_eq!(eval.select_nodes(&doc, c, "following::e").unwrap().len(), 1);
    assert_eq!(eval.select_nodes(&doc, d, "preceding::a").unwrap().len(), 1);
}

#[test]
fn evaluates_prefix_and_local_wildcards() {
    // Prefix wildcards use evaluator namespace bindings, while local-name
    // wildcards intentionally match the local part across namespaces.
    let xml = r#"
        <root xmlns:ns="urn:a" xmlns:other="urn:b">
            <ns:target/>
            <other:target/>
            <ns:other/>
        </root>
    "#;
    let doc = parse(xml).unwrap();
    let mut eval = XPath2Evaluator::new();
    eval.add_namespace("ns", "urn:a");

    assert_eq!(
        eval.select_nodes(&doc, doc.root(), "//ns:*").unwrap().len(),
        2
    );
    assert_eq!(
        eval.select_nodes(&doc, doc.root(), "//*:target")
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn evaluates_name_tests_against_namespace_uris() {
    // A document may bind the lexical prefix `a` to an unexpected URI. The
    // evaluator must compare namespace URIs from its static bindings, not the
    // prefix text found in the document.
    let xml = r#"
        <root xmlns:a="urn:evil" xmlns:b="urn:a">
            <a:item/>
            <b:item/>
            <item/>
        </root>
    "#;
    let doc = parse(xml).unwrap();
    let root = doc.document_element().unwrap();

    let eval = XPath2Evaluator::new();
    assert_eq!(eval.select_nodes(&doc, root, "a:item").unwrap().len(), 0);
    assert_eq!(eval.select_nodes(&doc, root, "item").unwrap().len(), 1);

    let mut eval = XPath2Evaluator::new();
    eval.add_namespace("a", "urn:a");

    let nodes = eval.select_nodes(&doc, root, "a:item").unwrap();
    assert_eq!(nodes.len(), 1);
    let element = doc.element(nodes[0]).unwrap();
    assert_eq!(element.name.namespace_uri.as_deref(), Some("urn:a"));
    assert_eq!(element.name.prefix.as_deref(), Some("b"));

    assert_eq!(eval.select_nodes(&doc, root, "a:*").unwrap().len(), 1);
}

#[test]
fn evaluates_ebv_and_core_sequence_functions() {
    // Effective boolean value and simple sequence functions are used by
    // predicates and control-flow forms, so keep their behavior pinned.
    assert_eq!(atom_strings(&eval("exists(1 to 2)")), ["true"]);
    assert_eq!(atom_strings(&eval("empty(())")), ["true"]);
    assert_eq!(atom_strings(&eval("count(1 to 3)")), ["3"]);
    assert_eq!(atom_strings(&eval("not(())")), ["true"]);
}

#[derive(Debug, Clone, Copy)]
struct MemoryResolver;

impl XPath2Resolver for MemoryResolver {
    fn resolve_doc(&self, uri: &str) -> XmlResult<Option<XPath2Value>> {
        // Test resolver used to prove external resources are opt-in and routed
        // through the configured resolver rather than ambient file/network I/O.
        if uri == "urn:test" {
            Ok(Some(XPath2Value::atomic(XPath2AtomicValue::String(
                "resolved".to_string(),
            ))))
        } else {
            Ok(None)
        }
    }
}

#[test]
fn resolver_backed_doc_function_has_no_default_access() {
    // The default resolver denies `doc()` access. Supplying a resolver should
    // make only the resolver-provided resource visible.
    let doc = parse("<root/>").unwrap();
    let eval = XPath2Evaluator::new();
    assert!(eval.evaluate(&doc, doc.root(), "doc('urn:test')").is_err());

    let eval = eval.with_resolver(MemoryResolver);
    let value = eval.evaluate(&doc, doc.root(), "doc('urn:test')").unwrap();
    assert_eq!(atom_strings(&value), ["resolved"]);
}
