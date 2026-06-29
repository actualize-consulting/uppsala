//! Integration tests for XSD 1.1 validation.

// ─── Simple type validation ─────────────────────────────

fn validate_xml_against_xsd(xml: &str, xsd: &str) -> Result<(), String> {
    let schema_doc = uppsala::parse(xsd).map_err(|e| format!("Schema parse error: {}", e))?;
    let validator = uppsala::XsdValidator::from_schema(&schema_doc)
        .map_err(|e| format!("Schema load error: {}", e))?;
    let doc = uppsala::parse(xml).map_err(|e| format!("XML parse error: {}", e))?;
    let errors = validator.validate(&doc);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors
            .iter()
            .map(|e| format!("{}", e))
            .collect::<Vec<_>>()
            .join("; "))
    }
}

#[test]
fn xsd_string_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="name" type="xs:string"/>
</xs:schema>"#;
    let xml = "<name>John Doe</name>";
    assert!(validate_xml_against_xsd(xml, xsd).is_ok());
}

#[test]
fn xsd_integer_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="count" type="xs:integer"/>
</xs:schema>"#;
    let xml = "<count>42</count>";
    assert!(validate_xml_against_xsd(xml, xsd).is_ok());
}

#[test]
fn xsd_integer_invalid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="count" type="xs:integer"/>
</xs:schema>"#;
    let xml = "<count>not_a_number</count>";
    assert!(validate_xml_against_xsd(xml, xsd).is_err());
}

#[test]
fn xsd_boolean_true() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="flag" type="xs:boolean"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<flag>true</flag>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<flag>1</flag>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<flag>false</flag>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<flag>0</flag>", xsd).is_ok());
}

#[test]
fn xsd_boolean_invalid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="flag" type="xs:boolean"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<flag>yes</flag>", xsd).is_err());
}

#[test]
fn xsd_decimal_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="price" type="xs:decimal"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<price>19.99</price>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<price>-3.14</price>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<price>42</price>", xsd).is_ok());
}

#[test]
fn xsd_decimal_invalid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="price" type="xs:decimal"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<price>abc</price>", xsd).is_err());
}

#[test]
fn xsd_float_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="value" type="xs:float"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<value>1.5e2</value>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<value>-0.5</value>", xsd).is_ok());
}

#[test]
fn xsd_double_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="value" type="xs:double"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<value>3.14159</value>", xsd).is_ok());
}

// ─── Integer subtypes ───────────────────────────────────

#[test]
fn xsd_long_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="val" type="xs:long"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<val>9223372036854775807</val>", xsd).is_ok());
}

#[test]
fn xsd_int_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="val" type="xs:int"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<val>2147483647</val>", xsd).is_ok());
}

#[test]
fn xsd_short_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="val" type="xs:short"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<val>32767</val>", xsd).is_ok());
}

#[test]
fn xsd_byte_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="val" type="xs:byte"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<val>127</val>", xsd).is_ok());
}

#[test]
fn xsd_unsigned_int_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="val" type="xs:unsignedInt"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<val>4294967295</val>", xsd).is_ok());
}

#[test]
fn xsd_non_negative_integer_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="val" type="xs:nonNegativeInteger"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<val>0</val>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<val>999</val>", xsd).is_ok());
}

#[test]
fn xsd_non_negative_integer_invalid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="val" type="xs:nonNegativeInteger"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<val>-1</val>", xsd).is_err());
}

#[test]
fn xsd_positive_integer_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="val" type="xs:positiveInteger"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<val>1</val>", xsd).is_ok());
}

#[test]
fn xsd_positive_integer_invalid_zero() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="val" type="xs:positiveInteger"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<val>0</val>", xsd).is_err());
}

// ─── Complex types ──────────────────────────────────────

#[test]
fn xsd_complex_type_sequence() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="person">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="name" type="xs:string"/>
        <xs:element name="age" type="xs:integer"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;
    let xml = "<person><name>Alice</name><age>30</age></person>";
    assert!(validate_xml_against_xsd(xml, xsd).is_ok());
}

#[test]
fn xsd_complex_type_sequence_wrong_order() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="person">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="name" type="xs:string"/>
        <xs:element name="age" type="xs:integer"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;
    let xml = "<person><age>30</age><name>Alice</name></person>";
    assert!(validate_xml_against_xsd(xml, xsd).is_err());
}

#[test]
fn xsd_complex_type_sequence_missing_element() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="person">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="name" type="xs:string"/>
        <xs:element name="age" type="xs:integer"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;
    let xml = "<person><name>Alice</name></person>";
    assert!(validate_xml_against_xsd(xml, xsd).is_err());
}

// ─── Attributes in schema ───────────────────────────────

#[test]
fn xsd_required_attribute_present() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="item">
    <xs:complexType>
      <xs:simpleContent>
        <xs:extension base="xs:string">
          <xs:attribute name="id" type="xs:string" use="required"/>
        </xs:extension>
      </xs:simpleContent>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;
    let xml = r#"<item id="123">content</item>"#;
    assert!(validate_xml_against_xsd(xml, xsd).is_ok());
}

#[test]
fn xsd_required_attribute_missing() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="item">
    <xs:complexType>
      <xs:simpleContent>
        <xs:extension base="xs:string">
          <xs:attribute name="id" type="xs:string" use="required"/>
        </xs:extension>
      </xs:simpleContent>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;
    let xml = "<item>content</item>";
    assert!(validate_xml_against_xsd(xml, xsd).is_err());
}

// ─── Facets ─────────────────────────────────────────────

#[test]
fn xsd_min_max_inclusive() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="score">
    <xs:simpleType>
      <xs:restriction base="xs:integer">
        <xs:minInclusive value="0"/>
        <xs:maxInclusive value="100"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<score>50</score>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<score>0</score>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<score>100</score>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<score>-1</score>", xsd).is_err());
    assert!(validate_xml_against_xsd("<score>101</score>", xsd).is_err());
}

#[test]
fn xsd_min_max_exclusive() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="score">
    <xs:simpleType>
      <xs:restriction base="xs:integer">
        <xs:minExclusive value="0"/>
        <xs:maxExclusive value="100"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<score>50</score>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<score>1</score>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<score>99</score>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<score>0</score>", xsd).is_err());
    assert!(validate_xml_against_xsd("<score>100</score>", xsd).is_err());
}

#[test]
fn xsd_enumeration() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="color">
    <xs:simpleType>
      <xs:restriction base="xs:string">
        <xs:enumeration value="red"/>
        <xs:enumeration value="green"/>
        <xs:enumeration value="blue"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<color>red</color>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<color>green</color>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<color>blue</color>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<color>yellow</color>", xsd).is_err());
}

#[test]
fn xsd_min_length() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="name">
    <xs:simpleType>
      <xs:restriction base="xs:string">
        <xs:minLength value="3"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<name>abc</name>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<name>ab</name>", xsd).is_err());
}

#[test]
fn xsd_max_length() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="code">
    <xs:simpleType>
      <xs:restriction base="xs:string">
        <xs:maxLength value="5"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<code>abc</code>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<code>abcdef</code>", xsd).is_err());
}

#[test]
fn xsd_exact_length() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="zip">
    <xs:simpleType>
      <xs:restriction base="xs:string">
        <xs:length value="5"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<zip>12345</zip>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<zip>1234</zip>", xsd).is_err());
    assert!(validate_xml_against_xsd("<zip>123456</zip>", xsd).is_err());
}

#[test]
fn xsd_total_digits() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="val">
    <xs:simpleType>
      <xs:restriction base="xs:decimal">
        <xs:totalDigits value="5"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<val>12345</val>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<val>123.45</val>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<val>123456</val>", xsd).is_err());
}

#[test]
fn xsd_fraction_digits() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="val">
    <xs:simpleType>
      <xs:restriction base="xs:decimal">
        <xs:fractionDigits value="2"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<val>12.34</val>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<val>12.345</val>", xsd).is_err());
}

// ─── Date/time types ────────────────────────────────────

#[test]
fn xsd_date_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="d" type="xs:date"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<d>2024-01-15</d>", xsd).is_ok());
}

#[test]
fn xsd_date_invalid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="d" type="xs:date"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<d>not-a-date</d>", xsd).is_err());
}

#[test]
fn xsd_datetime_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="dt" type="xs:dateTime"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<dt>2024-01-15T10:30:00</dt>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<dt>2024-01-15T10:30:00Z</dt>", xsd).is_ok());
}

#[test]
fn xsd_time_valid() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="t" type="xs:time"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<t>10:30:00</t>", xsd).is_ok());
}

// ─── anyURI type ────────────────────────────────────────

#[test]
fn xsd_any_uri() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="url" type="xs:anyURI"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<url>http://example.com</url>", xsd).is_ok());
}

// ─── Wrong root element ─────────────────────────────────

#[test]
fn xsd_wrong_root_element() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="expected" type="xs:string"/>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<unexpected>data</unexpected>", xsd).is_err());
}

// ─── Nested complex types ───────────────────────────────

#[test]
fn xsd_nested_complex_types() {
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="order">
    <xs:complexType>
      <xs:sequence>
        <xs:element name="customer">
          <xs:complexType>
            <xs:sequence>
              <xs:element name="name" type="xs:string"/>
            </xs:sequence>
          </xs:complexType>
        </xs:element>
        <xs:element name="total" type="xs:decimal"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;
    let xml = "<order><customer><name>Bob</name></customer><total>99.95</total></order>";
    assert!(validate_xml_against_xsd(xml, xsd).is_ok());
}

// ─── List-type inheritance (issue #12) ─────────────────────────────

#[test]
fn xsd_list_inheritance() {
    // An inline <xs:simpleType> restricting a named list type must inherit
    // `is_list` from the base, so a `length` facet counts list *items*, not
    // characters. Regression test for issue #12.
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:simpleType name="DoubleListSimpleType">
    <xs:list itemType="xs:double"/>
  </xs:simpleType>
  <xs:element name="ThreePoint" nillable="false">
    <xs:simpleType>
      <xs:restriction base="DoubleListSimpleType">
        <xs:length value="3" fixed="true"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;

    // Exactly three list items — valid regardless of each item's char length.
    assert!(validate_xml_against_xsd("<ThreePoint>1.2 3.4 4.5</ThreePoint>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<ThreePoint>1 2 3</ThreePoint>", xsd).is_ok());

    // Wrong number of items — must fail the length facet.
    assert!(validate_xml_against_xsd("<ThreePoint>1.2 3.4</ThreePoint>", xsd).is_err());
    assert!(validate_xml_against_xsd("<ThreePoint>1.2 3.4 3 4</ThreePoint>", xsd).is_err());

    // Item type is still enforced: a non-double item must fail.
    assert!(validate_xml_against_xsd("<ThreePoint>1.2 abc 4.5</ThreePoint>", xsd).is_err());
}

#[test]
fn xsd_derived_list_inherits_item_facets() {
    // A named list type derived via <restriction> over another list type must
    // inherit the base list's item-level facets (e.g. a pattern on a
    // user-defined item type). Otherwise item constraints are silently dropped
    // and invalid items are accepted (fail-open). Regression for the
    // differential-review finding on `list_bases` / builder list passes.
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:simpleType name="HexByte">
    <xs:restriction base="xs:string">
      <xs:pattern value="[0-9A-Fa-f]{2}"/>
    </xs:restriction>
  </xs:simpleType>
  <xs:simpleType name="HexList">
    <xs:list itemType="HexByte"/>
  </xs:simpleType>
  <xs:simpleType name="ShortHexList">
    <xs:restriction base="HexList">
      <xs:maxLength value="3"/>
    </xs:restriction>
  </xs:simpleType>
  <xs:element name="data" type="ShortHexList"/>
</xs:schema>"#;

    // Valid: two well-formed hex bytes, within maxLength.
    assert!(validate_xml_against_xsd("<data>AA BB</data>", xsd).is_ok());
    // Item pattern must still be enforced through the derived list type.
    assert!(validate_xml_against_xsd("<data>AA ZZ</data>", xsd).is_err());
    // List-level maxLength (item count) must still be enforced.
    assert!(validate_xml_against_xsd("<data>AA BB CC DD</data>", xsd).is_err());
}

#[test]
fn xsd_inline_restriction_over_derived_list_enforces_item_facets() {
    // Combines both fixes: an *inline* simpleType restricting a *derived* list
    // type must inherit both is_list and the ultimate base list's item facets.
    let xsd = r#"<?xml version="1.0"?>
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:simpleType name="HexByte">
    <xs:restriction base="xs:string">
      <xs:pattern value="[0-9A-Fa-f]{2}"/>
    </xs:restriction>
  </xs:simpleType>
  <xs:simpleType name="HexList">
    <xs:list itemType="HexByte"/>
  </xs:simpleType>
  <xs:simpleType name="ShortHexList">
    <xs:restriction base="HexList">
      <xs:maxLength value="4"/>
    </xs:restriction>
  </xs:simpleType>
  <xs:element name="data">
    <xs:simpleType>
      <xs:restriction base="ShortHexList">
        <xs:length value="2"/>
      </xs:restriction>
    </xs:simpleType>
  </xs:element>
</xs:schema>"#;

    // Exactly two well-formed hex bytes.
    assert!(validate_xml_against_xsd("<data>AA BB</data>", xsd).is_ok());
    // length=2 counts items, not characters.
    assert!(validate_xml_against_xsd("<data>AA BB CC</data>", xsd).is_err());
    // Item pattern enforced transitively through the inline restriction.
    assert!(validate_xml_against_xsd("<data>AA ZZ</data>", xsd).is_err());
}

// ─── Nullable xs:choice regression (fastkml / KML issue) ────────────────
//
// A `<choice>` whose alternatives are all optional (every alternative has
// minOccurs=0) is nullable: it is satisfied by matching nothing. Such a
// choice appearing in a sequence must not reject a following element that
// belongs to a later particle. Regression for the KML AbstractFeatureType
// pattern, where an optional `Snippet|snippet` choice precedes the
// substitution-group feature list and previously caused
// "Element 'Placemark' does not match any choice alternative".

#[test]
fn nullable_choice_in_sequence_skips_to_next_particle() {
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:sequence>
        <xs:choice>
          <xs:element name="a" type="xs:string" minOccurs="0"/>
          <xs:element name="b" type="xs:string" minOccurs="0"/>
        </xs:choice>
        <xs:element name="c" type="xs:string" minOccurs="0" maxOccurs="unbounded"/>
      </xs:sequence>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;
    // `c` follows the all-optional choice; the choice must consume nothing.
    assert!(validate_xml_against_xsd("<root><c>x</c></root>", xsd).is_ok());
    // The choice can still be taken.
    assert!(validate_xml_against_xsd("<root><a>x</a><c>y</c></root>", xsd).is_ok());
    // Empty is valid too.
    assert!(validate_xml_against_xsd("<root/>", xsd).is_ok());
}

#[test]
fn required_choice_with_no_optional_alternatives_still_errors() {
    // A choice with no optional alternative is NOT nullable: a non-matching
    // child must still be rejected (guards against the fix over-relaxing).
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="root">
    <xs:complexType>
      <xs:choice>
        <xs:element name="a" type="xs:string"/>
        <xs:element name="b" type="xs:string"/>
      </xs:choice>
    </xs:complexType>
  </xs:element>
</xs:schema>"#;
    assert!(validate_xml_against_xsd("<root><a>x</a></root>", xsd).is_ok());
    assert!(validate_xml_against_xsd("<root><c>x</c></root>", xsd).is_err());
}

#[test]
fn kml_style_optional_choice_before_substitution_group() {
    // Faithful reduction of the KML AbstractFeatureType / DocumentType shape:
    // an optional inline choice inside the (extended) sequence, followed by a
    // reference to an abstract substitution-group head. A substituting member
    // (Placemark) as the only child must validate.
    let xsd = r#"<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"
           targetNamespace="urn:kml" xmlns:k="urn:kml" elementFormDefault="qualified">
  <xs:element name="AbstractFeatureGroup" type="k:AbstractFeatureType" abstract="true"/>
  <xs:complexType name="AbstractFeatureType" abstract="true">
    <xs:sequence>
      <xs:element name="name" type="xs:string" minOccurs="0"/>
      <xs:choice>
        <xs:element name="Snippet" type="xs:string" minOccurs="0"/>
        <xs:element name="snippet" type="xs:string" minOccurs="0"/>
      </xs:choice>
    </xs:sequence>
  </xs:complexType>
  <xs:element name="Placemark" type="k:PlacemarkType" substitutionGroup="k:AbstractFeatureGroup"/>
  <xs:complexType name="PlacemarkType">
    <xs:complexContent><xs:extension base="k:AbstractFeatureType"/></xs:complexContent>
  </xs:complexType>
  <xs:element name="Document" type="k:DocumentType"/>
  <xs:complexType name="DocumentType">
    <xs:complexContent>
      <xs:extension base="k:AbstractFeatureType">
        <xs:sequence>
          <xs:element ref="k:AbstractFeatureGroup" minOccurs="0" maxOccurs="unbounded"/>
        </xs:sequence>
      </xs:extension>
    </xs:complexContent>
  </xs:complexType>
</xs:schema>"#;
    let xml = r#"<Document xmlns="urn:kml"><Placemark/></Document>"#;
    assert!(
        validate_xml_against_xsd(xml, xsd).is_ok(),
        "{:?}",
        validate_xml_against_xsd(xml, xsd)
    );
}
