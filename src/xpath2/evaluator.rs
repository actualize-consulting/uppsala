//! XPath 2.0 evaluator.

use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::rc::Rc;

use crate::dom::{Document, NodeId, NodeKind};
use crate::error::{XmlError, XmlResult};

use super::ast::{
    Axis, BinaryOp, Expr, ForBinding, ItemType, Literal, NodeTest, Occurrence, PathExpr, PathStep,
    QName, Quantifier, SequenceType, SingleType, UnaryOp,
};
use super::functions;
use super::parser::{parse_expression, DEFAULT_MAX_XPATH2_DEPTH};
use super::types::{datetime_from_unix, AtomicType, DateTimeValue, DurationValue};
use super::value::{QNameValue, XPath2AtomicValue, XPath2Item, XPath2Value};
use crate::xsd::XS_NAMESPACE;

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

    /// Resolve a call to a function that is not a built-in. `namespace` is the
    /// resolved namespace URI of the function name (or `None` for the default
    /// function namespace). Returning `Ok(None)` lets the evaluator raise its
    /// own "unknown function" diagnostic.
    fn resolve_function(
        &self,
        _namespace: Option<&str>,
        _local: &str,
        _args: &[XPath2Value],
    ) -> XmlResult<Option<XPath2Value>> {
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
    /// Maximum nested expression depth (parser).
    pub max_depth: u32,
    /// Maximum items an eager sequence constructor such as `to` may allocate.
    pub max_sequence_items: usize,
    /// Maximum total units of evaluation work (nodes selected, comparisons,
    /// sequence items produced). Bounds CPU/memory for a single evaluation so
    /// a small expression cannot explode (e.g. nested `for`, cartesian
    /// comparisons, repeated `//` predicates).
    pub max_work: usize,
    /// Maximum evaluator recursion depth. Bounds AST recursion independently of
    /// the parser's nesting cap so flat operator chains (`1 or 1 or …`) — which
    /// the parser builds iteratively — cannot overflow the stack.
    pub max_eval_depth: usize,
    /// Static base URI, returned by `fn:base-uri`/`fn:static-base-uri`.
    pub base_uri: Option<String>,
    /// Implicit timezone offset in minutes, used by date/time functions.
    pub implicit_timezone_minutes: i32,
    /// Override for `fn:current-dateTime` and friends, as unix seconds. When
    /// `None`, the wall clock is used. Set this for deterministic evaluation.
    pub current_datetime_unix: Option<i64>,
}

impl Default for XPath2Options {
    fn default() -> Self {
        Self {
            xpath1_compatibility: false,
            max_depth: DEFAULT_MAX_XPATH2_DEPTH,
            max_sequence_items: DEFAULT_MAX_XPATH2_SEQUENCE_ITEMS,
            max_work: DEFAULT_MAX_XPATH2_WORK,
            max_eval_depth: DEFAULT_MAX_XPATH2_EVAL_DEPTH,
            base_uri: None,
            implicit_timezone_minutes: 0,
            current_datetime_unix: None,
        }
    }
}

/// Default maximum items for eager XPath 2.0 sequence construction.
pub const DEFAULT_MAX_XPATH2_SEQUENCE_ITEMS: usize = 100_000;

/// Default evaluation-work budget for a single XPath 2.0 evaluation.
pub const DEFAULT_MAX_XPATH2_WORK: usize = 10_000_000;

/// Default evaluator recursion-depth cap. Comfortably above any realistic
/// expression nesting yet far below the stack-overflow threshold.
pub const DEFAULT_MAX_XPATH2_EVAL_DEPTH: usize = 1_000;

/// Shared per-evaluation resource budget: a work counter and a recursion-depth
/// counter, both with interior mutability so they survive context forks.
struct EvalBudget {
    remaining: Cell<usize>,
    max_work: usize,
    depth: Cell<usize>,
    max_eval_depth: usize,
}

impl EvalBudget {
    fn new(max_work: usize, max_eval_depth: usize) -> Self {
        Self {
            remaining: Cell::new(max_work),
            max_work,
            depth: Cell::new(0),
            max_eval_depth,
        }
    }

    /// Charge `amount` units of work, failing closed when the budget is spent.
    fn charge(&self, amount: usize) -> XmlResult<()> {
        let remaining = self.remaining.get();
        if amount > remaining {
            return Err(XmlError::xpath(format!(
                "XPath 2.0 evaluation exceeded work budget of {}",
                self.max_work
            )));
        }
        self.remaining.set(remaining - amount);
        Ok(())
    }

    /// Enter one recursion level, returning a guard that releases it on drop.
    ///
    /// Takes the shared `Rc` so the returned guard owns an independent handle to
    /// the counter and does not borrow the surrounding `DynamicContext` (which
    /// the evaluator still needs mutably).
    fn enter(budget: &Rc<EvalBudget>) -> XmlResult<DepthGuard> {
        let depth = budget.depth.get() + 1;
        if depth > budget.max_eval_depth {
            return Err(XmlError::xpath(format!(
                "XPath 2.0 expression nesting exceeded maximum evaluation depth of {}",
                budget.max_eval_depth
            )));
        }
        budget.depth.set(depth);
        Ok(DepthGuard {
            budget: Rc::clone(budget),
        })
    }
}

/// RAII guard decrementing the evaluator depth counter when a recursion level
/// returns (on every path, including `?` early-exit).
struct DepthGuard {
    budget: Rc<EvalBudget>,
}

impl Drop for DepthGuard {
    fn drop(&mut self) {
        self.budget
            .depth
            .set(self.budget.depth.get().saturating_sub(1));
    }
}

/// XPath 2.0 evaluator.
#[derive(Debug, Clone)]
pub struct XPath2Evaluator<R = NoopXPath2Resolver> {
    options: XPath2Options,
    resolver: R,
    namespaces: HashMap<String, String>,
    default_element_namespace: Option<String>,
    external_variables: HashMap<String, XPath2Value>,
}

impl XPath2Evaluator<NoopXPath2Resolver> {
    /// Create an XPath 2.0 evaluator with default options and no resolver.
    ///
    /// The `xs` and `xsd` prefixes are pre-bound to the XML Schema namespace so
    /// type names such as `xs:integer` resolve without explicit configuration.
    /// Callers may override these bindings via [`Self::add_namespace`].
    pub fn new() -> Self {
        let mut namespaces = HashMap::new();
        namespaces.insert("xs".to_string(), XS_NAMESPACE.to_string());
        namespaces.insert("xsd".to_string(), XS_NAMESPACE.to_string());
        Self {
            options: XPath2Options::default(),
            resolver: NoopXPath2Resolver,
            namespaces,
            default_element_namespace: None,
            external_variables: HashMap::new(),
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
            default_element_namespace: self.default_element_namespace,
            external_variables: self.external_variables,
        }
    }

    /// Set the default element/type namespace applied to unprefixed name tests.
    pub fn with_default_element_namespace(mut self, uri: impl Into<String>) -> Self {
        self.default_element_namespace = Some(uri.into());
        self
    }

    /// Bind an external variable, available to expressions as `$name`. The name
    /// is the lexical `prefix:local` form used in the expression.
    pub fn with_variable(mut self, name: impl Into<String>, value: XPath2Value) -> Self {
        self.external_variables.insert(name.into(), value);
        self
    }

    /// Bind an external variable in place (builder-free).
    pub fn set_variable(&mut self, name: impl Into<String>, value: XPath2Value) {
        self.external_variables.insert(name.into(), value);
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

    /// Set the total evaluation-work budget (DoS bound for a single evaluation).
    pub fn with_max_work(mut self, max_work: usize) -> Self {
        self.options.max_work = max_work;
        self
    }

    /// Set the evaluator recursion-depth cap.
    pub fn with_max_eval_depth(mut self, max_eval_depth: usize) -> Self {
        self.options.max_eval_depth = max_eval_depth;
        self
    }

    /// Set the static base URI returned by `fn:base-uri`/`fn:static-base-uri`.
    pub fn with_base_uri(mut self, base_uri: impl Into<String>) -> Self {
        self.options.base_uri = Some(base_uri.into());
        self
    }

    /// Set the implicit timezone (offset in minutes from UTC) used by the
    /// date/time functions.
    pub fn with_implicit_timezone_minutes(mut self, minutes: i32) -> Self {
        self.options.implicit_timezone_minutes = minutes;
        self
    }

    /// Pin `fn:current-dateTime`/`current-date`/`current-time` to a fixed
    /// instant expressed as unix seconds, for deterministic evaluation.
    pub fn with_current_datetime_unix(mut self, unix_seconds: i64) -> Self {
        self.options.current_datetime_unix = Some(unix_seconds);
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
            self.default_element_namespace.as_deref(),
            &self.external_variables,
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
    default_element_namespace: Option<&'doc str>,
    external_variables: &'doc HashMap<String, XPath2Value>,
    context_item: XPath2Item,
    position: usize,
    size: usize,
    variables: Vec<(String, XPath2Value)>,
    budget: Rc<EvalBudget>,
}

impl<'doc, 'input, R> DynamicContext<'doc, 'input, R>
where
    R: XPath2Resolver,
{
    #[allow(clippy::too_many_arguments)]
    fn new(
        doc: &'doc Document<'input>,
        resolver: &'doc R,
        options: &'doc XPath2Options,
        namespaces: &'doc HashMap<String, String>,
        default_element_namespace: Option<&'doc str>,
        external_variables: &'doc HashMap<String, XPath2Value>,
        context_item: XPath2Item,
    ) -> Self {
        Self {
            doc,
            resolver,
            options,
            namespaces,
            default_element_namespace,
            external_variables,
            context_item,
            position: 1,
            size: 1,
            variables: Vec::new(),
            budget: Rc::new(EvalBudget::new(options.max_work, options.max_eval_depth)),
        }
    }

    fn fork_with_context(&self, context_item: XPath2Item, position: usize, size: usize) -> Self {
        Self {
            doc: self.doc,
            resolver: self.resolver,
            options: self.options,
            namespaces: self.namespaces,
            default_element_namespace: self.default_element_namespace,
            external_variables: self.external_variables,
            context_item,
            position,
            size,
            variables: self.variables.clone(),
            budget: Rc::clone(&self.budget),
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
            .or_else(|| self.external_variables.get(&key).cloned())
    }
}

fn evaluate_expr<R>(expr: &Expr<'_>, ctx: &mut DynamicContext<'_, '_, R>) -> XmlResult<XPath2Value>
where
    R: XPath2Resolver,
{
    // Bound recursion depth so flat operator chains (parsed iteratively, hence
    // not covered by the parser's nesting cap) cannot overflow the stack.
    let _depth_guard = EvalBudget::enter(&ctx.budget)?;
    match expr {
        Expr::EmptySequence => Ok(XPath2Value::empty()),
        Expr::Sequence(items) => {
            let mut value = XPath2Value::empty();
            for item in items {
                let part = evaluate_expr(item, ctx)?;
                ctx.budget.charge(part.len().max(1))?;
                value.extend(part);
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
        Expr::InstanceOf { expr, seq_type } => {
            let value = evaluate_expr(expr, ctx)?;
            Ok(XPath2Value::boolean(matches_sequence_type(
                &value, seq_type, ctx,
            )?))
        }
        Expr::TreatAs { expr, seq_type } => {
            let value = evaluate_expr(expr, ctx)?;
            if matches_sequence_type(&value, seq_type, ctx)? {
                Ok(value)
            } else {
                Err(XmlError::xpath_code(
                    "XPDY0050",
                    "treat as: operand does not match the required sequence type",
                ))
            }
        }
        Expr::Castable { expr, single_type } => {
            let value = evaluate_expr(expr, ctx)?;
            Ok(XPath2Value::boolean(evaluate_castable(
                &value,
                single_type,
                ctx,
            )?))
        }
        Expr::Cast { expr, single_type } => {
            let value = evaluate_expr(expr, ctx)?;
            evaluate_cast(&value, single_type, ctx)
        }
        Expr::Path(path) => evaluate_path(path, ctx),
    }
}

/// Resolve a type-name QName to a built-in atomic type using the static
/// namespace bindings.
fn resolve_atomic_type<R>(
    name: &QName<'_>,
    ctx: &DynamicContext<'_, '_, R>,
) -> XmlResult<AtomicType>
where
    R: XPath2Resolver,
{
    let uri = match name.prefix {
        Some(prefix) => ctx.namespaces.get(prefix).map(String::as_str),
        None => None,
    };
    AtomicType::from_name(uri, name.local)
        .ok_or_else(|| XmlError::xpath_code("XPST0051", format!("unknown atomic type '{}'", name)))
}

/// `instance of`: test a value against a sequence type.
fn matches_sequence_type<R>(
    value: &XPath2Value,
    seq_type: &SequenceType<'_>,
    ctx: &DynamicContext<'_, '_, R>,
) -> XmlResult<bool>
where
    R: XPath2Resolver,
{
    let len = value.len();
    let Some(item_type) = &seq_type.item else {
        // empty-sequence()
        return Ok(len == 0);
    };

    let cardinality_ok = match seq_type.occurrence {
        Occurrence::One => len == 1,
        Occurrence::ZeroOrOne => len <= 1,
        Occurrence::ZeroOrMore => true,
        Occurrence::OneOrMore => len >= 1,
    };
    if !cardinality_ok {
        return Ok(false);
    }

    for item in value.items() {
        if !matches_item_type(item, item_type, ctx)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn matches_item_type<R>(
    item: &XPath2Item,
    item_type: &ItemType<'_>,
    ctx: &DynamicContext<'_, '_, R>,
) -> XmlResult<bool>
where
    R: XPath2Resolver,
{
    match item_type {
        ItemType::Item => Ok(true),
        ItemType::Atomic(name) => {
            let target = resolve_atomic_type(name, ctx)?;
            match item {
                XPath2Item::Atomic(value) => Ok(value.type_of().is_subtype_of(target)),
                XPath2Item::Node(_) => Ok(false),
            }
        }
        ItemType::Kind(test) => match item {
            XPath2Item::Node(node) => Ok(node_matches(ctx.doc, *node, Axis::Child, test, ctx)),
            XPath2Item::Atomic(_) => Ok(false),
        },
    }
}

/// `castable as`: whether `cast as` would succeed.
fn evaluate_castable<R>(
    value: &XPath2Value,
    single_type: &SingleType<'_>,
    ctx: &DynamicContext<'_, '_, R>,
) -> XmlResult<bool>
where
    R: XPath2Resolver,
{
    let target = resolve_atomic_type(&single_type.type_name, ctx)?;
    let atoms = value.atomized(ctx.doc);
    match atoms.len() {
        0 => Ok(single_type.optional),
        1 => Ok(functions::castable(&atoms[0], target, ctx.namespaces)),
        _ => Ok(false),
    }
}

/// `cast as`: cast a single atomized item to the target atomic type.
fn evaluate_cast<R>(
    value: &XPath2Value,
    single_type: &SingleType<'_>,
    ctx: &DynamicContext<'_, '_, R>,
) -> XmlResult<XPath2Value>
where
    R: XPath2Resolver,
{
    let target = resolve_atomic_type(&single_type.type_name, ctx)?;
    let atoms = value.atomized(ctx.doc);
    match atoms.len() {
        0 => {
            if single_type.optional {
                Ok(XPath2Value::empty())
            } else {
                Err(XmlError::xpath_code(
                    "XPTY0004",
                    "cast as: cannot cast the empty sequence to a required single type",
                ))
            }
        }
        1 => Ok(XPath2Value::atomic(functions::cast_to(
            &atoms[0],
            target,
            ctx.namespaces,
        )?)),
        _ => Err(XmlError::xpath_code(
            "XPTY0004",
            "cast as: input sequence has more than one item",
        )),
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
        let body_value = evaluate_expr(body, ctx)?;
        // Charge for items appended to the aggregate result. This bounds the
        // total output of nested `for` expressions (cartesian blow-up), which
        // `max_sequence_items` (a per-`to` cap) does not cover.
        ctx.budget.charge(body_value.len().max(1))?;
        result.extend(body_value);
        return Ok(());
    }

    let binding = &bindings[index];
    let sequence = evaluate_expr(&binding.in_expr, ctx)?;
    for item in sequence.into_items() {
        // Charge per iteration so an empty-bodied deep `for` nest is still bounded.
        ctx.budget.charge(1)?;
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
    let value = compat_first(evaluate_expr(expr, ctx)?, ctx.options.xpath1_compatibility);
    let Some(atomic) = single_atomic_or_empty(&value, ctx.doc)? else {
        return Ok(XPath2Value::empty());
    };

    match op {
        UnaryOp::Plus => Ok(XPath2Value::atomic(atomic)),
        UnaryOp::Minus => match atomic {
            XPath2AtomicValue::Integer(value) => {
                let parsed = value.parse::<i128>().map_err(|_| {
                    XmlError::xpath(format!("cannot apply unary minus to '{}'", value))
                })?;
                let negated = parsed
                    .checked_neg()
                    .ok_or_else(|| XmlError::xpath("integer arithmetic overflow"))?;
                Ok(XPath2Value::atomic(XPath2AtomicValue::integer(negated)))
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
            // A general comparison is an O(n*m) cartesian scan; charge for it so
            // `(//a) = (//b)` over large node-sets cannot run uncharged.
            ctx.budget
                .charge(left.len().saturating_mul(right.len()).max(1))?;
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
            let compat = ctx.options.xpath1_compatibility;
            let left = compat_first(evaluate_expr(left, ctx)?, compat);
            let right = compat_first(evaluate_expr(right, ctx)?, compat);
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
                ctx.budget.charge(len as usize)?;
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
            let compat = ctx.options.xpath1_compatibility;
            let left = compat_first(evaluate_expr(left, ctx)?, compat);
            let right = compat_first(evaluate_expr(right, ctx)?, compat);
            arithmetic(op, &left, &right, ctx.doc, compat)
        }
        BinaryOp::Union | BinaryOp::Intersect | BinaryOp::Except => {
            let left = evaluate_expr(left, ctx)?;
            let right = evaluate_expr(right, ctx)?;
            ctx.budget
                .charge(left.len().saturating_add(right.len()).max(1))?;
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
        let mut candidates: Vec<NodeId> = axis_nodes(doc, *node, step.axis)?
            .into_iter()
            .filter(|candidate| node_matches(doc, *candidate, step.axis, &step.test, ctx))
            .collect();

        // Predicate position semantics: forward axes number positions in
        // document order, reverse axes in reverse document order (closest to the
        // context node first). Normalize the candidate order accordingly before
        // predicates run so `ancestor::x[1]` selects the nearest ancestor.
        if is_reverse_axis(step.axis) {
            candidates.sort_by_key(|n| std::cmp::Reverse(n.index()));
            candidates.dedup();
        } else {
            candidates = dedup_document_order(candidates);
        }

        // Charge for every candidate the axis produced — this is the primary
        // node-multiplying operation and bounds `//`, predicate re-evaluation,
        // and large axis fan-out.
        ctx.budget.charge(candidates.len().max(1))?;

        for predicate in &step.predicates {
            candidates = apply_predicate(doc, ctx, &candidates, predicate)?;
        }

        selected.extend(candidates);
    }

    Ok(dedup_document_order(selected))
}

/// Whether an axis is a reverse axis, for predicate position semantics.
fn is_reverse_axis(axis: Axis) -> bool {
    matches!(
        axis,
        Axis::Ancestor
            | Axis::AncestorOrSelf
            | Axis::Preceding
            | Axis::PrecedingSibling
            | Axis::Parent
    )
}

fn axis_nodes(doc: &Document<'_>, node: NodeId, axis: Axis) -> XmlResult<Vec<NodeId>> {
    Ok(match axis {
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
        Axis::Namespace => {
            // The DOM arena has no namespace nodes, and the namespace axis is
            // deprecated in XPath 2.0. Surface an explicit diagnostic rather
            // than silently returning the empty sequence.
            return Err(XmlError::xpath_code(
                "XPST0010",
                "the namespace axis is not supported (no namespace nodes in the data model); use fn:namespace-uri-for-prefix or in-scope-prefixes instead",
            ));
        }
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
    })
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

fn node_matches<R>(
    doc: &Document<'_>,
    node: NodeId,
    axis: Axis,
    test: &NodeTest<'_>,
    ctx: &DynamicContext<'_, '_, R>,
) -> bool
where
    R: XPath2Resolver,
{
    let namespaces = ctx.namespaces;
    // The default element namespace applies to unprefixed element name tests
    // (never to attributes, which are in no namespace unless prefixed).
    let element_default_ns = ctx.default_element_namespace;
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
                element_default_ns,
            ),
            Some(NodeKind::Attribute(attr_name, _)) => expanded_name_matches(
                name,
                attr_name.local_name.as_ref(),
                attr_name.namespace_uri.as_deref(),
                namespaces,
                None,
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
        NodeTest::Document(inner) => match doc.node_kind(node) {
            Some(NodeKind::Document) => match inner {
                None => true,
                Some(inner) => doc
                    .children(node)
                    .into_iter()
                    .any(|child| node_matches(doc, child, Axis::Child, inner, ctx)),
            },
            _ => false,
        },
        NodeTest::Element(name, _type) => match doc.node_kind(node) {
            Some(NodeKind::Element(element)) => name.as_ref().is_none_or(|name| {
                expanded_name_matches(
                    name,
                    element.name.local_name.as_ref(),
                    element.name.namespace_uri.as_deref(),
                    namespaces,
                    element_default_ns,
                )
            }),
            _ => false,
        },
        // schema-element() degrades to a name-only element test without PSVI.
        NodeTest::SchemaElement(name) => match doc.node_kind(node) {
            Some(NodeKind::Element(element)) => expanded_name_matches(
                name,
                element.name.local_name.as_ref(),
                element.name.namespace_uri.as_deref(),
                namespaces,
                element_default_ns,
            ),
            _ => false,
        },
        NodeTest::Attribute(name, _type) => match doc.node_kind(node) {
            Some(NodeKind::Attribute(attr_name, _)) => name.as_ref().is_none_or(|name| {
                expanded_name_matches(
                    name,
                    attr_name.local_name.as_ref(),
                    attr_name.namespace_uri.as_deref(),
                    namespaces,
                    None,
                )
            }),
            _ => false,
        },
        NodeTest::SchemaAttribute(name) => match doc.node_kind(node) {
            Some(NodeKind::Attribute(attr_name, _)) => expanded_name_matches(
                name,
                attr_name.local_name.as_ref(),
                attr_name.namespace_uri.as_deref(),
                namespaces,
                None,
            ),
            _ => false,
        },
    }
}

fn expanded_name_matches(
    name: &QName<'_>,
    actual_local: &str,
    actual_namespace: Option<&str>,
    namespaces: &HashMap<String, String>,
    default_namespace: Option<&str>,
) -> bool {
    // XPath expressions bind prefixes in the static context. Comparing URI
    // values prevents a document from rebinding the same lexical prefix to a
    // different namespace and still matching a prefixed name test.
    if actual_local != name.local {
        return false;
    }

    match name.prefix {
        Some(prefix) => namespace_matches(prefix, actual_namespace, namespaces),
        // An unprefixed name test matches the default element namespace when
        // one is configured, otherwise only no-namespace nodes.
        None => match default_namespace {
            Some(uri) => actual_namespace == Some(uri),
            None => actual_namespace.is_none(),
        },
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
    // Constructor functions: a call whose name resolves to the XSD namespace
    // is a cast of its single argument to that atomic type.
    if let Some(prefix) = name.prefix {
        if ctx.namespaces.get(prefix).map(String::as_str) == Some(XS_NAMESPACE) {
            if let Some(target) = AtomicType::from_name(Some(XS_NAMESPACE), name.local) {
                expect_arity(name.local, args, 1)?;
                let value = evaluate_expr(&args[0], ctx)?;
                let atoms = value.atomized(ctx.doc);
                return match atoms.len() {
                    0 => Ok(XPath2Value::empty()),
                    1 => Ok(XPath2Value::atomic(functions::cast_to(
                        &atoms[0],
                        target,
                        ctx.namespaces,
                    )?)),
                    _ => Err(XmlError::xpath_code(
                        "XPTY0004",
                        "constructor function expects at most one item",
                    )),
                };
            }
        }
        if !matches!(prefix, "fn") {
            return Err(XmlError::xpath_code(
                "XPST0017",
                format!(
                    "unsupported XPath 2.0 function namespace prefix '{}'",
                    prefix
                ),
            ));
        }
    }

    // Eagerly evaluate all arguments (function calls are not short-circuiting).
    let mut argv: Vec<XPath2Value> = Vec::with_capacity(args.len());
    for arg in args {
        let v = evaluate_expr(arg, ctx)?;
        ctx.budget.charge(v.len().max(1))?;
        argv.push(v);
    }

    dispatch_function(name.local, &argv, ctx)
}

/// The first argument value, or the context item as a singleton if no
/// arguments were supplied (used by `string()`, `number()`, accessors, ...).
fn arg_or_context<R>(argv: &[XPath2Value], ctx: &DynamicContext<'_, '_, R>) -> XPath2Value
where
    R: XPath2Resolver,
{
    match argv.first() {
        Some(v) => v.clone(),
        None => XPath2Value::new(vec![ctx.context_item.clone()]),
    }
}

fn bool_value(b: bool) -> XPath2Value {
    XPath2Value::atomic(XPath2AtomicValue::Boolean(b))
}

fn str_value(s: impl Into<String>) -> XPath2Value {
    XPath2Value::atomic(XPath2AtomicValue::String(s.into()))
}

fn int_value(n: i128) -> XPath2Value {
    XPath2Value::atomic(XPath2AtomicValue::integer(n))
}

/// Dispatch a `fn:*` call with already-evaluated arguments.
fn dispatch_function<R>(
    local: &str,
    argv: &[XPath2Value],
    ctx: &DynamicContext<'_, '_, R>,
) -> XmlResult<XPath2Value>
where
    R: XPath2Resolver,
{
    let doc = ctx.doc;
    let unsupported = || {
        Err(XmlError::xpath_code(
            "XPST0017",
            format!("unsupported XPath 2.0 function 'fn:{}'", local),
        ))
    };

    match (local, argv.len()) {
        // ---- Boolean ----
        ("true", 0) => Ok(bool_value(true)),
        ("false", 0) => Ok(bool_value(false)),
        ("not", 1) => Ok(bool_value(!argv[0].effective_boolean_value(doc)?)),
        ("boolean", 1) => Ok(bool_value(argv[0].effective_boolean_value(doc)?)),

        // ---- Context position ----
        ("position", 0) => Ok(int_value(ctx.position as i128)),
        ("last", 0) => Ok(int_value(ctx.size as i128)),

        // ---- String value / accessors ----
        ("string", 0..=1) => Ok(str_value(arg_or_context(argv, ctx).to_string_value(doc))),
        ("data", 1) => {
            let atoms = argv[0].atomized(doc);
            Ok(XPath2Value::new(
                atoms.into_iter().map(XPath2Item::Atomic).collect(),
            ))
        }
        ("string-length", 0..=1) => {
            let s = arg_or_context(argv, ctx).to_string_value(doc);
            Ok(int_value(s.chars().count() as i128))
        }
        ("normalize-space", 0..=1) => {
            let s = arg_or_context(argv, ctx).to_string_value(doc);
            Ok(str_value(
                s.split_whitespace().collect::<Vec<_>>().join(" "),
            ))
        }
        ("normalize-unicode", 1..=2) => {
            // Without a Unicode normalization table this returns the input for
            // NFC (the default) and errors for unsupported explicit forms.
            if argv.len() == 2 {
                let form = argv[1].to_string_value(doc);
                let form = form.trim().to_uppercase();
                if !form.is_empty() && form != "NFC" {
                    return Err(XmlError::xpath_code(
                        "FOCH0003",
                        format!("unsupported normalization form '{}'", form),
                    ));
                }
            }
            Ok(str_value(argv[0].to_string_value(doc)))
        }
        ("upper-case", 1) => Ok(str_value(argv[0].to_string_value(doc).to_uppercase())),
        ("lower-case", 1) => Ok(str_value(argv[0].to_string_value(doc).to_lowercase())),
        ("concat", n) if n >= 2 => {
            let mut out = String::new();
            for v in argv {
                out.push_str(&v.to_string_value(doc));
            }
            Ok(str_value(out))
        }
        ("string-join", 2) => {
            let sep = argv[1].to_string_value(doc);
            let parts: Vec<String> = argv[0]
                .items()
                .iter()
                .map(|item| item.string_value(doc))
                .collect();
            Ok(str_value(parts.join(&sep)))
        }
        ("substring", 2..=3) => fn_substring(argv, doc),
        ("substring-before", 2) => {
            let a = argv[0].to_string_value(doc);
            let b = argv[1].to_string_value(doc);
            Ok(str_value(match a.find(&b) {
                Some(idx) if !b.is_empty() => a[..idx].to_string(),
                _ => String::new(),
            }))
        }
        ("substring-after", 2) => {
            let a = argv[0].to_string_value(doc);
            let b = argv[1].to_string_value(doc);
            Ok(str_value(match a.find(&b) {
                Some(idx) => a[idx + b.len()..].to_string(),
                None => String::new(),
            }))
        }
        ("contains", 2..=3) => {
            let a = argv[0].to_string_value(doc);
            let b = argv[1].to_string_value(doc);
            Ok(bool_value(a.contains(&b)))
        }
        ("starts-with", 2..=3) => {
            let a = argv[0].to_string_value(doc);
            let b = argv[1].to_string_value(doc);
            Ok(bool_value(a.starts_with(&b)))
        }
        ("ends-with", 2..=3) => {
            let a = argv[0].to_string_value(doc);
            let b = argv[1].to_string_value(doc);
            Ok(bool_value(a.ends_with(&b)))
        }
        ("translate", 3) => {
            let s = argv[0].to_string_value(doc);
            let map = argv[1].to_string_value(doc);
            let to = argv[2].to_string_value(doc);
            let to_chars: Vec<char> = to.chars().collect();
            let mut out = String::new();
            for c in s.chars() {
                match map.chars().position(|m| m == c) {
                    Some(i) => {
                        if let Some(r) = to_chars.get(i) {
                            out.push(*r);
                        }
                    }
                    None => out.push(c),
                }
            }
            Ok(str_value(out))
        }
        ("string-to-codepoints", 1) => {
            let s = argv[0].to_string_value(doc);
            Ok(XPath2Value::new(
                s.chars()
                    .map(|c| XPath2Item::Atomic(XPath2AtomicValue::integer(c as i128)))
                    .collect(),
            ))
        }
        ("codepoints-to-string", 1) => {
            let mut out = String::new();
            for item in argv[0].atomized(doc) {
                let cp = item.as_i128()?;
                let c = u32::try_from(cp)
                    .ok()
                    .and_then(char::from_u32)
                    .ok_or_else(|| XmlError::xpath_code("FOCH0001", "invalid codepoint"))?;
                out.push(c);
            }
            Ok(str_value(out))
        }
        ("compare", 2..=3) => {
            if argv[0].is_empty() || argv[1].is_empty() {
                return Ok(XPath2Value::empty());
            }
            let a = argv[0].to_string_value(doc);
            let b = argv[1].to_string_value(doc);
            let ord = functions::codepoint_compare(&a, &b);
            Ok(int_value(match ord {
                Ordering::Less => -1,
                Ordering::Equal => 0,
                Ordering::Greater => 1,
            }))
        }
        ("codepoint-equal", 2) => {
            if argv[0].is_empty() || argv[1].is_empty() {
                return Ok(XPath2Value::empty());
            }
            Ok(bool_value(
                argv[0].to_string_value(doc) == argv[1].to_string_value(doc),
            ))
        }
        ("encode-for-uri", 1) => Ok(str_value(percent_encode(
            &argv[0].to_string_value(doc),
            |c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~'),
        ))),
        ("iri-to-uri", 1) => Ok(str_value(percent_encode(
            &argv[0].to_string_value(doc),
            |c| c.is_ascii_graphic() && !matches!(c, ' '),
        ))),
        ("escape-html-uri", 1) => Ok(str_value(percent_encode(
            &argv[0].to_string_value(doc),
            |c| (' '..='~').contains(&c),
        ))),

        // ---- Regex ----
        ("matches", 2..=3) => {
            let input = argv[0].to_string_value(doc);
            let pattern = argv[1].to_string_value(doc);
            let flags = optional_string(argv, 2, doc);
            Ok(bool_value(functions::regex_matches(
                &input, &pattern, &flags,
            )?))
        }
        ("replace", 3..=4) => {
            let input = argv[0].to_string_value(doc);
            let pattern = argv[1].to_string_value(doc);
            let replacement = argv[2].to_string_value(doc);
            let flags = optional_string(argv, 3, doc);
            Ok(str_value(functions::regex_replace(
                &input,
                &pattern,
                &replacement,
                &flags,
            )?))
        }
        ("tokenize", 2..=3) => {
            let input = argv[0].to_string_value(doc);
            let pattern = argv[1].to_string_value(doc);
            let flags = optional_string(argv, 2, doc);
            let parts = functions::regex_tokenize(&input, &pattern, &flags)?;
            Ok(XPath2Value::new(
                parts
                    .into_iter()
                    .map(|p| XPath2Item::Atomic(XPath2AtomicValue::String(p)))
                    .collect(),
            ))
        }

        // ---- Numeric ----
        ("number", 0..=1) => {
            let value = arg_or_context(argv, ctx);
            match single_atomic_or_empty(&value, doc)? {
                Some(a) => Ok(XPath2Value::atomic(XPath2AtomicValue::Double(
                    a.as_f64().unwrap_or(f64::NAN),
                ))),
                None => Ok(XPath2Value::atomic(XPath2AtomicValue::Double(f64::NAN))),
            }
        }
        ("abs", 1) => numeric_unary(argv, doc, |n| n.abs(), |i| i.abs()),
        ("ceiling", 1) => numeric_unary(argv, doc, |n| n.ceil(), |i| i),
        ("floor", 1) => numeric_unary(argv, doc, |n| n.floor(), |i| i),
        ("round", 1) => numeric_unary(argv, doc, round_half_up, |i| i),
        ("round-half-to-even", 1..=2) => numeric_unary(argv, doc, |n| round_half_even(n, 0), |i| i),

        // ---- Aggregate ----
        ("count", 1) => Ok(int_value(argv[0].len() as i128)),
        ("sum", 1..=2) => fn_sum(argv, doc),
        ("avg", 1) => fn_avg(argv, doc),
        ("max", 1..=2) => fn_min_max(argv, doc, true),
        ("min", 1..=2) => fn_min_max(argv, doc, false),

        // ---- Sequence ----
        ("empty", 1) => Ok(bool_value(argv[0].is_empty())),
        ("exists", 1) => Ok(bool_value(!argv[0].is_empty())),
        ("distinct-values", 1..=2) => {
            // Dedup is O(n^2) (linear scan of `seen` per item); charge the
            // quadratic cost up front so a cheaply-built large sequence cannot
            // drive uncharged CPU work (see security audit, F5).
            let n = argv[0].len();
            ctx.budget.charge(n.saturating_mul(n))?;
            fn_distinct_values(argv, doc)
        }
        ("index-of", 2..=3) => fn_index_of(argv, doc),
        ("reverse", 1) => {
            let mut items = argv[0].items().to_vec();
            items.reverse();
            Ok(XPath2Value::new(items))
        }
        ("remove", 2) => {
            let pos = argv[1]
                .atomized(doc)
                .first()
                .map(|a| a.as_i128())
                .transpose()?
                .unwrap_or(0);
            let items: Vec<XPath2Item> = argv[0]
                .items()
                .iter()
                .enumerate()
                .filter(|(i, _)| (*i as i128) + 1 != pos)
                .map(|(_, item)| item.clone())
                .collect();
            Ok(XPath2Value::new(items))
        }
        ("insert-before", 3) => fn_insert_before(argv, doc),
        ("subsequence", 2..=3) => fn_subsequence(argv, doc),
        ("unordered", 1) => Ok(argv[0].clone()),
        ("deep-equal", 2..=3) => Ok(bool_value(deep_equal(&argv[0], &argv[1], doc))),
        ("zero-or-one", 1) => {
            if argv[0].len() > 1 {
                Err(XmlError::xpath_code(
                    "FORG0003",
                    "fn:zero-or-one called with a sequence of more than one item",
                ))
            } else {
                Ok(argv[0].clone())
            }
        }
        ("one-or-more", 1) => {
            if argv[0].is_empty() {
                Err(XmlError::xpath_code(
                    "FORG0004",
                    "fn:one-or-more called with the empty sequence",
                ))
            } else {
                Ok(argv[0].clone())
            }
        }
        ("exactly-one", 1) => {
            if argv[0].len() == 1 {
                Ok(argv[0].clone())
            } else {
                Err(XmlError::xpath_code(
                    "FORG0005",
                    "fn:exactly-one called with a sequence not of length one",
                ))
            }
        }

        // ---- Node accessors ----
        ("name", 0..=1) => fn_node_name_string(argv, ctx, true),
        ("local-name", 0..=1) => fn_node_name_string(argv, ctx, false),
        ("namespace-uri", 0..=1) => {
            let value = arg_or_context(argv, ctx);
            let Some(node) = single_node_opt(&value) else {
                return Ok(str_value(""));
            };
            Ok(str_value(node_namespace_uri(doc, node).unwrap_or_default()))
        }
        ("node-name", 1) => {
            let Some(node) = single_node_opt(&argv[0]) else {
                return Ok(XPath2Value::empty());
            };
            match node_qname_value(doc, node) {
                Some(q) => Ok(XPath2Value::atomic(XPath2AtomicValue::QName(q))),
                None => Ok(XPath2Value::empty()),
            }
        }
        ("root", 0..=1) => {
            let value = arg_or_context(argv, ctx);
            let Some(node) = single_node_opt(&value) else {
                return Ok(XPath2Value::empty());
            };
            let root = doc.ancestors(node).into_iter().last().unwrap_or(node);
            Ok(XPath2Value::node(root))
        }
        ("nilled", 1) => Ok(XPath2Value::empty()),
        ("base-uri", 0..=1) | ("static-base-uri", 0) => Ok(match &ctx.options.base_uri {
            Some(uri) => XPath2Value::atomic(XPath2AtomicValue::AnyUri(uri.clone())),
            None => XPath2Value::empty(),
        }),
        ("document-uri", 1) => Ok(XPath2Value::empty()),
        ("lang", 1..=2) => fn_lang(argv, ctx),
        ("id", 1..=2) => fn_id(argv, ctx),

        // ---- QName functions ----
        ("QName", 2) => fn_construct_qname(argv, doc),
        ("local-name-from-QName", 1) => qname_component(argv, doc, QNameComponent::Local),
        ("namespace-uri-from-QName", 1) => qname_component(argv, doc, QNameComponent::Uri),
        ("prefix-from-QName", 1) => qname_component(argv, doc, QNameComponent::Prefix),
        ("namespace-uri-for-prefix", 2) => {
            let prefix = argv[0].to_string_value(doc);
            match ctx.namespaces.get(&prefix) {
                Some(uri) => Ok(XPath2Value::atomic(XPath2AtomicValue::AnyUri(uri.clone()))),
                None => Ok(XPath2Value::empty()),
            }
        }

        // ---- Date/time accessors ----
        ("current-dateTime", 0) => Ok(XPath2Value::atomic(current_datetime(ctx))),
        ("current-date", 0) => Ok(XPath2Value::atomic(current_date(ctx))),
        ("current-time", 0) => Ok(XPath2Value::atomic(current_time(ctx))),
        ("implicit-timezone", 0) => Ok(XPath2Value::atomic(XPath2AtomicValue::Duration(
            DurationValue {
                months: 0,
                seconds: ctx.options.implicit_timezone_minutes as f64 * 60.0,
            },
            AtomicType::DayTimeDuration,
        ))),

        // ---- Resolver-backed resources ----
        ("doc", 1) => {
            if argv[0].is_empty() {
                return Ok(XPath2Value::empty());
            }
            let uri = argv[0].to_string_value(doc);
            ctx.resolver.resolve_doc(&uri)?.ok_or_else(|| {
                XmlError::xpath_code(
                    "FODC0005",
                    format!("doc('{}') is unavailable from the configured resolver", uri),
                )
            })
        }
        ("doc-available", 1) => {
            let uri = argv[0].to_string_value(doc);
            Ok(bool_value(ctx.resolver.resolve_doc(&uri)?.is_some()))
        }
        ("collection", 0..=1) => {
            let uri = argv.first().map(|v| v.to_string_value(doc));
            ctx.resolver
                .resolve_collection(uri.as_deref())?
                .ok_or_else(|| {
                    XmlError::xpath_code(
                        "FODC0004",
                        "collection() is unavailable from the configured resolver",
                    )
                })
        }

        // ---- Error/diagnostics ----
        ("error", 0..=3) => {
            let msg = if argv.len() >= 2 {
                argv[1].to_string_value(doc)
            } else {
                "fn:error was called".to_string()
            };
            Err(XmlError::xpath_code("FOER0000", msg))
        }
        ("trace", 2) => Ok(argv[0].clone()),

        _ => {
            // Defer to a caller-supplied external function resolver before
            // raising the unknown-function diagnostic.
            if let Some(value) = ctx.resolver.resolve_function(None, local, argv)? {
                return Ok(value);
            }
            unsupported()
        }
    }
}

fn optional_string(argv: &[XPath2Value], idx: usize, doc: &Document<'_>) -> String {
    argv.get(idx)
        .map(|v| v.to_string_value(doc))
        .unwrap_or_default()
}

fn percent_encode(s: &str, keep: impl Fn(char) -> bool) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if keep(c) {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{:02X}", b));
            }
        }
    }
    out
}

/// `fn:substring` with XPath rounding and 1-based indexing.
fn fn_substring(argv: &[XPath2Value], doc: &Document<'_>) -> XmlResult<XPath2Value> {
    let s: Vec<char> = argv[0].to_string_value(doc).chars().collect();
    let start = single_atomic_or_empty(&argv[1], doc)?
        .map(|a| a.as_f64())
        .transpose()?
        .unwrap_or(f64::NAN);
    // round() per spec; positions are 1-based.
    let start = round_half_up(start);
    let len = if argv.len() == 3 {
        let l = single_atomic_or_empty(&argv[2], doc)?
            .map(|a| a.as_f64())
            .transpose()?
            .unwrap_or(f64::NAN);
        Some(round_half_up(l))
    } else {
        None
    };

    let n = s.len() as f64;
    // The substring covers positions p where start <= p < start+len (1-based).
    let begin = start;
    let end = match len {
        Some(l) => begin + l,
        None => n + 1.0,
    };
    let mut out = String::new();
    for (i, c) in s.iter().enumerate() {
        let pos = (i as f64) + 1.0;
        if pos >= begin && pos < end {
            out.push(*c);
        }
    }
    Ok(str_value(out))
}

fn round_half_up(n: f64) -> f64 {
    if n.is_nan() || n.is_infinite() {
        return n;
    }
    (n + 0.5).floor()
}

fn round_half_even(n: f64, _precision: i32) -> f64 {
    if n.is_nan() || n.is_infinite() {
        return n;
    }
    let rounded = n.round();
    if (n - n.trunc()).abs() == 0.5 {
        // Tie: round to even.
        let lower = n.floor();
        if (lower as i64) % 2 == 0 {
            lower
        } else {
            lower + 1.0
        }
    } else {
        rounded
    }
}

/// Apply a numeric unary operation, preserving the integer type when the input
/// is an integer and the operation is integer-preserving.
fn numeric_unary(
    argv: &[XPath2Value],
    doc: &Document<'_>,
    op_f64: impl Fn(f64) -> f64,
    op_int: impl Fn(i128) -> i128,
) -> XmlResult<XPath2Value> {
    let Some(atomic) = single_atomic_or_empty(&argv[0], doc)? else {
        return Ok(XPath2Value::empty());
    };
    match atomic.base() {
        XPath2AtomicValue::Integer(s) => {
            let i: i128 = s.parse().map_err(|_| {
                XmlError::xpath_code("FORG0001", "invalid integer in numeric function")
            })?;
            Ok(XPath2Value::atomic(XPath2AtomicValue::integer(op_int(i))))
        }
        XPath2AtomicValue::Decimal(_) => {
            let n = atomic.as_f64()?;
            Ok(XPath2Value::atomic(XPath2AtomicValue::decimal(
                format_decimalish(op_f64(n)),
            )))
        }
        XPath2AtomicValue::Float(_) => Ok(XPath2Value::atomic(XPath2AtomicValue::Float(op_f64(
            atomic.as_f64()?,
        )))),
        _ => Ok(XPath2Value::atomic(XPath2AtomicValue::Double(op_f64(
            atomic.as_f64()?,
        )))),
    }
}

fn format_decimalish(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i128)
    } else {
        format!("{}", n)
    }
}

fn fn_sum(argv: &[XPath2Value], doc: &Document<'_>) -> XmlResult<XPath2Value> {
    let atoms = argv[0].atomized(doc);
    if atoms.is_empty() {
        return Ok(match argv.get(1) {
            Some(zero) => zero.clone(),
            None => int_value(0),
        });
    }
    // Integer sum when all are integers; otherwise double.
    if atoms
        .iter()
        .all(|a| matches!(a.base(), XPath2AtomicValue::Integer(_)))
    {
        let mut total: i128 = 0;
        for a in &atoms {
            total = total
                .checked_add(a.as_i128()?)
                .ok_or_else(|| XmlError::xpath_code("FOAR0002", "integer overflow in sum"))?;
        }
        return Ok(int_value(total));
    }
    let mut total = 0.0;
    for a in &atoms {
        total += a.as_f64()?;
    }
    Ok(XPath2Value::atomic(XPath2AtomicValue::Double(total)))
}

fn fn_avg(argv: &[XPath2Value], doc: &Document<'_>) -> XmlResult<XPath2Value> {
    let atoms = argv[0].atomized(doc);
    if atoms.is_empty() {
        return Ok(XPath2Value::empty());
    }
    let mut total = 0.0;
    for a in &atoms {
        total += a.as_f64()?;
    }
    Ok(XPath2Value::atomic(XPath2AtomicValue::Double(
        total / atoms.len() as f64,
    )))
}

fn fn_min_max(argv: &[XPath2Value], doc: &Document<'_>, want_max: bool) -> XmlResult<XPath2Value> {
    let atoms = argv[0].atomized(doc);
    if atoms.is_empty() {
        return Ok(XPath2Value::empty());
    }
    let numeric = atoms.iter().all(|a| a.is_numeric());
    let mut best = atoms[0].clone();
    for a in &atoms[1..] {
        let greater = if numeric {
            a.as_f64()? > best.as_f64()?
        } else {
            functions::codepoint_compare(&a.to_xpath_string(), &best.to_xpath_string())
                == Ordering::Greater
        };
        if greater == want_max {
            best = a.clone();
        }
    }
    Ok(XPath2Value::atomic(best))
}

fn fn_distinct_values(argv: &[XPath2Value], doc: &Document<'_>) -> XmlResult<XPath2Value> {
    let atoms = argv[0].atomized(doc);
    let mut seen: Vec<XPath2AtomicValue> = Vec::new();
    let mut out = Vec::new();
    for a in atoms {
        if !seen.iter().any(|s| functions::atomic_deep_equal(s, &a)) {
            seen.push(a.clone());
            out.push(XPath2Item::Atomic(a));
        }
    }
    Ok(XPath2Value::new(out))
}

fn fn_index_of(argv: &[XPath2Value], doc: &Document<'_>) -> XmlResult<XPath2Value> {
    let haystack = argv[0].atomized(doc);
    let Some(needle) = argv[1].atomized(doc).into_iter().next() else {
        return Ok(XPath2Value::empty());
    };
    let mut out = Vec::new();
    for (i, a) in haystack.iter().enumerate() {
        if functions::atomic_deep_equal(a, &needle) {
            out.push(XPath2Item::Atomic(XPath2AtomicValue::integer(
                (i + 1) as i128,
            )));
        }
    }
    Ok(XPath2Value::new(out))
}

fn fn_insert_before(argv: &[XPath2Value], doc: &Document<'_>) -> XmlResult<XPath2Value> {
    let target = argv[0].items();
    let position = single_atomic_or_empty(&argv[1], doc)?
        .map(|a| a.as_i128())
        .transpose()?
        .unwrap_or(1)
        .max(1);
    let inserts = argv[2].items();
    let idx = ((position - 1) as usize).min(target.len());
    let mut out = Vec::with_capacity(target.len() + inserts.len());
    out.extend_from_slice(&target[..idx]);
    out.extend_from_slice(inserts);
    out.extend_from_slice(&target[idx..]);
    Ok(XPath2Value::new(out))
}

fn fn_subsequence(argv: &[XPath2Value], doc: &Document<'_>) -> XmlResult<XPath2Value> {
    let items = argv[0].items();
    let start = single_atomic_or_empty(&argv[1], doc)?
        .map(|a| a.as_f64())
        .transpose()?
        .unwrap_or(f64::NAN);
    let start = round_half_up(start);
    let end = if argv.len() == 3 {
        let len = single_atomic_or_empty(&argv[2], doc)?
            .map(|a| a.as_f64())
            .transpose()?
            .unwrap_or(0.0);
        start + round_half_up(len)
    } else {
        items.len() as f64 + 1.0
    };
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let pos = (i as f64) + 1.0;
        if pos >= start && pos < end {
            out.push(item.clone());
        }
    }
    Ok(XPath2Value::new(out))
}

/// `fn:deep-equal` over two sequences (atomic + node aware).
fn deep_equal(a: &XPath2Value, b: &XPath2Value, doc: &Document<'_>) -> bool {
    if a.len() != b.len() {
        return false;
    }
    for (x, y) in a.items().iter().zip(b.items()) {
        let equal = match (x, y) {
            (XPath2Item::Atomic(x), XPath2Item::Atomic(y)) => functions::atomic_deep_equal(x, y),
            (XPath2Item::Node(x), XPath2Item::Node(y)) => deep_equal_nodes(*x, *y, doc),
            _ => false,
        };
        if !equal {
            return false;
        }
    }
    true
}

fn deep_equal_nodes(x: NodeId, y: NodeId, doc: &Document<'_>) -> bool {
    if x == y {
        return true;
    }
    match (doc.node_kind(x), doc.node_kind(y)) {
        (Some(NodeKind::Element(ex)), Some(NodeKind::Element(ey))) => {
            if ex.name.local_name != ey.name.local_name
                || ex.name.namespace_uri != ey.name.namespace_uri
            {
                return false;
            }
            // Compare attributes as a set.
            let ax = doc.get_attribute_nodes(x);
            let ay = doc.get_attribute_nodes(y);
            if ax.len() != ay.len() {
                return false;
            }
            // Compare children element/text content recursively in order.
            let cx: Vec<NodeId> = doc
                .children(x)
                .into_iter()
                .filter(|c| significant_for_deep_equal(doc, *c))
                .collect();
            let cy: Vec<NodeId> = doc
                .children(y)
                .into_iter()
                .filter(|c| significant_for_deep_equal(doc, *c))
                .collect();
            if cx.len() != cy.len() {
                return false;
            }
            cx.iter()
                .zip(&cy)
                .all(|(a, b)| deep_equal_nodes(*a, *b, doc))
        }
        (Some(NodeKind::Text(a)), Some(NodeKind::Text(b)))
        | (Some(NodeKind::CData(a)), Some(NodeKind::CData(b))) => a == b,
        _ => {
            crate::xpath2::value::XPath2Item::Node(x).string_value(doc)
                == crate::xpath2::value::XPath2Item::Node(y).string_value(doc)
        }
    }
}

fn significant_for_deep_equal(doc: &Document<'_>, node: NodeId) -> bool {
    matches!(
        doc.node_kind(node),
        Some(NodeKind::Element(_) | NodeKind::Text(_) | NodeKind::CData(_))
    )
}

fn single_node_opt(value: &XPath2Value) -> Option<NodeId> {
    match value.items().first() {
        Some(XPath2Item::Node(node)) => Some(*node),
        _ => None,
    }
}

fn node_namespace_uri(doc: &Document<'_>, node: NodeId) -> Option<String> {
    match doc.node_kind(node) {
        Some(NodeKind::Element(e)) => e.name.namespace_uri.as_ref().map(|s| s.to_string()),
        Some(NodeKind::Attribute(name, _)) => name.namespace_uri.as_ref().map(|s| s.to_string()),
        _ => None,
    }
}

fn node_qname_value(doc: &Document<'_>, node: NodeId) -> Option<QNameValue> {
    match doc.node_kind(node) {
        Some(NodeKind::Element(e)) => Some(QNameValue {
            prefix: e.name.prefix.as_ref().map(|s| s.to_string()),
            uri: e.name.namespace_uri.as_ref().map(|s| s.to_string()),
            local: e.name.local_name.to_string(),
        }),
        Some(NodeKind::Attribute(name, _)) => Some(QNameValue {
            prefix: name.prefix.as_ref().map(|s| s.to_string()),
            uri: name.namespace_uri.as_ref().map(|s| s.to_string()),
            local: name.local_name.to_string(),
        }),
        _ => None,
    }
}

fn fn_node_name_string<R>(
    argv: &[XPath2Value],
    ctx: &DynamicContext<'_, '_, R>,
    full_name: bool,
) -> XmlResult<XPath2Value>
where
    R: XPath2Resolver,
{
    let value = arg_or_context(argv, ctx);
    let Some(node) = single_node_opt(&value) else {
        return Ok(str_value(""));
    };
    match node_qname_value(ctx.doc, node) {
        Some(q) => Ok(str_value(if full_name { q.lexical() } else { q.local })),
        None => Ok(str_value("")),
    }
}

fn fn_lang<R>(argv: &[XPath2Value], ctx: &DynamicContext<'_, '_, R>) -> XmlResult<XPath2Value>
where
    R: XPath2Resolver,
{
    let test = argv[0].to_string_value(ctx.doc).to_lowercase();
    let node = if argv.len() == 2 {
        single_node_opt(&argv[1])
    } else {
        single_node_opt(&XPath2Value::new(vec![ctx.context_item.clone()]))
    };
    let Some(node) = node else {
        return Ok(bool_value(false));
    };
    // Walk self-or-ancestor looking for xml:lang.
    let mut chain = vec![node];
    chain.extend(ctx.doc.ancestors(node));
    for n in chain {
        if let Some(NodeKind::Element(_)) = ctx.doc.node_kind(n) {
            if let Some(lang) = ctx.doc.get_attribute(n, "lang") {
                let lang = lang.to_lowercase();
                return Ok(bool_value(
                    lang == test || lang.starts_with(&format!("{}-", test)),
                ));
            }
        }
    }
    Ok(bool_value(false))
}

fn fn_id<R>(argv: &[XPath2Value], ctx: &DynamicContext<'_, '_, R>) -> XmlResult<XPath2Value>
where
    R: XPath2Resolver,
{
    // Without DTD/schema type information, approximate id() by matching elements
    // that carry an attribute with local-name "id" equal to one of the tokens.
    let mut wanted: Vec<String> = Vec::new();
    for v in argv[0].atomized(ctx.doc) {
        for token in v.to_xpath_string().split_whitespace() {
            wanted.push(token.to_string());
        }
    }
    let root = ctx.doc.root();
    let mut out = Vec::new();
    let mut all = vec![root];
    all.extend(ctx.doc.descendants(root));
    for node in all {
        if let Some(NodeKind::Element(_)) = ctx.doc.node_kind(node) {
            if let Some(id) = ctx.doc.get_attribute(node, "id") {
                if wanted.iter().any(|w| w == id) {
                    out.push(XPath2Item::Node(node));
                }
            }
        }
    }
    Ok(XPath2Value::new(out))
}

fn fn_construct_qname(argv: &[XPath2Value], doc: &Document<'_>) -> XmlResult<XPath2Value> {
    let uri = argv[0].to_string_value(doc);
    let uri = if uri.is_empty() { None } else { Some(uri) };
    let lexical = argv[1].to_string_value(doc);
    let (prefix, local) = match lexical.split_once(':') {
        Some((p, l)) => (Some(p.to_string()), l.to_string()),
        None => (None, lexical),
    };
    Ok(XPath2Value::atomic(XPath2AtomicValue::QName(QNameValue {
        prefix,
        uri,
        local,
    })))
}

enum QNameComponent {
    Local,
    Uri,
    Prefix,
}

fn qname_component(
    argv: &[XPath2Value],
    doc: &Document<'_>,
    component: QNameComponent,
) -> XmlResult<XPath2Value> {
    let Some(atom) = single_atomic_or_empty(&argv[0], doc)? else {
        return Ok(XPath2Value::empty());
    };
    let XPath2AtomicValue::QName(q) = atom.base() else {
        return Err(XmlError::xpath_code(
            "XPTY0004",
            "expected an xs:QName argument",
        ));
    };
    match component {
        QNameComponent::Local => Ok(XPath2Value::atomic(XPath2AtomicValue::Derived(
            AtomicType::NCName,
            Box::new(XPath2AtomicValue::String(q.local.clone())),
        ))),
        QNameComponent::Uri => Ok(XPath2Value::atomic(XPath2AtomicValue::AnyUri(
            q.uri.clone().unwrap_or_default(),
        ))),
        QNameComponent::Prefix => match &q.prefix {
            Some(p) if !p.is_empty() => Ok(XPath2Value::atomic(XPath2AtomicValue::Derived(
                AtomicType::NCName,
                Box::new(XPath2AtomicValue::String(p.clone())),
            ))),
            _ => Ok(XPath2Value::empty()),
        },
    }
}

fn now_datetime<R>(ctx: &DynamicContext<'_, '_, R>) -> DateTimeValue
where
    R: XPath2Resolver,
{
    let unix = ctx.options.current_datetime_unix.unwrap_or_else(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    });
    // Reported in UTC (`Z`); callers wanting a different zone configure the
    // implicit timezone, which is surfaced via fn:implicit-timezone.
    let _ = ctx;
    datetime_from_unix(unix)
}

fn current_datetime<R>(ctx: &DynamicContext<'_, '_, R>) -> XPath2AtomicValue
where
    R: XPath2Resolver,
{
    XPath2AtomicValue::DateTime(now_datetime(ctx))
}

fn current_date<R>(ctx: &DynamicContext<'_, '_, R>) -> XPath2AtomicValue
where
    R: XPath2Resolver,
{
    let mut v = now_datetime(ctx);
    v.hour = 0;
    v.minute = 0;
    v.second = 0.0;
    XPath2AtomicValue::Date(v)
}

fn current_time<R>(ctx: &DynamicContext<'_, '_, R>) -> XPath2AtomicValue
where
    R: XPath2Resolver,
{
    XPath2AtomicValue::Time(now_datetime(ctx))
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

/// In XPath 1.0 compatibility mode, an operand that must be a single value is
/// reduced to its first item rather than raising a type error.
fn compat_first(value: XPath2Value, compat: bool) -> XPath2Value {
    if compat && value.len() > 1 {
        XPath2Value::new(value.into_items().into_iter().take(1).collect())
    } else {
        value
    }
}

fn arithmetic(
    op: BinaryOp,
    left: &XPath2Value,
    right: &XPath2Value,
    doc: &Document<'_>,
    compat: bool,
) -> XmlResult<XPath2Value> {
    let Some(left) = single_atomic_or_empty(left, doc)? else {
        return Ok(XPath2Value::empty());
    };
    let Some(right) = single_atomic_or_empty(right, doc)? else {
        return Ok(XPath2Value::empty());
    };

    match op {
        // XPath 1.0 compatibility casts numeric operands to xs:double, so the
        // integer fast path is bypassed.
        BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Mod
            if !compat
                && matches!(left, XPath2AtomicValue::Integer(_))
                && matches!(right, XPath2AtomicValue::Integer(_)) =>
        {
            let left = left.as_i128()?;
            let right = right.as_i128()?;
            let value = match op {
                BinaryOp::Add => left.checked_add(right),
                BinaryOp::Subtract => left.checked_sub(right),
                BinaryOp::Multiply => left.checked_mul(right),
                BinaryOp::Mod => {
                    if right == 0 {
                        return Err(XmlError::xpath("division by zero in mod"));
                    }
                    left.checked_rem(right)
                }
                _ => unreachable!("integer arithmetic operator constrained by match guard"),
            }
            .ok_or_else(|| XmlError::xpath("integer arithmetic overflow"))?;
            Ok(XPath2Value::atomic(XPath2AtomicValue::integer(value)))
        }
        BinaryOp::Idiv => {
            let right_number = right.as_f64()?;
            if right_number == 0.0 {
                return Err(XmlError::xpath("division by zero in idiv"));
            }
            let quotient = (left.as_f64()? / right_number).trunc();
            // A `f64 -> i128` cast saturates silently; raise the spec overflow
            // error (FOAR0002) instead of returning a wrong i128::MAX/MIN value
            // (see security audit, F7).
            if !quotient.is_finite() || quotient.abs() >= 1.701_411_8e38 {
                return Err(XmlError::xpath_code(
                    "FOAR0002",
                    "integer division result overflows xs:integer",
                ));
            }
            Ok(XPath2Value::atomic(XPath2AtomicValue::integer(
                quotient as i128,
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

    // Temporal types compare on their position on the UTC timeline so that
    // values written with different timezones order correctly.
    if let (Some(a), Some(b)) = (temporal_seconds(left), temporal_seconds(right)) {
        return Ok(ordered_compare(op, a, b));
    }
    // Durations compare on their canonical magnitude (months, then seconds).
    if let (XPath2AtomicValue::Duration(a, _), XPath2AtomicValue::Duration(b, _)) =
        (left.base(), right.base())
    {
        let am = a.months as f64 * 2_592_000.0 + a.seconds;
        let bm = b.months as f64 * 2_592_000.0 + b.seconds;
        return Ok(ordered_compare(op, am, bm));
    }

    match (left.base(), right.base()) {
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

/// The UTC-timeline position (in seconds) of a date/time-family value, used for
/// type-correct comparison. Returns `None` for non-temporal values.
fn temporal_seconds(value: &XPath2AtomicValue) -> Option<f64> {
    match value.base() {
        XPath2AtomicValue::DateTime(v)
        | XPath2AtomicValue::Date(v)
        | XPath2AtomicValue::Time(v)
        | XPath2AtomicValue::Gregorian(v, _) => Some(v.timeline_seconds()),
        _ => None,
    }
}

fn ordered_compare(op: BinaryOp, a: f64, b: f64) -> bool {
    match op {
        BinaryOp::GeneralEq | BinaryOp::ValueEq => a == b,
        BinaryOp::GeneralNe | BinaryOp::ValueNe => a != b,
        BinaryOp::GeneralLt | BinaryOp::ValueLt => a < b,
        BinaryOp::GeneralLe | BinaryOp::ValueLe => a <= b,
        BinaryOp::GeneralGt | BinaryOp::ValueGt => a > b,
        BinaryOp::GeneralGe | BinaryOp::ValueGe => a >= b,
        _ => false,
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
    let nodes = match op {
        BinaryOp::Union => union_sorted_nodes(&left, &right),
        BinaryOp::Intersect => intersect_sorted_nodes(&left, &right),
        BinaryOp::Except => except_sorted_nodes(&left, &right),
        _ => unreachable!("node set operator constrained by caller"),
    };
    Ok(XPath2Value::new(
        nodes.into_iter().map(XPath2Item::Node).collect(),
    ))
}

fn union_sorted_nodes(left: &[NodeId], right: &[NodeId]) -> Vec<NodeId> {
    let mut nodes = Vec::with_capacity(left.len().saturating_add(right.len()));
    let mut left_pos = 0;
    let mut right_pos = 0;
    while left_pos < left.len() && right_pos < right.len() {
        match left[left_pos].index().cmp(&right[right_pos].index()) {
            std::cmp::Ordering::Less => {
                nodes.push(left[left_pos]);
                left_pos += 1;
            }
            std::cmp::Ordering::Greater => {
                nodes.push(right[right_pos]);
                right_pos += 1;
            }
            std::cmp::Ordering::Equal => {
                nodes.push(left[left_pos]);
                left_pos += 1;
                right_pos += 1;
            }
        }
    }
    nodes.extend_from_slice(&left[left_pos..]);
    nodes.extend_from_slice(&right[right_pos..]);
    nodes
}

fn intersect_sorted_nodes(left: &[NodeId], right: &[NodeId]) -> Vec<NodeId> {
    let mut nodes = Vec::new();
    let mut left_pos = 0;
    let mut right_pos = 0;
    while left_pos < left.len() && right_pos < right.len() {
        match left[left_pos].index().cmp(&right[right_pos].index()) {
            std::cmp::Ordering::Less => left_pos += 1,
            std::cmp::Ordering::Greater => right_pos += 1,
            std::cmp::Ordering::Equal => {
                nodes.push(left[left_pos]);
                left_pos += 1;
                right_pos += 1;
            }
        }
    }
    nodes
}

fn except_sorted_nodes(left: &[NodeId], right: &[NodeId]) -> Vec<NodeId> {
    let mut nodes = Vec::new();
    let mut left_pos = 0;
    let mut right_pos = 0;
    while left_pos < left.len() {
        if right_pos >= right.len() {
            nodes.extend_from_slice(&left[left_pos..]);
            break;
        }
        match left[left_pos].index().cmp(&right[right_pos].index()) {
            std::cmp::Ordering::Less => {
                nodes.push(left[left_pos]);
                left_pos += 1;
            }
            std::cmp::Ordering::Greater => right_pos += 1,
            std::cmp::Ordering::Equal => {
                left_pos += 1;
                right_pos += 1;
            }
        }
    }
    nodes
}

fn dedup_document_order(mut nodes: Vec<NodeId>) -> Vec<NodeId> {
    nodes.sort_by_key(NodeId::index);
    nodes.dedup();
    nodes
}
