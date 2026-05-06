use std::fmt::{Debug, Display, Formatter, Result, Write};

use crate::{
    Ast, Function,
    ast::{
        Atom, BinaryOperator, BinaryPlaceOperator, BindingPower, Body, Command, Expr, ExprNode,
        Getline, Place, Rule, RulePattern, SimpleStatement, Statement, Ternary, UnaryOperator,
        UnaryPlaceOperator, Variable,
    },
};

/// Bit trick to pack `{ indent: u8, bp: u8 }` inside `Formatter::width`.
fn encode(indent: u8, prec: u8) -> usize {
    ((indent as usize) << 8) | (prec as usize)
}

/// Bit trick to unpack the `{ indent: u8, bp: u8 }` inside `Formatter::width`.
fn decode(f: &Formatter<'_>) -> (u8, u8) {
    let w = f.width().unwrap_or(0);
    ((w >> 8) as u8, (w & 0xFF) as u8)
}

impl Display for Ast<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        for load in &self.loads {
            writeln!(f, "@load \"{load}\"")?;
        }
        for body in &self.begin {
            write!(f, "BEGIN ")?;
            write_body_ln(f, body, 0)?;
            writeln!(f)?;
        }
        for body in &self.begin_file {
            write!(f, "BEGINFILE ")?;
            write_body_ln(f, body, 0)?;
            writeln!(f)?;
        }
        for rule in &self.rules {
            writeln!(f, "{rule}")?;
        }
        for body in &self.end_file {
            write!(f, "ENDFILE ")?;
            write_body_ln(f, body, 0)?;
            writeln!(f)?;
        }
        for body in &self.end {
            write!(f, "END ")?;
            write_body_ln(f, body, 0)?;
            writeln!(f)?;
        }
        for (fun, Function { args, body }) in &self.functions {
            // GNU appears to place two newlines before functions.
            // I think it's a bug but we'll keep it.
            write!(f, "\nfunction {fun}(")?;
            write_args(f, args, 0)?;
            writeln!(f, ") ")?;
            write_body_ln(f, body, 0)?;
        }
        Ok(())
    }
}

impl Display for Body<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let (indent, _) = decode(f);
        for stmnt in &self.0 {
            write!(f, "{stmnt:width$}", width = encode(indent, 0))?;
            writeln!(f)?;
        }
        Ok(())
    }
}

impl Display for Statement<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let (indent, _) = decode(f);
        let ew = encode(indent, 0);

        match self {
            Self::Simple(simple) => <_ as Display>::fmt(simple, f),
            Self::If {
                condition,
                then_body,
                else_body,
            } => {
                write!(f, "if ({condition:ew$}) ")?;
                write_body(f, then_body, indent)?;
                if let Some(else_body) = else_body {
                    write!(f, " else ")?;
                    write_body(f, else_body, indent)?;
                }
                Ok(())
            }
            Self::While {
                condition,
                then_body,
            } => {
                write!(f, "while ({condition:ew$}) ")?;
                write_body(f, then_body, indent)
            }
            Self::DoWhile {
                then_body,
                condition,
            } => {
                write!(f, "do ")?;
                write_body(f, then_body, indent)?;
                write!(f, " while ({condition:ew$})")
            }
            Self::For {
                init,
                condition,
                update,
                body,
            } => {
                write!(f, "for (")?;
                if let Some(e) = init {
                    write!(f, "{e:ew$}")?;
                }
                write!(f, ";")?;
                if let Some(e) = condition {
                    write!(f, " {e:ew$}")?;
                }
                write!(f, ";")?;
                if let Some(e) = update {
                    write!(f, " {e:ew$}")?;
                }
                write!(f, ") ")?;
                write_body(f, body, indent)
            }
            Self::ForEach {
                variable,
                array,
                body,
            } => {
                write!(f, "for ({variable} in {array}) ")?;
                write_body(f, body, indent)
            }
            Self::Switch {
                scrutinee,
                branches,
                default,
            } => {
                writeln!(f, "switch ({scrutinee:ew$}) {{")?;
                let default_pos = default.as_ref().map_or(branches.len(), |x| x.1);
                for i in 0..default_pos {
                    let (case, branch) = &branches[i];
                    tabs(f, indent)?;
                    writeln!(f, "case {case}:")?;
                    write_stmnts(f, branch, indent + 1)?;
                }
                if let Some((body, pos)) = default {
                    tabs(f, indent)?;
                    writeln!(f, "default:")?;
                    write_stmnts(f, body, indent + 1)?;
                    for i in *pos..branches.len() {
                        let (case, branch) = &branches[i];
                        tabs(f, indent)?;
                        writeln!(f, "case {case}:")?;
                        write_stmnts(f, branch, indent + 1)?;
                    }
                }
                tabs(f, indent)?;
                f.write_char('}')
            }
            Self::Break => write!(f, "break"),
            Self::Continue => write!(f, "continue"),
            Self::Return(Some(expr)) => write!(f, "return {expr:ew$}"),
            Self::Return(None) => write!(f, "return"),
            Self::Exit(Some(expr)) => write!(f, "exit {expr:ew$}"),
            Self::Exit(None) => write!(f, "exit"),
            Self::Next => write!(f, "next"),
            Self::NextFile => write!(f, "nextfile"),
        }
    }
}

impl Display for SimpleStatement<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let (indent, _) = decode(f);
        let ew = encode(indent, 0);

        match self {
            SimpleStatement::Expression(expr) => write!(f, "{expr:ew$}"),
            SimpleStatement::Command {
                name,
                args,
                redirection,
            } => {
                write!(f, "{name} ")?;
                write_args(f, args, indent)?;
                if let Some(_redir) = redirection {
                    write!(f, "TODO")?;
                }
                Ok(())
            }
            SimpleStatement::Delete(array, Some(index)) => write!(f, "delete {array}[{index}]"),
            SimpleStatement::Delete(array, None) => write!(f, "delete {array}"),
        }
    }
}

impl Display for Rule<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match (&self.pattern, &self.actions) {
            (None, None) => Ok(()),
            (None, Some(body)) => write_body_ln(f, body, 0),
            (Some(pat), None) => writeln!(f, "{pat};"),
            (Some(pat), Some(body)) => {
                write!(f, "{pat} ")?;
                write_body_ln(f, body, 0)
            }
        }
    }
}

impl Display for RulePattern<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::Range(a, b) => write!(f, "{a}, {b}"),
            Self::Expression(x) => write!(f, "{x}"),
        }
    }
}

impl Display for Expr<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let (indent, prec) = decode(f);
        match self {
            Expr::Leaf(atom) => write!(f, "{atom}"),
            Expr::Node(node) => write!(f, "{node:width$}", width = encode(indent, prec)),
        }
    }
}

impl Display for Atom<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        <Self as Debug>::fmt(self, f)
    }
}

impl Display for Variable<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        <Self as Debug>::fmt(self, f)
    }
}

impl Display for Place<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::Variable(var) => <_ as Display>::fmt(var, f),
            Self::Record(Expr::Leaf(leaf)) => write!(f, "${leaf}"),
            Self::Record(Expr::Node(node)) => write!(f, "$({node})"),
            Self::ArrayElement(var, expr) => write!(f, "{var}[{expr}]"),
        }
    }
}

impl Display for ExprNode<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let (indent, parent_bp) = decode(f);

        match self {
            Self::FunctionCall(fun, args) => {
                write!(f, "{fun}(")?;
                write_args(f, args, indent)?;
                write!(f, ")")
            }
            Self::UnaryOperation(op, x) => {
                let bp = op.binding_power();
                let child_w = encode(indent, bp.saturating_add(1));
                if bp < parent_bp {
                    write!(f, "({op}{x:child_w$})")
                } else {
                    write!(f, "{op}{x:child_w$}")
                }
            }
            Self::BinaryOperation(op, a, b) => {
                let (left_bp, right_bp) = op.binding_power();
                let left_w = encode(indent, left_bp);
                let right_w = encode(indent, right_bp);
                if left_bp < parent_bp {
                    write!(f, "({a:left_w$}{op}{b:right_w$})")
                } else {
                    write!(f, "{a:left_w$}{op}{b:right_w$}")
                }
            }
            Self::UnaryPlaceOperation(op, place) => match op {
                UnaryPlaceOperator::IncrementL => write!(f, "++{place}"),
                UnaryPlaceOperator::DecrementL => write!(f, "--{place}"),
                UnaryPlaceOperator::DecrementR => write!(f, "{place}--"),
                UnaryPlaceOperator::IncrementR => write!(f, "{place}++"),
            },
            Self::BinaryPlaceOperation(op, place, idx) => {
                let (left_bp, right_bp) = op.binding_power();
                if op == &BinaryPlaceOperator::ArrayAccess {
                    let idx_w = encode(indent, 0);
                    if left_bp < parent_bp {
                        write!(f, "({place}[{idx:idx_w$}])")
                    } else {
                        write!(f, "{place}[{idx:idx_w$}]")
                    }
                } else {
                    let right_w = encode(indent, right_bp);
                    if left_bp < parent_bp {
                        write!(f, "({place}{op}{idx:right_w$})")
                    } else {
                        write!(f, "{place}{op}{idx:right_w$}")
                    }
                }
            }
            Self::Ternary(cond, then_expr, else_expr) => {
                let ternary_bp = Ternary.binding_power().0;
                let inner_w = encode(indent, 0);
                if ternary_bp < parent_bp {
                    write!(
                        f,
                        "({cond:inner_w$} ? {then_expr:inner_w$} : {else_expr:inner_w$})"
                    )
                } else {
                    write!(
                        f,
                        "{cond:inner_w$} ? {then_expr:inner_w$} : {else_expr:inner_w$}"
                    )
                }
            }
            Self::Getline(getline) => match getline {
                Getline::FromInput(Some(var)) => write!(f, "getline {var}"),
                Getline::FromInput(None) => write!(f, "getline"),
                Getline::FromFile(Some(var), file) => write!(f, "getline {var} < {file}"),
                Getline::FromFile(None, file) => write!(f, "getline < {file}"),
                Getline::PipeOut(Some(place), e) => write!(f, "{e} | getline {place}"),
                Getline::PipeOut(None, e) => write!(f, "{e} | getline"),
                Getline::CoprocessOut(Some(place), e) => write!(f, "{e} |& getline {place}"),
                Getline::CoprocessOut(None, e) => write!(f, "{e} |& getline"),
            },
        }
    }
}

impl Display for UnaryOperator {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::Record => write!(f, "$"),
            Self::Negation => write!(f, "!"),
            Self::ToInt => write!(f, "+"),
            Self::Negative => write!(f, "-"),
        }
    }
}

impl Display for BinaryOperator {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::Concat => write!(f, " "),
            Self::Eq => write!(f, " == "),
            Self::NEq => write!(f, " != "),
            Self::Gt => write!(f, " > "),
            Self::Lt => write!(f, " < "),
            Self::LtE => write!(f, " <= "),
            Self::GtE => write!(f, " >= "),
            Self::And => write!(f, " && "),
            Self::Or => write!(f, " || "),
            Self::MatchRegex => write!(f, " ~ "),
            Self::NotMatchRegex => write!(f, " !~ "),
            Self::Add => write!(f, " + "),
            Self::Subtract => write!(f, " - "),
            Self::Multiply => write!(f, " * "),
            Self::Divide => write!(f, " / "),
            Self::Raise => write!(f, " ^ "),
            Self::Modulo => write!(f, " % "),
        }
    }
}

impl Display for BinaryPlaceOperator {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::Assignment => write!(f, " = "),
            Self::AddAssign => write!(f, " += "),
            Self::SubAssign => write!(f, " -= "),
            Self::MulAssign => write!(f, " *= "),
            Self::DivAssign => write!(f, " /= "),
            Self::PowAssign => write!(f, " ^= "),
            Self::ModAssign => write!(f, " %= "),
            Self::InArray => write!(f, " in "),
            Self::ArrayAccess => unreachable!("Array access needs special handling."),
        }
    }
}

impl Display for Command {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::Print => write!(f, "print"),
            Self::Printf => write!(f, "printf"),
        }
    }
}

fn write_args(f: &mut Formatter<'_>, args: &[impl Display], indent: u8) -> Result {
    let ew = encode(indent, 0);
    for (i, arg) in args.iter().enumerate() {
        if i != 0 {
            write!(f, ", ")?;
        }
        write!(f, "{arg:ew$}")?;
    }
    Ok(())
}

fn write_stmnts(f: &mut Formatter<'_>, body: &Body, indent: u8) -> Result {
    let sw = encode(indent, 0);
    for stmnt in &body.0 {
        tabs(f, indent)?;
        writeln!(f, "{stmnt:sw$}")?;
    }
    Ok(())
}

fn write_body(f: &mut Formatter<'_>, body: &Body, indent: u8) -> Result {
    writeln!(f, "{{")?;
    write_stmnts(f, body, indent + 1)?;
    tabs(f, indent)?;
    f.write_char('}')
}

fn write_body_ln(f: &mut Formatter<'_>, body: &Body, indent: u8) -> Result {
    write_body(f, body, indent)?;
    writeln!(f)
}

fn tabs(f: &mut Formatter<'_>, indent: u8) -> Result {
    for _ in 0..indent {
        f.write_char('\t')?;
    }
    Ok(())
}
