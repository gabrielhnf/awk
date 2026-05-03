// This file is part of the uutils awk package.
//
// For the full copyright and license information, please view the LICENSE
// files that was distributed with this source code.

use std::fmt::Debug;

use bumpalo::{Bump, collections::Vec};
use either::Either;
use hashbrown::{DefaultHashBuilder, HashMap};
use lexer::{Slice, Span, Token};

use crate::{ParsingError, Result, lex::TokenExt};

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Debug, Clone)]
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

#[derive(Clone)]
pub enum Expr<'a> {
    Leaf(Atom<'a>),
    Node(&'a ExprNode<'a>),
}

#[derive(Clone)]
pub struct Body<'a>(pub Vec<'a, Statement<'a>>);
pub type Pattern<'a> = Either<RulePattern<'a>, SpecialPattern>;

#[derive(Debug, Clone)]
pub enum ExprNode<'a> {
    FunctionCall(Identifier<'a>, Vec<'a, Expr<'a>>),
    UnaryOperation(UnaryOperator, Expr<'a>),
    BinaryOperation(BinaryOperator, Expr<'a>, Expr<'a>),
    PlaceOperation(PlaceOperator, Variable<'a>, Expr<'a>),
    Ternary(Expr<'a>, Expr<'a>, Expr<'a>),
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
pub enum PlaceOperator {
    Assignment,
    ArrayAccess,
    InArray,
}

/// GNU docs: https://www.gnu.org/software/gawk/manual/html_node/Redirection.html
#[derive(Debug, Clone)]
pub enum Redirection<'a> {
    Pipe(&'a str),
    Write(&'a str),
    Append(&'a str),
    /// GNU Extension.
    WriteCoprocess(&'a [u8]),
    /// GNU Extension; only on `getline`.
    ReadCoprocess(&'a [u8]),
    WriteVariable(),
}

#[derive(Clone)]
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
        place: Variable<'a>,
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
}

#[derive(Debug, Clone)]
pub struct Function<'a> {
    pub args: Vec<'a, Identifier<'a>>,
    pub body: Body<'a>,
}

#[derive(Debug, Clone)]
pub enum Command {
    Print,
    Printf,
    Getline,
    Next,
    NextFile,
    Exit,
}

#[derive(Debug, Clone)]
pub enum CommandArity {
    Nullary,
    Unary,
    Variadic,
}

impl Command {
    pub fn arity(&self) -> CommandArity {
        match self {
            Self::Next | Self::NextFile => CommandArity::Nullary,
            Self::Getline | Self::Exit => CommandArity::Unary,
            Self::Print | Self::Printf => CommandArity::Variadic,
        }
    }
}

impl<'a> Expr<'a> {
    pub fn leaf(from: impl Into<Atom<'a>>) -> Self {
        Self::Leaf(from.into())
    }

    pub fn node(op: ExprNode<'a>, arena: &'a Bump) -> Self {
        Self::Node(arena.alloc(op))
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

impl PlaceOperator {
    pub fn expr<'a>(self, a: Variable<'a>, b: Expr<'a>) -> ExprNode<'a> {
        ExprNode::PlaceOperation(self, a, b)
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
            t if t.is_expr_start() => Ok(Self::Concat),
            _ => Err(ParsingError::UnexpectedToken(
                span.clone(),
                "expected a binary operator.".into(),
            )),
        }
    }
}

impl<'a> PlaceOperator {
    pub fn parse(value: &Token<'a>, span: &Span) -> Result<Self> {
        match value {
            Token::Assignment
            | Token::PlusAssign
            | Token::MinusAssign
            | Token::StarAssign
            | Token::SlashAssign
            | Token::CaretAssign
            | Token::PercentAssign => Ok(Self::Assignment),
            Token::OpenBracket => Ok(Self::ArrayAccess),
            Token::In => Ok(Self::InArray),
            _ => Err(ParsingError::UnexpectedToken(
                span.clone(),
                "expected a place operator.".into(),
            )),
        }
    }
}

impl BinaryOperator {
    pub fn unfold(token: &Token) -> Option<Self> {
        match token {
            Token::PlusAssign => Some(Self::Add),
            Token::MinusAssign => Some(Self::Subtract),
            Token::StarAssign => Some(Self::Multiply),
            Token::SlashAssign => Some(Self::Divide),
            Token::PercentAssign => Some(Self::Modulo),
            Token::CaretAssign => Some(Self::Raise),
            _ => None,
        }
    }

    pub fn unfold_prefix(token: &Token<'_>) -> Option<(Self, u8)> {
        match token {
            Token::Increment => Some((Self::Add, binding_powers::BP_INC_DEC)),
            Token::Decrement => Some((Self::Subtract, binding_powers::BP_INC_DEC)),
            _ => None,
        }
    }
    pub fn unfold_suffix(token: &Token<'_>) -> Option<(Self, Self, u8)> {
        match token {
            Token::Increment => Some((Self::Add, Self::Subtract, binding_powers::BP_INC_DEC)),
            Token::Decrement => Some((Self::Subtract, Self::Add, binding_powers::BP_INC_DEC)),
            _ => None,
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

impl BindingPower for PlaceOperator {
    type Bp = (u8, u8);
    fn binding_power(&self) -> Self::Bp {
        match self {
            Self::Assignment => binding_powers::BP_ASSIGN,
            Self::ArrayAccess => (binding_powers::BP_GROUPING, 0),
            Self::InArray => binding_powers::BP_IN,
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
            Self::Node(expr) => match expr {
                ExprNode::FunctionCall(ident, args) => {
                    write!(f, "({ident:?}")?;
                    for arg in args {
                        write!(f, " {arg:?}")?;
                    }
                    write!(f, ")")
                }
                ExprNode::UnaryOperation(op, a) => write!(f, "({op:?} {a:?})"),
                ExprNode::BinaryOperation(op, a, b) => write!(f, "({op:?} {a:?} {b:?})"),
                ExprNode::PlaceOperation(op, a, b) => write!(f, "({op:?} {a:?} {b:?})"),
                ExprNode::Ternary(a, b, c) => write!(f, "(?: {a:?} {b:?} {c:?})"),
            },
        }
    }
}
