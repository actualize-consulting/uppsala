//! XPath 2.0 abstract syntax tree.

use std::borrow::Cow;
use std::fmt;

/// A parsed XPath 2.0 expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr<'expr> {
    /// `()`
    EmptySequence,
    /// `ExprSingle (, ExprSingle)*`
    Sequence(Vec<Expr<'expr>>),
    /// Literal atomic value.
    Literal(Literal<'expr>),
    /// `$name`
    VarRef(QName<'expr>),
    /// `.`
    ContextItem,
    /// Function call.
    FunctionCall {
        /// Function name.
        name: QName<'expr>,
        /// Function arguments.
        args: Vec<Expr<'expr>>,
    },
    /// Unary `+` or `-`.
    Unary {
        /// Operator.
        op: UnaryOp,
        /// Operand.
        expr: Box<Expr<'expr>>,
    },
    /// Binary expression.
    Binary {
        /// Operator.
        op: BinaryOp,
        /// Left operand.
        left: Box<Expr<'expr>>,
        /// Right operand.
        right: Box<Expr<'expr>>,
    },
    /// `if (test) then a else b`
    If {
        /// Test expression.
        test: Box<Expr<'expr>>,
        /// Then branch.
        then_branch: Box<Expr<'expr>>,
        /// Else branch.
        else_branch: Box<Expr<'expr>>,
    },
    /// `for $x in seq return body`
    For {
        /// Variable bindings.
        bindings: Vec<ForBinding<'expr>>,
        /// Return expression.
        body: Box<Expr<'expr>>,
    },
    /// `some/every $x in seq satisfies test`
    Quantified {
        /// Quantifier kind.
        quantifier: Quantifier,
        /// Variable bindings.
        bindings: Vec<ForBinding<'expr>>,
        /// Test expression.
        satisfies: Box<Expr<'expr>>,
    },
    /// `expr instance of SequenceType`
    InstanceOf {
        /// Operand expression.
        expr: Box<Expr<'expr>>,
        /// Tested sequence type.
        seq_type: SequenceType<'expr>,
    },
    /// `expr treat as SequenceType`
    TreatAs {
        /// Operand expression.
        expr: Box<Expr<'expr>>,
        /// Asserted sequence type.
        seq_type: SequenceType<'expr>,
    },
    /// `expr castable as SingleType`
    Castable {
        /// Operand expression.
        expr: Box<Expr<'expr>>,
        /// Target single type.
        single_type: SingleType<'expr>,
    },
    /// `expr cast as SingleType`
    Cast {
        /// Operand expression.
        expr: Box<Expr<'expr>>,
        /// Target single type.
        single_type: SingleType<'expr>,
    },
    /// XPath path expression.
    Path(PathExpr<'expr>),
}

/// Occurrence indicator on a sequence type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Occurrence {
    /// Exactly one (no indicator).
    One,
    /// `?` — zero or one.
    ZeroOrOne,
    /// `*` — zero or more.
    ZeroOrMore,
    /// `+` — one or more.
    OneOrMore,
}

/// A `SequenceType`: an item type plus an occurrence indicator, or the special
/// `empty-sequence()`.
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceType<'expr> {
    /// `None` represents `empty-sequence()`.
    pub item: Option<ItemType<'expr>>,
    /// Occurrence indicator.
    pub occurrence: Occurrence,
}

/// An `ItemType` used by sequence types.
#[derive(Debug, Clone, PartialEq)]
pub enum ItemType<'expr> {
    /// `item()` — any single item.
    Item,
    /// A named atomic type, e.g. `xs:integer`.
    Atomic(QName<'expr>),
    /// A node kind test, e.g. `element()` or `text()`.
    Kind(NodeTest<'expr>),
}

/// A `SingleType` used by `cast as` / `castable as`: an atomic type name with
/// an optional `?` permitting the empty sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct SingleType<'expr> {
    /// Atomic type name.
    pub type_name: QName<'expr>,
    /// Whether a trailing `?` allows the empty sequence.
    pub optional: bool,
}

/// XPath literal forms.
#[derive(Debug, Clone, PartialEq)]
pub enum Literal<'expr> {
    /// `StringLiteral`
    String(Cow<'expr, str>),
    /// `IntegerLiteral`
    Integer(&'expr str),
    /// `DecimalLiteral`
    Decimal(&'expr str),
    /// `DoubleLiteral`
    Double(&'expr str),
}

/// Qualified name in lexical form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct QName<'expr> {
    /// Optional namespace prefix.
    pub prefix: Option<&'expr str>,
    /// Local name.
    pub local: &'expr str,
}

impl<'expr> QName<'expr> {
    /// Create an unprefixed QName.
    pub fn local(local: &'expr str) -> Self {
        Self {
            prefix: None,
            local,
        }
    }

    /// Return a stable lexical key for dynamic variable/function lookup.
    pub fn lexical_key(&self) -> String {
        match self.prefix {
            Some(prefix) => format!("{}:{}", prefix, self.local),
            None => self.local.to_string(),
        }
    }
}

impl fmt::Display for QName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.prefix {
            Some(prefix) => write!(f, "{}:{}", prefix, self.local),
            None => f.write_str(self.local),
        }
    }
}

/// A `for` or quantified expression binding.
#[derive(Debug, Clone, PartialEq)]
pub struct ForBinding<'expr> {
    /// Bound variable name.
    pub name: QName<'expr>,
    /// Input sequence expression.
    pub in_expr: Expr<'expr>,
}

/// XPath unary operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    /// Unary plus.
    Plus,
    /// Unary minus.
    Minus,
}

/// XPath binary operators implemented by the first XPath 2.0 slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Or,
    And,
    GeneralEq,
    GeneralNe,
    GeneralLt,
    GeneralLe,
    GeneralGt,
    GeneralGe,
    ValueEq,
    ValueNe,
    ValueLt,
    ValueLe,
    ValueGt,
    ValueGe,
    NodeIs,
    NodeBefore,
    NodeAfter,
    RangeTo,
    Add,
    Subtract,
    Multiply,
    Div,
    Idiv,
    Mod,
    Union,
    Intersect,
    Except,
}

/// `some` or `every`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantifier {
    Some,
    Every,
}

/// Path expression with a starting point and ordered path steps.
#[derive(Debug, Clone, PartialEq)]
pub struct PathExpr<'expr> {
    /// Whether this path starts at the document root.
    pub absolute: bool,
    /// Whether this path begins with `//`.
    pub descendant_start: bool,
    /// Optional non-root start expression, such as `.` in `./a`.
    pub start: Option<Box<Expr<'expr>>>,
    /// Relative steps.
    pub steps: Vec<PathStep<'expr>>,
}

/// One XPath path step.
#[derive(Debug, Clone, PartialEq)]
pub struct PathStep<'expr> {
    /// Step axis.
    pub axis: Axis,
    /// Node test.
    pub test: NodeTest<'expr>,
    /// Predicates attached to this step.
    pub predicates: Vec<Expr<'expr>>,
}

impl<'expr> PathStep<'expr> {
    pub(crate) fn descendant_or_self_node() -> Self {
        Self {
            axis: Axis::DescendantOrSelf,
            test: NodeTest::Node,
            predicates: Vec::new(),
        }
    }
}

/// XPath axis subset currently evaluated by `xpath2`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Child,
    Descendant,
    Attribute,
    Self_,
    DescendantOrSelf,
    Parent,
    Ancestor,
    AncestorOrSelf,
    FollowingSibling,
    Following,
    Namespace,
    PrecedingSibling,
    Preceding,
}

/// XPath node tests, covering both the name-test forms and the full XPath 2.0
/// `KindTest` grammar.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeTest<'expr> {
    Name(QName<'expr>),
    Any,
    PrefixWildcard(&'expr str),
    LocalNameWildcard(&'expr str),
    Text,
    Node,
    Comment,
    ProcessingInstruction(Option<Cow<'expr, str>>),
    /// `document-node()` or `document-node(element(...))`.
    Document(Option<Box<NodeTest<'expr>>>),
    /// `element()`, `element(name)`, `element(name, type)`, `element(*, type)`.
    Element(Option<QName<'expr>>, Option<QName<'expr>>),
    /// `attribute()`, `attribute(name)`, `attribute(name, type)`.
    Attribute(Option<QName<'expr>>, Option<QName<'expr>>),
    /// `schema-element(name)`.
    SchemaElement(QName<'expr>),
    /// `schema-attribute(name)`.
    SchemaAttribute(QName<'expr>),
}
