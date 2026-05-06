use ariadne::{Color, Label, Report, ReportKind, Source};
use lexer::{LexingError, Span};
use thiserror::Error;

#[derive(Debug, Error, Clone)]
pub enum ParsingError {
    #[error("{}", .0)]
    LexingError(LexingError),
    #[error("Unclosed scope.")]
    UnclosedScope(Span),
    #[error("Unexpected token: {}", .1)]
    UnexpectedToken(Span, String),
    #[error("Duplicated argument `{}` to function `{}`.", .1, .2)]
    DuplicatedArgument(Span, String, String),
    #[error("Expected statement end.")]
    ExpectedStatementEnd(Span),
    #[error("Expected opening brace `{{`.")]
    ExpectedOpeningBrace(Span),
    #[error("Expected parenthesis `(`.")]
    ExpectedOpeningParenthesis(Span),
    #[error("Malformed for loop.")]
    InvalidForLoop(Span),
    #[error("All case branches must be followed by a colon `:`.")]
    ColonMustFollowCase(Span),
    #[error("There may only be one default branch in a switch statement.")]
    DuplicatedDefaultBranch(Span),
    #[error("Switch statements must start with a case or default branch.")]
    MissingSwitchBranch(Span),
    #[error("Case values may only be literal values; not variables or expressions.")]
    InvalidCaseValue(Span),
    #[error("Expected a while statement after a do block.")]
    MissingWhileAfterDo(Span),
    #[error("This statement must have its operand wrapped in parenthesis.")]
    MissingParenthesisInStatement(Span),
    #[error("Missing closing parenthesis `(` in statement operand.")]
    UnclosedParenthesisInStatement(Span),
    #[error("Missing function signature for `{}`.", .1)]
    NoFunctionSignature(Span, String),
    #[error("Missing closing parenthesis `(` in function `{}`'s signature.", .1)]
    UnclosedSignature(Span, String),
    #[error("Missing closing parenthesis `(` in expression.")]
    UnclosedParenthesisExpression(Span),
    #[error("Missing closing bracket `]` in array access.")]
    UnclosedArrayAccess(Span),
    #[error("Expected operand to be a variable.")]
    OperatorExpectsVariable(Span),
    #[error("Malformed expression.")]
    InvalidExpression(Span),
    #[error("Missing alternate branch in ternary expression.")]
    MissingTernaryOr(Span),
    #[error("Missing closing parenthesis in function call to `{}`.", .1)]
    FunctionCallMissingParenthesis(Span, String),
    #[error("Functions calls must have their name yuxtaposed to the parenthesis `(`.")]
    FunctionCallSeparatedIdent(Span),
    #[error("Missing closing parenthesis `(` in function call to `{}`.", .1)]
    FunctionCallUnclosed(Span, String),
    #[error("Expected to be an identifier.")]
    ExpectedIdentifier(Span),
    #[error("Expected an unary operation.")]
    ExpectedUnaryOperator(Span),
    #[error("Expected a binary operation")]
    ExpectedBinaryOperator(Span),
    #[error("Expected a placing operation.")]
    ExpectedPlaceOperator(Span),
}

impl ParsingError {
    pub fn span(&self) -> Option<Span> {
        match self {
            Self::LexingError(err) => match err {
                LexingError::Unknown => panic!("Unknown lexing error!"),
                LexingError::Unexpected(span, _) => Some(span.clone()),
                LexingError::UnterminatedString(span) => Some(span.clone()),
                LexingError::UnterminatedRegex(span) => Some(span.clone()),
                LexingError::UnexpectedEof => None,
            },
            Self::UnclosedScope(span) => Some(span.clone()),
            Self::UnexpectedToken(span, _) => Some(span.clone()),
            Self::DuplicatedArgument(span, _, _) => Some(span.clone()),
            Self::ExpectedStatementEnd(span) => Some(span.clone()),
            Self::ExpectedOpeningBrace(span) => Some(span.clone()),
            Self::ExpectedOpeningParenthesis(span) => Some(span.clone()),
            Self::InvalidForLoop(span) => Some(span.clone()),
            Self::ColonMustFollowCase(span) => Some(span.clone()),
            Self::DuplicatedDefaultBranch(span) => Some(span.clone()),
            Self::MissingSwitchBranch(span) => Some(span.clone()),
            Self::InvalidCaseValue(span) => Some(span.clone()),
            Self::MissingWhileAfterDo(span) => Some(span.clone()),
            Self::MissingParenthesisInStatement(span) => Some(span.clone()),
            Self::UnclosedParenthesisInStatement(span) => Some(span.clone()),
            Self::NoFunctionSignature(span, _) => Some(span.clone()),
            Self::UnclosedSignature(span, _) => Some(span.clone()),
            Self::UnclosedParenthesisExpression(span) => Some(span.clone()),
            Self::UnclosedArrayAccess(span) => Some(span.clone()),
            Self::OperatorExpectsVariable(span) => Some(span.clone()),
            Self::InvalidExpression(span) => Some(span.clone()),
            Self::MissingTernaryOr(span) => Some(span.clone()),
            Self::FunctionCallMissingParenthesis(span, _) => Some(span.clone()),
            Self::FunctionCallSeparatedIdent(span) => Some(span.clone()),
            Self::FunctionCallUnclosed(span, _) => Some(span.clone()),
            Self::ExpectedIdentifier(span) => Some(span.clone()),
            Self::ExpectedUnaryOperator(span) => Some(span.clone()),
            Self::ExpectedBinaryOperator(span) => Some(span.clone()),
            Self::ExpectedPlaceOperator(span) => Some(span.clone()),
        }
    }
    fn hint(&self) -> Option<&'static str> {
        match self {
            Self::DuplicatedArgument(_, _, _) => Some("Consider giving the argument another name."),
            Self::ExpectedStatementEnd(_) => Some(
                "Valid statement ends are newlines, semicolons `;` and right brackets `}` if on a block.",
            ),
            Self::InvalidForLoop(_) => Some(
                "Valid syntaxes are `for (init; condition; end)` and `for (element in array)`.",
            ),
            Self::ColonMustFollowCase(_) => Some("Consider appending a colon like so: `case 1:`"),
            Self::InvalidCaseValue(_) => {
                Some("Consider an if statement if you need to check against an expression.")
            }
            Self::NoFunctionSignature(_, _) => {
                Some("Declare the signature as `foo()` if you require no arguments.")
            }
            Self::OperatorExpectsVariable(_) => Some(
                "This operand must modify the value of a variable. Consider alternatives like `+` or `-`.",
            ),
            Self::MissingTernaryOr(_) => Some(
                "Ternaries select between to expression based on a condition, like `bool ? foo : bar`.",
            ),
            _ => None,
        }
    }
}

pub fn report_error<'a>(
    error: ParsingError,
    name: &'a str,
    source: &'a [u8],
) -> super::AriadneErr<'a> {
    let span = error.span().unwrap();
    let source = str::from_utf8(source).unwrap();
    let mut report = Report::build(ReportKind::Error, (name, span.clone()))
        .with_message("Syntax error")
        .with_label(
            Label::new((name, span.clone()))
                .with_message(format!("{error}"))
                .with_color(Color::Red),
        );
    if let Some(str) = error.hint() {
        report.set_help(str);
    }
    (Box::new(report.finish()), Source::from(source))
}

impl<T> From<(T, Self)> for ParsingError {
    fn from(value: (T, Self)) -> Self {
        value.1
    }
}
