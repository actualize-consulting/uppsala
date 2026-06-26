//! XPath 2.0/XDM value model.

use std::fmt;

use crate::dom::{Document, NodeId, NodeKind};
use crate::error::{XmlError, XmlResult};

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
            return Err(XmlError::xpath(
                "effective boolean value is undefined for a sequence of multiple atomic values",
            ));
        }

        match first {
            XPath2Item::Node(_) => Ok(true),
            XPath2Item::Atomic(value) => Ok(value.effective_boolean_value()),
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

/// XPath 2.0 atomic values used by the evaluator.
///
/// Decimal and integer values are stored in lexical form to preserve the
/// planned arbitrary-precision representation without adding dependencies.
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
    /// `xs:untypedAtomic`, primarily from atomized DOM nodes.
    UntypedAtomic(String),
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

    pub(crate) fn is_numeric(&self) -> bool {
        matches!(
            self,
            XPath2AtomicValue::Integer(_)
                | XPath2AtomicValue::Decimal(_)
                | XPath2AtomicValue::Double(_)
        )
    }

    pub(crate) fn as_f64(&self) -> XmlResult<f64> {
        match self {
            XPath2AtomicValue::Integer(value)
            | XPath2AtomicValue::Decimal(value)
            | XPath2AtomicValue::UntypedAtomic(value)
            | XPath2AtomicValue::String(value) => value
                .trim()
                .parse::<f64>()
                .map_err(|_| XmlError::xpath(format!("cannot convert '{}' to a number", value))),
            XPath2AtomicValue::Double(value) => Ok(*value),
            XPath2AtomicValue::Boolean(value) => Ok(if *value { 1.0 } else { 0.0 }),
        }
    }

    pub(crate) fn as_i128(&self) -> XmlResult<i128> {
        match self {
            XPath2AtomicValue::Integer(value)
            | XPath2AtomicValue::UntypedAtomic(value)
            | XPath2AtomicValue::String(value) => value
                .trim()
                .parse::<i128>()
                .map_err(|_| XmlError::xpath(format!("cannot convert '{}' to an integer", value))),
            XPath2AtomicValue::Decimal(value) => parse_integerish_decimal(value),
            XPath2AtomicValue::Double(value) if value.fract() == 0.0 => Ok(*value as i128),
            XPath2AtomicValue::Double(value) => Err(XmlError::xpath(format!(
                "cannot convert '{}' to an integer",
                value
            ))),
            XPath2AtomicValue::Boolean(value) => Ok(if *value { 1 } else { 0 }),
        }
    }

    /// Return the XPath lexical string form.
    pub fn to_xpath_string(&self) -> String {
        match self {
            XPath2AtomicValue::String(value)
            | XPath2AtomicValue::Integer(value)
            | XPath2AtomicValue::Decimal(value)
            | XPath2AtomicValue::UntypedAtomic(value) => value.clone(),
            XPath2AtomicValue::Boolean(true) => "true".to_string(),
            XPath2AtomicValue::Boolean(false) => "false".to_string(),
            XPath2AtomicValue::Double(value) => {
                if value.is_nan() {
                    "NaN".to_string()
                } else {
                    value.to_string()
                }
            }
        }
    }

    pub(crate) fn effective_boolean_value(&self) -> bool {
        match self {
            XPath2AtomicValue::Boolean(value) => *value,
            XPath2AtomicValue::String(value) | XPath2AtomicValue::UntypedAtomic(value) => {
                !value.is_empty()
            }
            XPath2AtomicValue::Integer(value) | XPath2AtomicValue::Decimal(value) => {
                value.parse::<f64>().map(|n| n != 0.0).unwrap_or(false)
            }
            XPath2AtomicValue::Double(value) => *value != 0.0 && !value.is_nan(),
        }
    }
}

impl fmt::Display for XPath2AtomicValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_xpath_string())
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
                Err(XmlError::xpath(format!(
                    "cannot convert '{}' to an integer",
                    value
                )))
            }
        })
        .unwrap_or(Ok(trimmed))?;
    integer_part
        .parse::<i128>()
        .map_err(|_| XmlError::xpath(format!("cannot convert '{}' to an integer", value)))
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
