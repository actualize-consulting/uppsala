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
    /// XPath path expression.
    Path(PathExpr<'expr>),
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

/// XPath node tests.
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
}
