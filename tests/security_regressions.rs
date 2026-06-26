//! Security regression coverage for hardening shipped in the 0.5.0 cycle.
//!
//! These tests intentionally favor small, hand-written inputs over large
//! conformance fixtures. Each case captures a previously risky behavior and
//! documents the fail-closed outcome expected from the public API.

use std::borrow::Cow;
use std::fs;
use std::path::PathBuf;

use uppsala::{parse, Document, NodeId, QName, XPathEvaluator, XmlWriter, XsdValidator};

fn validate(schema: &str, instance: &str) -> Vec<String> {
    // Keep XSD assertions compact by returning display strings rather than
    // leaking validator internals into each test.
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
    // XSD import/include tests need a real base path; use a process-unique
    // directory so parallel test runs do not collide.
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
    // Namespace-aware XML forbids duplicate attributes after expansion, even
    // when the lexical prefixes differ. Accepting both would let the same
    // logical attribute carry conflicting values.
    let err = parse(r#"<r xmlns:a="urn:x" xmlns:b="urn:x" a:id="first" b:id="second"/>"#)
        .expect_err("duplicate expanded attributes must be rejected");
    assert!(err.to_string().contains("Duplicate attribute"));
}

#[test]
fn writer_sanitizes_structural_names() {
    // Programmatic writer callers can pass arbitrary strings as element and
    // attribute names. Invalid structural names are collapsed to a safe QName
    // instead of being emitted verbatim into markup position.
    let mut writer = XmlWriter::new();
    writer.start_element("bad name", &[("bad attr", "value")]);
    writer.end_element("bad name");
    let output = writer.into_string();

    assert_eq!(output, r#"<_ _="value"></_>"#);
    parse(&output).expect("sanitized writer output must reparse");
}

#[test]
fn writer_disambiguates_sanitized_attribute_collisions() {
    // Distinct invalid names can sanitize to the same fallback `_`. The writer
    // must make later names unique so the output remains well-formed XML.
    let mut writer = XmlWriter::new();
    writer.start_element(
        "r",
        &[("bad attr", "one"), ("bad\tattr", "two"), ("_", "three")],
    );
    writer.end_element("r");
    let output = writer.into_string();

    assert_eq!(output, r#"<r _="one" __1="two" __2="three"></r>"#);
    parse(&output).expect("collision-disambiguated writer output must reparse");
}

#[test]
fn dom_serializer_sanitizes_programmatic_names() {
    // The DOM path has the same structural-name threat model as XmlWriter:
    // names constructed in memory must not be able to break serialized XML.
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
fn dom_serializer_disambiguates_sanitized_attribute_collisions() {
    // DOM serialization also needs collision handling after sanitization,
    // because programmatic attributes can contain arbitrary invalid QNames.
    let mut doc = Document::new();
    let root = doc.root();
    let elem = doc.create_element(QName::local("r"));
    doc.append_child(root, elem);
    let elem = doc.element_mut(elem).unwrap();
    elem.set_attribute(QName::local("bad attr"), Cow::Borrowed("one"));
    elem.set_attribute(QName::local("bad\tattr"), Cow::Borrowed("two"));
    elem.set_attribute(QName::local("_"), Cow::Borrowed("three"));

    let output = doc.to_xml();
    assert_eq!(output, r#"<r _="one" __1="two" __2="three"/>"#);
    parse(&output).expect("collision-disambiguated DOM output must reparse");
}

#[test]
fn parse_bytes_rejects_odd_utf16_tail() {
    // A UTF-16 byte stream is a sequence of 16-bit code units. A trailing
    // orphan byte means the input is truncated and must not be silently
    // discarded by the decoder.
    let mut le = vec![0xFF, 0xFE];
    for code_unit in "<r/>".encode_utf16() {
        le.extend_from_slice(&code_unit.to_le_bytes());
    }
    le.push(0x41);
    assert!(uppsala::parse_bytes(&le).is_err());

    let mut be = vec![0xFE, 0xFF];
    for code_unit in "<r/>".encode_utf16() {
        be.extend_from_slice(&code_unit.to_be_bytes());
    }
    be.push(0x41);
    assert!(uppsala::parse_bytes(&be).is_err());
}

#[test]
fn doctype_is_omitted_unless_explicitly_requested() {
    // Parsed DOCTYPE declarations are preserved for callers that need them,
    // but serialization omits them by default to avoid handing DTDs to
    // downstream processors unintentionally.
    let xml = r#"<?xml version="1.0"?><!DOCTYPE root SYSTEM "root.dtd"><root/>"#;
    let doc = parse(xml).unwrap();

    assert_eq!(doc.to_xml(), r#"<?xml version="1.0"?><root/>"#);
    let opts = uppsala::XmlWriteOptions::compact().with_doctype(true);
    assert_eq!(doc.to_xml_with_options(&opts), xml);
}

#[test]
fn dom_mutation_rejects_invalid_and_cyclic_node_ids() {
    // Public mutation APIs accept NodeId values, so they must ignore invalid
    // IDs and ancestry cycles instead of corrupting the arena links.
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
    // XPath prefixes are expression bindings, not document prefix text. This
    // verifies unprefixed tests match only no-namespace nodes and unbound
    // prefixes fail closed.
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
    // Descendant-style axes can expand over the entire DOM. A low visit budget
    // must stop expansion with a stable diagnostic, while a normal budget still
    // returns the expected nodes.
    let xml = "<r><a><b/><b/><b/><b/></a></r>";
    let doc = parse(xml).unwrap();
    let root = doc.root();

    let err = XPathEvaluator::new()
        .with_max_node_visits(2)
        .select_nodes(&doc, root, "//b")
        .expect_err("low node visit budget must stop expansion");
    assert!(err.to_string().contains("maximum node visit budget of 2"));

    let nodes = XPathEvaluator::new()
        .with_max_node_visits(100)
        .select_nodes(&doc, root, "//b")
        .unwrap();
    assert_eq!(nodes.len(), 4);
}

#[test]
fn xpath_id_function_scan_is_budgeted() {
    // id() performs a document-wide lookup. It must consume the same visit
    // budget as axis expansion so callers can bound adversarial lookups.
    let mut xml = String::from("<r>");
    for i in 0..64 {
        xml.push_str(&format!(r#"<item id="item-{i}"/>"#));
    }
    xml.push_str(r#"<item id="target"/></r>"#);

    let doc = parse(&xml).unwrap();
    let root = doc.root();

    let err = XPathEvaluator::new()
        .with_max_node_visits(0)
        .select_nodes(&doc, root, "id('target')")
        .expect_err("low node visit budget must stop id() expansion");
    assert!(err.to_string().contains("maximum node visit budget of 0"));

    let nodes = XPathEvaluator::new()
        .with_max_node_visits(100)
        .select_nodes(&doc, root, "id('target')")
        .unwrap();
    assert_eq!(nodes.len(), 1);
}

#[test]
fn serializers_replace_invalid_xml_characters() {
    let mut writer = XmlWriter::new();
    writer.start_element("r", &[("a", "x\u{0001}y")]);
    writer.text("t\u{0000}u");
    writer.comment("c\u{0008}d");
    writer.processing_instruction("p", Some("q\u{000C}r"));
    writer.cdata("z\u{000B}w");
    writer.end_element("r");
    let xml = writer.into_string();

    assert!(!xml.contains('\u{0000}'));
    assert!(!xml.contains('\u{0001}'));
    assert!(!xml.contains('\u{0008}'));
    assert!(!xml.contains('\u{000B}'));
    assert!(!xml.contains('\u{000C}'));
    assert!(xml.contains('\u{FFFD}'));
    parse(&xml).expect("sanitized writer output must reparse");

    let mut doc = Document::new();
    let root = doc.root();
    let elem = doc.create_element(QName::local("r"));
    doc.append_child(root, elem);
    doc.element_mut(elem)
        .unwrap()
        .set_attribute(QName::local("a"), Cow::Borrowed("x\u{0001}y"));
    let text = doc.create_text("t\u{0000}u");
    doc.append_child(elem, text);
    let output = doc.to_xml();

    assert!(!output.contains('\u{0000}'));
    assert!(!output.contains('\u{0001}'));
    assert!(output.contains('\u{FFFD}'));
    parse(&output).expect("sanitized DOM output must reparse");
}

#[test]
fn xsd_rejects_malformed_time_and_datetime_values() {
    // Timezone parsing must reject invalid offsets and malformed Unicode input
    // as validation errors. The non-ASCII cases exercise byte-boundary safety.
    let schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="t" type="xs:time"/>
  <xs:element name="dt" type="xs:dateTime"/>
</xs:schema>"#;

    assert!(validate(schema, "<t>23:59:59Z</t>").is_empty());
    assert!(!validate(schema, "<t>12:00:00:99</t>").is_empty());
    assert!(!validate(schema, "<t>12:00:00+99:00</t>").is_empty());
    assert!(!validate(schema, "<t>24:00:01</t>").is_empty());
    assert!(!validate(schema, "<t>12:00:0é0+000</t>").is_empty());
    assert!(!validate(schema, "<dt>2024-01-01T12:00:00+99:00</dt>").is_empty());
    assert!(!validate(schema, "<dt>2024-01-01T12:00:0é0+000</dt>").is_empty());
}

#[test]
fn xsd_date_like_types_reject_out_of_range_timezone_offsets() {
    // Date-like XSD types use the same timezone range as xs:time/dateTime:
    // offsets are limited to +/-14:00 and minutes must be 00-59.
    let schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="date" type="xs:date"/>
  <xs:element name="gy" type="xs:gYear"/>
  <xs:element name="gym" type="xs:gYearMonth"/>
  <xs:element name="gm" type="xs:gMonth"/>
  <xs:element name="gmd" type="xs:gMonthDay"/>
  <xs:element name="gd" type="xs:gDay"/>
</xs:schema>"#;

    for invalid in [
        "<date>2024-01-01+99:99</date>",
        "<gy>2024+99:99</gy>",
        "<gym>2024-01+99:99</gym>",
        "<gm>--01+99:99</gm>",
        "<gmd>--01-01+99:99</gmd>",
        "<gd>---01+99:99</gd>",
        "<date>2024-01-01+14:01</date>",
        "<gy>2024+14:01</gy>",
        "<gym>2024-01+14:01</gym>",
        "<gm>--01+14:01</gm>",
        "<gmd>--01-01+14:01</gmd>",
        "<gd>---01+14:01</gd>",
    ] {
        let errors = validate(schema, invalid);
        assert!(!errors.is_empty(), "expected {invalid} to be invalid");
    }

    for valid in [
        "<date>2024-01-01+14:00</date>",
        "<gy>2024+14:00</gy>",
        "<gym>2024-01+14:00</gym>",
        "<gm>--01+14:00</gm>",
        "<gmd>--01-01+14:00</gmd>",
        "<gd>---01+14:00</gd>",
    ] {
        let errors = validate(schema, valid);
        assert!(
            errors.is_empty(),
            "expected {valid} to be valid, got {errors:?}"
        );
    }
}

#[test]
fn namespaced_root_does_not_fall_back_to_no_namespace_declaration() {
    // A no-namespace declaration must not validate a namespaced document root.
    // Falling back by local name would accept documents outside the schema's
    // declared namespace.
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
    // Prefixed `type` QNames in schemas are resolved through in-scope namespace
    // declarations. This keeps imported types precise and prevents local-name
    // fallback from masking invalid values.
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
    // An unresolved schema type reference must be a validation error. Treating
    // it as string-like content would silently bypass the intended constraint.
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
    // Identity-constraint selectors and fields must compare expanded names.
    // The valid document reuses a local name in another namespace; only the
    // target namespace should participate in the key.
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
fn xsd_identity_constraint_selector_uses_local_namespace_scope() {
    // Namespace declarations on xs:selector are in scope for that XPath. If
    // ignored, the selector matches nothing and duplicate key values bypass.
    let schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="urn:v"
           elementFormDefault="qualified">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="vehicle" type="xs:string" maxOccurs="unbounded"/>
      </xs:sequence>
    </xs:complexType>
    <xs:key name="vehicle_texts">
      <xs:selector xmlns:v="urn:v" xpath="v:vehicle"/>
      <xs:field xpath="."/>
    </xs:key>
  </xs:element>
</xs:schema>"#;

    let invalid = r#"<v:root xmlns:v="urn:v">
  <v:vehicle>same</v:vehicle>
  <v:vehicle>same</v:vehicle>
</v:root>"#;
    let errors = validate(schema, invalid);
    assert!(
        errors.iter().any(|e| e.contains("duplicate value")),
        "expected selector-local namespace to expose duplicate key, got {errors:?}"
    );
}

#[test]
fn xsd_identity_constraint_field_uses_local_namespace_scope() {
    // Namespace declarations on xs:field are in scope for that XPath. xs:unique
    // skips absent fields, so ignoring the local binding would accept duplicates.
    let schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="urn:v"
           elementFormDefault="qualified">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="vehicle" maxOccurs="unbounded">
          <xs:complexType>
            <xs:sequence>
              <xs:element name="id" type="xs:string"/>
            </xs:sequence>
          </xs:complexType>
        </xs:element>
      </xs:sequence>
    </xs:complexType>
    <xs:unique xmlns:s="urn:v" name="vehicle_ids">
      <xs:selector xpath="s:vehicle"/>
      <xs:field xmlns:f="urn:v" xpath="f:id"/>
    </xs:unique>
  </xs:element>
</xs:schema>"#;

    let invalid = r#"<v:root xmlns:v="urn:v">
  <v:vehicle><v:id>same</v:id></v:vehicle>
  <v:vehicle><v:id>same</v:id></v:vehicle>
</v:root>"#;
    let errors = validate(schema, invalid);
    assert!(
        errors.iter().any(|e| e.contains("duplicate value")),
        "expected field-local namespace to expose duplicate unique value, got {errors:?}"
    );
}

#[test]
fn xsd_keyref_field_uses_local_namespace_scope() {
    // Keyref fields with locally declared prefixes must resolve before the
    // tuple lookup. Otherwise missing field values are skipped and bad refs pass.
    let schema = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="urn:v"
           elementFormDefault="qualified">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="vehicle" maxOccurs="unbounded">
          <xs:complexType>
            <xs:sequence>
              <xs:element name="id" type="xs:string"/>
            </xs:sequence>
          </xs:complexType>
        </xs:element>
        <xs:element name="ref" maxOccurs="unbounded">
          <xs:complexType>
            <xs:sequence>
              <xs:element name="id" type="xs:string"/>
            </xs:sequence>
          </xs:complexType>
        </xs:element>
      </xs:sequence>
    </xs:complexType>
    <xs:key xmlns:s="urn:v" name="vehicle_ids">
      <xs:selector xpath="s:vehicle"/>
      <xs:field xpath="s:id"/>
    </xs:key>
    <xs:keyref xmlns:s="urn:v" name="vehicle_refs" refer="vehicle_ids">
      <xs:selector xpath="s:ref"/>
      <xs:field xmlns:f="urn:v" xpath="f:id"/>
    </xs:keyref>
  </xs:element>
</xs:schema>"#;

    let invalid = r#"<v:root xmlns:v="urn:v">
  <v:vehicle><v:id>known</v:id></v:vehicle>
  <v:ref><v:id>missing</v:id></v:ref>
</v:root>"#;
    let errors = validate(schema, invalid);
    assert!(
        errors.iter().any(|e| e.contains("no matching key value")),
        "expected field-local namespace to expose bad keyref, got {errors:?}"
    );
}

#[test]
fn xsd_attribute_wildcard_union_stays_namespace_precise() {
    // Attribute wildcard extension combines namespace constraints. The union
    // should allow only the local and target namespaces configured by the
    // schema, not arbitrary foreign attributes.
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
    // Recursive complex-type derivation can otherwise loop while resolving the
    // effective content model. The validator should detect the cycle and report
    // it instead of recursing indefinitely.
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
