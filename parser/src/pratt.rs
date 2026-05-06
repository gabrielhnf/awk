use lexer::Token;

use crate::{
    IdentifierExt, Lexer, Parser, Result,
    ast::{
        BinaryOperator, BinaryPlaceOperator, BindingPower, Expr, ExprNode, Getline, Place, Ternary,
        UnaryOperator, UnaryPlaceOperator, Variable, WriteKind,
    },
    diagnostics::ParsingError,
    lex::TokenExt,
};

pub struct Pratt<'a, 'b> {
    parser: &'b mut Parser<'a>,
}

impl<'a, 'b> Pratt<'a, 'b> {
    pub fn parse(parser: &'b mut Parser<'a>, lex: &mut Lexer<'a>) -> Result<Expr<'a>> {
        Self { parser }.parse_expression(lex, 0)
    }

    fn parse_lhs(&mut self, lex: &mut Lexer<'a>) -> Result<Expr<'a>> {
        if lex.consume(&Token::OpenParent) {
            self.parse_parenthesized(lex)
        } else if lex.peek_with(Token::is_prefix_op) {
            self.parse_prefix(lex)
        } else if lex.consume(&Token::Getline) {
            self.parse_prefix_getline(lex)
        } else {
            self.parse_atom_or_call(lex)
        }
    }

    fn parse_expression(&mut self, lex: &mut Lexer<'a>, min_bp: u8) -> Result<Expr<'a>> {
        let lhs = self.parse_lhs(lex)?;
        self.fold_rhs(lex, lhs, min_bp)
    }

    fn fold_rhs(&mut self, lex: &mut Lexer<'a>, mut lhs: Expr<'a>, min_bp: u8) -> Result<Expr<'a>> {
        while let Some((next, span)) = lex.peek_with_span() {
            let next = next?;
            lhs = if let Ok(op) = UnaryPlaceOperator::parse_suffix(next, &span) {
                if op.binding_power() < min_bp {
                    break;
                }
                lex.next();
                let place = Place::promote_from(lhs.take(), lex.span())?;
                Expr::node(op.expr(place), self.parser.arena)
            } else if let Ok(op) = BinaryPlaceOperator::parse(next, &span) {
                if op.binding_power().0 < min_bp {
                    break;
                }
                let place = Place::promote_from(lhs.take(), lex.span())?;
                self.parse_place_op(lex, op, place)?
            } else if let Ok(op) = BinaryOperator::parse(next, &span)
                && !matches!(next, Token::Increment | Token::Decrement)
            {
                if op.binding_power().0 < min_bp {
                    break;
                }
                self.parse_infix_op(lex, op, lhs)?
            } else if next == &Token::QuestionMark {
                if Ternary.binding_power().0 < min_bp {
                    break;
                }
                self.parse_ternary(lex, lhs)?
            } else if let Some(op) = WriteKind::parse(next) {
                if BinaryOperator::Concat.binding_power().0 < min_bp {
                    break;
                }
                self.parse_getline_pipe(lex, op, lhs)?
            } else {
                break;
            }
        }
        Ok(lhs)
    }

    fn parse_parenthesized(&mut self, lex: &mut Lexer<'a>) -> Result<Expr<'a>> {
        let inner = self.parse_expression(lex, 0);
        lex.expect(
            &Token::ClosedParent,
            ParsingError::UnclosedParenthesisExpression,
        )
        .and(inner)
    }

    fn parse_prefix(&mut self, lex: &mut Lexer<'a>) -> Result<Expr<'a>> {
        let next = lex.expect_next()?;
        if let Ok(op) = UnaryPlaceOperator::parse_prefix(&next, &lex.span()) {
            let rhs = self.parse_expression(lex, op.binding_power())?;
            Ok(Expr::node(
                op.expr(Place::promote_from(rhs, lex.span())?),
                self.parser.arena,
            ))
        } else if let Ok(op) = UnaryOperator::parse(&next, &lex.peeked_span()?) {
            let rhs = self.parse_expression(lex, op.binding_power())?;
            Ok(Expr::node(op.expr(rhs), self.parser.arena))
        } else {
            Err(ParsingError::InvalidExpression(lex.span()))
        }
    }

    fn parse_prefix_getline(&mut self, lex: &mut Lexer<'a>) -> Result<Expr<'a>> {
        // Consumes with maximum precedence the following place and/or
        // redirection reading from file.
        let place = if lex.peek_with(Token::is_place) {
            Some(Place::promote_from(
                self.parse_expression(lex, BinaryOperator::Concat.binding_power().1)?,
                lex.span(),
            ))
        } else {
            None
        }
        .transpose();

        let getline = |gl| Expr::node(ExprNode::Getline(gl), self.parser.arena);
        match place {
            Err((expr, _)) => Ok(Expr::node(
                BinaryOperator::Concat.expr(getline(Getline::FromInput(None)), expr),
                self.parser.arena,
            )),
            Ok(place) => {
                if lex.consume(&Token::LesserThan) {
                    let file = self.parse_expression(lex, BinaryOperator::Lt.binding_power().1)?;
                    Ok(getline(Getline::FromFile(place, file)))
                } else {
                    Ok(getline(Getline::FromInput(place)))
                }
            }
        }
    }

    fn parse_atom_or_call(&mut self, lex: &mut Lexer<'a>) -> Result<Expr<'a>> {
        let next = lex.expect_next()?;
        if let Token::Identifier(name) = next
            && lex.peek_is(&Token::OpenParent)
            && lex.is_yuxtaposed()
        {
            self.parser
                .parse_function_call(lex, name.qualify(self.parser.namespace), lex.span())
        } else {
            Ok(Expr::leaf(self.parser.parse_atom(lex, next)?))
        }
    }

    fn parse_infix_op(
        &mut self,
        lex: &mut Lexer<'a>,
        op: BinaryOperator,
        lhs: Expr<'a>,
    ) -> Result<Expr<'a>> {
        lex.consume_with(|_| op != BinaryOperator::Concat);

        let rhs = self.parse_expression(lex, op.binding_power().1)?;
        Ok(Expr::node(op.expr(lhs, rhs), self.parser.arena))
    }

    fn parse_place_op(
        &mut self,
        lex: &mut Lexer<'a>,
        op: BinaryPlaceOperator,
        place: Place<'a>,
    ) -> Result<Expr<'a>> {
        lex.next();
        let mut rhs = self.parse_expression(lex, op.binding_power().1)?;
        if op == BinaryPlaceOperator::ArrayAccess {
            if !matches!(place, Place::Variable(_)) {
                return Err(ParsingError::OperatorExpectsVariable(lex.span()));
            }
            while lex.consume(&Token::Comma) {
                rhs = Expr::node(
                    BinaryOperator::Concat.expr(
                        rhs,
                        Expr::node(
                            BinaryOperator::Concat
                                .expr(Expr::leaf(Variable::Subsep), self.parse_expression(lex, 0)?),
                            self.parser.arena,
                        ),
                    ),
                    self.parser.arena,
                );
            }
            lex.expect(&Token::ClosedBracket, ParsingError::UnclosedArrayAccess)?;
        }
        Ok(Expr::node(op.expr(place, rhs), self.parser.arena))
    }

    fn parse_ternary(&mut self, lex: &mut Lexer<'a>, lhs: Expr<'a>) -> Result<Expr<'a>> {
        let right_bp = Ternary.binding_power().1;
        lex.next();
        let then_branch = self.parse_expression(lex, right_bp)?;
        lex.expect(&Token::Colon, ParsingError::MissingTernaryOr)?;
        let else_branch = self.parse_expression(lex, right_bp)?;
        Ok(Expr::node(
            ExprNode::Ternary(lhs, then_branch, else_branch),
            self.parser.arena,
        ))
    }

    fn parse_getline_pipe(
        &mut self,
        lex: &mut Lexer<'a>,
        op: WriteKind,
        lhs: Expr<'a>,
    ) -> Result<Expr<'a>> {
        lex.next();
        lex.expect(&Token::Getline, |span| {
            ParsingError::UnexpectedToken(
                span,
                "operand must precede `getline` in an expression.".into(),
            )
        })?;

        let pipe = |place| Expr::node(op.expr_getline(place, lhs), self.parser.arena);
        if lex.peek_with(Token::is_place) {
            let expr = self.parse_expression(lex, BinaryOperator::Concat.binding_power().1)?;
            match Place::promote_from(expr, lex.span()) {
                Ok(place) => Ok(pipe(Some(place))),
                Err((expr, _)) => Ok(Expr::node(
                    BinaryOperator::Concat.expr(pipe(None), expr),
                    self.parser.arena,
                )),
            }
        } else {
            Ok(pipe(None))
        }
    }
}
