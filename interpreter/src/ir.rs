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

pub mod lower;

use std::fmt::{Debug, Display};

pub use lower::test_interpreter;

use crate::ir::lower::ValueContext;

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct NonLocal(pub u16);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
pub struct Reg(pub u16);

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct Label(pub u16);

#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct ArgCount(u16);

#[repr(u8, align(1))]
#[derive(Clone, Copy, Debug)]
pub enum OpCode {
    // Unary operations
    Record,
    Negation,
    ToInt,
    Negative,

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
    Concat,

    // Intrinsic operations
    LoadUser,
    LoadBultin,
    LoadConst,
    Copy,
    StoreUser,
    StoreBuiltin,
    IntrinsicCall,
    UserCall,
    IndirectCall,
    Jump,
    Return,
    Branch,
}

const _: () = const { assert!(size_of::<Instruction>() <= 8) };

#[derive(Clone, Copy)]
#[repr(C, align(8))]
pub struct Instruction {
    pub opcode: OpCode,
    pub hint: Hint,
    pub args: Arguments,
}

#[derive(Clone, Copy)]
#[repr(C, align(2))]
pub union Arguments {
    unary_local: UnaryArg,
    binary_local: BinaryArg,
    load_store: LoadStoreArg,
    jump: JumpArg,
    ret: RetArg,
    branch: BranchArg,
    call: CallArgs,
    ind_call: IndCallArgs,
}

pub type UnaryArg = (Reg, Reg);
pub type BinaryArg = (Reg, Reg, Reg);
pub type LoadStoreArg = (Reg, NonLocal);
pub type JumpArg = Label;
pub type RetArg = Reg;
pub type BranchArg = (Reg, Label, Label);
pub type CallArgs = (Reg, NonLocal, ArgCount);
pub type IndCallArgs = (Reg, Reg, ArgCount);

impl Instruction {
    fn unary(opcode: impl Into<OpCode>, dest: Reg, src: &impl HintedReg) -> Self {
        let opcode = opcode.into();
        debug_assert!(opcode.is_unary());
        Self {
            opcode,
            args: Arguments { unary_local: (dest, src.reg()) },
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
            args: Arguments { binary_local: (dest, lhs.reg(), rhs.reg()) },
            hint,
        }
    }

    fn load_store(opcode: impl Into<OpCode>, dest: Reg, src: NonLocal, ctx: ValueContext) -> Self {
        let opcode = opcode.into();
        debug_assert!(opcode.is_load_store());
        Self {
            opcode,
            args: Arguments { load_store: (dest, src) },
            hint: ctx.into(),
        }
    }

    fn jump(to: Label) -> Self {
        Self {
            opcode: OpCode::Jump,
            args: Arguments { jump: to },
            hint: Hint::None,
        }
    }

    fn branch(cond: Reg, true_to: Label, false_to: Label) -> Self {
        Self {
            opcode: OpCode::Branch,
            args: Arguments { branch: (cond, true_to, false_to) },
            hint: Hint::None,
        }
    }

    pub fn get_unary(&self) -> Option<&UnaryArg> {
        self.opcode
            .is_unary()
            .then_some(unsafe { &self.args.unary_local })
    }

    pub fn get_binary(&self) -> Option<&BinaryArg> {
        self.opcode
            .is_binary()
            .then_some(unsafe { &self.args.binary_local })
    }

    pub fn get_load_store(&self) -> Option<&LoadStoreArg> {
        self.opcode
            .is_load_store()
            .then_some(unsafe { &self.args.load_store })
    }

    pub fn get_branch(&self) -> Option<&BranchArg> {
        self.opcode
            .is_branch()
            .then_some(unsafe { &self.args.branch })
    }

    pub fn get_jump(&self) -> Option<&JumpArg> {
        self.opcode.is_jump().then_some(unsafe { &self.args.jump })
    }
}

impl OpCode {
    fn is_unary(self) -> bool {
        matches!(
            self,
            Self::Record | Self::Negation | Self::ToInt | Self::Negative
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
                | Self::Concat
        )
    }

    fn is_load_store(self) -> bool {
        matches!(
            self,
            Self::LoadUser
                | Self::LoadBultin
                | Self::LoadConst
                | Self::StoreUser
                | Self::StoreBuiltin
        )
    }

    fn is_jump(self) -> bool {
        matches!(self, Self::Jump)
    }

    fn is_branch(self) -> bool {
        matches!(self, Self::Branch)
    }
}

#[repr(u8, align(1))]
#[derive(Clone, Copy, Debug)]
pub enum Hint {
    None = 0,
    UnboxedFloat64,
    UnboxedLhsFloat64,
    UnboxedRhsFloat64,
    ScalarCtx,
    ArrayCtx,
}

trait HintedReg {
    fn reg(&self) -> Reg;
    fn hint(&self) -> Hint;
}

impl From<ValueContext> for Hint {
    fn from(value: ValueContext) -> Self {
        match value {
            ValueContext::Scalar => Self::ScalarCtx,
            ValueContext::Array => Self::ArrayCtx,
            ValueContext::Untyped => Self::None,
        }
    }
}

impl From<Hint> for ValueContext {
    fn from(value: Hint) -> Self {
        match value {
            Hint::ScalarCtx => Self::Scalar,
            Hint::ArrayCtx => Self::Array,
            _ => Self::Untyped,
        }
    }
}

impl Debug for Instruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Instruction::{:?}", self.opcode)?;
        match self.opcode {
            op if op.is_unary() => {
                let (dest, data) = unsafe { &self.args.unary_local };
                write!(f, "({dest:?}, {data:?})")
            }
            op if op.is_binary() => {
                let (dest, lhs, rhs) = unsafe { &self.args.binary_local };
                write!(f, "({dest:?}, {lhs:?}, {rhs:?})")
            }
            op if op.is_load_store() => {
                let (dest, src) = unsafe { &self.args.load_store };
                write!(f, "({dest:?}, {src:?})")
            }
            OpCode::Branch => {
                let (cond, label_then, label_else) = unsafe { self.args.branch };
                write!(f, "({cond:?}, {label_then:?}, {label_else:?})")
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
            | OpCode::Concat
            | OpCode::Modulo) => {
                let (dest, lhs, rhs) = unsafe { &self.args.binary_local };
                write!(f, "{dest} <- {op} {lhs}, {rhs}")
            }
            op @ (OpCode::LoadUser | OpCode::StoreUser) => {
                let (dest, src) = unsafe { &self.args.load_store };
                write!(f, "{dest} <- {op} user[{src}]")
            }
            op @ OpCode::LoadConst => {
                let (dest, src) = unsafe { &self.args.load_store };
                write!(f, "{dest} <- {op} mem[{src}]")
            }
            op @ (OpCode::StoreBuiltin | OpCode::LoadBultin) => {
                let (dest, src) = unsafe { &self.args.load_store };
                write!(f, "{dest} <- {op} intrinsic[{src}]")
            }
            op @ OpCode::Branch => {
                let (cond, label_then, label_else) = unsafe { self.args.branch };
                write!(f, "{op} {cond}, {label_then}, {label_else}")
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
            Hint::ScalarCtx => write!(f, "@ scalar_ctx"),
            Hint::ArrayCtx => write!(f, "@ array_ctx"),
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
            Self::LoadUser => "vload",
            Self::LoadBultin => "iload",
            Self::LoadConst => "cload",
            Self::StoreUser => "vstore",
            Self::StoreBuiltin => "istore",
            Self::Copy => "cpy",
            Self::IntrinsicCall => "icall",
            Self::UserCall => "ucall",
            Self::IndirectCall => "vcall",
            Self::Jump => "jmp",
            Self::Return => "ret",
            Self::Branch => "brif",
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
