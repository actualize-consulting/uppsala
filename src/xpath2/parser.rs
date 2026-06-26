//! Recursive-descent parser for the initial XPath 2.0 grammar slice.

use crate::error::{XmlError, XmlResult};

use super::ast::{
    Axis, BinaryOp, Expr, ForBinding, ItemType, Literal, NodeTest, Occurrence, PathExpr, PathStep,
    QName, Quantifier, SequenceType, SingleType, UnaryOp,
};
use super::lexer::{tokenize, Token, TokenKind};

/// Default expression-nesting limit for XPath 2.0 parsing.
pub const DEFAULT_MAX_XPATH2_DEPTH: u32 = 256;

/// Parse a complete XPath 2.0 expression.
pub fn parse_expression<'expr>(source: &'expr str, max_depth: u32) -> XmlResult<Expr<'expr>> {
    let tokens = tokenize(source)?;
    XPath2Parser::new(&tokens, max_depth).parse_complete()
}

struct XPath2Parser<'tokens, 'expr> {
    tokens: &'tokens [Token<'expr>],
    position: usize,
    depth: u32,
    max_depth: u32,
    nodes: usize,
    max_nodes: usize,
}

impl<'tokens, 'expr> XPath2Parser<'tokens, 'expr> {
    fn new(tokens: &'tokens [Token<'expr>], max_depth: u32) -> Self {
        Self {
            tokens,
            position: 0,
            depth: 0,
            max_depth,
            nodes: 0,
            // Cap total AST nodes. Left-associative operator chains
            // (`1 or 1 or …`) are parsed iteratively, so the `max_depth`
            // nesting guard does not bound them; an unbounded chain builds a
            // deep AST that overflows the stack when evaluated *or dropped*
            // (recursive `Box<Expr>` destructor). This monotonic cap bounds the
            // whole tree. The multiplier keeps it generous for real expressions
            // while staying well below any stack-overflow threshold.
            max_nodes: (max_depth as usize).saturating_mul(64).max(1024),
        }
    }

    /// Count one AST node, failing closed when the tree grows too large.
    fn charge_node(&mut self) -> XmlResult<()> {
        self.nodes += 1;
        if self.nodes > self.max_nodes {
            return Err(XmlError::xpath(format!(
                "XPath 2.0 expression exceeds maximum of {} nodes",
                self.max_nodes
            )));
        }
        Ok(())
    }

    fn parse_complete(mut self) -> XmlResult<Expr<'expr>> {
        let expr = self.parse_expr()?;
        if let Some(token) = self.peek() {
            return Err(XmlError::xpath(format!(
                "unexpected token {:?} after XPath expression",
                token.kind
            )));
        }
        Ok(expr)
    }

    fn parse_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let first = self.parse_expr_single()?;
        if !self.consume_comma() {
            return Ok(first);
        }

        let mut items = vec![first];
        loop {
            items.push(self.parse_expr_single()?);
            if !self.consume_comma() {
                break;
            }
        }
        Ok(Expr::Sequence(items))
    }

    fn parse_expr_single(&mut self) -> XmlResult<Expr<'expr>> {
        if self.peek_name("for") {
            self.parse_for_expr()
        } else if self.peek_name("some") || self.peek_name("every") {
            self.parse_quantified_expr()
        } else if self.peek_name("if") {
            self.parse_if_expr()
        } else {
            self.parse_or_expr()
        }
    }

    fn parse_for_expr(&mut self) -> XmlResult<Expr<'expr>> {
        self.expect_name("for")?;
        let bindings = self.parse_bindings_until("return")?;
        self.expect_name("return")?;
        let body = self.parse_nested_expr_single()?;
        Ok(Expr::For {
            bindings,
            body: Box::new(body),
        })
    }

    fn parse_quantified_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let quantifier = if self.consume_name("some") {
            Quantifier::Some
        } else {
            self.expect_name("every")?;
            Quantifier::Every
        };
        let bindings = self.parse_bindings_until("satisfies")?;
        self.expect_name("satisfies")?;
        let satisfies = self.parse_nested_expr_single()?;
        Ok(Expr::Quantified {
            quantifier,
            bindings,
            satisfies: Box::new(satisfies),
        })
    }

    fn parse_bindings_until(&mut self, terminator: &str) -> XmlResult<Vec<ForBinding<'expr>>> {
        let mut bindings = Vec::new();
        loop {
            self.expect_token(TokenDiscriminant::Dollar)?;
            let name = self.parse_qname()?;
            self.expect_name("in")?;
            let in_expr = self.parse_nested_expr_single()?;
            bindings.push(ForBinding { name, in_expr });

            if self.peek_name(terminator) {
                break;
            }
            self.expect_token(TokenDiscriminant::Comma)?;
        }
        Ok(bindings)
    }

    fn parse_if_expr(&mut self) -> XmlResult<Expr<'expr>> {
        self.expect_name("if")?;
        self.expect_token(TokenDiscriminant::LeftParen)?;
        let test = if self.consume_token(TokenDiscriminant::RightParen) {
            Expr::EmptySequence
        } else {
            let expr = self.parse_nested_expr()?;
            self.expect_token(TokenDiscriminant::RightParen)?;
            expr
        };
        self.expect_name("then")?;
        let then_branch = self.parse_nested_expr_single()?;
        self.expect_name("else")?;
        let else_branch = self.parse_nested_expr_single()?;
        Ok(Expr::If {
            test: Box::new(test),
            then_branch: Box::new(then_branch),
            else_branch: Box::new(else_branch),
        })
    }

    fn parse_or_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let mut expr = self.parse_and_expr()?;
        while self.consume_name("or") {
            self.charge_node()?;
            expr = Expr::Binary {
                op: BinaryOp::Or,
                left: Box::new(expr),
                right: Box::new(self.parse_and_expr()?),
            };
        }
        Ok(expr)
    }

    fn parse_and_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let mut expr = self.parse_comparison_expr()?;
        while self.consume_name("and") {
            self.charge_node()?;
            expr = Expr::Binary {
                op: BinaryOp::And,
                left: Box::new(expr),
                right: Box::new(self.parse_comparison_expr()?),
            };
        }
        Ok(expr)
    }

    fn parse_comparison_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let left = self.parse_range_expr()?;
        let Some(op) = self.parse_comparison_operator() else {
            return Ok(left);
        };
        self.charge_node()?;
        Ok(Expr::Binary {
            op,
            left: Box::new(left),
            right: Box::new(self.parse_range_expr()?),
        })
    }

    fn parse_range_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let left = self.parse_additive_expr()?;
        if self.consume_name("to") {
            self.charge_node()?;
            return Ok(Expr::Binary {
                op: BinaryOp::RangeTo,
                left: Box::new(left),
                right: Box::new(self.parse_additive_expr()?),
            });
        }
        Ok(left)
    }

    fn parse_additive_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let mut expr = self.parse_multiplicative_expr()?;
        loop {
            let op = if self.consume_token(TokenDiscriminant::Plus) {
                Some(BinaryOp::Add)
            } else if self.consume_token(TokenDiscriminant::Minus) {
                Some(BinaryOp::Subtract)
            } else {
                None
            };
            let Some(op) = op else {
                break;
            };
            self.charge_node()?;
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(self.parse_multiplicative_expr()?),
            };
        }
        Ok(expr)
    }

    fn parse_multiplicative_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let mut expr = self.parse_union_expr()?;
        loop {
            let op = if self.consume_token(TokenDiscriminant::Star) {
                Some(BinaryOp::Multiply)
            } else if self.consume_name("div") {
                Some(BinaryOp::Div)
            } else if self.consume_name("idiv") {
                Some(BinaryOp::Idiv)
            } else if self.consume_name("mod") {
                Some(BinaryOp::Mod)
            } else {
                None
            };
            let Some(op) = op else {
                break;
            };
            self.charge_node()?;
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(self.parse_union_expr()?),
            };
        }
        Ok(expr)
    }

    fn parse_union_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let mut expr = self.parse_intersect_except_expr()?;
        while self.consume_token(TokenDiscriminant::Pipe) || self.consume_name("union") {
            self.charge_node()?;
            expr = Expr::Binary {
                op: BinaryOp::Union,
                left: Box::new(expr),
                right: Box::new(self.parse_intersect_except_expr()?),
            };
        }
        Ok(expr)
    }

    fn parse_intersect_except_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let mut expr = self.parse_instanceof_expr()?;
        loop {
            let op = if self.consume_name("intersect") {
                Some(BinaryOp::Intersect)
            } else if self.consume_name("except") {
                Some(BinaryOp::Except)
            } else {
                None
            };
            let Some(op) = op else {
                break;
            };
            self.charge_node()?;
            expr = Expr::Binary {
                op,
                left: Box::new(expr),
                right: Box::new(self.parse_instanceof_expr()?),
            };
        }
        Ok(expr)
    }

    fn parse_instanceof_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let expr = self.parse_treat_expr()?;
        if self.peek_name("instance") && self.peek_n_name(1, "of") {
            self.advance();
            self.advance();
            self.charge_node()?;
            let seq_type = self.parse_sequence_type()?;
            return Ok(Expr::InstanceOf {
                expr: Box::new(expr),
                seq_type,
            });
        }
        Ok(expr)
    }

    fn parse_treat_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let expr = self.parse_castable_expr()?;
        if self.peek_name("treat") && self.peek_n_name(1, "as") {
            self.advance();
            self.advance();
            self.charge_node()?;
            let seq_type = self.parse_sequence_type()?;
            return Ok(Expr::TreatAs {
                expr: Box::new(expr),
                seq_type,
            });
        }
        Ok(expr)
    }

    fn parse_castable_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let expr = self.parse_cast_expr()?;
        if self.peek_name("castable") && self.peek_n_name(1, "as") {
            self.advance();
            self.advance();
            self.charge_node()?;
            let single_type = self.parse_single_type()?;
            return Ok(Expr::Castable {
                expr: Box::new(expr),
                single_type,
            });
        }
        Ok(expr)
    }

    fn parse_cast_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let expr = self.parse_unary_expr()?;
        if self.peek_name("cast") && self.peek_n_name(1, "as") {
            self.advance();
            self.advance();
            self.charge_node()?;
            let single_type = self.parse_single_type()?;
            return Ok(Expr::Cast {
                expr: Box::new(expr),
                single_type,
            });
        }
        Ok(expr)
    }

    /// Parse `SingleType ::= AtomicType "?"?`.
    fn parse_single_type(&mut self) -> XmlResult<SingleType<'expr>> {
        let type_name = self.parse_qname()?;
        let optional = self.consume_token(TokenDiscriminant::Question);
        Ok(SingleType {
            type_name,
            optional,
        })
    }

    /// Parse `SequenceType ::= ("empty-sequence" "(" ")") | (ItemType OccurrenceIndicator?)`.
    fn parse_sequence_type(&mut self) -> XmlResult<SequenceType<'expr>> {
        if self.peek_name("empty-sequence")
            && matches!(self.peek_n(1).map(|t| &t.kind), Some(TokenKind::LeftParen))
        {
            self.advance();
            self.expect_token(TokenDiscriminant::LeftParen)?;
            self.expect_token(TokenDiscriminant::RightParen)?;
            return Ok(SequenceType {
                item: None,
                occurrence: Occurrence::One,
            });
        }

        let item = self.parse_item_type()?;
        let occurrence = if self.consume_token(TokenDiscriminant::Question) {
            Occurrence::ZeroOrOne
        } else if self.consume_token(TokenDiscriminant::Star) {
            Occurrence::ZeroOrMore
        } else if self.consume_token(TokenDiscriminant::Plus) {
            Occurrence::OneOrMore
        } else {
            Occurrence::One
        };
        Ok(SequenceType {
            item: Some(item),
            occurrence,
        })
    }

    /// Parse `ItemType ::= KindTest | ("item" "(" ")") | AtomicType`.
    fn parse_item_type(&mut self) -> XmlResult<ItemType<'expr>> {
        if let Some(name) = self.peek_any_name() {
            let is_paren = matches!(self.peek_n(1).map(|t| &t.kind), Some(TokenKind::LeftParen));
            if name == "item" && is_paren {
                self.advance();
                self.expect_token(TokenDiscriminant::LeftParen)?;
                self.expect_token(TokenDiscriminant::RightParen)?;
                return Ok(ItemType::Item);
            }
            if is_paren && is_extended_kind_test_name(name) {
                return Ok(ItemType::Kind(self.parse_kind_test()?));
            }
        }
        Ok(ItemType::Atomic(self.parse_qname()?))
    }

    fn parse_unary_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let mut ops = Vec::new();
        loop {
            if self.consume_token(TokenDiscriminant::Plus) {
                ops.push(UnaryOp::Plus);
            } else if self.consume_token(TokenDiscriminant::Minus) {
                ops.push(UnaryOp::Minus);
            } else {
                break;
            }
        }

        let mut expr = self.parse_path_expr()?;
        for op in ops.into_iter().rev() {
            self.charge_node()?;
            expr = Expr::Unary {
                op,
                expr: Box::new(expr),
            };
        }
        Ok(expr)
    }

    fn parse_path_expr(&mut self) -> XmlResult<Expr<'expr>> {
        if self.consume_token(TokenDiscriminant::Slash) {
            let steps = if self.path_can_continue() {
                self.parse_relative_path(false)?
            } else {
                Vec::new()
            };
            return Ok(Expr::Path(PathExpr {
                absolute: true,
                descendant_start: false,
                start: None,
                steps,
            }));
        }

        if self.consume_token(TokenDiscriminant::DoubleSlash) {
            let mut steps = vec![PathStep::descendant_or_self_node()];
            if self.path_can_continue() {
                steps.extend(self.parse_relative_path(false)?);
            }
            return Ok(Expr::Path(PathExpr {
                absolute: true,
                descendant_start: true,
                start: None,
                steps,
            }));
        }

        if self.can_start_axis_step() {
            let steps = self.parse_relative_path(false)?;
            return Ok(Expr::Path(PathExpr {
                absolute: false,
                descendant_start: false,
                start: None,
                steps,
            }));
        }

        let start = self.parse_filter_expr()?;
        if self.consume_token(TokenDiscriminant::Slash)
            || self.consume_token(TokenDiscriminant::DoubleSlash)
        {
            let was_double_slash = matches!(
                self.tokens
                    .get(self.position.saturating_sub(1))
                    .map(|token| &token.kind),
                Some(TokenKind::DoubleSlash)
            );
            let mut steps = Vec::new();
            if was_double_slash {
                steps.push(PathStep::descendant_or_self_node());
            }
            steps.extend(self.parse_relative_path(true)?);
            return Ok(Expr::Path(PathExpr {
                absolute: false,
                descendant_start: false,
                start: Some(Box::new(start)),
                steps,
            }));
        }
        Ok(start)
    }

    fn parse_relative_path(
        &mut self,
        separator_already_consumed: bool,
    ) -> XmlResult<Vec<PathStep<'expr>>> {
        let mut steps = Vec::new();
        if separator_already_consumed && !self.path_can_continue() {
            return Err(XmlError::xpath("expected path step after path separator"));
        }
        steps.push(self.parse_axis_step()?);

        loop {
            if self.consume_token(TokenDiscriminant::Slash) {
                steps.push(self.parse_axis_step()?);
            } else if self.consume_token(TokenDiscriminant::DoubleSlash) {
                steps.push(PathStep::descendant_or_self_node());
                steps.push(self.parse_axis_step()?);
            } else {
                break;
            }
        }
        Ok(steps)
    }

    fn parse_axis_step(&mut self) -> XmlResult<PathStep<'expr>> {
        if self.consume_token(TokenDiscriminant::DoubleDot) {
            return Ok(PathStep {
                axis: Axis::Parent,
                test: NodeTest::Node,
                predicates: self.parse_predicates()?,
            });
        }

        let axis = if self.consume_token(TokenDiscriminant::At) {
            Axis::Attribute
        } else if self.peek_axis_name() {
            let axis_name = self.expect_any_name()?;
            self.expect_token(TokenDiscriminant::ColonColon)?;
            parse_axis(axis_name)?
        } else {
            Axis::Child
        };

        let test = self.parse_node_test()?;
        let predicates = self.parse_predicates()?;
        Ok(PathStep {
            axis,
            test,
            predicates,
        })
    }

    fn parse_node_test(&mut self) -> XmlResult<NodeTest<'expr>> {
        if self.consume_token(TokenDiscriminant::Star) {
            if self.consume_token(TokenDiscriminant::Colon) {
                let local = self.expect_any_name()?;
                return Ok(NodeTest::LocalNameWildcard(local));
            }
            return Ok(NodeTest::Any);
        }

        if let Some(name) = self.peek_any_name() {
            if self.is_kind_test_name(name) {
                return self.parse_kind_test();
            }

            if matches!(
                (
                    self.peek_n(1).map(|token| &token.kind),
                    self.peek_n(2).map(|token| &token.kind)
                ),
                (Some(TokenKind::Colon), Some(TokenKind::Star))
            ) {
                let prefix = self.expect_any_name()?;
                self.expect_token(TokenDiscriminant::Colon)?;
                self.expect_token(TokenDiscriminant::Star)?;
                return Ok(NodeTest::PrefixWildcard(prefix));
            }
        }

        Ok(NodeTest::Name(self.parse_qname()?))
    }

    /// Parse a full XPath 2.0 `KindTest`. The caller has verified the upcoming
    /// token is a kind-test name immediately followed by `(`.
    fn parse_kind_test(&mut self) -> XmlResult<NodeTest<'expr>> {
        let name = self.expect_any_name()?;
        self.expect_token(TokenDiscriminant::LeftParen)?;
        let test = match name {
            "text" => NodeTest::Text,
            "node" => NodeTest::Node,
            "comment" => NodeTest::Comment,
            "processing-instruction" => {
                let target = match self.peek().map(|token| &token.kind) {
                    Some(TokenKind::StringLiteral(value)) => {
                        let value = value.clone();
                        self.advance();
                        Some(value)
                    }
                    Some(TokenKind::Name(value)) => {
                        let value = *value;
                        self.advance();
                        Some(std::borrow::Cow::Borrowed(value))
                    }
                    _ => None,
                };
                NodeTest::ProcessingInstruction(target)
            }
            "document-node" => {
                let inner = if matches!(self.peek().map(|t| &t.kind), Some(TokenKind::RightParen)) {
                    None
                } else {
                    // `document-node(document-node(...))` nests recursively; route
                    // through the depth guard so a deeply nested kind test fails
                    // closed instead of overflowing the stack (see security audit).
                    Some(Box::new(self.with_depth(|p| p.parse_kind_test())?))
                };
                NodeTest::Document(inner)
            }
            "element" => {
                let (name, ty) = self.parse_element_attribute_args()?;
                NodeTest::Element(name, ty)
            }
            "attribute" => {
                let (name, ty) = self.parse_element_attribute_args()?;
                NodeTest::Attribute(name, ty)
            }
            "schema-element" => {
                let name = self.parse_qname()?;
                NodeTest::SchemaElement(name)
            }
            "schema-attribute" => {
                let name = self.parse_qname()?;
                NodeTest::SchemaAttribute(name)
            }
            // Reachable via a recursive inner test such as
            // `document-node(unknown())`: the recursion at the `document-node`
            // arm above does not pre-check `is_kind_test_name`, so an invalid
            // inner test name lands here. A parser must never panic on input, so
            // fail closed with an error instead of `unreachable!` (see security
            // audit / QT3 conformance).
            other => {
                return Err(XmlError::xpath(format!("unsupported kind test '{other}'")));
            }
        };
        self.expect_token(TokenDiscriminant::RightParen)?;
        Ok(test)
    }

    /// Parse the optional `(NameOrWildcard (, TypeName "?"?)?)` arguments of an
    /// `element(...)` or `attribute(...)` test.
    fn parse_element_attribute_args(
        &mut self,
    ) -> XmlResult<(Option<QName<'expr>>, Option<QName<'expr>>)> {
        if matches!(self.peek().map(|t| &t.kind), Some(TokenKind::RightParen)) {
            return Ok((None, None));
        }
        let name = if self.consume_token(TokenDiscriminant::Star) {
            None
        } else {
            Some(self.parse_qname()?)
        };
        let ty = if self.consume_comma() {
            let ty = self.parse_qname()?;
            // Tolerate the nillable `?` marker that follows a type name.
            self.consume_token(TokenDiscriminant::Question);
            Some(ty)
        } else {
            None
        };
        Ok((name, ty))
    }

    fn parse_predicates(&mut self) -> XmlResult<Vec<Expr<'expr>>> {
        let mut predicates = Vec::new();
        while self.consume_token(TokenDiscriminant::LeftBracket) {
            let expr = self.parse_nested_expr()?;
            self.expect_token(TokenDiscriminant::RightBracket)?;
            predicates.push(expr);
        }
        Ok(predicates)
    }

    fn parse_filter_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let primary = self.parse_primary_expr()?;
        let predicates = self.parse_predicates()?;
        if predicates.is_empty() {
            return Ok(primary);
        }

        Ok(Expr::Path(PathExpr {
            absolute: false,
            descendant_start: false,
            start: Some(Box::new(primary)),
            steps: predicates
                .into_iter()
                .map(|predicate| PathStep {
                    axis: Axis::Self_,
                    test: NodeTest::Node,
                    predicates: vec![predicate],
                })
                .collect(),
        }))
    }

    fn parse_primary_expr(&mut self) -> XmlResult<Expr<'expr>> {
        let Some(token) = self.peek() else {
            return Err(XmlError::xpath("expected XPath expression"));
        };

        match &token.kind {
            TokenKind::StringLiteral(value) => {
                let value = value.clone();
                self.advance();
                Ok(Expr::Literal(Literal::String(value)))
            }
            TokenKind::IntegerLiteral(value) => {
                let value = *value;
                self.advance();
                Ok(Expr::Literal(Literal::Integer(value)))
            }
            TokenKind::DecimalLiteral(value) => {
                let value = *value;
                self.advance();
                Ok(Expr::Literal(Literal::Decimal(value)))
            }
            TokenKind::DoubleLiteral(value) => {
                let value = *value;
                self.advance();
                Ok(Expr::Literal(Literal::Double(value)))
            }
            TokenKind::Dollar => {
                self.advance();
                Ok(Expr::VarRef(self.parse_qname()?))
            }
            TokenKind::Dot => {
                self.advance();
                Ok(Expr::ContextItem)
            }
            TokenKind::LeftParen => {
                self.advance();
                if self.consume_token(TokenDiscriminant::RightParen) {
                    return Ok(Expr::EmptySequence);
                }
                let expr = self.parse_nested_expr()?;
                self.expect_token(TokenDiscriminant::RightParen)?;
                Ok(expr)
            }
            TokenKind::Name(_) if self.lookahead_is_function_call() => self.parse_function_call(),
            _ => Err(XmlError::xpath(format!(
                "unexpected token {:?} in XPath expression",
                token.kind
            ))),
        }
    }

    fn parse_function_call(&mut self) -> XmlResult<Expr<'expr>> {
        let name = self.parse_qname()?;
        self.expect_token(TokenDiscriminant::LeftParen)?;
        let mut args = Vec::new();
        if !self.consume_token(TokenDiscriminant::RightParen) {
            loop {
                args.push(self.parse_nested_expr_single()?);
                if !self.consume_comma() {
                    break;
                }
            }
            self.expect_token(TokenDiscriminant::RightParen)?;
        }
        Ok(Expr::FunctionCall { name, args })
    }

    fn parse_nested_expr(&mut self) -> XmlResult<Expr<'expr>> {
        self.with_depth(|parser| parser.parse_expr())
    }

    fn parse_nested_expr_single(&mut self) -> XmlResult<Expr<'expr>> {
        self.with_depth(|parser| parser.parse_expr_single())
    }

    fn with_depth<T>(
        &mut self,
        f: impl FnOnce(&mut XPath2Parser<'tokens, 'expr>) -> XmlResult<T>,
    ) -> XmlResult<T> {
        if self.depth >= self.max_depth {
            return Err(XmlError::xpath(format!(
                "XPath 2.0 expression nesting exceeds maximum depth of {}",
                self.max_depth
            )));
        }
        self.depth += 1;
        let result = f(self);
        self.depth -= 1;
        result
    }

    fn parse_comparison_operator(&mut self) -> Option<BinaryOp> {
        let op = match self.peek().map(|token| &token.kind) {
            Some(TokenKind::Equal) => BinaryOp::GeneralEq,
            Some(TokenKind::NotEqual) => BinaryOp::GeneralNe,
            Some(TokenKind::LessThan) => BinaryOp::GeneralLt,
            Some(TokenKind::LessThanOrEqual) => BinaryOp::GeneralLe,
            Some(TokenKind::GreaterThan) => BinaryOp::GeneralGt,
            Some(TokenKind::GreaterThanOrEqual) => BinaryOp::GeneralGe,
            Some(TokenKind::NodeBefore) => BinaryOp::NodeBefore,
            Some(TokenKind::NodeAfter) => BinaryOp::NodeAfter,
            Some(TokenKind::Name("eq")) => BinaryOp::ValueEq,
            Some(TokenKind::Name("ne")) => BinaryOp::ValueNe,
            Some(TokenKind::Name("lt")) => BinaryOp::ValueLt,
            Some(TokenKind::Name("le")) => BinaryOp::ValueLe,
            Some(TokenKind::Name("gt")) => BinaryOp::ValueGt,
            Some(TokenKind::Name("ge")) => BinaryOp::ValueGe,
            Some(TokenKind::Name("is")) => BinaryOp::NodeIs,
            _ => return None,
        };
        self.advance();
        Some(op)
    }

    fn parse_qname(&mut self) -> XmlResult<QName<'expr>> {
        let first = self.expect_any_name()?;
        if self.consume_token(TokenDiscriminant::Colon) {
            let local = self.expect_any_name()?;
            Ok(QName {
                prefix: Some(first),
                local,
            })
        } else {
            Ok(QName::local(first))
        }
    }

    fn peek(&self) -> Option<&Token<'expr>> {
        self.tokens.get(self.position)
    }

    fn peek_n(&self, n: usize) -> Option<&Token<'expr>> {
        self.tokens.get(self.position + n)
    }

    fn advance(&mut self) -> Option<&Token<'expr>> {
        let token = self.tokens.get(self.position);
        if token.is_some() {
            self.position += 1;
        }
        token
    }

    fn peek_name(&self, expected: &str) -> bool {
        matches!(
            self.peek().map(|token| &token.kind),
            Some(TokenKind::Name(name)) if *name == expected
        )
    }

    fn peek_n_name(&self, n: usize, expected: &str) -> bool {
        matches!(
            self.peek_n(n).map(|token| &token.kind),
            Some(TokenKind::Name(name)) if *name == expected
        )
    }

    fn peek_any_name(&self) -> Option<&'expr str> {
        match self.peek().map(|token| &token.kind) {
            Some(TokenKind::Name(name)) => Some(*name),
            _ => None,
        }
    }

    fn consume_name(&mut self, expected: &str) -> bool {
        if self.peek_name(expected) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect_name(&mut self, expected: &str) -> XmlResult<()> {
        if self.consume_name(expected) {
            Ok(())
        } else {
            Err(XmlError::xpath(format!("expected '{}'", expected)))
        }
    }

    fn expect_any_name(&mut self) -> XmlResult<&'expr str> {
        match self.advance().map(|token| &token.kind) {
            Some(TokenKind::Name(name)) => Ok(*name),
            other => Err(XmlError::xpath(format!("expected QName, got {:?}", other))),
        }
    }

    fn consume_comma(&mut self) -> bool {
        self.consume_token(TokenDiscriminant::Comma)
    }

    fn consume_token(&mut self, expected: TokenDiscriminant) -> bool {
        if self
            .peek()
            .is_some_and(|token| TokenDiscriminant::from(&token.kind) == Some(expected))
        {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn expect_token(&mut self, expected: TokenDiscriminant) -> XmlResult<()> {
        if self.consume_token(expected) {
            Ok(())
        } else {
            Err(XmlError::xpath(format!(
                "expected {}",
                expected.description()
            )))
        }
    }

    fn lookahead_is_function_call(&self) -> bool {
        let offset = if matches!(
            self.peek_n(1).map(|token| &token.kind),
            Some(TokenKind::Colon)
        ) {
            3
        } else {
            1
        };
        matches!(
            self.peek_n(offset).map(|token| &token.kind),
            Some(TokenKind::LeftParen)
        )
    }

    fn can_start_axis_step(&self) -> bool {
        match self.peek().map(|token| &token.kind) {
            Some(TokenKind::DoubleDot | TokenKind::At | TokenKind::Star) => true,
            Some(TokenKind::Name(name)) => {
                if matches!(
                    self.peek_n(1).map(|token| &token.kind),
                    Some(TokenKind::ColonColon)
                ) {
                    return true;
                }
                if matches!(
                    self.peek_n(1).map(|token| &token.kind),
                    Some(TokenKind::LeftParen)
                ) {
                    return is_kind_test_name(name);
                }
                // A prefixed function call (`prefix:local(`) is a primary
                // expression, not an axis step. `prefix:*` and `prefix:local`
                // remain name tests.
                if matches!(self.peek_n(1).map(|t| &t.kind), Some(TokenKind::Colon))
                    && matches!(self.peek_n(3).map(|t| &t.kind), Some(TokenKind::LeftParen))
                {
                    return false;
                }
                !is_reserved_operator_name(name)
            }
            _ => false,
        }
    }

    fn path_can_continue(&self) -> bool {
        !matches!(
            self.peek().map(|token| &token.kind),
            None | Some(TokenKind::RightParen)
                | Some(TokenKind::RightBracket)
                | Some(TokenKind::Comma)
                | Some(TokenKind::Pipe)
        )
    }

    fn peek_axis_name(&self) -> bool {
        matches!(
            (
                self.peek().map(|token| &token.kind),
                self.peek_n(1).map(|token| &token.kind)
            ),
            (Some(TokenKind::Name(name)), Some(TokenKind::ColonColon)) if parse_axis(name).is_ok()
        )
    }

    fn is_kind_test_name(&self, name: &str) -> bool {
        is_kind_test_name(name)
            && matches!(
                self.peek_n(1).map(|token| &token.kind),
                Some(TokenKind::LeftParen)
            )
    }
}

fn parse_axis(name: &str) -> XmlResult<Axis> {
    match name {
        "child" => Ok(Axis::Child),
        "descendant" => Ok(Axis::Descendant),
        "attribute" => Ok(Axis::Attribute),
        "self" => Ok(Axis::Self_),
        "descendant-or-self" => Ok(Axis::DescendantOrSelf),
        "parent" => Ok(Axis::Parent),
        "ancestor" => Ok(Axis::Ancestor),
        "ancestor-or-self" => Ok(Axis::AncestorOrSelf),
        "following-sibling" => Ok(Axis::FollowingSibling),
        "following" => Ok(Axis::Following),
        "namespace" => Ok(Axis::Namespace),
        "preceding-sibling" => Ok(Axis::PrecedingSibling),
        "preceding" => Ok(Axis::Preceding),
        other => Err(XmlError::xpath(format!(
            "unsupported XPath 2.0 axis '{}'",
            other
        ))),
    }
}

fn is_kind_test_name(name: &str) -> bool {
    matches!(
        name,
        "text"
            | "node"
            | "comment"
            | "processing-instruction"
            | "document-node"
            | "element"
            | "attribute"
            | "schema-element"
            | "schema-attribute"
    )
}

/// Kind-test names usable as an `ItemType` in a sequence type. Identical to the
/// path-step kind tests today, named separately for grammar clarity.
fn is_extended_kind_test_name(name: &str) -> bool {
    is_kind_test_name(name)
}

fn is_reserved_operator_name(name: &str) -> bool {
    matches!(
        name,
        "and"
            | "or"
            | "div"
            | "idiv"
            | "mod"
            | "eq"
            | "ne"
            | "lt"
            | "le"
            | "gt"
            | "ge"
            | "is"
            | "to"
            | "union"
            | "intersect"
            | "except"
            | "return"
            | "then"
            | "else"
            | "satisfies"
            | "in"
            | "instance"
            | "of"
            | "treat"
            | "as"
            | "castable"
            | "cast"
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenDiscriminant {
    Slash,
    DoubleSlash,
    DoubleDot,
    At,
    Star,
    Pipe,
    Plus,
    Minus,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Comma,
    Dollar,
    Colon,
    ColonColon,
    Question,
}

impl TokenDiscriminant {
    fn from(kind: &TokenKind<'_>) -> Option<Self> {
        Some(match kind {
            TokenKind::Slash => TokenDiscriminant::Slash,
            TokenKind::DoubleSlash => TokenDiscriminant::DoubleSlash,
            TokenKind::DoubleDot => TokenDiscriminant::DoubleDot,
            TokenKind::At => TokenDiscriminant::At,
            TokenKind::Star => TokenDiscriminant::Star,
            TokenKind::Pipe => TokenDiscriminant::Pipe,
            TokenKind::Plus => TokenDiscriminant::Plus,
            TokenKind::Minus => TokenDiscriminant::Minus,
            TokenKind::LeftParen => TokenDiscriminant::LeftParen,
            TokenKind::RightParen => TokenDiscriminant::RightParen,
            TokenKind::LeftBracket => TokenDiscriminant::LeftBracket,
            TokenKind::RightBracket => TokenDiscriminant::RightBracket,
            TokenKind::Comma => TokenDiscriminant::Comma,
            TokenKind::Dollar => TokenDiscriminant::Dollar,
            TokenKind::Colon => TokenDiscriminant::Colon,
            TokenKind::ColonColon => TokenDiscriminant::ColonColon,
            TokenKind::Question => TokenDiscriminant::Question,
            _ => return None,
        })
    }

    fn description(self) -> &'static str {
        match self {
            TokenDiscriminant::Slash => "'/'",
            TokenDiscriminant::DoubleSlash => "'//'",
            TokenDiscriminant::DoubleDot => "'..'",
            TokenDiscriminant::At => "'@'",
            TokenDiscriminant::Star => "'*'",
            TokenDiscriminant::Pipe => "'|'",
            TokenDiscriminant::Plus => "'+'",
            TokenDiscriminant::Minus => "'-'",
            TokenDiscriminant::LeftParen => "'('",
            TokenDiscriminant::RightParen => "')'",
            TokenDiscriminant::LeftBracket => "'['",
            TokenDiscriminant::RightBracket => "']'",
            TokenDiscriminant::Comma => "','",
            TokenDiscriminant::Dollar => "'$'",
            TokenDiscriminant::Colon => "':'",
            TokenDiscriminant::ColonColon => "'::'",
            TokenDiscriminant::Question => "'?'",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sequence_constructor() {
        let expr = parse_expression("(1, 2, 3)", DEFAULT_MAX_XPATH2_DEPTH).unwrap();
        match expr {
            Expr::Sequence(items) => assert_eq!(items.len(), 3),
            other => panic!("expected sequence, got {other:?}"),
        }
    }

    #[test]
    fn parses_for_expression() {
        let expr =
            parse_expression("for $x in 1 to 3 return $x + 1", DEFAULT_MAX_XPATH2_DEPTH).unwrap();
        assert!(matches!(expr, Expr::For { .. }));
    }

    #[test]
    fn parses_basic_path() {
        let expr = parse_expression("//book/title[1]", DEFAULT_MAX_XPATH2_DEPTH).unwrap();
        match expr {
            Expr::Path(path) => assert!(path.absolute),
            other => panic!("expected path, got {other:?}"),
        }
    }
}
