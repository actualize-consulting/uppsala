//! DOM (Document Object Model) based on the XML Information Set specification.
//!
//! This module provides an arena-based tree representation of XML documents.
//! Each node is identified by a [`NodeId`] and stored in a central arena within
//! the [`Document`]. This avoids reference-counting overhead and makes tree
//! mutation straightforward.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;

/// A unique identifier for a node within a [`Document`].
///
/// Node IDs are lightweight handles (just a `usize` index into the document's
/// arena). They are [`Copy`], [`Hash`], and can be compared for equality.
/// Use [`NodeId::index()`] to get the raw index and [`NodeId::new()`] to
/// construct from a raw index (e.g. for FFI or serialization).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub(crate) usize);

impl NodeId {
    /// Create a `NodeId` from a raw arena index.
    ///
    /// The caller is responsible for ensuring the index refers to a valid node
    /// in the intended [`Document`]. Passing an out-of-range index will not
    /// cause undefined behaviour, but operations on the resulting `NodeId` will
    /// return `None` or silently do nothing.
    pub fn new(index: usize) -> Self {
        NodeId(index)
    }

    /// Return the raw arena index of this node.
    pub fn index(&self) -> usize {
        self.0
    }
}

/// A qualified name consisting of an optional namespace URI, optional prefix,
/// and a local name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct QName<'a> {
    /// The namespace URI, if any.
    pub namespace_uri: Option<Cow<'a, str>>,
    /// The namespace prefix, if any (e.g. `"soap"` in `soap:Envelope`).
    pub prefix: Option<Cow<'a, str>>,
    /// The local part of the name.
    pub local_name: Cow<'a, str>,
}

impl<'a> QName<'a> {
    /// Create a QName with only a local name (no namespace).
    pub fn local(name: impl Into<Cow<'a, str>>) -> Self {
        QName {
            namespace_uri: None,
            prefix: None,
            local_name: name.into(),
        }
    }

    /// Create a QName with a namespace URI and local name.
    pub fn with_namespace(
        namespace_uri: impl Into<Cow<'a, str>>,
        local_name: impl Into<Cow<'a, str>>,
    ) -> Self {
        QName {
            namespace_uri: Some(namespace_uri.into()),
            prefix: None,
            local_name: local_name.into(),
        }
    }

    /// Create a QName with prefix, namespace URI, and local name.
    pub fn full(
        prefix: impl Into<Cow<'a, str>>,
        namespace_uri: impl Into<Cow<'a, str>>,
        local_name: impl Into<Cow<'a, str>>,
    ) -> Self {
        QName {
            namespace_uri: Some(namespace_uri.into()),
            prefix: Some(prefix.into()),
            local_name: local_name.into(),
        }
    }

    /// Check whether this QName matches the given namespace URI and local name.
    ///
    /// Pass `Some("...")` for namespaced names or `None` for names without a
    /// namespace.
    ///
    /// # Example
    ///
    /// ```
    /// use uppsala::QName;
    /// let q = QName::with_namespace("urn:example", "Foo");
    /// assert!(q.matches(Some("urn:example"), "Foo"));
    /// assert!(!q.matches(Some("urn:other"), "Foo"));
    /// assert!(!q.matches(None, "Foo"));
    /// ```
    pub fn matches(&self, namespace_uri: Option<&str>, local_name: &str) -> bool {
        *self.local_name == *local_name && self.namespace_uri.as_deref() == namespace_uri
    }

    /// Returns the prefixed form (e.g. `"soap:Envelope"`) or just the local name.
    pub fn prefixed_name(&self) -> Cow<'_, str> {
        match &self.prefix {
            Some(p) => Cow::Owned(format!("{}:{}", p, self.local_name)),
            None => Cow::Borrowed(&self.local_name),
        }
    }

    /// Convert this QName into a `'static` lifetime by taking ownership of all data.
    pub fn into_static(self) -> QName<'static> {
        QName {
            namespace_uri: self.namespace_uri.map(|s| Cow::Owned(s.into_owned())),
            prefix: self.prefix.map(|s| Cow::Owned(s.into_owned())),
            local_name: Cow::Owned(self.local_name.into_owned()),
        }
    }
}

impl<'a> fmt::Display for QName<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.namespace_uri, &self.prefix) {
            (Some(ns), Some(p)) => write!(f, "{{{}}}{}:{}", ns, p, self.local_name),
            (Some(ns), None) => write!(f, "{{{}}}{}", ns, self.local_name),
            _ => write!(f, "{}", self.local_name),
        }
    }
}

/// An iterator over the children of a node.
///
/// This is a zero-allocation alternative to [`Document::children()`] which
/// returns a `Vec<NodeId>`. It walks the linked sibling chain directly.
pub struct ChildrenIter<'d, 'a> {
    doc: &'d Document<'a>,
    next: Option<NodeId>,
}

impl<'d, 'a> Iterator for ChildrenIter<'d, 'a> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        let id = self.next?;
        self.next = self.doc.nodes.get(id.0).and_then(|n| n.next_sibling);
        Some(id)
    }
}

/// An XML attribute (part of the Infoset attribute information item).
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute<'a> {
    /// The qualified name of the attribute.
    pub name: QName<'a>,
    /// The normalized attribute value.
    pub value: Cow<'a, str>,
}

impl<'a> Attribute<'a> {
    /// Convert this Attribute into a `'static` lifetime.
    pub fn into_static(self) -> Attribute<'static> {
        Attribute {
            name: self.name.into_static(),
            value: Cow::Owned(self.value.into_owned()),
        }
    }
}

/// The XML declaration (`<?xml version="1.0" encoding="UTF-8"?>`).
#[derive(Debug, Clone, PartialEq)]
pub struct XmlDeclaration<'a> {
    /// The XML version (e.g. `"1.0"`).
    pub version: Cow<'a, str>,
    /// The declared encoding (e.g. `"UTF-8"`), if specified.
    pub encoding: Option<Cow<'a, str>>,
    /// The standalone declaration, if specified (`true` for `"yes"`, `false` for `"no"`).
    pub standalone: Option<bool>,
}

impl<'a> XmlDeclaration<'a> {
    /// Convert this XmlDeclaration into a `'static` lifetime.
    pub fn into_static(self) -> XmlDeclaration<'static> {
        XmlDeclaration {
            version: Cow::Owned(self.version.into_owned()),
            encoding: self.encoding.map(|s| Cow::Owned(s.into_owned())),
            standalone: self.standalone,
        }
    }
}

/// A processing instruction (`<?target data?>`).
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessingInstruction<'a> {
    /// The PI target name (e.g. `"xml-stylesheet"`).
    pub target: Cow<'a, str>,
    /// The PI data string, if any.
    pub data: Option<Cow<'a, str>>,
}

impl<'a> ProcessingInstruction<'a> {
    /// Convert this ProcessingInstruction into a `'static` lifetime.
    pub fn into_static(self) -> ProcessingInstruction<'static> {
        ProcessingInstruction {
            target: Cow::Owned(self.target.into_owned()),
            data: self.data.map(|s| Cow::Owned(s.into_owned())),
        }
    }
}

/// The different kinds of nodes in the DOM tree.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind<'a> {
    /// The document root (Infoset document information item).
    Document,
    /// An element node (Infoset element information item).
    Element(Element<'a>),
    /// A text node (Infoset character information item).
    Text(Cow<'a, str>),
    /// A CDATA section.
    CData(Cow<'a, str>),
    /// A comment node (Infoset comment information item).
    Comment(Cow<'a, str>),
    /// A processing instruction (Infoset PI information item).
    ProcessingInstruction(ProcessingInstruction<'a>),
    /// A virtual attribute node (used by XPath evaluation).
    /// Not part of the normal child tree.
    Attribute(QName<'a>, Cow<'a, str>),
}

impl<'a> NodeKind<'a> {
    /// Convert this NodeKind into a `'static` lifetime.
    pub fn into_static(self) -> NodeKind<'static> {
        match self {
            NodeKind::Document => NodeKind::Document,
            NodeKind::Element(e) => NodeKind::Element(e.into_static()),
            NodeKind::Text(t) => NodeKind::Text(Cow::Owned(t.into_owned())),
            NodeKind::CData(t) => NodeKind::CData(Cow::Owned(t.into_owned())),
            NodeKind::Comment(t) => NodeKind::Comment(Cow::Owned(t.into_owned())),
            NodeKind::ProcessingInstruction(pi) => {
                NodeKind::ProcessingInstruction(pi.into_static())
            }
            NodeKind::Attribute(name, value) => {
                NodeKind::Attribute(name.into_static(), Cow::Owned(value.into_owned()))
            }
        }
    }
}

/// An element with its qualified name and attributes.
#[derive(Debug, Clone, PartialEq)]
pub struct Element<'a> {
    /// The qualified name of the element.
    pub name: QName<'a>,
    /// The element's attributes.
    pub attributes: Vec<Attribute<'a>>,
    /// In-scope namespace declarations on this element.
    /// Each pair is (prefix, namespace_uri). Empty prefix for default namespace.
    pub namespace_declarations: Vec<(Cow<'a, str>, Cow<'a, str>)>,
}

impl<'a> Element<'a> {
    /// Get an attribute value by local name (ignoring namespace).
    pub fn get_attribute(&self, local_name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| *a.name.local_name == *local_name)
            .map(|a| &*a.value)
    }

    /// Get an attribute value by namespace URI and local name.
    pub fn get_attribute_ns(&self, namespace_uri: &str, local_name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|a| {
                *a.name.local_name == *local_name
                    && a.name.namespace_uri.as_deref() == Some(namespace_uri)
            })
            .map(|a| &*a.value)
    }

    /// Check whether this element matches the given namespace URI and local name.
    ///
    /// Convenience wrapper around `self.name.matches(Some(ns), local)`.
    ///
    /// # Example
    ///
    /// ```
    /// let xml = r#"<saml:Issuer xmlns:saml="urn:oasis:names:tc:SAML:2.0:assertion">x</saml:Issuer>"#;
    /// let doc = uppsala::parse(xml).unwrap();
    /// let root = doc.document_element().unwrap();
    /// let elem = doc.element(root).unwrap();
    /// assert!(elem.matches_name_ns("urn:oasis:names:tc:SAML:2.0:assertion", "Issuer"));
    /// assert!(!elem.matches_name_ns("urn:other", "Issuer"));
    /// ```
    pub fn matches_name_ns(&self, namespace_uri: &str, local_name: &str) -> bool {
        self.name.matches(Some(namespace_uri), local_name)
    }

    /// Set or update an attribute. Returns the old value if the attribute already existed.
    pub fn set_attribute(&mut self, name: QName<'a>, value: Cow<'a, str>) -> Option<Cow<'a, str>> {
        for attr in &mut self.attributes {
            if attr.name == name {
                let old = std::mem::replace(&mut attr.value, value);
                return Some(old);
            }
        }
        self.attributes.push(Attribute { name, value });
        None
    }

    /// Remove an attribute by local name. Returns the removed value if found.
    pub fn remove_attribute(&mut self, local_name: &str) -> Option<Cow<'a, str>> {
        if let Some(pos) = self
            .attributes
            .iter()
            .position(|a| *a.name.local_name == *local_name)
        {
            Some(self.attributes.remove(pos).value)
        } else {
            None
        }
    }

    /// Convert this Element into a `'static` lifetime.
    pub fn into_static(self) -> Element<'static> {
        Element {
            name: self.name.into_static(),
            attributes: self
                .attributes
                .into_iter()
                .map(|a| a.into_static())
                .collect(),
            namespace_declarations: self
                .namespace_declarations
                .into_iter()
                .map(|(k, v)| (Cow::Owned(k.into_owned()), Cow::Owned(v.into_owned())))
                .collect::<Vec<_>>(),
        }
    }
}

/// Internal representation of a node in the arena.
#[derive(Debug, Clone)]
pub(crate) struct NodeData<'a> {
    pub kind: NodeKind<'a>,
    pub parent: Option<NodeId>,
    pub first_child: Option<NodeId>,
    pub last_child: Option<NodeId>,
    pub next_sibling: Option<NodeId>,
    pub prev_sibling: Option<NodeId>,
    /// Byte position in the original input for lazy line/column computation.
    pub byte_pos: usize,
    /// Byte position of the end of this node in the original input.
    pub byte_end_pos: usize,
}

impl<'a> NodeData<'a> {
    /// Convert this NodeData into a `'static` lifetime.
    pub fn into_static(self) -> NodeData<'static> {
        NodeData {
            kind: self.kind.into_static(),
            parent: self.parent,
            first_child: self.first_child,
            last_child: self.last_child,
            next_sibling: self.next_sibling,
            prev_sibling: self.prev_sibling,
            byte_pos: self.byte_pos,
            byte_end_pos: self.byte_end_pos,
        }
    }
}

/// An XML document represented as an arena-based tree.
///
/// Nodes are stored in a flat `Vec` and referenced by [`NodeId`]. This provides
/// O(1) node access and simple tree mutation without reference counting.
#[derive(Debug, Clone)]
pub struct Document<'a> {
    /// The node arena.
    pub(crate) nodes: Vec<NodeData<'a>>,
    /// The root node id (always NodeId(0), the Document node).
    root: NodeId,
    /// Optional XML declaration.
    pub xml_declaration: Option<XmlDeclaration<'a>>,
    /// Raw DOCTYPE declaration text, preserved verbatim for round-trip fidelity.
    /// e.g. `<!DOCTYPE root SYSTEM "root.dtd">` or `<!DOCTYPE html>`.
    pub doctype: Option<Cow<'a, str>>,
    /// Attribute nodes for each element, keyed by element NodeId.
    /// These are virtual nodes used by XPath attribute axis traversal.
    pub(crate) attribute_nodes: HashMap<NodeId, Vec<NodeId>>,
    /// Original input for lazy line/column computation from byte positions.
    pub(crate) input: &'a str,
}

impl<'a> Document<'a> {
    /// Create a new empty document.
    pub fn new() -> Self {
        let root_node = NodeData {
            kind: NodeKind::Document,
            parent: None,
            first_child: None,
            last_child: None,
            next_sibling: None,
            prev_sibling: None,
            byte_pos: 0,
            byte_end_pos: 0,
        };
        Document {
            nodes: vec![root_node],
            root: NodeId(0),
            xml_declaration: None,
            doctype: None,
            attribute_nodes: HashMap::new(),
            input: "",
        }
    }

    /// Convert this Document into a `'static` lifetime by taking ownership of all data.
    pub fn into_static(self) -> Document<'static> {
        Document {
            nodes: self.nodes.into_iter().map(|n| n.into_static()).collect(),
            root: self.root,
            xml_declaration: self.xml_declaration.map(|d| d.into_static()),
            doctype: self.doctype.map(|s| Cow::Owned(s.into_owned())),
            attribute_nodes: self.attribute_nodes,
            input: "",
        }
    }

    /// Returns the root (Document) node id.
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Returns the document element (the single top-level element), if any.
    pub fn document_element(&self) -> Option<NodeId> {
        self.children(self.root)
            .into_iter()
            .find(|&id| matches!(self.node_kind(id), Some(NodeKind::Element(_))))
    }

    /// Allocate a new node in the arena and return its id.
    pub(crate) fn alloc_node(&mut self, kind: NodeKind<'a>, byte_pos: usize) -> NodeId {
        let id = NodeId(self.nodes.len());
        self.nodes.push(NodeData {
            kind,
            parent: None,
            first_child: None,
            last_child: None,
            next_sibling: None,
            prev_sibling: None,
            byte_pos,
            byte_end_pos: 0,
        });
        id
    }

    /// Set the byte end position of a node.
    pub(crate) fn set_byte_end_pos(&mut self, id: NodeId, pos: usize) {
        if let Some(node) = self.nodes.get_mut(id.0) {
            node.byte_end_pos = pos;
        }
    }

    /// Allocate virtual attribute nodes for an element.
    /// Call this after adding an element with attributes to enable XPath attribute axis.
    pub(crate) fn build_attribute_nodes(&mut self, element_id: NodeId) {
        let attrs: Vec<(QName<'a>, Cow<'a, str>)> = match self.node_kind(element_id) {
            Some(NodeKind::Element(e)) => e
                .attributes
                .iter()
                .map(|a| (a.name.clone(), a.value.clone()))
                .collect(),
            _ => return,
        };
        let mut attr_ids = Vec::with_capacity(attrs.len());
        for (name, value) in attrs {
            let attr_id = self.alloc_node(NodeKind::Attribute(name, value), 0);
            // Set parent to the element (attribute nodes have an owner element)
            if let Some(node) = self.nodes.get_mut(attr_id.0) {
                node.parent = Some(element_id);
            }
            attr_ids.push(attr_id);
        }
        if !attr_ids.is_empty() {
            self.attribute_nodes.insert(element_id, attr_ids);
        }
    }

    /// Get the virtual attribute node IDs for an element.
    ///
    /// Returns an empty slice if [`prepare_xpath()`](Self::prepare_xpath) has
    /// not been called or the element has no attributes.
    pub fn get_attribute_nodes(&self, element_id: NodeId) -> &[NodeId] {
        self.attribute_nodes
            .get(&element_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Build virtual attribute nodes for all elements in the document.
    /// Must be called before XPath evaluation if the document was parsed
    /// without attribute node construction (the default for performance).
    pub fn prepare_xpath(&mut self) {
        if !self.attribute_nodes.is_empty() {
            return; // Already prepared
        }
        let element_ids: Vec<NodeId> = self
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(i, n)| match &n.kind {
                NodeKind::Element(e) if !e.attributes.is_empty() => Some(NodeId(i)),
                _ => None,
            })
            .collect();
        for elem_id in element_ids {
            self.build_attribute_nodes(elem_id);
        }
    }

    /// Create a new element node (not yet attached to the tree).
    pub fn create_element(&mut self, name: QName<'a>) -> NodeId {
        self.alloc_node(
            NodeKind::Element(Element {
                name,
                attributes: Vec::new(),
                namespace_declarations: Vec::new(),
            }),
            0,
        )
    }

    /// Create a new text node (not yet attached to the tree).
    pub fn create_text(&mut self, text: impl Into<Cow<'a, str>>) -> NodeId {
        self.alloc_node(NodeKind::Text(text.into()), 0)
    }

    /// Create a new comment node (not yet attached to the tree).
    pub fn create_comment(&mut self, text: impl Into<Cow<'a, str>>) -> NodeId {
        self.alloc_node(NodeKind::Comment(text.into()), 0)
    }

    /// Create a new processing instruction node (not yet attached to the tree).
    pub fn create_processing_instruction(
        &mut self,
        target: impl Into<Cow<'a, str>>,
        data: Option<Cow<'a, str>>,
    ) -> NodeId {
        self.alloc_node(
            NodeKind::ProcessingInstruction(ProcessingInstruction {
                target: target.into(),
                data,
            }),
            0,
        )
    }

    /// Create a new CDATA node (not yet attached to the tree).
    pub fn create_cdata(&mut self, text: impl Into<Cow<'a, str>>) -> NodeId {
        self.alloc_node(NodeKind::CData(text.into()), 0)
    }

    // ─── Tree access ───

    /// Get the kind of a node.
    pub fn node_kind(&self, id: NodeId) -> Option<&NodeKind<'a>> {
        self.nodes.get(id.0).map(|n| &n.kind)
    }

    /// Get a mutable reference to a node's kind.
    pub fn node_kind_mut(&mut self, id: NodeId) -> Option<&mut NodeKind<'a>> {
        self.nodes.get_mut(id.0).map(|n| &mut n.kind)
    }

    /// Get the element data for an element node.
    pub fn element(&self, id: NodeId) -> Option<&Element<'a>> {
        match self.node_kind(id) {
            Some(NodeKind::Element(e)) => Some(e),
            _ => None,
        }
    }

    /// Get mutable element data for an element node.
    pub fn element_mut(&mut self, id: NodeId) -> Option<&mut Element<'a>> {
        match self.node_kind_mut(id) {
            Some(NodeKind::Element(e)) => Some(e),
            _ => None,
        }
    }

    /// Get the text content of a text or CDATA node.
    pub fn text_content(&self, id: NodeId) -> Option<&str> {
        match self.node_kind(id) {
            Some(NodeKind::Text(t)) => Some(t),
            Some(NodeKind::CData(t)) => Some(t),
            _ => None,
        }
    }

    /// Get the text of an element's first Text or CDATA child, zero-copy.
    ///
    /// This is the common operation of reading the text inside an element like
    /// `<Name>value</Name>`. Unlike [`text_content_deep`](Self::text_content_deep)
    /// this does **not** allocate — it returns a borrowed `&str` from the
    /// original parsed input.
    ///
    /// Returns `None` if the node has no text/CDATA children.
    ///
    /// # Example
    ///
    /// ```
    /// let doc = uppsala::parse("<name>hello</name>").unwrap();
    /// let root = doc.document_element().unwrap();
    /// assert_eq!(doc.element_text(root), Some("hello"));
    /// ```
    pub fn element_text(&self, id: NodeId) -> Option<&str> {
        let mut child = self.nodes.get(id.0).and_then(|n| n.first_child);
        while let Some(cid) = child {
            match self.node_kind(cid) {
                Some(NodeKind::Text(t)) => return Some(t),
                Some(NodeKind::CData(t)) => return Some(t),
                _ => {}
            }
            child = self.nodes.get(cid.0).and_then(|n| n.next_sibling);
        }
        None
    }

    /// Get an attribute value by local name directly from a node ID.
    ///
    /// This is a convenience shortcut for `doc.element(id)?.get_attribute(name)`.
    ///
    /// # Example
    ///
    /// ```
    /// let doc = uppsala::parse(r#"<item id="42" status="active"/>"#).unwrap();
    /// let root = doc.document_element().unwrap();
    /// assert_eq!(doc.get_attribute(root, "id"), Some("42"));
    /// assert_eq!(doc.get_attribute(root, "missing"), None);
    /// ```
    pub fn get_attribute(&self, id: NodeId, local_name: &str) -> Option<&str> {
        self.element(id)?.get_attribute(local_name)
    }

    /// Get an attribute value by namespace URI and local name directly from a node ID.
    ///
    /// This is a convenience shortcut for `doc.element(id)?.get_attribute_ns(ns, name)`.
    pub fn get_attribute_ns(
        &self,
        id: NodeId,
        namespace_uri: &str,
        local_name: &str,
    ) -> Option<&str> {
        self.element(id)?
            .get_attribute_ns(namespace_uri, local_name)
    }

    /// Get the parent of a node.
    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        self.nodes.get(id.0).and_then(|n| n.parent)
    }

    /// Get the children of a node.
    pub fn children(&self, id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut current = self.nodes.get(id.0).and_then(|n| n.first_child);
        let mut steps = 0usize;
        while let Some(child_id) = current {
            if child_id.0 >= self.nodes.len() || steps >= self.nodes.len() {
                break;
            }
            result.push(child_id);
            current = self.nodes.get(child_id.0).and_then(|n| n.next_sibling);
            steps += 1;
        }
        result
    }

    /// Return a zero-allocation iterator over the children of a node.
    ///
    /// This is more efficient than [`children()`](Self::children) when you
    /// don't need the results as a `Vec`.
    ///
    /// # Example
    ///
    /// ```
    /// let doc = uppsala::parse("<r><a/><b/><c/></r>").unwrap();
    /// let root = doc.document_element().unwrap();
    /// let names: Vec<&str> = doc.children_iter(root)
    ///     .filter_map(|id| doc.element(id))
    ///     .map(|e| e.name.local_name.as_ref())
    ///     .collect();
    /// assert_eq!(names, vec!["a", "b", "c"]);
    /// ```
    pub fn children_iter(&self, id: NodeId) -> ChildrenIter<'_, 'a> {
        ChildrenIter {
            doc: self,
            next: self.nodes.get(id.0).and_then(|n| n.first_child),
        }
    }

    /// Get the source line of a node (computed lazily from byte position).
    pub fn node_line(&self, id: NodeId) -> usize {
        let byte_pos = match self.nodes.get(id.0) {
            Some(n) => n.byte_pos,
            None => return 0,
        };
        if self.input.is_empty() || byte_pos == 0 {
            return 1;
        }
        self.input.as_bytes()[..byte_pos]
            .iter()
            .filter(|&&b| b == b'\n')
            .count()
            + 1
    }

    /// Get the source column of a node (computed lazily from byte position).
    pub fn node_column(&self, id: NodeId) -> usize {
        let byte_pos = match self.nodes.get(id.0) {
            Some(n) => n.byte_pos,
            None => return 0,
        };
        if self.input.is_empty() || byte_pos == 0 {
            return 1;
        }
        let bytes = &self.input.as_bytes()[..byte_pos];
        match bytes.iter().rposition(|&b| b == b'\n') {
            Some(nl_pos) => byte_pos - nl_pos,
            None => byte_pos + 1,
        }
    }

    /// Returns the byte range of a node in the original source text.
    ///
    /// The range spans from the opening `<` of the element (or start of text/comment/PI)
    /// to the closing `>` of the end tag (or `/>` for self-closing elements).
    ///
    /// Returns `None` if the node was programmatically created (not parsed from source)
    /// or if the node ID is invalid.
    ///
    /// # Example
    /// ```
    /// let xml = r#"<root><child>text</child></root>"#;
    /// let doc = uppsala::parse(xml).unwrap();
    /// let root = doc.document_element().unwrap();
    /// let child_id = doc.children(root)[0];
    /// let range = doc.node_range(child_id).unwrap();
    /// assert_eq!(&xml[range], "<child>text</child>");
    /// ```
    pub fn node_range(&self, id: NodeId) -> Option<std::ops::Range<usize>> {
        let node = self.nodes.get(id.0)?;
        if node.byte_end_pos == 0 && id.0 != 0 {
            return None; // Programmatically created node
        }
        Some(node.byte_pos..node.byte_end_pos)
    }

    /// Returns the original source text of a node as a string slice.
    ///
    /// This is a convenience method equivalent to `&input[doc.node_range(id)?]`.
    /// Returns the exact text from the original XML input that produced this node.
    ///
    /// Returns `None` if the node was programmatically created or the ID is invalid.
    ///
    /// # Example
    /// ```
    /// let xml = r#"<root><item id="1">hello</item></root>"#;
    /// let doc = uppsala::parse(xml).unwrap();
    /// let root = doc.document_element().unwrap();
    /// let item = doc.children(root)[0];
    /// assert_eq!(doc.node_source(item).unwrap(), r#"<item id="1">hello</item>"#);
    /// ```
    pub fn node_source(&self, id: NodeId) -> Option<&'a str> {
        let range = self.node_range(id)?;
        if range.end > self.input.len() {
            return None;
        }
        Some(&self.input[range])
    }

    /// Returns the original input text that was parsed to create this document.
    ///
    /// Returns an empty string for programmatically constructed documents.
    pub fn input_text(&self) -> &'a str {
        self.input
    }

    /// Get all descendant element nodes matching a local name.
    pub fn get_elements_by_tag_name(&self, local_name: &str) -> Vec<NodeId> {
        let mut results = Vec::new();
        self.collect_elements_by_tag_name(self.root, local_name, &mut results);
        results
    }

    fn collect_elements_by_tag_name(
        &self,
        id: NodeId,
        local_name: &str,
        results: &mut Vec<NodeId>,
    ) {
        if let Some(NodeKind::Element(e)) = self.node_kind(id) {
            if *e.name.local_name == *local_name {
                results.push(id);
            }
        }
        for child in self.children(id) {
            self.collect_elements_by_tag_name(child, local_name, results);
        }
    }

    /// Get all descendant element nodes matching a namespace URI and local name.
    pub fn get_elements_by_tag_name_ns(
        &self,
        namespace_uri: &str,
        local_name: &str,
    ) -> Vec<NodeId> {
        let mut results = Vec::new();
        self.collect_elements_by_tag_name_ns(self.root, namespace_uri, local_name, &mut results);
        results
    }

    fn collect_elements_by_tag_name_ns(
        &self,
        id: NodeId,
        namespace_uri: &str,
        local_name: &str,
        results: &mut Vec<NodeId>,
    ) {
        if let Some(NodeKind::Element(e)) = self.node_kind(id) {
            if *e.name.local_name == *local_name
                && e.name.namespace_uri.as_deref() == Some(namespace_uri)
            {
                results.push(id);
            }
        }
        for child in self.children(id) {
            self.collect_elements_by_tag_name_ns(child, namespace_uri, local_name, results);
        }
    }

    /// Find the first direct child element matching a namespace URI and local name.
    ///
    /// Unlike [`get_elements_by_tag_name_ns`](Self::get_elements_by_tag_name_ns)
    /// which searches all descendants, this only looks at immediate children.
    ///
    /// # Example
    ///
    /// ```
    /// let xml = r#"<r xmlns:a="urn:a"><a:x/><a:y/><a:x/></r>"#;
    /// let doc = uppsala::parse(xml).unwrap();
    /// let root = doc.document_element().unwrap();
    /// let x = doc.first_child_element_by_name_ns(root, "urn:a", "x");
    /// assert!(x.is_some());
    /// let elem = doc.element(x.unwrap()).unwrap();
    /// assert_eq!(elem.name.local_name.as_ref(), "x");
    /// ```
    pub fn first_child_element_by_name_ns(
        &self,
        parent: NodeId,
        namespace_uri: &str,
        local_name: &str,
    ) -> Option<NodeId> {
        let mut child = self.nodes.get(parent.0).and_then(|n| n.first_child);
        while let Some(cid) = child {
            if let Some(elem) = self.element(cid) {
                if elem.matches_name_ns(namespace_uri, local_name) {
                    return Some(cid);
                }
            }
            child = self.nodes.get(cid.0).and_then(|n| n.next_sibling);
        }
        None
    }

    /// Find all direct child elements matching a namespace URI and local name.
    ///
    /// Unlike [`get_elements_by_tag_name_ns`](Self::get_elements_by_tag_name_ns)
    /// which searches all descendants, this only looks at immediate children.
    ///
    /// # Example
    ///
    /// ```
    /// let xml = r#"<r xmlns:a="urn:a"><a:x/><a:y/><a:x/></r>"#;
    /// let doc = uppsala::parse(xml).unwrap();
    /// let root = doc.document_element().unwrap();
    /// let xs = doc.child_elements_by_name_ns(root, "urn:a", "x");
    /// assert_eq!(xs.len(), 2);
    /// ```
    pub fn child_elements_by_name_ns(
        &self,
        parent: NodeId,
        namespace_uri: &str,
        local_name: &str,
    ) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut child = self.nodes.get(parent.0).and_then(|n| n.first_child);
        while let Some(cid) = child {
            if let Some(elem) = self.element(cid) {
                if elem.matches_name_ns(namespace_uri, local_name) {
                    result.push(cid);
                }
            }
            child = self.nodes.get(cid.0).and_then(|n| n.next_sibling);
        }
        result
    }

    /// Collect all text content of this node and its descendants (depth-first).
    pub fn text_content_deep(&self, id: NodeId) -> String {
        let mut buf = String::new();
        self.collect_text(id, &mut buf);
        buf
    }

    fn collect_text(&self, id: NodeId, buf: &mut String) {
        match self.node_kind(id) {
            Some(NodeKind::Text(t)) => buf.push_str(t),
            Some(NodeKind::CData(t)) => buf.push_str(t),
            _ => {
                for child in self.children(id) {
                    self.collect_text(child, buf);
                }
            }
        }
    }

    // ─── Tree mutation ───

    /// Append a child node to a parent. Detaches the child from any previous parent.
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) {
        if !self.can_reparent(parent, child) {
            return;
        }
        // Detach from old parent first
        self.detach(child);
        self.append_child_unchecked(parent, child);
    }

    /// Append a freshly-allocated child node to a parent without detaching.
    /// The child must have no parent, no siblings. Used during parsing for speed.
    #[inline]
    pub(crate) fn append_child_unchecked(&mut self, parent: NodeId, child: NodeId) {
        debug_assert!(self.valid_node_id(parent));
        debug_assert!(self.valid_node_id(child));
        debug_assert!(!self.is_ancestor_of(child, parent));
        // Set new parent
        self.nodes[child.0].parent = Some(parent);
        // Link into parent's child list
        let last = self.nodes[parent.0].last_child;
        if let Some(last_id) = last {
            // Append after last child
            self.nodes[last_id.0].next_sibling = Some(child);
            self.nodes[child.0].prev_sibling = Some(last_id);
            self.nodes[parent.0].last_child = Some(child);
        } else {
            // First child
            self.nodes[parent.0].first_child = Some(child);
            self.nodes[parent.0].last_child = Some(child);
        }
    }

    /// Insert a child before a reference node. Both must share the same parent.
    pub fn insert_before(&mut self, parent: NodeId, new_child: NodeId, reference: NodeId) {
        if new_child == reference
            || !self.can_reparent(parent, new_child)
            || self.parent(reference) != Some(parent)
        {
            return;
        }
        self.detach(new_child);
        if let Some(node) = self.nodes.get_mut(new_child.0) {
            node.parent = Some(parent);
        }
        let prev = self.nodes.get(reference.0).and_then(|n| n.prev_sibling);
        // Link new_child before reference
        if let Some(nc) = self.nodes.get_mut(new_child.0) {
            nc.prev_sibling = prev;
            nc.next_sibling = Some(reference);
        }
        if let Some(r) = self.nodes.get_mut(reference.0) {
            r.prev_sibling = Some(new_child);
        }
        if let Some(prev_id) = prev {
            if let Some(p) = self.nodes.get_mut(prev_id.0) {
                p.next_sibling = Some(new_child);
            }
        } else {
            // new_child is now the first child
            if let Some(p) = self.nodes.get_mut(parent.0) {
                p.first_child = Some(new_child);
            }
        }
    }

    /// Insert a child after a reference node.
    pub fn insert_after(&mut self, parent: NodeId, new_child: NodeId, reference: NodeId) {
        if new_child == reference
            || !self.can_reparent(parent, new_child)
            || self.parent(reference) != Some(parent)
        {
            return;
        }
        self.detach(new_child);
        if let Some(node) = self.nodes.get_mut(new_child.0) {
            node.parent = Some(parent);
        }
        let next = self.nodes.get(reference.0).and_then(|n| n.next_sibling);
        if let Some(nc) = self.nodes.get_mut(new_child.0) {
            nc.prev_sibling = Some(reference);
            nc.next_sibling = next;
        }
        if let Some(r) = self.nodes.get_mut(reference.0) {
            r.next_sibling = Some(new_child);
        }
        if let Some(next_id) = next {
            if let Some(n) = self.nodes.get_mut(next_id.0) {
                n.prev_sibling = Some(new_child);
            }
        } else {
            // new_child is now the last child
            if let Some(p) = self.nodes.get_mut(parent.0) {
                p.last_child = Some(new_child);
            }
        }
    }

    /// Remove a child from its parent. The node remains in the arena but is detached.
    pub fn remove_child(&mut self, parent: NodeId, child: NodeId) {
        if self.parent(child) != Some(parent) {
            return;
        }
        self.detach(child);
    }

    /// Replace an old child with a new child under the given parent.
    pub fn replace_child(&mut self, parent: NodeId, new_child: NodeId, old_child: NodeId) {
        if new_child == old_child
            || !self.can_reparent(parent, new_child)
            || self.parent(old_child) != Some(parent)
        {
            return;
        }
        self.detach(new_child);
        let prev = self.nodes.get(old_child.0).and_then(|n| n.prev_sibling);
        let next = self.nodes.get(old_child.0).and_then(|n| n.next_sibling);
        // Set new_child links
        if let Some(nc) = self.nodes.get_mut(new_child.0) {
            nc.parent = Some(parent);
            nc.prev_sibling = prev;
            nc.next_sibling = next;
        }
        // Update neighbors
        if let Some(prev_id) = prev {
            if let Some(p) = self.nodes.get_mut(prev_id.0) {
                p.next_sibling = Some(new_child);
            }
        } else if let Some(p) = self.nodes.get_mut(parent.0) {
            p.first_child = Some(new_child);
        }
        if let Some(next_id) = next {
            if let Some(n) = self.nodes.get_mut(next_id.0) {
                n.prev_sibling = Some(new_child);
            }
        } else if let Some(p) = self.nodes.get_mut(parent.0) {
            p.last_child = Some(new_child);
        }
        // Detach old_child
        if let Some(oc) = self.nodes.get_mut(old_child.0) {
            oc.parent = None;
            oc.prev_sibling = None;
            oc.next_sibling = None;
        }
    }

    /// Detach a node from its parent, removing it from the tree.
    ///
    /// The node remains in the arena and can be re-attached elsewhere with
    /// [`append_child`](Self::append_child), [`insert_before`](Self::insert_before),
    /// or [`insert_after`](Self::insert_after).
    pub fn detach(&mut self, id: NodeId) {
        let (parent_id, prev, next) = match self.nodes.get(id.0) {
            Some(n) => (n.parent, n.prev_sibling, n.next_sibling),
            None => return,
        };
        if let Some(parent_id) = parent_id {
            // Update prev sibling or parent's first_child
            if let Some(prev_id) = prev {
                if let Some(p) = self.nodes.get_mut(prev_id.0) {
                    p.next_sibling = next;
                }
            } else if let Some(p) = self.nodes.get_mut(parent_id.0) {
                p.first_child = next;
            }
            // Update next sibling or parent's last_child
            if let Some(next_id) = next {
                if let Some(n) = self.nodes.get_mut(next_id.0) {
                    n.prev_sibling = prev;
                }
            } else if let Some(p) = self.nodes.get_mut(parent_id.0) {
                p.last_child = prev;
            }
            // Clear the detached node's links
            if let Some(node) = self.nodes.get_mut(id.0) {
                node.parent = None;
                node.prev_sibling = None;
                node.next_sibling = None;
            }
        }
    }

    fn valid_node_id(&self, id: NodeId) -> bool {
        id.0 < self.nodes.len()
    }

    fn can_reparent(&self, parent: NodeId, child: NodeId) -> bool {
        self.valid_node_id(parent)
            && self.valid_node_id(child)
            && parent != child
            && !self.is_ancestor_of(child, parent)
    }

    fn is_ancestor_of(&self, maybe_ancestor: NodeId, node: NodeId) -> bool {
        let mut current = Some(node);
        let mut steps = 0usize;
        while let Some(id) = current {
            if id == maybe_ancestor {
                return true;
            }
            if steps >= self.nodes.len() {
                return true;
            }
            current = self.nodes.get(id.0).and_then(|n| n.parent);
            steps += 1;
        }
        false
    }

    // ─── Navigation helpers ───

    /// Get the first child of a node.
    pub fn first_child(&self, id: NodeId) -> Option<NodeId> {
        self.nodes.get(id.0).and_then(|n| n.first_child)
    }

    /// Get the last child of a node.
    pub fn last_child(&self, id: NodeId) -> Option<NodeId> {
        self.nodes.get(id.0).and_then(|n| n.last_child)
    }

    /// Get the next sibling of a node.
    pub fn next_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.nodes.get(id.0).and_then(|n| n.next_sibling)
    }

    /// Get the previous sibling of a node.
    pub fn previous_sibling(&self, id: NodeId) -> Option<NodeId> {
        self.nodes.get(id.0).and_then(|n| n.prev_sibling)
    }

    /// Return all ancestor node ids from the node up to (but not including) the root.
    pub fn ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut current = self.parent(id);
        while let Some(pid) = current {
            result.push(pid);
            current = self.parent(pid);
        }
        result
    }

    /// Depth-first pre-order traversal of descendants (not including the node itself).
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        self.collect_descendants(id, &mut result);
        result
    }

    fn collect_descendants(&self, id: NodeId, result: &mut Vec<NodeId>) {
        for child in self.children(id) {
            result.push(child);
            self.collect_descendants(child, result);
        }
    }

    // ─── Serialization ───

    /// Serialize the document back to an XML string (compact, no indentation).
    pub fn to_xml(&self) -> String {
        let mut output = String::new();
        // write_document_to cannot fail when writing to String
        self.write_document_to(&mut output, &XmlWriteOptions::default())
            .unwrap();
        output
    }

    /// Serialize the document with formatting options.
    pub fn to_xml_with_options(&self, opts: &XmlWriteOptions) -> String {
        let mut output = String::new();
        self.write_document_to(&mut output, opts).unwrap();
        output
    }

    /// Serialize a single node (and its subtree) to an XML string.
    ///
    /// Useful for extracting XML fragments without the XML declaration or DOCTYPE.
    pub fn node_to_xml(&self, id: NodeId) -> String {
        let mut output = String::new();
        let binds = self.ancestor_ns_bindings(id);
        let scope = NsScope {
            parent: None,
            local: &binds,
        };
        self.write_node_to(
            id,
            &mut output,
            &XmlWriteOptions::default(),
            0,
            false,
            &scope,
        )
        .unwrap();
        output
    }

    /// Serialize a single node (and its subtree) with formatting options.
    pub fn node_to_xml_with_options(&self, id: NodeId, opts: &XmlWriteOptions) -> String {
        let mut output = String::new();
        let binds = self.ancestor_ns_bindings(id);
        let scope = NsScope {
            parent: None,
            local: &binds,
        };
        self.write_node_to(id, &mut output, opts, 0, false, &scope)
            .unwrap();
        output
    }

    /// Write the entire document to any `io::Write` sink (file, socket, `Vec<u8>`, etc.)
    /// without intermediate String allocation.
    pub fn write_to(&self, writer: &mut dyn std::io::Write) -> std::io::Result<()> {
        let opts = XmlWriteOptions::default();
        self.write_to_with_options(writer, &opts)
    }

    /// Write the entire document to an `io::Write` sink with formatting options.
    pub fn write_to_with_options(
        &self,
        writer: &mut dyn std::io::Write,
        opts: &XmlWriteOptions,
    ) -> std::io::Result<()> {
        let mut adapter = IoWriteAdapter { inner: writer };
        self.write_document_to(&mut adapter, opts)
            .map_err(|e| std::io::Error::other(e.to_string()))
    }

    /// Internal: write the full document (declaration + DOCTYPE + nodes) to a `fmt::Write` sink.
    fn write_document_to(&self, out: &mut dyn fmt::Write, opts: &XmlWriteOptions) -> fmt::Result {
        if let Some(decl) = &self.xml_declaration {
            out.write_str("<?xml version=\"")?;
            out.write_str(&crate::writer::safe_xml_version(&decl.version))?;
            out.write_char('"')?;
            if let Some(enc) = &decl.encoding {
                out.write_str(" encoding=\"")?;
                out.write_str(&crate::writer::safe_xml_encoding(enc))?;
                out.write_char('"')?;
            }
            if let Some(sa) = decl.standalone {
                out.write_str(" standalone=\"")?;
                out.write_str(if sa { "yes" } else { "no" })?;
                out.write_char('"')?;
            }
            out.write_str("?>")?;
        }
        if opts.include_doctype {
            if let Some(dt) = &self.doctype {
                out.write_str(dt)?;
            }
        }
        let root_scope = NsScope {
            parent: None,
            local: &[],
        };
        for child in self.children(self.root) {
            self.write_node_to(child, out, opts, 0, opts.indent.is_some(), &root_scope)?;
        }
        Ok(())
    }

    /// Collect the namespace bindings in scope at `id` from its ancestors (not
    /// including `id` itself), outermost-first. Used to seed fragment
    /// serialization so a namespace already declared on an enclosing element is
    /// treated as in-scope and not redundantly re-declared.
    fn ancestor_ns_bindings(&self, id: NodeId) -> Vec<NsDecl<'_>> {
        let mut chain = Vec::new();
        let mut cur = self.parent(id);
        while let Some(p) = cur {
            chain.push(p);
            cur = self.parent(p);
        }
        chain.reverse();
        let mut binds = Vec::new();
        for nid in chain {
            if let Some(NodeKind::Element(e)) = self.node_kind(nid) {
                for (pfx, uri) in &e.namespace_declarations {
                    binds.push((Cow::Borrowed(pfx.as_ref()), Cow::Borrowed(uri.as_ref())));
                }
            }
        }
        binds
    }

    /// Internal: write a single node and its subtree to a `fmt::Write` sink.
    ///
    /// `indent_self` — if true, write indentation before this node (set by parent
    /// when it detects element-only content during pretty-printing).
    fn write_node_to(
        &self,
        id: NodeId,
        out: &mut dyn fmt::Write,
        opts: &XmlWriteOptions,
        depth: usize,
        indent_self: bool,
        scope: &NsScope,
    ) -> fmt::Result {
        match self.node_kind(id) {
            Some(NodeKind::Element(elem)) => {
                if indent_self {
                    write_indent(out, opts, depth)?;
                }
                out.write_char('<')?;
                // Namespace-aware serialization: alongside the element's stored
                // declarations, synthesize any declarations needed so its own
                // QName and its attributes' QNames resolve under the current
                // scope (issue #2). For parsed documents the stored declarations
                // already satisfy every QName, so nothing extra is emitted.
                let (elem_name_override, attr_overrides, child_local) =
                    plan_element_namespaces(elem, scope);
                let raw_pname = match &elem_name_override {
                    Some(name) => Cow::Borrowed(name.as_str()),
                    None => elem.name.prefixed_name(),
                };
                let pname = crate::writer::safe_xml_qname(&raw_pname);
                out.write_str(&pname)?;
                // Track names already emitted for this start tag so sanitized
                // programmatic attributes cannot collide into duplicate XML.
                let mut seen_attrs = Vec::new();
                // Namespace declarations. `child_local` holds every binding this
                // element introduces (stored + synthesized) in order; emit only
                // the *last* binding per prefix so a synthesized override — e.g. an
                // `xmlns=""` undeclaration — wins over a conflicting stored
                // declaration instead of being dropped as a duplicate.
                // Sanitized prefixes can also collide (two distinct invalid
                // prefixes both collapse to `_`), which is disambiguated against
                // names already emitted for this start tag so output re-parses.
                //
                // Precompute the last index per prefix so the "last binding wins"
                // dedup is O(n) rather than O(n^2) in the number of bindings.
                let mut last_idx: HashMap<&str, usize> = HashMap::with_capacity(child_local.len());
                for (i, (prefix, _)) in child_local.iter().enumerate() {
                    last_idx.insert(prefix.as_ref(), i);
                }
                for (i, (prefix, uri)) in child_local.iter().enumerate() {
                    if last_idx.get(prefix.as_ref()) != Some(&i) {
                        continue; // shadowed by a later binding for the same prefix
                    }
                    let (prefix, uri) = (prefix.as_ref(), uri.as_ref());
                    if prefix.is_empty() {
                        // A default-namespace declaration has the fixed name
                        // `xmlns`, which cannot be disambiguated; skip a
                        // duplicate rather than emit a malformed document.
                        if seen_attrs.iter().any(|name| name == "xmlns") {
                            continue;
                        }
                        seen_attrs.push("xmlns".to_string());
                        out.write_str(" xmlns=\"")?;
                    } else {
                        let safe = crate::writer::safe_xml_ncname(prefix).into_owned();
                        let mut candidate = safe.clone();
                        let mut suffix = 1usize;
                        while seen_attrs
                            .iter()
                            .any(|name| name == &format!("xmlns:{}", candidate))
                        {
                            candidate = format!("{}_{}", safe, suffix);
                            suffix += 1;
                        }
                        seen_attrs.push(format!("xmlns:{}", candidate));
                        out.write_str(" xmlns:")?;
                        out.write_str(&candidate)?;
                        out.write_str("=\"")?;
                    }
                    write_escaped_attr(out, uri)?;
                    out.write_char('"')?;
                }
                // Attributes. A namespaced attribute without a usable prefix
                // gets one via `attr_overrides` (see `plan_element_namespaces`).
                for (attr, override_name) in elem.attributes.iter().zip(attr_overrides.iter()) {
                    out.write_char(' ')?;
                    let raw_aname = match override_name {
                        Some(name) => Cow::Borrowed(name.as_str()),
                        None => attr.name.prefixed_name(),
                    };
                    let aname = crate::writer::unique_safe_xml_qname(&raw_aname, &mut seen_attrs);
                    out.write_str(&aname)?;
                    out.write_str("=\"")?;
                    write_escaped_attr(out, &attr.value)?;
                    out.write_char('"')?;
                }
                // Child scope: the inherited scope extended with the bindings this
                // element introduced (stored + synthesized).
                let child_scope = NsScope {
                    parent: Some(scope),
                    local: &child_local,
                };
                let children = self.children(id);
                if children.is_empty() {
                    if opts.expand_empty_elements {
                        out.write_str("></")?;
                        out.write_str(&pname)?;
                        out.write_char('>')?;
                    } else {
                        out.write_str("/>")?;
                    }
                } else {
                    out.write_char('>')?;
                    // Determine if this is "element-only" content for pretty-printing.
                    // If any child is text or CDATA, we treat it as mixed content
                    // and do NOT insert newlines/indent (to preserve whitespace semantics).
                    let element_only = opts.indent.is_some()
                        && children.iter().all(|&cid| {
                            !matches!(
                                self.node_kind(cid),
                                Some(NodeKind::Text(_)) | Some(NodeKind::CData(_))
                            )
                        });
                    if element_only {
                        out.write_char('\n')?;
                    }
                    for child in &children {
                        self.write_node_to(
                            *child,
                            out,
                            opts,
                            depth + 1,
                            element_only,
                            &child_scope,
                        )?;
                    }
                    if element_only {
                        write_indent(out, opts, depth)?;
                    }
                    out.write_str("</")?;
                    out.write_str(&pname)?;
                    out.write_char('>')?;
                }
                // Trailing newline after the document element when pretty-printing
                if indent_self {
                    out.write_char('\n')?;
                }
            }
            Some(NodeKind::Text(text)) => {
                write_escaped_text(out, text)?;
            }
            Some(NodeKind::CData(text)) => {
                // F-15: split content containing `]]>` across adjacent
                // CDATA sections so attacker-crafted DOM nodes cannot
                // smuggle markup through the serializer.
                out.write_str("<![CDATA[")?;
                out.write_str(&crate::writer::split_cdata_content(text))?;
                out.write_str("]]>")?;
            }
            Some(NodeKind::Comment(text)) => {
                if indent_self {
                    write_indent(out, opts, depth)?;
                }
                // F-13: pad consecutive dashes so content cannot break
                // XML 1.0 comment well-formedness and terminate the
                // comment early.
                out.write_str("<!--")?;
                out.write_str(&crate::writer::sanitize_comment_content(text))?;
                out.write_str("-->")?;
                if indent_self {
                    out.write_char('\n')?;
                }
            }
            Some(NodeKind::ProcessingInstruction(pi)) => {
                if indent_self {
                    write_indent(out, opts, depth)?;
                }
                // F-14: rename a reserved `xml` target so the emitted PI
                // cannot collide with an XML declaration, and insert a
                // space between `?` and `>` in data so the PI cannot
                // terminate early.
                out.write_str("<?")?;
                out.write_str(&crate::writer::sanitize_pi_target(&pi.target))?;
                if let Some(data) = &pi.data {
                    out.write_char(' ')?;
                    out.write_str(&crate::writer::sanitize_pi_data(data))?;
                }
                out.write_str("?>")?;
                if indent_self {
                    out.write_char('\n')?;
                }
            }
            Some(NodeKind::Document) => {
                for child in self.children(id) {
                    self.write_node_to(child, out, opts, depth, indent_self, scope)?;
                }
            }
            Some(NodeKind::Attribute(_, _)) => {
                // Virtual attribute nodes are not serialized as children.
            }
            None => {}
        }
        Ok(())
    }
}

/// A `(prefix, namespace_uri)` binding; an empty prefix is the default namespace.
/// `Cow` so stored declarations can be borrowed from the element (no per-element
/// allocation), while synthesized bindings own their strings.
type NsDecl<'a> = (Cow<'a, str>, Cow<'a, str>);

/// In-scope namespace bindings during serialization, modeled as a borrowed
/// linked list of per-element frames so no cloning happens per element. Each
/// frame's `local` holds the declarations introduced by one element. The `xml`
/// prefix is always implicitly bound.
struct NsScope<'a> {
    parent: Option<&'a NsScope<'a>>,
    local: &'a [NsDecl<'a>],
}

impl<'a> NsScope<'a> {
    /// Resolve a prefix to its namespace URI in scope. Returns `Some("")` for an
    /// explicitly undeclared default namespace, `None` if the prefix is unbound.
    fn resolve(&self, prefix: &str) -> Option<&str> {
        if prefix == "xml" {
            return Some(crate::namespace::XML_NAMESPACE);
        }
        let mut cur = Some(self);
        while let Some(s) = cur {
            for (p, u) in s.local.iter().rev() {
                if p.as_ref() == prefix {
                    return Some(u.as_ref());
                }
            }
            cur = s.parent;
        }
        None
    }

    /// Find a non-empty prefix currently bound to `uri`, respecting shadowing
    /// (the innermost binding for each prefix wins). The reserved `xml` and
    /// `xmlns` prefixes are never returned: reusing them for an arbitrary URI
    /// would defeat the "reserved prefixes are never rebound" guarantee and
    /// produce output that re-parses into the wrong namespace. Prefixes that are
    /// not valid XML NCNames are also skipped: reusing an invalid programmatic
    /// prefix would yield a QName like `bad prefix:Foo` that `safe_xml_qname`
    /// collapses to `_` (dropping the local name); the caller allocates a fresh
    /// `nsN` prefix instead.
    fn prefix_for(&self, uri: &str) -> Option<String> {
        // Track seen prefixes by reference (no cloning); the innermost binding
        // for each prefix is its effective one, so a prefix seen earlier shadows
        // any later (outer) binding.
        let mut seen: HashSet<&str> = HashSet::new();
        let mut cur = Some(self);
        while let Some(s) = cur {
            for (p, u) in s.local.iter().rev() {
                if seen.insert(p.as_ref())
                    && !p.is_empty()
                    && p.as_ref() != "xml"
                    && p.as_ref() != "xmlns"
                    && crate::writer::is_valid_xml_ncname(p.as_ref())
                    && u.as_ref() == uri
                {
                    return Some(p.as_ref().to_string());
                }
            }
            cur = s.parent;
        }
        None
    }
}

/// Allocate a fresh `nsN` prefix that is not currently bound in `scope` plus the
/// declarations collected so far for this element.
fn alloc_ns_prefix(scope: &NsScope, child_local: &[NsDecl]) -> String {
    let mut n = 0usize;
    loop {
        let cand = format!("ns{}", n);
        let taken = {
            let cur = NsScope {
                parent: Some(scope),
                local: child_local,
            };
            cur.resolve(&cand).is_some()
        };
        if !taken {
            return cand;
        }
        n += 1;
    }
}

/// Return an existing non-empty prefix bound to `uri`, or allocate a fresh one
/// and record a declaration for it in `child_local`. Used when a prefix is
/// required (namespaced attribute) or when the desired prefix is unusable.
fn prefix_for_or_alloc<'e>(
    scope: &NsScope,
    child_local: &mut Vec<NsDecl<'e>>,
    uri: &'e str,
) -> String {
    let reuse = {
        let cur = NsScope {
            parent: Some(scope),
            local: child_local,
        };
        cur.prefix_for(uri)
    };
    if let Some(p) = reuse {
        return p;
    }
    let p = alloc_ns_prefix(scope, child_local);
    child_local.push((Cow::Owned(p.clone()), Cow::Borrowed(uri)));
    p
}

/// Ensure `desired_prefix` (empty string = default namespace) resolves to `uri`
/// for this element, recording any declaration that must be emitted in
/// `child_local`. Returns `Some(prefix)` when the QName must be rewritten to use
/// a *different* prefix — because `desired_prefix` is already declared on this
/// same start tag bound to another URI and a start tag cannot carry two bindings
/// for one prefix — or `None` when `desired_prefix` works as-is.
fn ensure_binding<'e>(
    scope: &NsScope,
    child_local: &mut Vec<NsDecl<'e>>,
    desired_prefix: &'e str,
    uri: &'e str,
) -> Option<String> {
    let already_bound = {
        let cur = NsScope {
            parent: Some(scope),
            local: child_local,
        };
        cur.resolve(desired_prefix) == Some(uri)
    };
    if already_bound {
        return None; // already correctly bound (here or via an ancestor)
    }
    if !child_local
        .iter()
        .any(|(p, _)| p.as_ref() == desired_prefix)
    {
        // Not declared on this element yet: declare it here, shadowing any
        // ancestor binding. The QName keeps its own prefix.
        child_local.push((Cow::Borrowed(desired_prefix), Cow::Borrowed(uri)));
        return None;
    }
    // Same-element conflict: `desired_prefix` is already bound here to a different
    // URI and cannot be redeclared, so bind `uri` to a different prefix and
    // rewrite the QName to use it (otherwise the QName would silently resolve to
    // the colliding declaration).
    Some(prefix_for_or_alloc(scope, child_local, uri))
}

/// Compute the namespace bindings a serialized element introduces so its own
/// QName and its attributes' QNames resolve correctly under the inherited
/// `scope`. Returns:
/// - `elem_name_override` — a replacement element tag name, set only when the
///   element's own prefix collides with a stored declaration or is the reserved
///   `xml` prefix used for a non-XML namespace,
/// - `attr_overrides` — a per-attribute display-name override, set only for a
///   namespaced attribute that needs a prefix it does not carry (or whose prefix
///   collides / is reserved), and
/// - `child_local` — every binding (stored + synthesized) this element
///   introduces, in order; the serializer emits the last binding per prefix and
///   uses it as the child scope.
///
/// For a parsed document the stored declarations already satisfy every QName, so
/// nothing is synthesized and output is byte-identical to before. The per-element
/// cost is building the small planning vectors (`child_local` borrows the stored
/// declarations rather than cloning them; `attr_overrides`); synthesized bindings
/// and renamed QNames allocate only when actually required.
fn plan_element_namespaces<'e>(
    elem: &'e Element,
    scope: &NsScope,
) -> (Option<String>, Vec<Option<String>>, Vec<NsDecl<'e>>) {
    let xml_ns = crate::namespace::XML_NAMESPACE;
    let xmlns_ns = crate::namespace::XMLNS_NAMESPACE;
    // Borrow the stored declarations, but drop the reserved bindings the parser's
    // `NamespaceResolver::declare` ignores (namespace.rs): the `xmlns` prefix can
    // never be declared, the `xml` prefix may only bind the XML namespace, and no
    // prefix may bind the XMLNS namespace. Emitting them (e.g. `xmlns:xmlns=...`)
    // would produce namespace-not-well-formed output.
    let mut child_local: Vec<NsDecl<'e>> = elem
        .namespace_declarations
        .iter()
        .filter(|(p, u)| {
            p.as_ref() != "xmlns"
                && !(p.as_ref() == "xml" && u.as_ref() != xml_ns)
                && u.as_ref() != xmlns_ns
        })
        .map(|(p, u)| (Cow::Borrowed(p.as_ref()), Cow::Borrowed(u.as_ref())))
        .collect();

    // Element QName.
    let elem_name_override = match (
        elem.name.prefix.as_deref(),
        elem.name.namespace_uri.as_deref(),
    ) {
        (None, None) => {
            // Unprefixed element in no namespace: if a non-empty default
            // namespace is in scope it would otherwise capture this element, so
            // undeclare it with `xmlns=""`.
            let default_ns = {
                let cur = NsScope {
                    parent: Some(scope),
                    local: &child_local,
                };
                cur.resolve("").map(|s| s.to_string())
            };
            if matches!(default_ns.as_deref(), Some(d) if !d.is_empty()) {
                child_local.push((Cow::Borrowed(""), Cow::Borrowed("")));
            }
            None
        }
        // A reserved prefix with no namespace URI must still be stripped: `xml`
        // and `xmlns` are implicitly bound, so `xml:Foo` would re-parse into the
        // XML namespace and `xmlns:Foo` into the XMLNS namespace, silently
        // changing the element's namespace. Serialize the bare local name.
        (Some("xml"), None) | (Some("xmlns"), None) => Some(elem.name.local_name.to_string()),
        (Some(_), None) => None, // prefixed but no URI: leave the name as-is
        // The XML namespace is bound to `xml` and only `xml`, and is never
        // declared. Any other prefix (or none) for that URI is rewritten to
        // `xml`; the `xml` prefix used for any other URI is reassigned, since it
        // is reserved and cannot be rebound.
        (Some("xml"), Some(u)) if u == xml_ns => None,
        (_, Some(u)) if u == xml_ns => Some(format!("xml:{}", elem.name.local_name)),
        // The XMLNS namespace cannot be bound to any prefix: the parser ignores
        // every binding to it, so a synthesized `xmlns:nsN="...2000/xmlns/"`
        // declaration would not be namespace-well-formed and would not re-parse.
        // The namespace is unrepresentable, so drop it and serialize the bare
        // local name (never emitting an `xmlns`/`xmlns:*` name).
        (_, Some(u)) if u == xmlns_ns => Some(elem.name.local_name.to_string()),
        // The reserved `xml`/`xmlns` prefixes bound to any *other* (representable)
        // URI are rebound to a fresh non-reserved prefix; emitting them verbatim
        // would re-parse as the XML namespace / as a declaration.
        (Some("xml"), Some(u)) | (Some("xmlns"), Some(u)) => {
            let pfx = prefix_for_or_alloc(scope, &mut child_local, u);
            Some(format!("{}:{}", pfx, elem.name.local_name))
        }
        (Some(p), Some(u)) => ensure_binding(scope, &mut child_local, p, u)
            .map(|q| format!("{}:{}", q, elem.name.local_name)),
        (None, Some(u)) => ensure_binding(scope, &mut child_local, "", u)
            .map(|q| format!("{}:{}", q, elem.name.local_name)),
    };

    // Attributes. An empty prefix never works for an attribute (attributes are
    // not in the default namespace), so a namespaced-but-prefixless attribute is
    // always given a prefix.
    let mut attr_overrides = Vec::with_capacity(elem.attributes.len());
    for attr in &elem.attributes {
        let override_name = match (
            attr.name.prefix.as_deref(),
            attr.name.namespace_uri.as_deref(),
        ) {
            // A reserved prefix with no namespace URI is stripped: `xml`/`xmlns`
            // are implicitly bound, so `xml:foo` would re-parse into the XML
            // namespace and `xmlns:foo` would be read as a namespace declaration,
            // changing the attribute's effective namespace.
            (Some("xml"), None) | (Some("xmlns"), None) => Some(attr.name.local_name.to_string()),
            (_, None) => None,
            (Some("xml"), Some(u)) if u == xml_ns => None,
            (_, Some(u)) if u == xml_ns => Some(format!("xml:{}", attr.name.local_name)),
            // XMLNS namespace: unrepresentable (see the element-name planning
            // above), so drop it and serialize the bare local name rather than
            // emit a forbidden `xmlns:nsN="...2000/xmlns/"` declaration.
            (_, Some(u)) if u == xmlns_ns => Some(attr.name.local_name.to_string()),
            // Reserved `xml`/`xmlns` prefixes on a representable URI: rebind to a
            // fresh non-reserved prefix so the attribute does not masquerade as a
            // namespace declaration.
            (Some("xml"), Some(u)) | (Some("xmlns"), Some(u)) => {
                let pfx = prefix_for_or_alloc(scope, &mut child_local, u);
                Some(format!("{}:{}", pfx, attr.name.local_name))
            }
            (Some(p), Some(u)) => ensure_binding(scope, &mut child_local, p, u)
                .map(|q| format!("{}:{}", q, attr.name.local_name)),
            (None, Some(u)) => {
                let pfx = prefix_for_or_alloc(scope, &mut child_local, u);
                Some(format!("{}:{}", pfx, attr.name.local_name))
            }
        };
        attr_overrides.push(override_name);
    }

    (elem_name_override, attr_overrides, child_local)
}

impl<'a> Default for Document<'a> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'a> fmt::Display for Document<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.write_document_to(f, &XmlWriteOptions::default())
    }
}

// ─── Serialization options ───

/// Options controlling XML serialization output format.
#[derive(Debug, Clone)]
pub struct XmlWriteOptions {
    /// Indentation string per level (e.g. `"  "`, `"\t"`).
    /// `None` means compact output with no extra whitespace.
    pub indent: Option<String>,
    /// Use `<foo></foo>` instead of `<foo/>` for empty elements.
    /// Required for W3C Canonical XML (C14N).
    pub expand_empty_elements: bool,
    /// Include the raw DOCTYPE declaration when serializing.
    ///
    /// Disabled by default so parsed DTDs are not handed to downstream XML
    /// processors unless the caller deliberately opts into trusted DTD
    /// round-tripping.
    pub include_doctype: bool,
}

impl XmlWriteOptions {
    /// Compact output: no indentation, self-closing empty elements.
    pub fn compact() -> Self {
        XmlWriteOptions {
            indent: None,
            expand_empty_elements: false,
            include_doctype: false,
        }
    }

    /// Pretty-printed output with the given indentation string.
    pub fn pretty(indent: impl Into<String>) -> Self {
        XmlWriteOptions {
            indent: Some(indent.into()),
            expand_empty_elements: false,
            include_doctype: false,
        }
    }

    /// Set whether empty elements use expanded form (`<foo></foo>`).
    pub fn with_expand_empty_elements(mut self, expand: bool) -> Self {
        self.expand_empty_elements = expand;
        self
    }

    /// Set whether the parsed raw DOCTYPE declaration is serialized.
    pub fn with_doctype(mut self, include: bool) -> Self {
        self.include_doctype = include;
        self
    }
}

impl Default for XmlWriteOptions {
    fn default() -> Self {
        Self::compact()
    }
}

// ─── Escaping and helpers ───

/// Write indentation for the given depth.
fn write_indent(out: &mut dyn fmt::Write, opts: &XmlWriteOptions, depth: usize) -> fmt::Result {
    if let Some(ref indent) = opts.indent {
        for _ in 0..depth {
            out.write_str(indent)?;
        }
    }
    Ok(())
}

/// Write text content with XML escaping to a `fmt::Write` sink.
///
/// Per XML 1.0 and C14N rules:
/// - `&` → `&amp;`
/// - `<` → `&lt;`
/// - `>` → `&gt;`
/// - `\r` → `&#xD;` (preserves CR on round-trip; XML parser normalizes CR)
fn write_escaped_text(out: &mut dyn fmt::Write, s: &str) -> fmt::Result {
    for c in s.chars() {
        match c {
            '&' => out.write_str("&amp;")?,
            '<' => out.write_str("&lt;")?,
            '>' => out.write_str("&gt;")?,
            '\r' => out.write_str("&#xD;")?,
            _ => out.write_char(crate::writer::sanitized_xml_char(c))?,
        }
    }
    Ok(())
}

/// Write attribute value with XML escaping to a `fmt::Write` sink.
///
/// Per XML 1.0 and C14N rules:
/// - `&` → `&amp;`
/// - `<` → `&lt;`
/// - `>` → `&gt;`
/// - `"` → `&quot;`
/// - `\t` → `&#x9;` (preserves tab; XML parser normalizes to space)
/// - `\n` → `&#xA;` (preserves newline; XML parser normalizes to space)
/// - `\r` → `&#xD;` (preserves CR; XML parser normalizes CR)
fn write_escaped_attr(out: &mut dyn fmt::Write, s: &str) -> fmt::Result {
    for c in s.chars() {
        match c {
            '&' => out.write_str("&amp;")?,
            '<' => out.write_str("&lt;")?,
            '>' => out.write_str("&gt;")?,
            '"' => out.write_str("&quot;")?,
            '\t' => out.write_str("&#x9;")?,
            '\n' => out.write_str("&#xA;")?,
            '\r' => out.write_str("&#xD;")?,
            _ => out.write_char(crate::writer::sanitized_xml_char(c))?,
        }
    }
    Ok(())
}

/// Adapter that allows writing to an `io::Write` via the `fmt::Write` trait.
struct IoWriteAdapter<'w> {
    inner: &'w mut dyn std::io::Write,
}

impl<'w> fmt::Write for IoWriteAdapter<'w> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.inner.write_all(s.as_bytes()).map_err(|_| fmt::Error)
    }
}
