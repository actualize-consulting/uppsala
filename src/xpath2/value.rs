//! XPath 2.0/XDM value model.

use std::fmt;

use crate::dom::{Document, NodeId, NodeKind};
use crate::error::{XmlError, XmlResult};

use super::types::{
    base64_binary_string, duration_canonical, hex_binary_string, AtomicType, DateTimeValue,
    DurationValue,
};

/// An XPath 2.0 value, represented as an ordered XDM sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct XPath2Value {
    items: Vec<XPath2Item>,
}

impl XPath2Value {
    /// Create an empty XDM sequence.
    pub fn empty() -> Self {
        Self { items: Vec::new() }
    }

    /// Create a sequence from XDM items.
    pub fn new(items: Vec<XPath2Item>) -> Self {
        Self { items }
    }

    /// Create a singleton sequence containing one atomic value.
    pub fn atomic(value: XPath2AtomicValue) -> Self {
        Self {
            items: vec![XPath2Item::Atomic(value)],
        }
    }

    /// Create a singleton sequence containing one DOM node.
    pub fn node(node: NodeId) -> Self {
        Self {
            items: vec![XPath2Item::Node(node)],
        }
    }

    /// Convenience: a singleton boolean sequence.
    pub fn boolean(value: bool) -> Self {
        Self::atomic(XPath2AtomicValue::Boolean(value))
    }

    /// Convenience: a singleton string sequence.
    pub fn string(value: impl Into<String>) -> Self {
        Self::atomic(XPath2AtomicValue::String(value.into()))
    }

    /// Return the sequence items.
    pub fn items(&self) -> &[XPath2Item] {
        &self.items
    }

    /// Consume this value and return its items.
    pub fn into_items(self) -> Vec<XPath2Item> {
        self.items
    }

    /// Number of items in the sequence.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether the sequence is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub(crate) fn push(&mut self, item: XPath2Item) {
        self.items.push(item);
    }

    pub(crate) fn extend(&mut self, other: XPath2Value) {
        self.items.extend(other.items);
    }

    /// Atomize this sequence, converting nodes to untyped atomic string values.
    pub fn atomized(&self, doc: &Document<'_>) -> Vec<XPath2AtomicValue> {
        self.items.iter().map(|item| item.atomized(doc)).collect()
    }

    /// Coerce this sequence to an XPath 2.0 effective boolean value.
    pub fn effective_boolean_value(&self, _doc: &Document<'_>) -> XmlResult<bool> {
        let Some(first) = self.items.first() else {
            return Ok(false);
        };

        if matches!(first, XPath2Item::Node(_)) {
            return Ok(true);
        }

        if self.items.len() > 1 {
            return Err(XmlError::xpath_code(
                "FORG0006",
                "effective boolean value is undefined for a sequence of multiple atomic values",
            ));
        }

        match first {
            XPath2Item::Node(_) => Ok(true),
            XPath2Item::Atomic(value) => value.effective_boolean_value(),
        }
    }

    /// Convert the sequence to its string value.
    ///
    /// This follows the XPath 2.0 sequence-to-string behavior used by functions
    /// such as `string()`: empty sequence maps to the empty string, otherwise
    /// the first item is converted to a string value.
    pub fn to_string_value(&self, doc: &Document<'_>) -> String {
        self.items
            .first()
            .map(|item| item.string_value(doc))
            .unwrap_or_default()
    }
}

/// One XDM item: either a DOM node or an atomic value.
#[derive(Debug, Clone, PartialEq)]
pub enum XPath2Item {
    /// A node in the caller-provided document.
    Node(NodeId),
    /// A typed atomic value.
    Atomic(XPath2AtomicValue),
}

impl XPath2Item {
    /// Return the item string value.
    pub fn string_value(&self, doc: &Document<'_>) -> String {
        match self {
            XPath2Item::Node(node) => node_string_value(doc, *node),
            XPath2Item::Atomic(value) => value.to_xpath_string(),
        }
    }

    /// Return the atomized value of this item.
    pub fn atomized(&self, doc: &Document<'_>) -> XPath2AtomicValue {
        match self {
            XPath2Item::Node(node) => {
                XPath2AtomicValue::UntypedAtomic(node_string_value(doc, *node))
            }
            XPath2Item::Atomic(value) => value.clone(),
        }
    }
}

/// An `xs:QName` value retaining its prefix, namespace URI, and local name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QNameValue {
    /// Optional namespace prefix.
    pub prefix: Option<String>,
    /// Namespace URI, if the QName is in a namespace.
    pub uri: Option<String>,
    /// Local part.
    pub local: String,
}

impl QNameValue {
    /// The lexical form (`prefix:local` or `local`).
    pub fn lexical(&self) -> String {
        match &self.prefix {
            Some(prefix) if !prefix.is_empty() => format!("{}:{}", prefix, self.local),
            _ => self.local.clone(),
        }
    }
}

/// XPath 2.0 atomic values used by the evaluator.
///
/// Decimal and integer values are stored in lexical form to preserve the
/// dependency-free arbitrary-precision representation. Types derived from a
/// primitive (e.g. `xs:token`, `xs:long`) are represented with [`Derived`],
/// which pairs the precise [`AtomicType`] with its underlying primitive value.
///
/// [`Derived`]: XPath2AtomicValue::Derived
#[derive(Debug, Clone, PartialEq)]
pub enum XPath2AtomicValue {
    /// `xs:string`
    String(String),
    /// `xs:boolean`
    Boolean(bool),
    /// `xs:integer`
    Integer(String),
    /// `xs:decimal`
    Decimal(String),
    /// `xs:double`
    Double(f64),
    /// `xs:float`
    Float(f64),
    /// `xs:untypedAtomic`, primarily from atomized DOM nodes.
    UntypedAtomic(String),
    /// `xs:anyURI`
    AnyUri(String),
    /// `xs:QName`
    QName(QNameValue),
    /// `xs:date`
    Date(DateTimeValue),
    /// `xs:time`
    Time(DateTimeValue),
    /// `xs:dateTime`
    DateTime(DateTimeValue),
    /// A gregorian value (`xs:gYear`, `xs:gYearMonth`, `xs:gMonth`,
    /// `xs:gMonthDay`, `xs:gDay`) carrying its precise type.
    Gregorian(DateTimeValue, AtomicType),
    /// A duration value carrying its precise duration subtype.
    Duration(DurationValue, AtomicType),
    /// `xs:hexBinary`
    HexBinary(Vec<u8>),
    /// `xs:base64Binary`
    Base64Binary(Vec<u8>),
    /// A value of a type derived from a primitive, pairing the precise atomic
    /// type with the underlying primitive value.
    Derived(AtomicType, Box<XPath2AtomicValue>),
}

impl XPath2AtomicValue {
    pub(crate) fn integer(value: i128) -> Self {
        XPath2AtomicValue::Integer(value.to_string())
    }

    pub(crate) fn decimal(value: impl Into<String>) -> Self {
        XPath2AtomicValue::Decimal(value.into())
    }

    pub(crate) fn double(value: f64) -> Self {
        XPath2AtomicValue::Double(value)
    }

    /// The precise atomic type of this value.
    pub fn type_of(&self) -> AtomicType {
        match self {
            XPath2AtomicValue::String(_) => AtomicType::String,
            XPath2AtomicValue::Boolean(_) => AtomicType::Boolean,
            XPath2AtomicValue::Integer(_) => AtomicType::Integer,
            XPath2AtomicValue::Decimal(_) => AtomicType::Decimal,
            XPath2AtomicValue::Double(_) => AtomicType::Double,
            XPath2AtomicValue::Float(_) => AtomicType::Float,
            XPath2AtomicValue::UntypedAtomic(_) => AtomicType::UntypedAtomic,
            XPath2AtomicValue::AnyUri(_) => AtomicType::AnyUri,
            XPath2AtomicValue::QName(_) => AtomicType::QName,
            XPath2AtomicValue::Date(_) => AtomicType::Date,
            XPath2AtomicValue::Time(_) => AtomicType::Time,
            XPath2AtomicValue::DateTime(_) => AtomicType::DateTime,
            XPath2AtomicValue::Gregorian(_, ty) => *ty,
            XPath2AtomicValue::Duration(_, ty) => *ty,
            XPath2AtomicValue::HexBinary(_) => AtomicType::HexBinary,
            XPath2AtomicValue::Base64Binary(_) => AtomicType::Base64Binary,
            XPath2AtomicValue::Derived(ty, _) => *ty,
        }
    }

    /// Peel off any [`Derived`] wrapper, returning the underlying primitive
    /// value. [`Derived`]: XPath2AtomicValue::Derived
    pub fn base(&self) -> &XPath2AtomicValue {
        match self {
            XPath2AtomicValue::Derived(_, inner) => inner.base(),
            other => other,
        }
    }

    pub(crate) fn is_numeric(&self) -> bool {
        self.type_of().is_numeric()
    }

    pub(crate) fn as_f64(&self) -> XmlResult<f64> {
        match self.base() {
            XPath2AtomicValue::Integer(value)
            | XPath2AtomicValue::Decimal(value)
            | XPath2AtomicValue::UntypedAtomic(value)
            | XPath2AtomicValue::AnyUri(value)
            | XPath2AtomicValue::String(value) => parse_number(value),
            XPath2AtomicValue::Double(value) | XPath2AtomicValue::Float(value) => Ok(*value),
            XPath2AtomicValue::Boolean(value) => Ok(if *value { 1.0 } else { 0.0 }),
            other => Err(XmlError::xpath_code(
                "FORG0001",
                format!("cannot convert {} to a number", other.type_of()),
            )),
        }
    }

    pub(crate) fn as_i128(&self) -> XmlResult<i128> {
        match self.base() {
            XPath2AtomicValue::Integer(value)
            | XPath2AtomicValue::UntypedAtomic(value)
            | XPath2AtomicValue::String(value) => value.trim().parse::<i128>().map_err(|_| {
                XmlError::xpath_code(
                    "FORG0001",
                    format!("cannot convert '{}' to an integer", value),
                )
            }),
            XPath2AtomicValue::Decimal(value) => parse_integerish_decimal(value),
            XPath2AtomicValue::Double(value) | XPath2AtomicValue::Float(value)
                if value.fract() == 0.0 && value.is_finite() =>
            {
                Ok(*value as i128)
            }
            XPath2AtomicValue::Double(value) | XPath2AtomicValue::Float(value) => {
                Err(XmlError::xpath_code(
                    "FOCA0002",
                    format!("cannot convert '{}' to an integer", value),
                ))
            }
            XPath2AtomicValue::Boolean(value) => Ok(if *value { 1 } else { 0 }),
            other => Err(XmlError::xpath_code(
                "FORG0001",
                format!("cannot convert {} to an integer", other.type_of()),
            )),
        }
    }

    /// Return the XPath lexical string form.
    pub fn to_xpath_string(&self) -> String {
        match self {
            XPath2AtomicValue::String(value)
            | XPath2AtomicValue::Integer(value)
            | XPath2AtomicValue::Decimal(value)
            | XPath2AtomicValue::UntypedAtomic(value)
            | XPath2AtomicValue::AnyUri(value) => value.clone(),
            XPath2AtomicValue::Boolean(true) => "true".to_string(),
            XPath2AtomicValue::Boolean(false) => "false".to_string(),
            XPath2AtomicValue::Double(value) | XPath2AtomicValue::Float(value) => {
                format_floating(*value)
            }
            XPath2AtomicValue::QName(qname) => qname.lexical(),
            XPath2AtomicValue::Date(v) => format_date(v),
            XPath2AtomicValue::Time(v) => format_time(v),
            XPath2AtomicValue::DateTime(v) => format_date_time(v),
            XPath2AtomicValue::Gregorian(v, ty) => format_gregorian(v, *ty),
            XPath2AtomicValue::Duration(v, ty) => duration_canonical(v, *ty),
            XPath2AtomicValue::HexBinary(bytes) => hex_binary_string(bytes),
            XPath2AtomicValue::Base64Binary(bytes) => base64_binary_string(bytes),
            XPath2AtomicValue::Derived(_, inner) => inner.to_xpath_string(),
        }
    }

    pub(crate) fn effective_boolean_value(&self) -> XmlResult<bool> {
        match self.base() {
            XPath2AtomicValue::Boolean(value) => Ok(*value),
            XPath2AtomicValue::String(value)
            | XPath2AtomicValue::UntypedAtomic(value)
            | XPath2AtomicValue::AnyUri(value) => Ok(!value.is_empty()),
            XPath2AtomicValue::Integer(value) | XPath2AtomicValue::Decimal(value) => {
                Ok(value.parse::<f64>().map(|n| n != 0.0).unwrap_or(false))
            }
            XPath2AtomicValue::Double(value) | XPath2AtomicValue::Float(value) => {
                Ok(*value != 0.0 && !value.is_nan())
            }
            other => Err(XmlError::xpath_code(
                "FORG0006",
                format!(
                    "effective boolean value is undefined for {}",
                    other.type_of()
                ),
            )),
        }
    }
}

impl fmt::Display for XPath2AtomicValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_xpath_string())
    }
}

/// Parse a number following XPath rules: `INF`, `-INF`, `NaN` are recognized.
fn parse_number(value: &str) -> XmlResult<f64> {
    let trimmed = value.trim();
    match trimmed {
        "INF" | "+INF" => return Ok(f64::INFINITY),
        "-INF" => return Ok(f64::NEG_INFINITY),
        "NaN" => return Ok(f64::NAN),
        _ => {}
    }
    trimmed.parse::<f64>().map_err(|_| {
        XmlError::xpath_code(
            "FORG0001",
            format!("cannot convert '{}' to a number", value),
        )
    })
}

/// Format an `xs:double`/`xs:float` per the XPath canonical rules: integral
/// values render without a fractional part, special values use `INF`/`NaN`.
pub(crate) fn format_floating(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "INF" } else { "-INF" }.to_string();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    let abs = value.abs();
    // XPath uses scientific notation outside [1e-6, 1e6) for doubles.
    if (1e-6..1e21).contains(&abs) {
        if value.fract() == 0.0 && abs < 1e15 {
            return format!("{}", value as i64);
        }
        let s = format!("{}", value);
        s
    } else {
        // Rust's `{:e}` yields e.g. `1e7`; XPath wants `1.0E7`.
        let formatted = format!("{:E}", value);
        normalize_exponent(&formatted)
    }
}

fn normalize_exponent(s: &str) -> String {
    if let Some((mantissa, exp)) = s.split_once('E') {
        let mantissa = if mantissa.contains('.') {
            mantissa.to_string()
        } else {
            format!("{}.0", mantissa)
        };
        let exp = exp.strip_prefix('+').unwrap_or(exp);
        format!("{}E{}", mantissa, exp)
    } else {
        s.to_string()
    }
}

fn format_year(year: i64) -> String {
    if year < 0 {
        format!("-{:04}", -year)
    } else {
        format!("{:04}", year)
    }
}

fn format_tz(tz: Option<i32>) -> String {
    match tz {
        None => String::new(),
        Some(0) => "Z".to_string(),
        Some(minutes) => {
            let sign = if minutes < 0 { '-' } else { '+' };
            let abs = minutes.abs();
            format!("{}{:02}:{:02}", sign, abs / 60, abs % 60)
        }
    }
}

fn format_seconds_component(second: f64) -> String {
    if second.fract() == 0.0 {
        format!("{:02}", second as u8)
    } else {
        let whole = second.trunc() as u8;
        let frac = format!("{}", second)
            .split_once('.')
            .map(|(_, f)| f.to_string())
            .unwrap_or_default();
        format!("{:02}.{}", whole, frac)
    }
}

fn format_date(v: &DateTimeValue) -> String {
    format!(
        "{}-{:02}-{:02}{}",
        format_year(v.year),
        v.month,
        v.day,
        format_tz(v.tz)
    )
}

fn format_time(v: &DateTimeValue) -> String {
    format!(
        "{:02}:{:02}:{}{}",
        v.hour,
        v.minute,
        format_seconds_component(v.second),
        format_tz(v.tz)
    )
}

fn format_date_time(v: &DateTimeValue) -> String {
    format!(
        "{}-{:02}-{:02}T{:02}:{:02}:{}{}",
        format_year(v.year),
        v.month,
        v.day,
        v.hour,
        v.minute,
        format_seconds_component(v.second),
        format_tz(v.tz)
    )
}

fn format_gregorian(v: &DateTimeValue, ty: AtomicType) -> String {
    let tz = format_tz(v.tz);
    match ty {
        AtomicType::GYear => format!("{}{}", format_year(v.year), tz),
        AtomicType::GYearMonth => format!("{}-{:02}{}", format_year(v.year), v.month, tz),
        AtomicType::GMonth => format!("--{:02}{}", v.month, tz),
        AtomicType::GMonthDay => format!("--{:02}-{:02}{}", v.month, v.day, tz),
        AtomicType::GDay => format!("---{:02}{}", v.day, tz),
        _ => String::new(),
    }
}

fn parse_integerish_decimal(value: &str) -> XmlResult<i128> {
    let trimmed = value.trim();
    let integer_part = trimmed
        .split_once('.')
        .map(|(whole, fraction)| {
            if fraction.chars().all(|c| c == '0') {
                Ok(whole)
            } else {
                Err(XmlError::xpath_code(
                    "FOCA0002",
                    format!("cannot convert '{}' to an integer", value),
                ))
            }
        })
        .unwrap_or(Ok(trimmed))?;
    let integer_part = if integer_part.is_empty() || integer_part == "+" || integer_part == "-" {
        "0"
    } else {
        integer_part
    };
    integer_part.parse::<i128>().map_err(|_| {
        XmlError::xpath_code(
            "FORG0001",
            format!("cannot convert '{}' to an integer", value),
        )
    })
}

fn node_string_value(doc: &Document<'_>, node: NodeId) -> String {
    match doc.node_kind(node) {
        Some(NodeKind::Attribute(_, value))
        | Some(NodeKind::Text(value))
        | Some(NodeKind::CData(value))
        | Some(NodeKind::Comment(value)) => value.to_string(),
        Some(NodeKind::ProcessingInstruction(pi)) => pi
            .data
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default(),
        Some(NodeKind::Document | NodeKind::Element(_)) => doc.text_content_deep(node),
        None => String::new(),
    }
}
