// This file is part of the uutils awk package.
//
// For the full copyright and license information, please view the LICENSE
// files that was distributed with this source code.
#![forbid(unsafe_code)]

mod ast;
mod diagnostics;
mod idempotency;
mod lex;
mod pratt;
mod sexpr;
#[cfg(test)]
mod tests;

use std::{fmt::Debug, mem::replace};

use bumpalo::{Bump, boxed::Box, collections::Vec, vec};
use either::Either::{Left, Right};
use hashbrown::HashMap;
use lexer::{LexingError, Span, Token};

pub use crate::ast::Ast;
pub use crate::lex::Lexer;
use crate::{
    ast::{
        Atom, BinaryPlaceOperator, Body, Command, Expr, ExprNode, Function, Identifier, Pattern,
        Rule, RulePattern, SpecialPattern, Statement, Variable,
    },
    diagnostics::{ParsingError, report_error},
    lex::TokenExt,
    pratt::Pratt,
};

type Result<T, E = ParsingError> = std::result::Result<T, E>;

pub struct Parser<'a> {
    ast: Ast<'a>,
    arena: &'a Bump,
    preprocessor: Preprocessor,
    current_file: &'a str,
    namespace: &'a str,
    concurrent: bool,
}

impl From<LexingError> for ParsingError {
    fn from(value: LexingError) -> Self {
        Self::LexingError(value)
    }
}

type AriadneErr<'a> = (
    std::boxed::Box<ariadne::Report<'a, (&'a str, Span)>>,
    ariadne::Source<&'a str>,
);

impl<'a> Parser<'a> {
    #[tracing::instrument]
    pub fn new(arena: &'a Bump) -> Self {
        Self {
            ast: Ast::new(arena),
            arena,
            preprocessor: Preprocessor {},
            current_file: "",
            namespace: "awk",
            concurrent: false,
        }
    }

    pub fn parse(&mut self, name: &'a str, source: &'a [u8]) -> Result<&Ast<'a>, AriadneErr<'a>> {
        let source = self.arena.alloc_slice_copy(source);
        self.current_file = name;
        let mut lex = Lexer::new(source);
        let parsed = self.parse_top(&mut lex, true);
        parsed.map_err(|error| report_error(error, name, source))
    }

    #[tracing::instrument]
    fn parse_top(&mut self, lex: &mut Lexer<'a>, awk_namespace: bool) -> Result<&Ast<'a>> {
        // Expects:
        //   * Directive
        //     * Namespace: Either handle here or in interpreter; idk.
        //     * Include: recursively lex & parse the filename.
        //     * Concurrent: Pass on to interpreter.
        //     * Load: Pass on to interpreter.
        //   * Pattern (Expression)
        //     * Expects brackets afterwards (body) or a newline (default).
        //   * Action (Statement)
        //     * Expects a newline afterwards; inserts default pattern.
        while let Some(tok) = lex.peek() {
            if tok.as_ref().is_ok_and(Token::is_pattern_start) {
                match self.parse_pattern(lex)? {
                    Left(rule_pattern) => {
                        let body = lex.peek_is(&Token::OpenBrace).then(|| self.parse_body(lex));
                        self.add_rule(Rule {
                            pattern: Some(rule_pattern),
                            actions: body.transpose()?,
                        });
                    }
                    Right(special_pattern) => {
                        lex.next();
                        let body = self.parse_body(lex)?;
                        match special_pattern {
                            SpecialPattern::Begin => &mut self.ast.begin,
                            SpecialPattern::End => &mut self.ast.end,
                            SpecialPattern::BeginFile => &mut self.ast.begin_file,
                            SpecialPattern::EndFile => &mut self.ast.end_file,
                        }
                        .push(body);
                    }
                }
            } else if lex.peek_is(&Token::OpenBrace) {
                let actions = Some(self.parse_body(lex)?);
                self.add_rule(Rule {
                    pattern: None,
                    actions,
                });
            } else {
                match lex.expect_next()? {
                    Token::LoadDirective(lib) => {
                        self.ast.loads.push(lib);
                        lex.expect_with(Token::is_stmnt_end, "expected statement end.".into())?;
                    }
                    Token::IncludeDirective(path) => {
                        let old_namespace = self.namespace;
                        let content = self.preprocessor.include_in(path.as_ref(), self.arena);
                        self.parse_top(&mut Lexer::new(content), true)?;
                        lex.expect_with(Token::is_stmnt_end, "expected statement end.".into())?;
                        self.namespace = old_namespace;
                    }
                    Token::NsIncludeDirective(path) => {
                        let old_namespace = self.namespace;
                        let content = self.preprocessor.include_in(path.as_ref(), self.arena);
                        self.parse_top(&mut Lexer::new(content), false)?;
                        lex.expect_with(Token::is_stmnt_end, "expected statement end.".into())?;
                        self.namespace = old_namespace;
                    }
                    Token::NamespaceDirective(namespace) => {
                        self.namespace = namespace;
                        lex.expect_with(Token::is_stmnt_end, "expected statement end.".into())?;
                    }
                    Token::ConcurrentDirective => {
                        if lex.peek_with(|t| t.maps_to_special_pat().is_some()) || self.concurrent {
                            return Err(ParsingError::UnexpectedToken(
                                lex.span(),
                                "especified more than once.".into(),
                            ));
                        }
                        self.concurrent = true;
                    }
                    Token::Function => self.parse_function(lex)?,
                    Token::Newline | Token::Semicolon if self.concurrent => {
                        return Err(ParsingError::UnexpectedToken(
                            lex.span(),
                            "a pattern was expected.".into(),
                        ));
                    }
                    Token::Newline | Token::Semicolon => {}
                    x => unimplemented!("{x:?}"),
                }
            }
        }
        Ok(&self.ast)
    }

    /// Parses up until `{`.
    #[tracing::instrument]
    fn parse_pattern(&mut self, lex: &mut Lexer<'a>) -> Result<Pattern<'a>> {
        match lex.expect_peek()? {
            Token::BeginPattern => Ok(Right(SpecialPattern::Begin)),
            Token::EndPattern => Ok(Right(SpecialPattern::End)),
            Token::BeginFilePattern => Ok(Right(SpecialPattern::BeginFile)),
            Token::EndFilePattern => Ok(Right(SpecialPattern::EndFile)),
            _ => {
                let expr = self.parse_expression(lex)?;
                Ok(Left(if lex.consume(&Token::Comma) {
                    let expr_end = self.parse_expression(lex)?;
                    RulePattern::Range(expr, expr_end)
                } else {
                    RulePattern::Expression(expr)
                }))
            }
        }
    }

    /// Parses up until `}`. Inserts a lone print statement if none.
    #[tracing::instrument]
    fn parse_body(&mut self, lex: &mut Lexer<'a>) -> Result<Body<'a>> {
        lex.expect(&Token::OpenBrace, ParsingError::ExpectedOpeningBrace)?;
        let mut body = Vec::new_in(self.arena);
        let mut depth = 0;

        loop {
            if lex.peek_with(|tok| tok.is_stmnt_end() || tok.is_brace()) {
                match lex.expect_next()? {
                    Token::ClosedBrace => {
                        if depth == 0 {
                            break Ok(Body(body));
                        }
                        depth -= 1;
                    }
                    Token::OpenBrace => {
                        depth += 1;
                    }
                    _ => {}
                }
            } else if lex.peek().is_some() {
                body.push(self.parse_statement(lex)?);
            } else {
                break Err(ParsingError::UnclosedScope(
                    lex.peeked_span().unwrap_or_else(|_| lex.span()),
                ));
            }
        }
    }

    #[tracing::instrument]
    fn parse_statement(&mut self, lex: &mut Lexer<'a>) -> Result<Statement<'a>> {
        let statement = if lex.expect_peek()?.is_expr_start() {
            self.parse_expression(lex).map(Statement::Expression)?
        } else {
            match lex.expect_next()? {
                tok if let Some(name) = tok.maps_to_command() => self.parse_command(lex, name)?,
                Token::If => {
                    let condition = self.parse_parenthesized_expr(lex)?;
                    let then_body = self.parse_statement_body(lex)?;
                    let else_body = lex
                        .consume(&Token::Else)
                        .then(|| self.parse_statement_body(lex))
                        .transpose()?;
                    Statement::If {
                        condition,
                        then_body,
                        else_body,
                    }
                }
                Token::For => {
                    // FIXME(trivial): parser differential w/ GNU: they treat
                    // for (ident in ident; expr; expr) as a syntax error.
                    // It seems like a bug to me.
                    lex.expect(&Token::OpenParent, ParsingError::ExpectedOpeningParenthesis)?;
                    let init = (!lex.consume(&Token::Semicolon))
                        .then(|| self.parse_expression(lex))
                        .transpose()?;
                    if init.is_none() || lex.consume(&Token::Semicolon) {
                        let condition = self.parse_for_fragment::<false>(lex)?;
                        let update = self.parse_for_fragment::<true>(lex)?;
                        let body = self.parse_statement_body(lex)?;
                        Statement::For {
                            init,
                            condition,
                            update,
                            body,
                        }
                    } else {
                        let Some(Expr::Node(node)) = init else {
                            return Err(ParsingError::InvalidForLoop(lex.span()));
                        };
                        let ExprNode::BinaryPlaceOperation(
                            BinaryPlaceOperator::InArray,
                            ast::Place::Variable(variable),
                            Expr::Leaf(Atom::Variable(array)),
                        ) = Box::into_inner(node)
                        else {
                            return Err(ParsingError::InvalidForLoop(lex.span()));
                        };
                        lex.expect(
                            &Token::ClosedParent,
                            ParsingError::UnclosedParenthesisInStatement,
                        )?;
                        let body = self.parse_statement_body(lex)?;
                        Statement::ForEach {
                            variable,
                            array,
                            body,
                        }
                    }
                }
                Token::Switch => {
                    let scrutinee = self.parse_parenthesized_expr(lex)?;
                    lex.expect(&Token::OpenBrace, ParsingError::ExpectedOpeningBrace)?;
                    let mut default = None;
                    let mut branches = Vec::new_in(self.arena);
                    let mut case = None;
                    let mut body = Vec::new_in(self.arena);

                    while !lex.consume(&Token::ClosedBrace) {
                        if lex.peek_is(&Token::Case) {
                            match case.take() {
                                Some(Right(())) => {
                                    default = Some((
                                        replace(&mut body, Vec::new_in(self.arena)).into(),
                                        branches.len(),
                                    ));
                                }
                                Some(Left(atom)) => branches.push((
                                    atom,
                                    replace(&mut body, Vec::new_in(self.arena)).into(),
                                )),
                                _ => {}
                            }
                            case = Some(Left(self.parse_case(lex)?));
                        } else if lex.consume(&Token::Default) {
                            let span = lex.span();
                            lex.expect(&Token::Colon, ParsingError::ColonMustFollowCase)?;
                            if default.is_some() || matches!(case, Some(Right(()))) {
                                return Err(ParsingError::DuplicatedDefaultBranch(span));
                            } else if let Some(Left(atom)) = case {
                                branches.push((
                                    atom,
                                    replace(&mut body, Vec::new_in(self.arena)).into(),
                                ));
                            }
                            case = Some(Right(()));
                        } else {
                            if case.is_none() {
                                return Err(ParsingError::MissingSwitchBranch(lex.span()));
                            }
                            let statement = self.parse_statement(lex)?;
                            body.push(statement);
                        }
                    }
                    match case.take() {
                        Some(Right(())) => default = Some((body.into(), branches.len())),
                        Some(Left(atom)) => branches.push((atom, body.into())),
                        _ => {}
                    }

                    Statement::Switch {
                        scrutinee,
                        branches,
                        default,
                    }
                }
                Token::While => {
                    let condition = self.parse_parenthesized_expr(lex)?;
                    let then_body = self.parse_statement_body(lex)?;
                    Statement::While {
                        condition,
                        then_body,
                    }
                }
                Token::Do => {
                    let then_body = self.parse_body(lex)?;
                    lex.expect(&Token::While, ParsingError::MissingWhileAfterDo)?;
                    let condition = self.parse_parenthesized_expr(lex)?;
                    Statement::DoWhile {
                        then_body,
                        condition,
                    }
                }
                Token::Break => Statement::Break,
                Token::Continue => Statement::Continue,
                Token::Return => Statement::Return(
                    (!lex.peek_with(Token::is_stmnt_or_block_end))
                        .then(|| self.parse_expression(lex))
                        .transpose()?,
                ),
                Token::Next => Statement::Next,
                Token::NextFile => Statement::NextFile,
                Token::Exit => Statement::Exit(
                    (!lex.peek_with(Token::is_stmnt_or_block_end))
                        .then(|| self.parse_expression(lex))
                        .transpose()?,
                ),
                a => todo!("{a:?}"),
            }
        };

        lex.consume_with(Token::is_stmnt_end);
        Ok(statement)
    }

    #[tracing::instrument]
    fn parse_parenthesized_expr(&mut self, lex: &mut Lexer<'a>) -> Result<Expr<'a>> {
        lex.expect(
            &Token::OpenParent,
            ParsingError::MissingParenthesisInStatement,
        )?;
        let expr = self.parse_expression(lex)?;
        lex.expect(
            &Token::ClosedParent,
            ParsingError::UnclosedParenthesisInStatement,
        )?;
        Ok(expr)
    }

    #[tracing::instrument]
    fn parse_for_fragment<const END: bool>(
        &mut self,
        lex: &mut Lexer<'a>,
    ) -> Result<Option<Expr<'a>>> {
        let next = if END {
            Token::ClosedParent
        } else {
            Token::Semicolon
        };
        if lex.consume(&next) {
            Ok(None)
        } else {
            let expr = self.parse_expression(lex)?;
            lex.expect(&next, ParsingError::InvalidForLoop)?;
            Ok(Some(expr))
        }
    }

    #[tracing::instrument]
    fn parse_statement_body(&mut self, lex: &mut Lexer<'a>) -> Result<Body<'a>> {
        if lex.peek_is(&Token::OpenBrace) {
            self.parse_body(lex)
        } else {
            Ok(vec![in self.arena; self.parse_statement(lex)?].into())
        }
    }

    #[tracing::instrument]
    fn parse_case(&mut self, lex: &mut Lexer<'a>) -> Result<Atom<'a>> {
        lex.expect(&Token::Case, ParsingError::MissingSwitchBranch)?;
        let next = lex.expect_next()?;
        let value = self.parse_atom(lex, next)?;
        lex.expect(&Token::Colon, ParsingError::ColonMustFollowCase)?;
        match value {
            Atom::Variable(_) => Err(ParsingError::InvalidCaseValue(lex.span())),
            _ => Ok(value),
        }
    }

    #[tracing::instrument]
    fn parse_command(&mut self, lex: &mut Lexer<'a>, command: Command) -> Result<Statement<'a>> {
        let parent = lex.consume(&Token::OpenParent);
        let command = Statement::Command {
            name: command,
            args: self.parse_arguments(lex, |t| {
                if parent {
                    t == &Token::ClosedParent
                } else {
                    t.is_stmnt_end() || t == &Token::ClosedBrace
                }
            })?,
            redirection: None,
        };
        if parent {
            lex.expect(
                &Token::ClosedParent,
                ParsingError::UnclosedParenthesisInStatement,
            )?;
        }
        Ok(command)
    }

    /// Parses arguments to command or function calls; consumes to the end of
    /// the argument list or short-circuits with `delimiter` if empty.
    fn parse_arguments(
        &mut self,
        lex: &mut Lexer<'a>,
        delimiter: impl Fn(&Token<'a>) -> bool,
    ) -> Result<Vec<'a, Expr<'a>>> {
        let mut arguments = Vec::new_in(self.arena);
        if lex.peek_with(&delimiter) {
            return Ok(arguments);
        }

        arguments.push(self.parse_expression(lex)?);
        while lex.consume(&Token::Comma) {
            arguments.push(self.parse_expression(lex)?);
        }
        Ok(arguments)
    }

    #[tracing::instrument]
    fn parse_function(&mut self, lex: &mut Lexer<'a>) -> Result<()> {
        let name = lex.expect_identifier()?.qualify(self.namespace);
        let args = self.parse_signature(lex, &name)?;
        lex.consume(&Token::Newline);
        let body = self.parse_body(lex)?;

        self.ast.functions.insert(name, Function { args, body });
        Ok(())
    }

    #[tracing::instrument]
    fn parse_signature(
        &mut self,
        lex: &mut Lexer<'a>,
        name: &Identifier<'a>,
    ) -> Result<Vec<'a, Identifier<'a>>> {
        let mut args = Vec::new_in(self.arena);
        lex.expect(&Token::OpenParent, |s| {
            ParsingError::NoFunctionSignature(s, name.to_string())
        })?;

        if lex.consume(&Token::ClosedParent) {
            return Ok(args);
        }

        loop {
            let name = lex.expect_identifier()?.qualify(self.namespace);
            // Linear search is fine for the numbers we are working with.
            if let Some(arg) = args.iter().find(|&a| a == &name) {
                return Err(ParsingError::DuplicatedArgument(
                    lex.span(),
                    name.to_string(),
                    arg.to_string(),
                ));
            }
            args.push(name);

            if !lex.consume(&Token::Comma) {
                lex.expect(&Token::ClosedParent, |s| {
                    ParsingError::FunctionCallMissingParenthesis(s, name.to_string())
                })?;
                break;
            }
        }
        Ok(args)
    }

    #[tracing::instrument]
    fn parse_expression(&mut self, lex: &mut Lexer<'a>) -> Result<Expr<'a>> {
        Pratt::parse(self, lex)
    }

    #[tracing::instrument]
    fn add_rule(&mut self, rule: Rule<'a>) {
        if self.concurrent {
            self.concurrent = false;
            &mut self.ast.concurrent
        } else {
            &mut self.ast.rules
        }
        .push(rule);
    }

    #[tracing::instrument]
    fn parse_function_call(
        &mut self,
        lex: &mut Lexer<'a>,
        name: Identifier<'a>,
        span: Span,
    ) -> Result<Expr<'a>> {
        lex.expect(&Token::OpenParent, ParsingError::ExpectedOpeningParenthesis)?;
        if lex.span().start != span.end {
            return Err(ParsingError::FunctionCallSeparatedIdent(span));
        }
        let expr = ExprNode::FunctionCall(
            name,
            self.parse_arguments(lex, |t| t == &Token::ClosedParent)?,
        );
        lex.expect(&Token::ClosedParent, |s| {
            ParsingError::FunctionCallMissingParenthesis(s, name.to_string())
        })?;
        Ok(Expr::node(expr, self.arena))
    }

    #[tracing::instrument]
    fn parse_atom(&self, lex: &mut Lexer<'a>, token: Token<'a>) -> Result<Atom<'a>> {
        match token {
            Token::Number(n) => Ok(Atom::Number(n)),
            Token::String(s) => Ok(Atom::String(s)),
            Token::Regex(r) => Ok(Atom::Regex(r)),
            token => match self.get_place(lex, token) {
                Some(var) => Ok(Atom::Variable(var)),
                None => Err(ParsingError::UnexpectedToken(
                    lex.span(),
                    "is not valid data.".into(),
                )),
            },
        }
    }

    #[tracing::instrument]
    fn get_place(&self, lex: &mut Lexer<'a>, token: Token<'a>) -> Option<Variable<'a>> {
        match token {
            Token::Identifier(a) if !(lex.peek_is(&Token::OpenParent) && lex.is_yuxtaposed()) => {
                Some(a.qualify(self.namespace).into())
            }
            Token::NrVariable => Some(Variable::Nr),
            Token::NfVariable => Some(Variable::Nf),
            Token::FsVariable => Some(Variable::Fs),
            Token::RsVariable => Some(Variable::Rs),
            Token::OfsVariable => Some(Variable::Ofs),
            Token::OrsVariable => Some(Variable::Ors),
            Token::FilenameVariable => Some(Variable::Filename),
            Token::ArgcVariable => Some(Variable::Argc),
            Token::ArgvVariable => Some(Variable::Argv),
            Token::SubsepVariable => Some(Variable::Subsep),
            Token::FnrVariable => Some(Variable::Fnr),
            Token::OfmtVariable => Some(Variable::Ofmt),
            Token::RstartVariable => Some(Variable::Rstart),
            Token::RlengthVariable => Some(Variable::Rlength),
            Token::EnvironVariable => Some(Variable::Environ),
            _ => None,
        }
    }
}

impl<'a> Ast<'a> {
    fn new(arena: &'a Bump) -> Self {
        Self {
            loads: Vec::new_in(arena),
            begin: Vec::new_in(arena),
            end: Vec::new_in(arena),
            begin_file: Vec::new_in(arena),
            end_file: Vec::new_in(arena),
            rules: Vec::new_in(arena),
            concurrent: Vec::new_in(arena),
            functions: HashMap::new_in(arena),
        }
    }
}

#[derive(Debug)]
struct Preprocessor {}

impl Preprocessor {
    fn include_in<'a: 'b, 'b>(&mut self, _path: &'b [u8], _alloc: &'a Bump) -> &'a [u8] {
        todo!()
    }
}

trait IdentifierExt<'a> {
    fn qualify(self, namespace: &'a str) -> Identifier<'a>
    where
        Self: 'a;
}

impl<'a> IdentifierExt<'a> for lexer::Identifier<'_> {
    fn qualify(self, namespace: &'a str) -> Identifier<'a>
    where
        Self: 'a,
    {
        let literal = self.literal;
        if let Some(namespace) = self.namespace {
            Identifier { namespace, literal }
        } else {
            Identifier { namespace, literal }
        }
    }
}

impl Debug for Parser<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        #[derive(Debug)]
        #[allow(dead_code)]
        struct Parser<'a> {
            ast: &'a Ast<'a>,
            preprocessor: &'a Preprocessor,
            namespace: &'a str,
            concurrent: bool,
        }
        Parser {
            ast: &self.ast,
            preprocessor: &self.preprocessor,
            namespace: self.namespace,
            concurrent: self.concurrent,
        }
        .fmt(f)
    }
}
