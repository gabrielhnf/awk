// This file is part of the uutils awk package.
//
// For the full copyright and license information, please view the LICENSE
// files that was distributed with this source code.

//! This module contains the bytecode description, designed to be compact
//! for cache efficiency and isomorphic w.r.t Cranelift IR. Also, our bytecode
//! _is_ our IR; we lower the AST into it and can execute it right away, or do
//! an optimization or JIT pass. We don't do the hack Lua 5's VM does of
//! emitting bytecode without an intermediate AST because AWK contextual
//! shenanigans; _even_ if it was possible, good luck maintaining that.

#![allow(dead_code)]

mod lower;

pub use lower::test_interpreter;

use std::fmt::{Debug, Display};

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
struct NonLocal(u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
struct Reg(u16);

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
struct Label(u16);

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
struct ArgCount(u16);

#[repr(u8, align(1))]
#[derive(Clone, Copy, Debug)]
enum OpCode {
    // Unary operations
    Record,
    Negation,
    ToInt,
    Negative,
    Concat,

    // Binary operations
    Eq,
    NEq,
    Gt,
    Lt,
    LtE,
    GtE,
    And,
    Or,
    Matches,
    MatchesNot,
    Add,
    Subtract,
    Multiply,
    Divide,
    Raise,
    Modulo,

    // Intrinsic operations
    Load,
    LoadConst,
    Copy,
    Store,
    IntrinsicCall,
    UserCall,
    IndirectCall,
    Jump,
    Return,
    Branch,
    BrIf,
}

const _: () = const { assert!(size_of::<Instruction>() <= 8) };

#[derive(Clone, Copy)]
#[repr(C, align(8))]
struct Instruction {
    opcode: OpCode,
    hint: Hint,
    args: Arguments,
}

#[derive(Clone, Copy)]
#[repr(C, align(2))]
union Arguments {
    unary_local: (Reg, Reg),
    binary_local: (Reg, Reg, Reg),
    load_store: (Reg, NonLocal),
    jump: Label,
    ret: Reg,
    branch: (Reg, Label, Label),
    br_if: (Reg, Label),
    call: (Reg, NonLocal, ArgCount),
    ind_call: (Reg, Reg, ArgCount),
}

impl Instruction {
    fn unary(opcode: impl Into<OpCode>, dest: Reg, src: &impl HintedReg) -> Self {
        let opcode = opcode.into();
        debug_assert!(opcode.is_unary());
        Self {
            opcode,
            args: Arguments {
                unary_local: (dest, src.reg()),
            },
            hint: src.hint(),
        }
    }

    fn binary(
        opcode: impl Into<OpCode>,
        dest: Reg,
        lhs: &impl HintedReg,
        rhs: &impl HintedReg,
    ) -> Self {
        let opcode = opcode.into();
        debug_assert!(opcode.is_binary());
        let hint = match (lhs.hint(), rhs.hint()) {
            // TODO: Remove once we get const folding.
            (Hint::UnboxedFloat64, Hint::UnboxedFloat64) => Hint::UnboxedFloat64,
            (_, Hint::UnboxedFloat64) => Hint::UnboxedRhsFloat64,
            (Hint::UnboxedFloat64, _) => Hint::UnboxedLhsFloat64,
            _ => Hint::None,
        };
        Self {
            opcode,
            args: Arguments {
                binary_local: (dest, lhs.reg(), rhs.reg()),
            },
            hint,
        }
    }

    fn load_store(opcode: impl Into<OpCode>, dest: Reg, src: NonLocal) -> Self {
        let opcode = opcode.into();
        debug_assert!(opcode.is_load_store());
        Self {
            opcode,
            args: Arguments {
                load_store: (dest, src),
            },
            hint: Hint::None,
        }
    }

    fn jump(opcode: impl Into<OpCode>, to: Label) -> Self {
        let opcode = opcode.into();
        debug_assert!(opcode.is_jump());
        Self {
            opcode,
            args: Arguments { jump: to },
            hint: Hint::None,
        }
    }

    fn branch(opcode: impl Into<OpCode>, cond: Reg, true_to: Label, false_to: Label) -> Self {
        let opcode = opcode.into();
        debug_assert!(opcode.is_branch());
        Self {
            opcode,
            args: Arguments {
                branch: (cond, true_to, false_to),
            },
            hint: Hint::None,
        }
    }

    fn br_if(opcode: impl Into<OpCode>, cond: Reg, to: Label) -> Self {
        let opcode = opcode.into();
        debug_assert!(opcode.is_branch_if());
        Self {
            opcode,
            args: Arguments { br_if: (cond, to) },
            hint: Hint::None,
        }
    }
}

impl OpCode {
    fn is_unary(self) -> bool {
        matches!(
            self,
            Self::Record | Self::Negation | Self::ToInt | Self::Negative | Self::Concat
        )
    }

    fn is_binary(self) -> bool {
        matches!(
            self,
            Self::Eq
                | Self::NEq
                | Self::Gt
                | Self::Lt
                | Self::LtE
                | Self::GtE
                | Self::And
                | Self::Or
                | Self::Matches
                | Self::MatchesNot
                | Self::Add
                | Self::Subtract
                | Self::Multiply
                | Self::Divide
                | Self::Raise
                | Self::Modulo
        )
    }

    fn is_load_store(self) -> bool {
        matches!(self, Self::Load | Self::Store | Self::LoadConst)
    }

    fn is_jump(self) -> bool {
        matches!(self, Self::Jump)
    }

    fn is_branch(self) -> bool {
        matches!(self, Self::Branch)
    }

    fn is_branch_if(self) -> bool {
        matches!(self, Self::BrIf)
    }
}

#[repr(u8, align(1))]
#[derive(Clone, Copy, Debug)]
enum Hint {
    None = 0,
    UnboxedFloat64,
    UnboxedLhsFloat64,
    UnboxedRhsFloat64,
}

trait HintedReg {
    fn reg(&self) -> Reg;
    fn hint(&self) -> Hint;
}

impl Debug for Instruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Instruction::{:?}", self.opcode)?;
        match self.opcode {
            OpCode::Record
            | OpCode::Negation
            | OpCode::ToInt
            | OpCode::Negative
            | OpCode::Concat
            | OpCode::Copy => {
                let (dest, data) = unsafe { &self.args.unary_local };
                write!(f, "({dest:?}, {data:?})")
            }
            OpCode::Eq
            | OpCode::NEq
            | OpCode::Gt
            | OpCode::Lt
            | OpCode::LtE
            | OpCode::GtE
            | OpCode::And
            | OpCode::Or
            | OpCode::Matches
            | OpCode::MatchesNot
            | OpCode::Add
            | OpCode::Subtract
            | OpCode::Multiply
            | OpCode::Divide
            | OpCode::Raise
            | OpCode::Modulo => {
                let (dest, lhs, rhs) = unsafe { &self.args.binary_local };
                write!(f, "({dest:?}, {lhs:?}, {rhs:?})")
            }
            OpCode::Load | OpCode::LoadConst | OpCode::Store => {
                let (dest, src) = unsafe { &self.args.load_store };
                write!(f, "({dest:?}, {src:?})")
            }
            OpCode::BrIf => {
                let (cond, label) = unsafe { self.args.br_if };
                write!(f, "({cond:?}, {label:?})")
            }
            OpCode::Jump => {
                let label = unsafe { self.args.jump };
                write!(f, "({label:?})")
            }
            _ => todo!(),
        }
    }
}

impl Display for Instruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.opcode {
            op @ (OpCode::Record
            | OpCode::Negation
            | OpCode::ToInt
            | OpCode::Negative
            | OpCode::Concat
            | OpCode::Copy) => {
                let (dest, data) = unsafe { &self.args.unary_local };
                write!(f, "{dest} <- {op} {data}")
            }
            op @ (OpCode::Eq
            | OpCode::NEq
            | OpCode::Gt
            | OpCode::Lt
            | OpCode::LtE
            | OpCode::GtE
            | OpCode::And
            | OpCode::Or
            | OpCode::Matches
            | OpCode::MatchesNot
            | OpCode::Add
            | OpCode::Subtract
            | OpCode::Multiply
            | OpCode::Divide
            | OpCode::Raise
            | OpCode::Modulo) => {
                let (dest, lhs, rhs) = unsafe { &self.args.binary_local };
                write!(f, "{dest} <- {op} {lhs}, {rhs}")
            }
            op @ (OpCode::Load | OpCode::Store) => {
                let (dest, src) = unsafe { &self.args.load_store };
                write!(f, "{dest} <- {op} global[{src}]")
            }
            op @ OpCode::LoadConst => {
                let (dest, src) = unsafe { &self.args.load_store };
                write!(f, "{dest} <- {op} mem[{src}]")
            }
            op @ OpCode::BrIf => {
                let (cond, label) = unsafe { self.args.br_if };
                write!(f, "{op} {cond}, {label}")
            }
            op @ OpCode::Jump => {
                let label = unsafe { self.args.jump };
                write!(f, "{op} {label}")
            }
            _ => todo!(),
        }?;
        match self.hint {
            Hint::UnboxedFloat64 if self.opcode.is_binary() => write!(f, " @ all_unboxedf64"),
            Hint::UnboxedFloat64 => write!(f, " @ all_unboxedf64"),
            Hint::UnboxedLhsFloat64 => write!(f, " @ lhs_unboxedf64"),
            Hint::UnboxedRhsFloat64 => write!(f, " @ rhs_unboxedf64"),
            Hint::None => Ok(()),
        }
    }
}

impl Display for OpCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            Self::Record => "rec",
            Self::Negation => "not",
            Self::ToInt => "int",
            Self::Negative => "neg",
            Self::Concat => "cat",
            Self::Eq => "eq",
            Self::NEq => "neq",
            Self::Gt => "gt",
            Self::Lt => "lt",
            Self::LtE => "le",
            Self::GtE => "ge",
            Self::And => "and",
            Self::Or => "or",
            Self::Matches => "mtch",
            Self::MatchesNot => "nmtch",
            Self::Add => "add",
            Self::Subtract => "sub",
            Self::Multiply => "mul",
            Self::Divide => "div",
            Self::Raise => "pow",
            Self::Modulo => "mod",
            Self::Load => "vload",
            Self::LoadConst => "cload",
            Self::Store => "vstore",
            Self::Copy => "cpy",
            Self::IntrinsicCall => "icall",
            Self::UserCall => "ucall",
            Self::IndirectCall => "vcall",
            Self::Jump => "jmp",
            Self::Return => "ret",
            Self::Branch => "br",
            Self::BrIf => "brif",
        };
        <_ as Display>::fmt(str, f)
    }
}

impl Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        <_ as Display>::fmt(&self.0, f)
    }
}

impl Display for Reg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "r{}", self.0)
    }
}

impl Display for NonLocal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
