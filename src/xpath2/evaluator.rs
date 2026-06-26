//! XPath 2.0 evaluator.

use std::collections::HashMap;

use crate::dom::{Document, NodeId, NodeKind};
use crate::error::{XmlError, XmlResult};

use super::ast::{
    Axis, BinaryOp, Expr, ForBinding, Literal, NodeTest, PathExpr, PathStep, QName, Quantifier,
    UnaryOp,
};
use super::parser::{parse_expression, DEFAULT_MAX_XPATH2_DEPTH};
use super::value::{XPath2AtomicValue, XPath2Item, XPath2Value};

/// Resolver hooks for implementation-defined XPath 2.0 resource functions.
///
/// The default resolver returns `None` for every resource, so functions such as
/// `doc()` and `collection()` never touch the filesystem or network unless the
/// caller explicitly supplies a resolver.
pub trait XPath2Resolver {
    /// Resolve `doc($uri)`.
    fn resolve_doc(&self, _uri: &str) -> XmlResult<Option<XPath2Value>> {
        Ok(None)
    }

    /// Resolve `collection($uri)`.
    fn resolve_collection(&self, _uri: Option<&str>) -> XmlResult<Option<XPath2Value>> {
        Ok(None)
    }
}

/// Resolver that disables external resource access.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopXPath2Resolver;

impl XPath2Resolver for NoopXPath2Resolver {}

/// XPath 2.0 evaluator options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XPath2Options {
    /// Whether XPath 1.0 compatibility mode is enabled.
    pub xpath1_compatibility: bool,
    /// Maximum nested expression depth.
    pub max_depth: u32,
    /// Maximum items an eager sequence constructor such as `to` may allocate.
    pub max_sequence_items: usize,
}

impl Default for XPath2Options {
    fn default() -> Self {
        Self {
            xpath1_compatibility: false,
            max_depth: DEFAULT_MAX_XPATH2_DEPTH,
            max_sequence_items: DEFAULT_MAX_XPATH2_SEQUENCE_ITEMS,
        }
    }
}

/// Default maximum items for eager XPath 2.0 sequence construction.
pub const DEFAULT_MAX_XPATH2_SEQUENCE_ITEMS: usize = 100_000;

/// XPath 2.0 evaluator.
#[derive(Debug, Clone)]
pub struct XPath2Evaluator<R = NoopXPath2Resolver> {
    options: XPath2Options,
    resolver: R,
    namespaces: HashMap<String, String>,
}

impl XPath2Evaluator<NoopXPath2Resolver> {
    /// Create an XPath 2.0 evaluator with default options and no resolver.
    pub fn new() -> Self {
        Self {
            options: XPath2Options::default(),
            resolver: NoopXPath2Resolver,
            namespaces: HashMap::new(),
        }
    }
}

impl Default for XPath2Evaluator<NoopXPath2Resolver> {
    fn default() -> Self {
        Self::new()
    }
}

impl<R> XPath2Evaluator<R>
where
    R: XPath2Resolver,
{
    /// Replace the resolver used for `doc()` and `collection()`.
    pub fn with_resolver<NR>(self, resolver: NR) -> XPath2Evaluator<NR>
    where
        NR: XPath2Resolver,
    {
        XPath2Evaluator {
            options: self.options,
            resolver,
            namespaces: self.namespaces,
        }
    }

    /// Register a namespace prefix for use in XPath 2.0 expressions.
    ///
    /// Prefixed name tests fail closed when their prefix is not registered.
    /// Registered prefixes are matched against node namespace URIs, not the
    /// lexical prefixes used in the source document.
    pub fn add_namespace(&mut self, prefix: impl Into<String>, uri: impl Into<String>) {
        self.namespaces.insert(prefix.into(), uri.into());
    }

    /// Register a namespace prefix and return `self` for builder-style setup.
    ///
    /// This is equivalent to calling [`Self::add_namespace`] before
    /// evaluation.
    pub fn with_namespace(mut self, prefix: impl Into<String>, uri: impl Into<String>) -> Self {
        self.add_namespace(prefix, uri);
        self
    }

    /// Enable or disable XPath 1.0 compatibility mode.
    pub fn with_xpath1_compatibility(mut self, enabled: bool) -> Self {
        self.options.xpath1_compatibility = enabled;
        self
    }

    /// Set the parser nesting limit.
    pub fn with_max_depth(mut self, max_depth: u32) -> Self {
        self.options.max_depth = max_depth;
        self
    }

    /// Set the maximum number of items eager sequence constructors may allocate.
    pub fn with_max_sequence_items(mut self, max_sequence_items: usize) -> Self {
        self.options.max_sequence_items = max_sequence_items;
        self
    }

    /// Return evaluator options.
    pub fn options(&self) -> &XPath2Options {
        &self.options
    }

    /// Evaluate an XPath 2.0 expression from a context node.
    pub fn evaluate(
        &self,
        doc: &Document<'_>,
        context_node: NodeId,
        expression: &str,
    ) -> XmlResult<XPath2Value> {
        let expr = parse_expression(expression, self.options.max_depth)?;
        let mut ctx = DynamicContext::new(
            doc,
            &self.resolver,
            &self.options,
            &self.namespaces,
            XPath2Item::Node(context_node),
        );
        evaluate_expr(&expr, &mut ctx)
    }

    /// Evaluate an XPath expression and return only node items.
    pub fn select_nodes(
        &self,
        doc: &Document<'_>,
        context_node: NodeId,
        expression: &str,
    ) -> XmlResult<Vec<NodeId>> {
        let value = self.evaluate(doc, context_node, expression)?;
        value
            .into_items()
            .into_iter()
            .map(|item| match item {
                XPath2Item::Node(node) => Ok(node),
                XPath2Item::Atomic(_) => Err(XmlError::xpath(
                    "XPath 2.0 expression returned an atomic value where nodes were expected",
                )),
            })
            .collect()
    }
}

struct DynamicContext<'doc, 'input, R>
where
    R: XPath2Resolver,
{
    doc: &'doc Document<'input>,
    resolver: &'doc R,
    options: &'doc XPath2Options,
    namespaces: &'doc HashMap<String, String>,
    context_item: XPath2Item,
    position: usize,
    size: usize,
    variables: Vec<(String, XPath2Value)>,
}

impl<'doc, 'input, R> DynamicContext<'doc, 'input, R>
where
    R: XPath2Resolver,
{
    fn new(
        doc: &'doc Document<'input>,
        resolver: &'doc R,
        options: &'doc XPath2Options,
        namespaces: &'doc HashMap<String, String>,
        context_item: XPath2Item,
    ) -> Self {
        Self {
            doc,
            resolver,
            options,
            namespaces,
            context_item,
            position: 1,
            size: 1,
            variables: Vec::new(),
        }
    }

    fn fork_with_context(&self, context_item: XPath2Item, position: usize, size: usize) -> Self {
        Self {
            doc: self.doc,
            resolver: self.resolver,
            options: self.options,
            namespaces: self.namespaces,
            context_item,
            position,
            size,
            variables: self.variables.clone(),
        }
    }

    fn push_variable(&mut self, name: &QName<'_>, value: XPath2Value) {
        self.variables.push((name.lexical_key(), value));
    }

    fn pop_variable(&mut self) {
        self.variables.pop();
    }

    fn variable(&self, name: &QName<'_>) -> Option<XPath2Value> {
        let key = name.lexical_key();
        self.variables
            .iter()
            .rev()
            .find_map(|(candidate, value)| (candidate == &key).then(|| value.clone()))
    }
}

fn evaluate_expr<R>(expr: &Expr<'_>, ctx: &mut DynamicContext<'_, '_, R>) -> XmlResult<XPath2Value>
where
    R: XPath2Resolver,
{
    match expr {
        Expr::EmptySequence => Ok(XPath2Value::empty()),
        Expr::Sequence(items) => {
            let mut value = XPath2Value::empty();
            for item in items {
                value.extend(evaluate_expr(item, ctx)?);
            }
            Ok(value)
        }
        Expr::Literal(literal) => evaluate_literal(literal),
        Expr::VarRef(name) => ctx
            .variable(name)
            .ok_or_else(|| XmlError::xpath(format!("unbound XPath 2.0 variable ${}", name))),
        Expr::ContextItem => Ok(XPath2Value::new(vec![ctx.context_item.clone()])),
        Expr::FunctionCall { name, args } => evaluate_function(name, args, ctx),
        Expr::Unary { op, expr } => evaluate_unary(*op, expr, ctx),
        Expr::Binary { op, left, right } => evaluate_binary(*op, left, right, ctx),
        Expr::If {
            test,
            then_branch,
            else_branch,
        } => {
            if evaluate_expr(test, ctx)?.effective_boolean_value(ctx.doc)? {
                evaluate_expr(then_branch, ctx)
            } else {
                evaluate_expr(else_branch, ctx)
            }
        }
        Expr::For { bindings, body } => evaluate_for(bindings, body, ctx),
        Expr::Quantified {
            quantifier,
            bindings,
            satisfies,
        } => evaluate_quantified(*quantifier, bindings, satisfies, ctx),
        Expr::Path(path) => evaluate_path(path, ctx),
    }
}

fn evaluate_literal(literal: &Literal<'_>) -> XmlResult<XPath2Value> {
    let atomic = match literal {
        Literal::String(value) => XPath2AtomicValue::String(value.to_string()),
        Literal::Integer(value) => XPath2AtomicValue::Integer((*value).to_string()),
        Literal::Decimal(value) => XPath2AtomicValue::decimal(*value),
        Literal::Double(value) => XPath2AtomicValue::double(
            value
                .parse::<f64>()
                .map_err(|_| XmlError::xpath(format!("invalid double literal '{}'", value)))?,
        ),
    };
    Ok(XPath2Value::atomic(atomic))
}

fn evaluate_for<R>(
    bindings: &[ForBinding<'_>],
    body: &Expr<'_>,
    ctx: &mut DynamicContext<'_, '_, R>,
) -> XmlResult<XPath2Value>
where
    R: XPath2Resolver,
{
    let mut result = XPath2Value::empty();
    evaluate_for_binding(bindings, body, 0, ctx, &mut result)?;
    Ok(result)
}

fn evaluate_for_binding<R>(
    bindings: &[ForBinding<'_>],
    body: &Expr<'_>,
    index: usize,
    ctx: &mut DynamicContext<'_, '_, R>,
    result: &mut XPath2Value,
) -> XmlResult<()>
where
    R: XPath2Resolver,
{
    if index == bindings.len() {
        result.extend(evaluate_expr(body, ctx)?);
        return Ok(());
    }

    let binding = &bindings[index];
    let sequence = evaluate_expr(&binding.in_expr, ctx)?;
    for item in sequence.into_items() {
        ctx.push_variable(&binding.name, XPath2Value::new(vec![item]));
        evaluate_for_binding(bindings, body, index + 1, ctx, result)?;
        ctx.pop_variable();
    }
    Ok(())
}

fn evaluate_quantified<R>(
    quantifier: Quantifier,
    bindings: &[ForBinding<'_>],
    satisfies: &Expr<'_>,
    ctx: &mut DynamicContext<'_, '_, R>,
) -> XmlResult<XPath2Value>
where
    R: XPath2Resolver,
{
    let satisfied = evaluate_quantified_binding(bindings, satisfies, 0, quantifier, ctx)?;
    Ok(XPath2Value::atomic(XPath2AtomicValue::Boolean(satisfied)))
}

fn evaluate_quantified_binding<R>(
    bindings: &[ForBinding<'_>],
    satisfies: &Expr<'_>,
    index: usize,
    quantifier: Quantifier,
    ctx: &mut DynamicContext<'_, '_, R>,
) -> XmlResult<bool>
where
    R: XPath2Resolver,
{
    if index == bindings.len() {
        return evaluate_expr(satisfies, ctx)?.effective_boolean_value(ctx.doc);
    }

    let binding = &bindings[index];
    let sequence = evaluate_expr(&binding.in_expr, ctx)?;

    match quantifier {
        Quantifier::Some => {
            for item in sequence.into_items() {
                ctx.push_variable(&binding.name, XPath2Value::new(vec![item]));
                let matched =
                    evaluate_quantified_binding(bindings, satisfies, index + 1, quantifier, ctx)?;
                ctx.pop_variable();
                if matched {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Quantifier::Every => {
            for item in sequence.into_items() {
                ctx.push_variable(&binding.name, XPath2Value::new(vec![item]));
                let matched =
                    evaluate_quantified_binding(bindings, satisfies, index + 1, quantifier, ctx)?;
                ctx.pop_variable();
                if !matched {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }
}

fn evaluate_unary<R>(
    op: UnaryOp,
    expr: &Expr<'_>,
    ctx: &mut DynamicContext<'_, '_, R>,
) -> XmlResult<XPath2Value>
where
    R: XPath2Resolver,
{
    let value = evaluate_expr(expr, ctx)?;
    let Some(atomic) = single_atomic_or_empty(&value, ctx.doc)? else {
        return Ok(XPath2Value::empty());
    };

    match op {
        UnaryOp::Plus => Ok(XPath2Value::atomic(atomic)),
        UnaryOp::Minus => match atomic {
            XPath2AtomicValue::Integer(value) => {
                let value = value.parse::<i128>().map_err(|_| {
                    XmlError::xpath(format!("cannot apply unary minus to '{}'", value))
                })?;
                Ok(XPath2Value::atomic(XPath2AtomicValue::integer(-value)))
            }
            other => Ok(XPath2Value::atomic(XPath2AtomicValue::double(
                -other.as_f64()?,
            ))),
        },
    }
}

fn evaluate_binary<R>(
    op: BinaryOp,
    left: &Expr<'_>,
    right: &Expr<'_>,
    ctx: &mut DynamicContext<'_, '_, R>,
) -> XmlResult<XPath2Value>
where
    R: XPath2Resolver,
{
    match op {
        BinaryOp::Or => {
            if evaluate_expr(left, ctx)?.effective_boolean_value(ctx.doc)? {
                return Ok(XPath2Value::atomic(XPath2AtomicValue::Boolean(true)));
            }
            Ok(XPath2Value::atomic(XPath2AtomicValue::Boolean(
                evaluate_expr(right, ctx)?.effective_boolean_value(ctx.doc)?,
            )))
        }
        BinaryOp::And => {
            if !evaluate_expr(left, ctx)?.effective_boolean_value(ctx.doc)? {
                return Ok(XPath2Value::atomic(XPath2AtomicValue::Boolean(false)));
            }
            Ok(XPath2Value::atomic(XPath2AtomicValue::Boolean(
                evaluate_expr(right, ctx)?.effective_boolean_value(ctx.doc)?,
            )))
        }
        BinaryOp::GeneralEq
        | BinaryOp::GeneralNe
        | BinaryOp::GeneralLt
        | BinaryOp::GeneralLe
        | BinaryOp::GeneralGt
        | BinaryOp::GeneralGe => {
            let left = evaluate_expr(left, ctx)?;
            let right = evaluate_expr(right, ctx)?;
            Ok(XPath2Value::atomic(XPath2AtomicValue::Boolean(
                general_compare(op, &left, &right, ctx.doc)?,
            )))
        }
        BinaryOp::ValueEq
        | BinaryOp::ValueNe
        | BinaryOp::ValueLt
        | BinaryOp::ValueLe
        | BinaryOp::ValueGt
        | BinaryOp::ValueGe => {
            let left = evaluate_expr(left, ctx)?;
            let right = evaluate_expr(right, ctx)?;
            value_compare(op, &left, &right, ctx.doc)
        }
        BinaryOp::NodeIs | BinaryOp::NodeBefore | BinaryOp::NodeAfter => {
            let left = evaluate_expr(left, ctx)?;
            let right = evaluate_expr(right, ctx)?;
            node_compare(op, &left, &right)
        }
        BinaryOp::RangeTo => {
            let start = require_single_atomic(&evaluate_expr(left, ctx)?, ctx.doc)?.as_i128()?;
            let end = require_single_atomic(&evaluate_expr(right, ctx)?, ctx.doc)?.as_i128()?;
            let mut value = XPath2Value::empty();
            if start <= end {
                let len = end
                    .checked_sub(start)
                    .and_then(|delta| delta.checked_add(1))
                    .ok_or_else(|| XmlError::xpath("XPath 2.0 range length overflow"))?;
                if len > ctx.options.max_sequence_items as i128 {
                    return Err(XmlError::xpath(format!(
                        "XPath 2.0 range creates {} items, exceeding maximum of {}",
                        len, ctx.options.max_sequence_items
                    )));
                }
                for i in start..=end {
                    value.push(XPath2Item::Atomic(XPath2AtomicValue::integer(i)));
                }
            }
            Ok(value)
        }
        BinaryOp::Add
        | BinaryOp::Subtract
        | BinaryOp::Multiply
        | BinaryOp::Div
        | BinaryOp::Idiv
        | BinaryOp::Mod => {
            let left = evaluate_expr(left, ctx)?;
            let right = evaluate_expr(right, ctx)?;
            arithmetic(op, &left, &right, ctx.doc)
        }
        BinaryOp::Union | BinaryOp::Intersect | BinaryOp::Except => {
            let left = evaluate_expr(left, ctx)?;
            let right = evaluate_expr(right, ctx)?;
            node_set_operator(op, left, right)
        }
    }
}

fn evaluate_path<R>(
    path: &PathExpr<'_>,
    ctx: &mut DynamicContext<'_, '_, R>,
) -> XmlResult<XPath2Value>
where
    R: XPath2Resolver,
{
    let mut nodes = if path.absolute {
        vec![ctx.doc.root()]
    } else if let Some(start) = &path.start {
        nodes_from_value(evaluate_expr(start, ctx)?)?
    } else {
        match &ctx.context_item {
            XPath2Item::Node(node) => vec![*node],
            XPath2Item::Atomic(_) => {
                return Err(XmlError::xpath(
                    "relative path requires a node context item",
                ))
            }
        }
    };

    if path.descendant_start && path.steps.is_empty() {
        nodes = descendant_or_self_nodes(ctx.doc, &nodes);
    }

    for step in &path.steps {
        nodes = apply_step(ctx.doc, ctx, &nodes, step)?;
    }

    Ok(XPath2Value::new(
        nodes.into_iter().map(XPath2Item::Node).collect(),
    ))
}

fn apply_step<R>(
    doc: &Document<'_>,
    ctx: &DynamicContext<'_, '_, R>,
    context_nodes: &[NodeId],
    step: &PathStep<'_>,
) -> XmlResult<Vec<NodeId>>
where
    R: XPath2Resolver,
{
    let mut selected = Vec::new();
    for node in context_nodes {
        let mut candidates: Vec<NodeId> = axis_nodes(doc, *node, step.axis)
            .into_iter()
            .filter(|candidate| {
                node_matches(doc, *candidate, step.axis, &step.test, ctx.namespaces)
            })
            .collect();

        for predicate in &step.predicates {
            candidates = apply_predicate(doc, ctx, &candidates, predicate)?;
        }

        selected.extend(candidates);
    }

    Ok(dedup_document_order(selected))
}

fn axis_nodes(doc: &Document<'_>, node: NodeId, axis: Axis) -> Vec<NodeId> {
    match axis {
        Axis::Child => doc.children(node),
        Axis::Descendant => doc.descendants(node),
        Axis::Attribute => doc.get_attribute_nodes(node).to_vec(),
        Axis::Self_ => vec![node],
        Axis::DescendantOrSelf => {
            let mut result = vec![node];
            result.extend(doc.descendants(node));
            result
        }
        Axis::Parent => doc.parent(node).into_iter().collect(),
        Axis::Ancestor => doc.ancestors(node),
        Axis::AncestorOrSelf => {
            let mut result = vec![node];
            result.extend(doc.ancestors(node));
            result
        }
        Axis::FollowingSibling => {
            let mut result = Vec::new();
            let mut current = doc.next_sibling(node);
            while let Some(sibling) = current {
                result.push(sibling);
                current = doc.next_sibling(sibling);
            }
            result
        }
        Axis::Following => collect_following(doc, node),
        Axis::Namespace => Vec::new(),
        Axis::PrecedingSibling => {
            let mut result = Vec::new();
            let mut current = doc.previous_sibling(node);
            while let Some(sibling) = current {
                result.push(sibling);
                current = doc.previous_sibling(sibling);
            }
            result
        }
        Axis::Preceding => collect_preceding(doc, node),
    }
}

fn descendants(doc: &Document<'_>, node: NodeId, include_self: bool) -> Vec<NodeId> {
    let mut result = Vec::new();
    if include_self {
        result.push(node);
    }
    for child in doc.children(node) {
        result.push(child);
        result.extend(descendants(doc, child, false));
    }
    result
}

fn collect_following(doc: &Document<'_>, node: NodeId) -> Vec<NodeId> {
    let mut result = Vec::new();
    let mut current = node;
    loop {
        if let Some(next) = doc.next_sibling(current) {
            result.push(next);
            result.extend(doc.descendants(next));
            current = next;
            continue;
        }
        if let Some(parent) = doc.parent(current) {
            current = parent;
        } else {
            break;
        }
    }
    result
}

fn collect_preceding(doc: &Document<'_>, node: NodeId) -> Vec<NodeId> {
    let mut result = Vec::new();
    let mut current = node;
    loop {
        if let Some(prev) = doc.previous_sibling(current) {
            let descendants = doc.descendants(prev);
            for descendant in descendants.into_iter().rev() {
                result.push(descendant);
            }
            result.push(prev);
            current = prev;
            continue;
        }
        if let Some(parent) = doc.parent(current) {
            if doc.parent(parent).is_some() {
                current = parent;
            } else {
                break;
            }
        } else {
            break;
        }
    }
    result
}

fn descendant_or_self_nodes(doc: &Document<'_>, nodes: &[NodeId]) -> Vec<NodeId> {
    dedup_document_order(
        nodes
            .iter()
            .flat_map(|node| descendants(doc, *node, true))
            .collect(),
    )
}

fn node_matches(
    doc: &Document<'_>,
    node: NodeId,
    axis: Axis,
    test: &NodeTest<'_>,
    namespaces: &HashMap<String, String>,
) -> bool {
    match test {
        NodeTest::Any => match axis {
            Axis::Attribute => matches!(doc.node_kind(node), Some(NodeKind::Attribute(_, _))),
            _ => matches!(doc.node_kind(node), Some(NodeKind::Element(_))),
        },
        NodeTest::Name(name) => match doc.node_kind(node) {
            Some(NodeKind::Element(element)) => expanded_name_matches(
                name,
                element.name.local_name.as_ref(),
                element.name.namespace_uri.as_deref(),
                namespaces,
            ),
            Some(NodeKind::Attribute(attr_name, _)) => expanded_name_matches(
                name,
                attr_name.local_name.as_ref(),
                attr_name.namespace_uri.as_deref(),
                namespaces,
            ),
            _ => false,
        },
        NodeTest::PrefixWildcard(prefix) => match doc.node_kind(node) {
            Some(NodeKind::Element(element)) => {
                namespace_matches(prefix, element.name.namespace_uri.as_deref(), namespaces)
            }
            Some(NodeKind::Attribute(attr_name, _)) => {
                namespace_matches(prefix, attr_name.namespace_uri.as_deref(), namespaces)
            }
            _ => false,
        },
        NodeTest::LocalNameWildcard(local) => match doc.node_kind(node) {
            Some(NodeKind::Element(element)) => element.name.local_name.as_ref() == *local,
            Some(NodeKind::Attribute(attr_name, _)) => attr_name.local_name.as_ref() == *local,
            _ => false,
        },
        NodeTest::Text => matches!(
            doc.node_kind(node),
            Some(NodeKind::Text(_) | NodeKind::CData(_))
        ),
        NodeTest::Node => doc.node_kind(node).is_some(),
        NodeTest::Comment => matches!(doc.node_kind(node), Some(NodeKind::Comment(_))),
        NodeTest::ProcessingInstruction(target) => match doc.node_kind(node) {
            Some(NodeKind::ProcessingInstruction(pi)) => target
                .as_ref()
                .map(|expected| pi.target.as_ref() == expected.as_ref())
                .unwrap_or(true),
            _ => false,
        },
    }
}

fn expanded_name_matches(
    name: &QName<'_>,
    actual_local: &str,
    actual_namespace: Option<&str>,
    namespaces: &HashMap<String, String>,
) -> bool {
    // XPath expressions bind prefixes in the static context. Comparing URI
    // values prevents a document from rebinding the same lexical prefix to a
    // different namespace and still matching a prefixed name test.
    if actual_local != name.local {
        return false;
    }

    match name.prefix {
        Some(prefix) => namespace_matches(prefix, actual_namespace, namespaces),
        None => actual_namespace.is_none(),
    }
}

fn namespace_matches(
    prefix: &str,
    actual_namespace: Option<&str>,
    namespaces: &HashMap<String, String>,
) -> bool {
    // Unregistered prefixes fail closed. Callers must opt into every
    // namespace URI an expression is allowed to match.
    namespaces
        .get(prefix)
        .is_some_and(|expected| actual_namespace == Some(expected.as_str()))
}

fn apply_predicate<R>(
    doc: &Document<'_>,
    ctx: &DynamicContext<'_, '_, R>,
    nodes: &[NodeId],
    predicate: &Expr<'_>,
) -> XmlResult<Vec<NodeId>>
where
    R: XPath2Resolver,
{
    let mut result = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        let mut predicate_ctx =
            ctx.fork_with_context(XPath2Item::Node(*node), index + 1, nodes.len());
        let value = evaluate_expr(predicate, &mut predicate_ctx)?;
        if predicate_matches(&value, doc, index + 1)? {
            result.push(*node);
        }
    }
    Ok(result)
}

fn predicate_matches(value: &XPath2Value, doc: &Document<'_>, position: usize) -> XmlResult<bool> {
    if let Some(number) = predicate_numeric_value(value, doc)? {
        return Ok(number == position as f64);
    }
    value.effective_boolean_value(doc)
}

fn predicate_numeric_value(value: &XPath2Value, doc: &Document<'_>) -> XmlResult<Option<f64>> {
    if value.len() != 1 {
        return Ok(None);
    }
    let atomic = value.items()[0].atomized(doc);
    if atomic.is_numeric() {
        Ok(Some(atomic.as_f64()?))
    } else {
        Ok(None)
    }
}

fn evaluate_function<R>(
    name: &QName<'_>,
    args: &[Expr<'_>],
    ctx: &mut DynamicContext<'_, '_, R>,
) -> XmlResult<XPath2Value>
where
    R: XPath2Resolver,
{
    if !matches!(name.prefix, None | Some("fn")) {
        return Err(XmlError::xpath(format!(
            "unsupported XPath 2.0 function namespace prefix '{}'",
            name.prefix.unwrap_or_default()
        )));
    }

    match name.local {
        "true" => {
            expect_arity(name.local, args, 0)?;
            Ok(XPath2Value::atomic(XPath2AtomicValue::Boolean(true)))
        }
        "false" => {
            expect_arity(name.local, args, 0)?;
            Ok(XPath2Value::atomic(XPath2AtomicValue::Boolean(false)))
        }
        "position" => {
            expect_arity(name.local, args, 0)?;
            Ok(XPath2Value::atomic(XPath2AtomicValue::integer(
                ctx.position as i128,
            )))
        }
        "last" => {
            expect_arity(name.local, args, 0)?;
            Ok(XPath2Value::atomic(XPath2AtomicValue::integer(
                ctx.size as i128,
            )))
        }
        "string" => {
            expect_arity_range(name.local, args, 0, 1)?;
            let value = if args.is_empty() {
                XPath2Value::new(vec![ctx.context_item.clone()])
            } else {
                evaluate_expr(&args[0], ctx)?
            };
            Ok(XPath2Value::atomic(XPath2AtomicValue::String(
                value.to_string_value(ctx.doc),
            )))
        }
        "boolean" => {
            expect_arity(name.local, args, 1)?;
            Ok(XPath2Value::atomic(XPath2AtomicValue::Boolean(
                evaluate_expr(&args[0], ctx)?.effective_boolean_value(ctx.doc)?,
            )))
        }
        "not" => {
            expect_arity(name.local, args, 1)?;
            Ok(XPath2Value::atomic(XPath2AtomicValue::Boolean(
                !evaluate_expr(&args[0], ctx)?.effective_boolean_value(ctx.doc)?,
            )))
        }
        "empty" => {
            expect_arity(name.local, args, 1)?;
            Ok(XPath2Value::atomic(XPath2AtomicValue::Boolean(
                evaluate_expr(&args[0], ctx)?.is_empty(),
            )))
        }
        "exists" => {
            expect_arity(name.local, args, 1)?;
            Ok(XPath2Value::atomic(XPath2AtomicValue::Boolean(
                !evaluate_expr(&args[0], ctx)?.is_empty(),
            )))
        }
        "count" => {
            expect_arity(name.local, args, 1)?;
            Ok(XPath2Value::atomic(XPath2AtomicValue::integer(
                evaluate_expr(&args[0], ctx)?.len() as i128,
            )))
        }
        "number" => {
            expect_arity_range(name.local, args, 0, 1)?;
            let value = if args.is_empty() {
                XPath2Value::new(vec![ctx.context_item.clone()])
            } else {
                evaluate_expr(&args[0], ctx)?
            };
            let Some(atomic) = single_atomic_or_empty(&value, ctx.doc)? else {
                return Ok(XPath2Value::atomic(XPath2AtomicValue::Double(f64::NAN)));
            };
            Ok(XPath2Value::atomic(XPath2AtomicValue::Double(
                atomic.as_f64().unwrap_or(f64::NAN),
            )))
        }
        "concat" => {
            if args.len() < 2 {
                return Err(XmlError::xpath("concat() expects at least two arguments"));
            }
            let mut out = String::new();
            for arg in args {
                out.push_str(&evaluate_expr(arg, ctx)?.to_string_value(ctx.doc));
            }
            Ok(XPath2Value::atomic(XPath2AtomicValue::String(out)))
        }
        "doc" => {
            expect_arity(name.local, args, 1)?;
            let uri = evaluate_expr(&args[0], ctx)?.to_string_value(ctx.doc);
            ctx.resolver.resolve_doc(&uri)?.ok_or_else(|| {
                XmlError::xpath(format!(
                    "doc('{}') is unavailable from the configured resolver",
                    uri
                ))
            })
        }
        "doc-available" => {
            expect_arity(name.local, args, 1)?;
            let uri = evaluate_expr(&args[0], ctx)?.to_string_value(ctx.doc);
            Ok(XPath2Value::atomic(XPath2AtomicValue::Boolean(
                ctx.resolver.resolve_doc(&uri)?.is_some(),
            )))
        }
        "collection" => {
            expect_arity_range(name.local, args, 0, 1)?;
            let uri = if args.is_empty() {
                None
            } else {
                Some(evaluate_expr(&args[0], ctx)?.to_string_value(ctx.doc))
            };
            ctx.resolver
                .resolve_collection(uri.as_deref())?
                .ok_or_else(|| {
                    XmlError::xpath("collection() is unavailable from the configured resolver")
                })
        }
        other => Err(XmlError::xpath(format!(
            "unsupported XPath 2.0 function '{}'",
            other
        ))),
    }
}

fn expect_arity(name: &str, args: &[Expr<'_>], expected: usize) -> XmlResult<()> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(XmlError::xpath(format!(
            "{}() expects {} argument(s), got {}",
            name,
            expected,
            args.len()
        )))
    }
}

fn expect_arity_range(name: &str, args: &[Expr<'_>], min: usize, max: usize) -> XmlResult<()> {
    if (min..=max).contains(&args.len()) {
        Ok(())
    } else {
        Err(XmlError::xpath(format!(
            "{}() expects between {} and {} argument(s), got {}",
            name,
            min,
            max,
            args.len()
        )))
    }
}

fn arithmetic(
    op: BinaryOp,
    left: &XPath2Value,
    right: &XPath2Value,
    doc: &Document<'_>,
) -> XmlResult<XPath2Value> {
    let Some(left) = single_atomic_or_empty(left, doc)? else {
        return Ok(XPath2Value::empty());
    };
    let Some(right) = single_atomic_or_empty(right, doc)? else {
        return Ok(XPath2Value::empty());
    };

    match op {
        BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Mod
            if matches!(left, XPath2AtomicValue::Integer(_))
                && matches!(right, XPath2AtomicValue::Integer(_)) =>
        {
            let left = left.as_i128()?;
            let right = right.as_i128()?;
            let value = match op {
                BinaryOp::Add => left + right,
                BinaryOp::Subtract => left - right,
                BinaryOp::Multiply => left * right,
                BinaryOp::Mod => left % right,
                _ => unreachable!("integer arithmetic operator constrained by match guard"),
            };
            Ok(XPath2Value::atomic(XPath2AtomicValue::integer(value)))
        }
        BinaryOp::Idiv => {
            let right_number = right.as_f64()?;
            if right_number == 0.0 {
                return Err(XmlError::xpath("division by zero in idiv"));
            }
            Ok(XPath2Value::atomic(XPath2AtomicValue::integer(
                (left.as_f64()? / right_number).trunc() as i128,
            )))
        }
        BinaryOp::Div => {
            let right_number = right.as_f64()?;
            if right_number == 0.0 {
                return Err(XmlError::xpath("division by zero"));
            }
            Ok(XPath2Value::atomic(XPath2AtomicValue::Double(
                left.as_f64()? / right_number,
            )))
        }
        BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Mod => {
            let left = left.as_f64()?;
            let right = right.as_f64()?;
            let value = match op {
                BinaryOp::Add => left + right,
                BinaryOp::Subtract => left - right,
                BinaryOp::Multiply => left * right,
                BinaryOp::Mod => left % right,
                _ => unreachable!("numeric operator constrained by match arm"),
            };
            Ok(XPath2Value::atomic(XPath2AtomicValue::Double(value)))
        }
        _ => Err(XmlError::xpath("unsupported arithmetic operator")),
    }
}

fn general_compare(
    op: BinaryOp,
    left: &XPath2Value,
    right: &XPath2Value,
    doc: &Document<'_>,
) -> XmlResult<bool> {
    let left = left.atomized(doc);
    let right = right.atomized(doc);
    for left in &left {
        for right in &right {
            if compare_atomic(op, left, right)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn value_compare(
    op: BinaryOp,
    left: &XPath2Value,
    right: &XPath2Value,
    doc: &Document<'_>,
) -> XmlResult<XPath2Value> {
    let Some(left) = single_atomic_or_empty(left, doc)? else {
        return Ok(XPath2Value::empty());
    };
    let Some(right) = single_atomic_or_empty(right, doc)? else {
        return Ok(XPath2Value::empty());
    };
    Ok(XPath2Value::atomic(XPath2AtomicValue::Boolean(
        compare_atomic(op, &left, &right)?,
    )))
}

fn compare_atomic(
    op: BinaryOp,
    left: &XPath2AtomicValue,
    right: &XPath2AtomicValue,
) -> XmlResult<bool> {
    if left.is_numeric() || right.is_numeric() {
        let left = left.as_f64()?;
        let right = right.as_f64()?;
        return Ok(match op {
            BinaryOp::GeneralEq | BinaryOp::ValueEq => left == right,
            BinaryOp::GeneralNe | BinaryOp::ValueNe => left != right,
            BinaryOp::GeneralLt | BinaryOp::ValueLt => left < right,
            BinaryOp::GeneralLe | BinaryOp::ValueLe => left <= right,
            BinaryOp::GeneralGt | BinaryOp::ValueGt => left > right,
            BinaryOp::GeneralGe | BinaryOp::ValueGe => left >= right,
            _ => false,
        });
    }

    match (left, right) {
        (XPath2AtomicValue::Boolean(left), XPath2AtomicValue::Boolean(right)) => Ok(match op {
            BinaryOp::GeneralEq | BinaryOp::ValueEq => left == right,
            BinaryOp::GeneralNe | BinaryOp::ValueNe => left != right,
            _ => {
                return Err(XmlError::xpath(
                    "ordered comparison is undefined for booleans",
                ))
            }
        }),
        _ => {
            let left = left.to_xpath_string();
            let right = right.to_xpath_string();
            Ok(match op {
                BinaryOp::GeneralEq | BinaryOp::ValueEq => left == right,
                BinaryOp::GeneralNe | BinaryOp::ValueNe => left != right,
                BinaryOp::GeneralLt | BinaryOp::ValueLt => left < right,
                BinaryOp::GeneralLe | BinaryOp::ValueLe => left <= right,
                BinaryOp::GeneralGt | BinaryOp::ValueGt => left > right,
                BinaryOp::GeneralGe | BinaryOp::ValueGe => left >= right,
                _ => false,
            })
        }
    }
}

fn node_compare(op: BinaryOp, left: &XPath2Value, right: &XPath2Value) -> XmlResult<XPath2Value> {
    if left.is_empty() || right.is_empty() {
        return Ok(XPath2Value::empty());
    }
    let left = require_single_node(left)?;
    let right = require_single_node(right)?;
    let result = match op {
        BinaryOp::NodeIs => left == right,
        BinaryOp::NodeBefore => left.index() < right.index(),
        BinaryOp::NodeAfter => left.index() > right.index(),
        _ => unreachable!("node comparison operator constrained by caller"),
    };
    Ok(XPath2Value::atomic(XPath2AtomicValue::Boolean(result)))
}

fn single_atomic_or_empty(
    value: &XPath2Value,
    doc: &Document<'_>,
) -> XmlResult<Option<XPath2AtomicValue>> {
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > 1 {
        return Err(XmlError::xpath(
            "expected at most one item for XPath 2.0 value operation",
        ));
    }
    Ok(Some(value.items()[0].atomized(doc)))
}

fn require_single_atomic(value: &XPath2Value, doc: &Document<'_>) -> XmlResult<XPath2AtomicValue> {
    single_atomic_or_empty(value, doc)?
        .ok_or_else(|| XmlError::xpath("expected one atomic value, got the empty sequence"))
}

fn require_single_node(value: &XPath2Value) -> XmlResult<NodeId> {
    if value.len() != 1 {
        return Err(XmlError::xpath("expected exactly one node"));
    }
    match value.items()[0] {
        XPath2Item::Node(node) => Ok(node),
        XPath2Item::Atomic(_) => Err(XmlError::xpath("expected a node item")),
    }
}

fn nodes_from_value(value: XPath2Value) -> XmlResult<Vec<NodeId>> {
    value
        .into_items()
        .into_iter()
        .map(|item| match item {
            XPath2Item::Node(node) => Ok(node),
            XPath2Item::Atomic(_) => Err(XmlError::xpath(
                "path step input sequence contains an atomic value",
            )),
        })
        .collect()
}

fn node_set_operator(
    op: BinaryOp,
    left: XPath2Value,
    right: XPath2Value,
) -> XmlResult<XPath2Value> {
    let left = dedup_document_order(nodes_from_value(left)?);
    let right = dedup_document_order(nodes_from_value(right)?);
    let mut nodes = match op {
        BinaryOp::Union => {
            let mut nodes = left;
            nodes.extend(right);
            nodes
        }
        BinaryOp::Intersect => left
            .into_iter()
            .filter(|node| right.contains(node))
            .collect(),
        BinaryOp::Except => left
            .into_iter()
            .filter(|node| !right.contains(node))
            .collect(),
        _ => unreachable!("node set operator constrained by caller"),
    };
    nodes = dedup_document_order(nodes);
    Ok(XPath2Value::new(
        nodes.into_iter().map(XPath2Item::Node).collect(),
    ))
}

fn dedup_document_order(mut nodes: Vec<NodeId>) -> Vec<NodeId> {
    nodes.sort_by_key(NodeId::index);
    nodes.dedup();
    nodes
}
