// This file is part of the uutils awk package.
//
// For the full copyright and license information, please view the LICENSE
// files that was distributed with this source code.

use std::fmt::Debug;

use bumpalo::{Bump, boxed::Box, collections::Vec};
use either::Either;
use hashbrown::{DefaultHashBuilder, HashMap};
use lexer::{Slice, Span, Token};

use crate::{ParsingError, Result, lex::TokenExt};

#[derive(Debug)]
pub struct Ast<'a> {
    pub loads: Vec<'a, Slice<'a>>,
    pub begin: Vec<'a, Body<'a>>,
    pub end: Vec<'a, Body<'a>>,
    pub begin_file: Vec<'a, Body<'a>>,
    pub end_file: Vec<'a, Body<'a>>,
    pub rules: Vec<'a, Rule<'a>>,
    pub concurrent: Vec<'a, Rule<'a>>,
    pub functions: HashMap<Identifier<'a>, Function<'a>, DefaultHashBuilder, &'a Bump>,
}

#[derive(Debug)]
pub struct Rule<'a> {
    pub pattern: Option<RulePattern<'a>>,
    pub actions: Option<Body<'a>>,
}

#[derive(Clone)]
pub enum Atom<'a> {
    Variable(Variable<'a>),
    String(Slice<'a>),
    Number(f64),
    BigInt(),
    BigFloat(),
    Regex(Slice<'a>),
}

#[derive(Debug)]
pub enum RulePattern<'a> {
    Expression(Expr<'a>),
    Range(Expr<'a>, Expr<'a>),
}

#[derive(Debug, Clone)]
pub enum SpecialPattern {
    Begin,
    End,
    BeginFile,
    EndFile,
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Clone, Copy)]
pub struct Identifier<'a> {
    pub namespace: &'a str,
    pub literal: &'a str,
}

#[derive(Clone, Copy)]
pub enum Variable<'a> {
    User(Identifier<'a>),
    Nr,
    Nf,
    Fs,
    Rs,
    Ofs,
    Ors,
    Filename,
    Argc,
    Argv,
    Subsep,
    Fnr,
    Argind,
    Ofmt,
    Rstart,
    Rlength,
    Environ,
}

pub enum Expr<'a> {
    Leaf(Atom<'a>),
    Node(Box<'a, ExprNode<'a>>),
}

pub struct Body<'a>(pub Vec<'a, Statement<'a>>);
pub type Pattern<'a> = Either<RulePattern<'a>, SpecialPattern>;

#[derive(Debug)]
pub enum ExprNode<'a> {
    FunctionCall(Identifier<'a>, Vec<'a, Expr<'a>>),
    UnaryOperation(UnaryOperator, Expr<'a>),
    BinaryOperation(BinaryOperator, Expr<'a>, Expr<'a>),
    UnaryPlaceOperation(UnaryPlaceOperator, Place<'a>),
    BinaryPlaceOperation(BinaryPlaceOperator, Place<'a>, Expr<'a>),
    Ternary(Expr<'a>, Expr<'a>, Expr<'a>),
    Getline(Getline<'a>),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum UnaryOperator {
    Record,
    Negation,
    ToInt,
    Negative,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BinaryOperator {
    Concat,
    Eq,
    NEq,
    Gt,
    Lt,
    LtE,
    GtE,
    And,
    Or,
    MatchRegex,
    NotMatchRegex,
    Add,
    Subtract,
    Multiply,
    Divide,
    Raise,
    Modulo,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum UnaryPlaceOperator {
    IncrementL,
    DecrementL,
    IncrementR,
    DecrementR,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BinaryPlaceOperator {
    Assignment,
    AddAssign,
    SubAssign,
    MulAssign,
    DivAssign,
    PowAssign,
    ModAssign,
    ArrayAccess,
    InArray,
}

pub enum Place<'a> {
    Record(Expr<'a>),
    Variable(Variable<'a>),
    ArrayElement(Variable<'a>, Expr<'a>),
}

/// GNU docs: https://www.gnu.org/software/gawk/manual/html_node/Redirection.html
#[derive(Debug, Clone)]
pub enum Redirection<'a> {
    Write(WriteKind, Slice<'a>),
    WriteFile(&'a str),
    AppendFile(&'a str),
}

#[derive(Debug, Clone, Copy)]
pub enum WriteKind {
    Pipe,
    /// GNU Extension, `|&`.
    Coprocess,
}

#[derive(Debug)]
pub enum Getline<'a> {
    // getline (var)?
    FromInput(Option<Place<'a>>),
    // getline (var)? < (file)
    FromFile(Option<Place<'a>>, Expr<'a>),
    // (expr) | getline (var)?
    PipeOut(Option<Place<'a>>, Expr<'a>),
    // (expr) |& getline (var)?
    CoprocessOut(Option<Place<'a>>, Expr<'a>),
}

pub enum Statement<'a> {
    Expression(Expr<'a>),
    Command {
        name: Command,
        args: Vec<'a, Expr<'a>>,
        redirection: Option<Redirection<'a>>,
    },
    If {
        condition: Expr<'a>,
        then_body: Body<'a>,
        else_body: Option<Body<'a>>,
    },
    While {
        condition: Expr<'a>,
        then_body: Body<'a>,
    },
    DoWhile {
        then_body: Body<'a>,
        condition: Expr<'a>,
    },
    // TODO: Maybe Expr is too restrictive for init & update.
    // Maybe a subset of Statement?
    For {
        init: Option<Expr<'a>>,
        condition: Option<Expr<'a>>,
        update: Option<Expr<'a>>,
        body: Body<'a>,
    },
    ForEach {
        variable: Variable<'a>,
        array: Variable<'a>,
        body: Body<'a>,
    },
    Switch {
        scrutinee: Expr<'a>,
        branches: Vec<'a, (Atom<'a>, Body<'a>)>,
        default: Option<(Body<'a>, usize)>,
    },
    Break,
    Continue,
    Return(Option<Expr<'a>>),
    Next,
    NextFile,
    Exit(Option<Expr<'a>>),
}

#[derive(Debug)]
pub struct Function<'a> {
    pub args: Vec<'a, Identifier<'a>>,
    pub body: Body<'a>,
}

#[derive(Debug, Clone, Copy)]
pub enum Command {
    Print,
    Printf,
}

impl<'a> Expr<'a> {
    pub fn leaf(from: impl Into<Atom<'a>>) -> Self {
        Self::Leaf(from.into())
    }

    pub fn node(op: impl Into<ExprNode<'a>>, arena: &'a Bump) -> Self {
        Self::Node(Box::new_in(op.into(), arena))
    }
}

impl UnaryOperator {
    pub fn expr(self, a: Expr<'_>) -> ExprNode<'_> {
        ExprNode::UnaryOperation(self, a)
    }
}

impl BinaryOperator {
    pub fn expr<'a>(self, a: Expr<'a>, b: Expr<'a>) -> ExprNode<'a> {
        ExprNode::BinaryOperation(self, a, b)
    }
}

impl UnaryPlaceOperator {
    pub fn expr(self, a: Place<'_>) -> ExprNode<'_> {
        ExprNode::UnaryPlaceOperation(self, a)
    }
}

impl BinaryPlaceOperator {
    pub fn expr<'a>(self, a: Place<'a>, b: Expr<'a>) -> ExprNode<'a> {
        ExprNode::BinaryPlaceOperation(self, a, b)
    }
}

impl<'a> From<Getline<'a>> for ExprNode<'a> {
    fn from(value: Getline<'a>) -> Self {
        Self::Getline(value)
    }
}

impl Expr<'_> {
    pub fn take(&mut self) -> Self {
        std::mem::replace(self, Self::Leaf(Atom::Number(0.)))
    }
}

impl<'a> From<Identifier<'a>> for Variable<'a> {
    fn from(value: Identifier<'a>) -> Self {
        Self::User(value)
    }
}

impl<'a> From<Variable<'a>> for Atom<'a> {
    fn from(value: Variable<'a>) -> Self {
        Self::Variable(value)
    }
}

impl<'a> From<Variable<'a>> for Place<'a> {
    fn from(value: Variable<'a>) -> Self {
        Self::Variable(value)
    }
}

impl From<f64> for Atom<'_> {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl<'a> UnaryOperator {
    pub fn parse(value: &Token<'a>, span: &Span) -> Result<Self> {
        match value {
            Token::Record => Ok(Self::Record),
            Token::Negation => Ok(Self::Negation),
            Token::Plus => Ok(Self::ToInt),
            Token::Minus => Ok(Self::Negative),
            _ => Err(ParsingError::UnexpectedToken(
                span.clone(),
                "expected an unary operator.".into(),
            )),
        }
    }
}

impl<'a> BinaryOperator {
    pub fn parse(value: &Token<'a>, span: &Span) -> Result<Self> {
        match value {
            Token::EqualTo => Ok(Self::Eq),
            Token::NotEqualTo => Ok(Self::NEq),
            Token::GreaterThan => Ok(Self::Gt),
            Token::LesserThan => Ok(Self::Lt),
            Token::GreaterOrEqualThan => Ok(Self::GtE),
            Token::LesserOrEqualThan => Ok(Self::LtE),
            Token::Matching => Ok(Self::MatchRegex),
            Token::NotMatching => Ok(Self::NotMatchRegex),
            Token::Plus => Ok(Self::Add),
            Token::Minus => Ok(Self::Subtract),
            Token::Star => Ok(Self::Multiply),
            Token::Slash => Ok(Self::Divide),
            Token::BooleanAnd => Ok(Self::And),
            Token::BooleanOr => Ok(Self::Or),
            Token::Percent => Ok(Self::Modulo),
            t if t.is_expr_start() => Ok(Self::Concat),
            _ => Err(ParsingError::UnexpectedToken(
                span.clone(),
                "expected a binary operator.".into(),
            )),
        }
    }
}

impl UnaryPlaceOperator {
    pub fn parse_prefix(value: &Token<'_>, span: &Span) -> Result<Self> {
        match value {
            Token::Increment => Ok(Self::IncrementL),
            Token::Decrement => Ok(Self::DecrementL),
            _ => Err(ParsingError::OperatorExpectsVariable(span.clone())),
        }
    }

    pub fn parse_suffix(value: &Token<'_>, span: &Span) -> Result<Self> {
        match value {
            Token::Increment => Ok(Self::IncrementR),
            Token::Decrement => Ok(Self::DecrementR),
            _ => Err(ParsingError::OperatorExpectsVariable(span.clone())),
        }
    }
}

impl<'a> BinaryPlaceOperator {
    pub fn parse(value: &Token<'a>, span: &Span) -> Result<Self> {
        match value {
            Token::Assignment => Ok(Self::Assignment),
            Token::PlusAssign => Ok(Self::AddAssign),
            Token::MinusAssign => Ok(Self::SubAssign),
            Token::StarAssign => Ok(Self::MulAssign),
            Token::SlashAssign => Ok(Self::DivAssign),
            Token::CaretAssign => Ok(Self::PowAssign),
            Token::PercentAssign => Ok(Self::ModAssign),
            Token::OpenBracket => Ok(Self::ArrayAccess),
            Token::In => Ok(Self::InArray),
            _ => Err(ParsingError::UnexpectedToken(
                span.clone(),
                "expected a place operator.".into(),
            )),
        }
    }
}

impl<'a> Place<'a> {
    pub fn promote_from(expr: Expr<'a>, span: Span) -> Result<Self, (Expr<'a>, ParsingError)> {
        match expr {
            Expr::Leaf(Atom::Variable(var)) => Ok(Self::Variable(var)),
            Expr::Node(node)
                if matches!(
                    &*node,
                    &ExprNode::UnaryOperation(UnaryOperator::Record, _)
                        | &ExprNode::BinaryPlaceOperation(
                            BinaryPlaceOperator::ArrayAccess,
                            Place::Variable(_),
                            _
                        )
                ) =>
            {
                match Box::into_inner(node) {
                    ExprNode::UnaryOperation(_, index) => Ok(Self::Record(index)),
                    ExprNode::BinaryPlaceOperation(_, Place::Variable(var), index) => {
                        Ok(Self::ArrayElement(var, index))
                    }
                    _ => unreachable!("Box is magic; handled awkwardly in the match guard."),
                }
            }
            _ => Err((expr, ParsingError::OperatorExpectsVariable(span))),
        }
    }
}

impl WriteKind {
    pub fn parse(value: &Token) -> Option<Self> {
        match value {
            Token::Pipe => Some(Self::Pipe),
            Token::DoublePipe => Some(Self::Coprocess),
            _ => None,
        }
    }

    pub fn expr_getline<'a>(self, var: Option<Place<'a>>, expr: Expr<'a>) -> Getline<'a> {
        match self {
            Self::Pipe => Getline::PipeOut(var, expr),
            Self::Coprocess => Getline::CoprocessOut(var, expr),
        }
    }
}

pub struct Ternary;

mod binding_powers {
    pub const BP_GROUPING: u8 = 32; // [, (, ), ]
    pub const BP_RECORD: u8 = 30; // $
    pub const BP_INC_DEC: u8 = 28; // ++, --
    pub const BP_RAISE: (u8, u8) = (27, 26); // ^, **
    pub const BP_UNARY: u8 = 24; // +, -, !
    pub const BP_MULTI: (u8, u8) = (20, 21); // *, /, %
    pub const BP_ADDITION: (u8, u8) = (18, 19); // +, -
    pub const BP_CONCAT: (u8, u8) = (16, 17);
    pub const BP_CMP: (u8, u8) = (14, 15); // < <= == != > >= >> | |&
    pub const BP_MATCH: (u8, u8) = (12, 13); // ~, !~
    pub const BP_IN: (u8, u8) = (10, 11); // in
    pub const BP_AND: (u8, u8) = (8, 9); // &&
    pub const BP_OR: (u8, u8) = (6, 7); // ||
    pub const BP_TERNARY: (u8, u8) = (4, 3); // ?, :
    pub const BP_ASSIGN: (u8, u8) = (2, 1); // =, {+, -, *, /, %, ^, **}=
}

pub trait BindingPower {
    type Bp;
    fn binding_power(&self) -> Self::Bp;
}

impl BindingPower for BinaryOperator {
    type Bp = (u8, u8);
    fn binding_power(&self) -> Self::Bp {
        match self {
            Self::Concat => binding_powers::BP_CONCAT,
            Self::Eq | Self::NEq | Self::Gt | Self::Lt | Self::LtE | Self::GtE => {
                binding_powers::BP_CMP
            }
            Self::And => binding_powers::BP_AND,
            Self::Or => binding_powers::BP_OR,
            Self::MatchRegex | Self::NotMatchRegex => binding_powers::BP_MATCH,
            Self::Add => binding_powers::BP_ADDITION,
            Self::Subtract => binding_powers::BP_ADDITION,
            Self::Multiply => binding_powers::BP_MULTI,
            Self::Divide => binding_powers::BP_MULTI,
            Self::Raise => binding_powers::BP_RAISE,
            Self::Modulo => binding_powers::BP_RAISE,
        }
    }
}

impl BindingPower for UnaryPlaceOperator {
    type Bp = u8;
    fn binding_power(&self) -> Self::Bp {
        binding_powers::BP_INC_DEC
    }
}

impl BindingPower for BinaryPlaceOperator {
    type Bp = (u8, u8);
    fn binding_power(&self) -> Self::Bp {
        match self {
            Self::ArrayAccess => (binding_powers::BP_GROUPING, 0),
            Self::InArray => binding_powers::BP_IN,
            _ => binding_powers::BP_ASSIGN,
        }
    }
}

impl BindingPower for UnaryOperator {
    type Bp = u8;
    fn binding_power(&self) -> Self::Bp {
        match self {
            Self::Record => binding_powers::BP_RECORD,
            Self::Negation => binding_powers::BP_UNARY,
            Self::ToInt => binding_powers::BP_UNARY,
            Self::Negative => binding_powers::BP_UNARY,
        }
    }
}

impl BindingPower for Ternary {
    type Bp = (u8, u8);
    fn binding_power(&self) -> Self::Bp {
        binding_powers::BP_TERNARY
    }
}

impl<'a> From<Vec<'a, Statement<'a>>> for Body<'a> {
    fn from(value: Vec<'a, Statement<'a>>) -> Self {
        Self(value)
    }
}

impl Debug for Expr<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Leaf(atom) => write!(f, "{atom:?}"),
            Self::Node(expr) => match expr.as_ref() {
                ExprNode::FunctionCall(ident, args) => {
                    write!(f, "({ident:?}")?;
                    for arg in args {
                        write!(f, " {arg:?}")?;
                    }
                    write!(f, ")")
                }
                ExprNode::UnaryOperation(op, a) => write!(f, "({op:?} {a:?})"),
                ExprNode::BinaryOperation(op, a, b) => write!(f, "({op:?} {a:?} {b:?})"),
                ExprNode::BinaryPlaceOperation(op, a, b) => write!(f, "({op:?} {a:?} {b:?})"),
                ExprNode::UnaryPlaceOperation(op, a) => write!(f, "({op:?} {a:?})"),
                ExprNode::Ternary(a, b, c) => write!(f, "(?: {a:?} {b:?} {c:?})"),
                ExprNode::Getline(getline) => match getline {
                    Getline::FromInput(Some(a)) => write!(f, "(getline {a:?})"),
                    Getline::FromInput(None) => write!(f, "(getline)"),
                    Getline::FromFile(Some(a), b) => write!(f, "(getline< {b:?} {a:?})"),
                    Getline::FromFile(None, b) => write!(f, "(getline< {b:?})"),
                    Getline::PipeOut(Some(a), b) => write!(f, "(getline| {b:?} {a:?})"),
                    Getline::PipeOut(None, b) => write!(f, "(getline| {b:?})"),
                    Getline::CoprocessOut(Some(a), b) => write!(f, "(getline|& {b:?} {a:?})"),
                    Getline::CoprocessOut(None, b) => write!(f, "(getline|& {b:?})"),
                },
            },
        }
    }
}
