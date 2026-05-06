// This file is part of the uutils awk package.
//
// For the full copyright and license information, please view the LICENSE
// files that was distributed with this source code.

use std::fmt::{Debug, Display, Formatter, Result};

use crate::ast::{Atom, Body, Expr, Identifier, Place, Statement, Variable};

const PRETTY_PRINT_INDENT: usize = 2;

fn fmt_vars(f: &mut Formatter<'_>) -> (bool, usize, String) {
    let ni = f.width().unwrap_or(0) + PRETTY_PRINT_INDENT;
    (f.alternate(), ni, " ".repeat(ni))
}

impl Debug for Statement<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let (alt, ni, pad) = fmt_vars(f);

        match self {
            Statement::Expression(expr) => {
                if alt {
                    write!(f, "{expr:#ni$?}")
                } else {
                    write!(f, "{expr:?}")
                }
            }
            Self::Command {
                name,
                args,
                redirection,
            } => {
                if let Some(rx) = redirection {
                    if alt {
                        write!(
                            f,
                            "(redir {rx:?}\n{pad}({name:?}{:#ni$?}))",
                            ListLispFmt(args),
                        )
                    } else {
                        write!(f, "(redir {rx:?} ({name:?}{:?}))", ListLispFmt(args))
                    }
                } else {
                    write!(f, "({name:?}{:?})", ListLispFmt(args))
                }
            }
            Self::If {
                condition,
                then_body,
                else_body,
            } => {
                if alt {
                    write!(f, "(if {condition:?}\n{pad}")?;
                    write!(f, "{then_body:#ni$?}")?;
                    if let Some(else_body) = else_body {
                        write!(
                            f,
                            "\n{}(else\n{pad}{else_body:#ni$?}))",
                            &pad[PRETTY_PRINT_INDENT..],
                        )
                    } else {
                        write!(f, ")")
                    }
                } else if let Some(else_body) = else_body {
                    write!(f, "(if {condition:?} {then_body:?} (else {else_body:?}))")
                } else {
                    write!(f, "(if {condition:?} {then_body:?})")
                }
            }
            Self::While {
                condition,
                then_body,
            } => {
                if alt {
                    write!(f, "(while {condition:?}\n{pad}{then_body:#ni$?})")
                } else {
                    write!(f, "(while {condition:?} {then_body:?})")
                }
            }
            Self::DoWhile {
                then_body,
                condition,
            } => {
                if alt {
                    write!(f, "(do-while\n{pad}{then_body:#ni$?}\n{pad}{condition:?})")
                } else {
                    write!(f, "(do-while {then_body:?} {condition:?})")
                }
            }
            Self::For {
                init,
                condition,
                update,
                body,
            } => {
                if alt {
                    write!(f, "(for")?;
                    let write_fragment = |f: &mut Formatter, x: &Option<Expr>| {
                        if let Some(x) = x {
                            write!(f, "\n{pad}{x:?}")
                        } else {
                            write!(f, "\n{pad}None")
                        }
                    };
                    write_fragment(f, init)?;
                    write_fragment(f, condition)?;
                    write_fragment(f, update)?;
                    write!(f, "\n{pad}{body:#ni$?})")
                } else {
                    write!(f, "(for {init:?} {condition:?} {update:?} {body:?})")
                }
            }
            Self::ForEach {
                variable,
                array,
                body,
            } => {
                if alt {
                    write!(f, "(for-each {variable:?} {array:?}\n{pad}{body:#ni$?})")
                } else {
                    write!(f, "(for-each {variable:?} {array:?} {body:?})")
                }
            }
            Self::Switch {
                scrutinee,
                branches,
                default,
            } => {
                if alt {
                    if let Some((dx, i)) = default {
                        write!(
                            f,
                            "(switch {scrutinee:?}{:#ni$?}\n{pad}(default {i}\n{pad}  {dx:#width$?}))",
                            ListLispCasesFmt(branches.as_slice()),
                            width = ni + PRETTY_PRINT_INDENT,
                        )
                    } else {
                        write!(
                            f,
                            "(switch {scrutinee:?}\n{pad}(cases{:#width$?}))",
                            ListLispCasesFmt(branches),
                            width = ni
                        )
                    }
                } else {
                    if let Some((dx, i)) = default {
                        write!(
                            f,
                            "(switch {scrutinee:?}{:?} (default {i} {dx:?}))",
                            ListLispCasesFmt(branches.as_slice())
                        )
                    } else {
                        write!(f, "(switch {scrutinee:?}{:?})", ListLispCasesFmt(branches))
                    }
                }
            }
            Self::Continue => write!(f, "(continue)"),
            Self::Break => write!(f, "(break)"),
            Self::Return(Some(expr)) => write!(f, "(return {expr:?})"),
            Self::Return(None) => write!(f, "(return)"),
            Self::Exit(Some(expr)) => write!(f, "(exit {expr:?})"),
            Self::Exit(None) => write!(f, "(exit)"),
            Self::Next => write!(f, "(next)"),
            Self::NextFile => write!(f, "(nextfile)"),
        }
    }
}

struct ListLispFmt<'a, T: Debug>(&'a [T]);
impl<T: Debug> Debug for ListLispFmt<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let (alt, ni, pad) = fmt_vars(f);
        for e in self.0 {
            if alt {
                write!(f, "\n{pad}{e:#ni$?}")?;
            } else {
                write!(f, " {e:?}")?;
            }
        }
        Ok(())
    }
}

struct ListLispCasesFmt<'a, T: Debug>(&'a [(T, Body<'a>)]);
impl<T: Debug> Debug for ListLispCasesFmt<'_, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let (alt, ni, pad) = fmt_vars(f);
        let (ni, pad) = (ni - PRETTY_PRINT_INDENT, &pad[PRETTY_PRINT_INDENT..]);
        for (i, e) in self.0 {
            if alt {
                write!(
                    f,
                    "\n{pad}(case {i:?}\n{pad}  {e:#width$?})",
                    width = ni + PRETTY_PRINT_INDENT
                )?;
            } else {
                write!(f, " (case {i:?} {e:?})")?;
            }
        }
        Ok(())
    }
}

impl Debug for Identifier<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{}::{}", self.namespace, self.literal)
    }
}

impl Display for Identifier<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        <&str as Display>::fmt(&self.literal, f)
    }
}

impl Debug for Body<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        let (alt, ni, pad) = fmt_vars(f);
        write!(f, "(body")?;
        for e in &self.0 {
            if alt {
                write!(f, "\n{pad}{e:#ni$?}")?;
            } else {
                write!(f, " {e:?}")?;
            }
        }
        write!(f, ")")
    }
}

impl Debug for Atom<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::Variable(var) => write!(f, "{var:?}"),
            Self::String(str) => write!(f, "{str:?}"),
            Self::Number(num) => write!(f, "{num}"),
            Self::Regex(rgx) => write!(f, "/{rgx}/"),
            Self::BigInt() | Self::BigFloat() => unimplemented!(),
        }
    }
}

impl Debug for Place<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::Record(expr) => write!(f, "($ {expr:?})"),
            Self::Variable(var) => <_ as Debug>::fmt(var, f),
            Self::ArrayElement(var, index) => write!(f, "(index {var:?} {index:?})"),
        }
    }
}

impl Debug for Variable<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match self {
            Self::User(ident) => <Identifier as Debug>::fmt(ident, f),
            Self::Nr => write!(f, "NR"),
            Self::Nf => write!(f, "NF"),
            Self::Fs => write!(f, "FS"),
            Self::Rs => write!(f, "RS"),
            Self::Ofs => write!(f, "OFS"),
            Self::Ors => write!(f, "ORS"),
            Self::Filename => write!(f, "FILENAME"),
            Self::Argc => write!(f, "ARGC"),
            Self::Argv => write!(f, "ARGV"),
            Self::Subsep => write!(f, "SUBSEP"),
            Self::Fnr => write!(f, "FNR"),
            Self::Argind => write!(f, "ARGIND"),
            Self::Ofmt => write!(f, "OFMT"),
            Self::Rstart => write!(f, "RSTART"),
            Self::Rlength => write!(f, "RLENGTH"),
            Self::Environ => write!(f, "ENVIRON"),
        }
    }
}
