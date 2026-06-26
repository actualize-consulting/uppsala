use std::borrow::Cow;
use std::fs;
use std::path::PathBuf;

use uppsala::{
    parse, Document, NodeId, QName, XPath2Evaluator, XPathEvaluator, XmlWriter, XsdValidator,
};

fn validate(schema: &str, instance: &str) -> Vec<String> {
    let schema_doc = parse(schema).expect("parse schema");
    let validator = XsdValidator::from_schema(&schema_doc).expect("build validator");
    let doc = parse(instance).expect("parse instance");
    validator
        .validate(&doc)
        .into_iter()
        .map(|e| e.to_string())
        .collect()
}

fn mkdir_unique(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "uppsala-security-{}-{}-{}",
        label,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).expect("create tempdir");
    dir
}

#[test]
fn parser_rejects_duplicate_expanded_attributes() {
    let err = parse(r#"<r xmlns:a="urn:x" xmlns:b="urn:x" a:id="first" b:id="second"/>"#)
        .expect_err("duplicate expanded attributes must be rejected");
    assert!(err.to_string().contains("Duplicate attribute"));
}

#[test]
fn writer_sanitizes_structural_names() {
    let mut writer = XmlWriter::new();
    writer.start_element("bad name", &[("bad attr", "value")]);
    writer.end_element("bad name");
    let output = writer.into_string();

    assert_eq!(output, r#"<_ _="value"></_>"#);
    parse(&output).expect("sanitized writer output must reparse");
}

#[test]
fn dom_serializer_sanitizes_programmatic_names() {
    let mut doc = Document::new();
    let root = doc.root();
    let elem = doc.create_element(QName::local("bad name"));
    doc.append_child(root, elem);
    doc.element_mut(elem)
        .unwrap()
        .set_attribute(QName::local("bad attr"), Cow::Borrowed("value"));

    let output = doc.to_xml();
    assert_eq!(output, r#"<_ _="value"/>"#);
    parse(&output).expect("sanitized DOM output must reparse");
}

#[test]
fn doctype_is_omitted_unless_explicitly_requested() {
    let xml = r#"<?xml version="1.0"?><!DOCTYPE root SYSTEM "root.dtd"><root/>"#;
    let doc = parse(xml).unwrap();

    assert_eq!(doc.to_xml(), r#"<?xml version="1.0"?><root/>"#);
    let opts = uppsala::XmlWriteOptions::compact().with_doctype(true);
    assert_eq!(doc.to_xml_with_options(&opts), xml);
}

#[test]
fn dom_mutation_rejects_invalid_and_cyclic_node_ids() {
    let mut doc = Document::new();
    let root = doc.root();
    let a = doc.create_element(QName::local("a"));
    let b = doc.create_element(QName::local("b"));
    doc.append_child(root, a);
    doc.append_child(a, b);

    doc.append_child(b, a);
    assert_eq!(doc.parent(a), Some(root));
    assert_eq!(doc.parent(b), Some(a));

    doc.append_child(NodeId::new(99_999), a);
    doc.insert_before(root, NodeId::new(88_888), a);
    doc.replace_child(root, NodeId::new(77_777), a);

    assert_eq!(doc.parent(a), Some(root));
    assert_eq!(doc.children(root), vec![a]);
}

#[test]
fn xpath_name_tests_are_namespace_aware() {
    let doc = parse(r#"<r xmlns:a="urn:a"><a:item/><item/></r>"#).unwrap();
    let root = doc.document_element().unwrap();

    let eval = XPathEvaluator::new();
    let unprefixed = eval.select_nodes(&doc, root, "item").unwrap();
    assert_eq!(unprefixed.len(), 1);
    assert!(doc
        .element(unprefixed[0])
        .unwrap()
        .name
        .namespace_uri
        .is_none());

    let unbound = eval.select_nodes(&doc, root, "a:item").unwrap();
    assert!(unbound.is_empty());

    let mut bound = XPathEvaluator::new();
    bound.add_namespace("a", "urn:a");
    let prefixed = bound.select_nodes(&doc, root, "a:item").unwrap();
    assert_eq!(prefixed.len(), 1);
    assert_eq!(
        doc.element(prefixed[0])
            .unwrap()
            .name
            .namespace_uri
            .as_deref(),
        Some("urn:a")
    );
}

#[test]
fn xpath_axis_expansion_is_budgeted() {
    let xml = "<r><a><b/><b/><b/><b/></a></r>";
    let doc = parse(xml).unwrap();
    let root = doc.root();

    let err = XPathEvaluator::new()
        .with_max_node_visits(2)
        .select_nodes(&doc, root, "//b")
        .expect_err("low node visit budget must stop expansion");
    assert!(err.to_string().contains("node visit budget"));

    let nodes = XPathEvaluator::new()
        .with_max_node_visits(100)
        .select_nodes(&doc, root, "//b")
        .unwrap();
    assert_eq!(nodes.len(), 4);
}

#[test]
fn xpath2_range_allocation_is_bounded() {
    let doc = parse("<r/>").unwrap();
    let root = doc.root();

    let err = XPath2Evaluator::new()
        .with_max_sequence_items(3)
        .evaluate(&doc, root, "1 to 5")
        .expect_err("range over cap must fail");
    assert!(err.to_string().contains("exceeding maximum"));

    let value = XPath2Evaluator::new()
        .with_max_sequence_items(3)
        .evaluate(&doc, root, "1 to 3")
        .unwrap();
    assert_eq!(value.items().len(), 3);
}

#[test]
fn xsd_rejects_malformed_time_and_datetime_values() {
    let schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="t" type="xs:time"/>
  <xs:element name="dt" type="xs:dateTime"/>
</xs:schema>"#;

    assert!(validate(schema, "<t>23:59:59Z</t>").is_empty());
    assert!(!validate(schema, "<t>12:00:00:99</t>").is_empty());
    assert!(!validate(schema, "<t>12:00:00+99:00</t>").is_empty());
    assert!(!validate(schema, "<t>24:00:01</t>").is_empty());
    assert!(!validate(schema, "<dt>2024-01-01T12:00:00+99:00</dt>").is_empty());
}

#[test]
fn namespaced_root_does_not_fall_back_to_no_namespace_declaration() {
    let schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root" type="xs:string"/>
</xs:schema>"#;

    let errors = validate(schema, r#"<x:root xmlns:x="urn:x">ok</x:root>"#);
    assert!(
        errors.iter().any(|e| e.contains("No element declaration")),
        "expected no-declaration error, got {errors:?}"
    );
}

#[test]
fn prefixed_xsd_type_qnames_resolve_to_imported_namespace() {
    let dir = mkdir_unique("prefixed-type");

    let inner = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="urn:inner">
  <xs:simpleType name="Code">
    <xs:restriction base="xs:int"/>
  </xs:simpleType>
</xs:schema>"#;
    fs::write(dir.join("inner.xsd"), inner).unwrap();

    let outer = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:i="urn:inner"
           xmlns:o="urn:outer"
           targetNamespace="urn:outer"
           elementFormDefault="qualified">
  <xs:import namespace="urn:inner" schemaLocation="inner.xsd"/>
  <xs:element name="item" type="i:Code"/>
</xs:schema>"#;
    let outer_path = dir.join("outer.xsd");
    fs::write(&outer_path, outer).unwrap();

    let schema_doc = parse(outer).unwrap();
    let validator =
        XsdValidator::from_schema_with_base_path(&schema_doc, Some(&outer_path)).unwrap();
    let doc = parse(r#"<o:item xmlns:o="urn:outer">not-an-int</o:item>"#).unwrap();
    let errors: Vec<String> = validator
        .validate(&doc)
        .into_iter()
        .map(|e| e.to_string())
        .collect();

    fs::remove_dir_all(&dir).ok();
    assert!(
        errors.iter().any(|e| e.contains("not a valid int")),
        "imported int-based type should reject non-integer content, got {errors:?}"
    );
}

#[test]
fn unknown_named_xsd_type_fails_closed() {
    let schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:i="urn:inner"
           xmlns:o="urn:outer"
           targetNamespace="urn:outer"
           elementFormDefault="qualified">
  <xs:element name="item" type="i:Missing"/>
</xs:schema>"#;

    let errors = validate(schema, r#"<o:item xmlns:o="urn:outer">anything</o:item>"#);
    assert!(
        errors
            .iter()
            .any(|e| e.contains("Type '{urn:inner}Missing' not found")),
        "expected unresolved type error, got {errors:?}"
    );
}

#[test]
fn xsd_identity_constraints_use_namespace_uris() {
    let schema = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:r="urn:root"
           xmlns:v="urn:vehicle"
           targetNamespace="urn:root"
           elementFormDefault="qualified">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:any namespace="##any" processContents="skip" minOccurs="0" maxOccurs="unbounded"/>
      </xs:sequence>
    </xs:complexType>
    <xs:key name="vehicle_ids">
      <xs:selector xpath=".//v:vehicle"/>
      <xs:field xpath="@v:id"/>
    </xs:key>
  </xs:element>
</xs:schema>"###;

    let valid = r#"<r:root xmlns:r="urn:root" xmlns:v="urn:vehicle" xmlns:o="urn:other">
  <v:vehicle v:id="1"/>
  <o:vehicle v:id="1"/>
</r:root>"#;
    assert!(validate(schema, valid).is_empty());

    let invalid = r#"<r:root xmlns:r="urn:root" xmlns:v="urn:vehicle">
  <v:vehicle v:id="1"/>
  <v:vehicle v:id="1"/>
</r:root>"#;
    let errors = validate(schema, invalid);
    assert!(
        errors.iter().any(|e| e.contains("duplicate value")),
        "expected duplicate key error, got {errors:?}"
    );
}

#[test]
fn xsd_attribute_wildcard_union_stays_namespace_precise() {
    let schema = r###"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:t="urn:t"
           targetNamespace="urn:t"
           elementFormDefault="qualified">
  <xs:complexType name="Base">
    <xs:anyAttribute namespace="##local" processContents="skip"/>
  </xs:complexType>
  <xs:complexType name="Derived">
    <xs:complexContent>
      <xs:extension base="t:Base">
        <xs:anyAttribute namespace="##targetNamespace" processContents="skip"/>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>
  <xs:element name="r" type="t:Derived"/>
</xs:schema>"###;

    let valid = r#"<t:r xmlns:t="urn:t" local="ok" t:target="ok"/>"#;
    assert!(validate(schema, valid).is_empty());

    let invalid = r#"<t:r xmlns:t="urn:t" xmlns:f="urn:f" local="ok" t:target="ok" f:bad="no"/>"#;
    let errors = validate(schema, invalid);
    assert!(
        errors.iter().any(|e| e.contains("not allowed by wildcard")),
        "expected wildcard namespace error, got {errors:?}"
    );
}

#[test]
fn xsd_complex_type_derivation_cycles_are_rejected() {
    let schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           xmlns:t="urn:t"
           targetNamespace="urn:t"
           elementFormDefault="qualified">
  <xs:complexType name="A">
    <xs:complexContent><xs:extension base="t:B"/></xs:complexContent>
  </xs:complexType>
  <xs:complexType name="B">
    <xs:complexContent><xs:extension base="t:A"/></xs:complexContent>
  </xs:complexType>
  <xs:element name="r" type="t:A"/>
</xs:schema>"#;

    let errors = validate(schema, r#"<t:r xmlns:t="urn:t"/>"#);
    assert!(
        errors.iter().any(|e| e.contains("derivation cycle")),
        "expected derivation cycle error, got {errors:?}"
    );
}
