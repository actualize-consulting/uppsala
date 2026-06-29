# Uppsala

A **zero-dependency** pure Rust XML library.

Uppsala implements the core XML stack from parsing through schema validation,
with no external crates. The parser, DOM, XPath engine, XSD validator, and XSD
regex engine are built from scratch.

## Features

- **XML 1.0 (Fifth Edition)** parsing and well-formedness checking
- **Namespaces in XML 1.0 (Third Edition)** with prefix resolution and scoping
- **Arena-based DOM** with tree mutation (insert, remove, replace)
- **XPath 1.0** evaluation (all axes, functions, predicates, operators)
- **XSD 1.1 validation** (structures + datatypes, 40+ built-in types)
- **XSD regex engine** (custom NFA matcher for pattern facets)
- **Accelerated parsing** (SSE2 on x86_64, one-pass scalar delimiter scanning
  elsewhere)
- **Serialization** with round-trip fidelity, pretty-printing, and streaming output
- **XmlWriter** for imperative XML construction without a DOM
- **UTF-16 auto-detection** (LE/BE with or without BOM)

## Conformance

Uppsala is tested against the W3C conformance suites:

| Suite | Pass Rate | Tests |
|-------|-----------|-------|
| W3C XML Conformance (not-wf) | 100% | 631/631 |
| W3C XML Conformance (valid) | 100% | 531/531 |
| W3C XML Conformance (invalid) | 100% | 46/46 |
| W3C XSD -- NIST Datatypes | 100% | 19,217/19,217 |
| W3C XSD -- Sun Combined | 100% | 199/199 |
| W3C XSD -- MS DataTypes | 100% | 1,212/1,212 |

In addition there are 274 hand-crafted tests covering XML parsing, namespaces,
XPath evaluation, XSD validation, serialization round-trips, and source ranges.

```bash
# Run all tests
cargo test

# Run W3C XML Conformance Suite (~1208 tests)
cargo test --test w3c_xmlconf

# Run W3C XML Schema Test Suite (~20156 tests)
cargo test --test w3c_xsts -- --nocapture
```

## Performance

Uppsala uses accelerated byte scanning for text content and attribute values:
SSE2 on x86_64 and one-pass scalar delimiter scanning elsewhere. Performance
depends heavily on the document shape: long plain-text spans are favorable,
while very small documents are dominated by fixed parser overhead.

The table below compares release builds of Uppsala 0.5.0 against a local
checkout of roxmltree 0.21.1 using roxmltree's benchmark input files. Results
are median parse times from 101 samples on macOS arm64; values above 1.0 mean
Uppsala parsed faster than roxmltree.

| File | Size | Uppsala | roxmltree | Ratio |
|------|------|---------|-----------|-------|
| fonts.conf | 429 B | 3.6 us | 3.1 us | 0.85x |
| medium.svg | 155 KB | 279 us | 178 us | 0.64x |
| large.plist | 321 KB | 687 us | 730 us | 1.06x |
| huge.xml | 835 KB | 1.41 ms | 1.49 ms | 1.06x |
| gigantic.svg | 1.34 MB | 705 us | 861 us | 1.22x |
| cdata.xml | 102 KB | 83 us | 110 us | 1.33x |
| text.xml | 129 KB | 306 us | 2.54 ms | 8.32x |
| attributes.xml | 271 KB | 620 us | 2.30 ms | 3.72x |

The main production target is SAML: namespace-heavy documents in the 3-30 KB
range with signed assertions. On generated SAML-shaped inputs, default
namespace-aware parsing is at parity for medium/large responses and within a few
microseconds on tiny responses:

| File | Size | Uppsala | roxmltree | Ratio |
|------|------|---------|-----------|-------|
| SAML small | 3.5 KB | 4.8 us | 4.5 us | 0.92x |
| SAML medium | 9.1 KB | 11.1 us | 11.3 us | 1.02x |
| SAML large | 27.8 KB | 31.3 us | 31.0 us | 0.99x |

Disabling namespace resolution improves some ordinary XML inputs by roughly
5-20%, but SAML users should usually keep namespace-aware parsing enabled.

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
uppsala = "0.3"
```

### Parse and query

```rust
use uppsala::{parse, XPathEvaluator};
use uppsala::xpath::XPathValue;

let xml = r#"
<bookstore>
  <book category="fiction">
    <title>The Great Gatsby</title>
    <author>F. Scott Fitzgerald</author>
    <price>10.99</price>
  </book>
  <book category="non-fiction">
    <title>Sapiens</title>
    <author>Yuval Noah Harari</author>
    <price>14.99</price>
  </book>
</bookstore>
"#;

let mut doc = parse(xml).unwrap();

// DOM traversal
let titles = doc.get_elements_by_tag_name("title");
for id in &titles {
    println!("{}", doc.text_content_deep(*id));
}

// XPath queries
doc.prepare_xpath();
let eval = XPathEvaluator::new();
let root = doc.root();
if let Ok(XPathValue::NodeSet(nodes)) =
    eval.evaluate(&doc, root, "//book[@category='fiction']/title")
{
    for id in &nodes {
        println!("Fiction: {}", doc.text_content_deep(*id));
    }
}
```

### Validate against an XSD schema

```rust
use uppsala::{parse, XsdValidator};

let schema_xml = r#"
<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema">
  <xs:element name="temperature" type="xs:decimal"/>
</xs:schema>
"#;

let instance_xml = "<temperature>36.6</temperature>";

let schema_doc = parse(schema_xml).unwrap();
let instance_doc = parse(instance_xml).unwrap();
let validator = XsdValidator::from_schema(&schema_doc).unwrap();
let errors = validator.validate(&instance_doc);

if errors.is_empty() {
    println!("Valid!");
} else {
    for e in &errors {
        println!("Validation error: {}", e);
    }
}
```

### Build XML with XmlWriter

```rust
use uppsala::XmlWriter;

let mut w = XmlWriter::new();
w.write_declaration();
w.start_element("catalog", &[("xmlns", "urn:example:catalog")]);
w.start_element("item", &[("id", "1")]);
w.text("Widget");
w.end_element("item");
w.empty_element("item", &[("id", "2"), ("name", "Gadget")]);
w.end_element("catalog");

println!("{}", w.into_string());
```

### Pretty-print a document

```rust
use uppsala::{parse, XmlWriteOptions};

let xml = "<root><a><b>text</b></a></root>";
let doc = parse(xml).unwrap();
let opts = XmlWriteOptions::pretty("  ");
println!("{}", doc.to_xml_with_options(&opts));
```

## Architecture

Uppsala uses an arena-based DOM where all nodes live in a flat `Vec<NodeData>`
indexed by `NodeId(usize)`. Tree relationships are maintained through
parent/first_child/last_child/next_sibling/prev_sibling indices. This avoids
`Rc`/`RefCell` overhead and makes tree mutation straightforward.

```
src/
  lib.rs            Public API, parse(), parse_bytes(), encoding detection
  error.rs          XmlError enum, XmlResult type alias
  dom.rs            Arena-based DOM: Document, NodeId, QName, serialization
  parser.rs         XML 1.0 recursive-descent parser with full DTD internal subset
  simd.rs           SSE2-accelerated byte scanning (content + attribute delimiters)
  namespace.rs      Namespace prefix resolution with scope stack
  writer.rs         XmlWriter imperative builder
  xpath.rs          XPath 1.0 lexer, parser, and evaluator
  xsd/              XSD validator (split into submodules)
    mod.rs          Module declarations, re-exports
    types.rs        Core data structures (XsdValidator, ElementDecl, TypeDef, etc.)
    builder.rs      Multi-pass schema builder
    parser.rs       Schema element/type/attribute/group parsing
    validation.rs   Instance document validation
    builtins.rs     Built-in type validation, facet enforcement
    composition.rs  xs:include, xs:redefine, xs:import
    identity.rs     xs:key, xs:unique, xs:keyref
    datetime.rs     Date/time/duration validation
    decimal.rs      Arbitrary-precision decimal comparison
  xsd_regex.rs      XSD regex pattern engine (custom NFA matcher)
```

## Examples

The `examples/` directory contains runnable programs:

```bash
# Parse XML, traverse the DOM, and run XPath queries
cargo run --example parse_and_query

# Validate documents against XSD schemas
cargo run --example validate_schema

# Build XML programmatically with XmlWriter and DOM
cargo run --example build_xml
```

## Test Data Licensing

The `test-data/` directory contains third-party conformance test suites.
These files are **not** covered by Uppsala's BSD-2-Clause license; they
retain their original licenses as described below.

### W3C XML Conformance Test Suite

- **Location:** `test-data/xmlconf/`
- **Version:** 20130923
- **Source:** <https://www.w3.org/XML/Test/>
- **License:** [W3C Document License](https://www.w3.org/copyright/document-license-2023/)
- **Contributors:** James Clark (xmltest), Sun Microsystems, IBM,
  OASIS, Edinburgh University (eduni), and others

### W3C XML Schema Test Suite (XSTS)

- **Location:** `test-data/xsts/xmlschema2006-11-06/`
- **Version:** 2006-11-06
- **Source:** <https://www.w3.org/XML/2004/xml-schema-test-suite/>
- **License:** [W3C Document License](https://www.w3.org/copyright/document-license-2023/)
  (see `test-data/xsts/xmlschema2006-11-06/00COPYRIGHT`)
- **Contributors:** NIST, Microsoft, Sun Microsystems, Boeing

## License

Uppsala itself is licensed under the BSD-2-Clause license. See [LICENSE](LICENSE)
for details.
