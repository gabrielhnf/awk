// This file is part of the uutils awk package.
//
// For the full copyright and license information, please view the LICENSE
// files that was distributed with this source code.

use std::fmt::Display;
use std::hash::Hash;
use std::mem::forget;

use indexmap::IndexSet;
use parser::{Atom, BinaryOperator, Expr, ExprNode, UnaryOperator};

use crate::ir::{Hint, HintedReg, Instruction, Label, NonLocal, OpCode, Reg};

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct Value(f64); // TODO: use NaN-boxing.

#[derive(Debug)]
struct Code {
    bc: Bytecode,
    consts: IndexSet<Value>,
    free_regs: Vec<Reg>,
    reg_pointer: u16,
}

#[must_use]
#[derive(Debug)]
struct LinearReg(Reg, Hint);

impl Code {
    fn lower_expr(&mut self, expr: &Expr) -> LinearReg {
        let dest = self.alloc_reg();
        let hint = self.lower_expr_into(expr, dest);
        LinearReg(dest, hint)
    }
    fn lower_expr_into(&mut self, expr: &Expr, dest: Reg) -> Hint {
        match expr {
            Expr::Leaf(atom) => match atom {
                Atom::Variable(var) => {
                    let src = NonLocal(crate::index_for_global(var));
                    self.bc
                        .emit(Instruction::load_store(OpCode::Load, dest, src));
                }
                &Atom::Number(n) => {
                    let src = self.register_const(Value(n));
                    self.bc
                        .emit(Instruction::load_store(OpCode::LoadConst, dest, src));
                    return Hint::UnboxedFloat64;
                }
                _ => todo!(),
            },
            Expr::Node(node) => match node.as_ref() {
                ExprNode::UnaryOperation(op, expr) => {
                    let src = self.lower_expr(expr);
                    self.bc.emit(Instruction::unary(*op, dest, &src));
                    self.free_reg(src);
                }
                ExprNode::BinaryOperation(op, lhs, rhs) => {
                    let lhs = self.lower_expr(lhs);
                    let rhs = self.lower_expr(rhs);
                    self.bc.emit(Instruction::binary(*op, dest, &lhs, &rhs));
                    self.free_reg(lhs);
                    self.free_reg(rhs);
                }
                ExprNode::Ternary(cond, true_then, false_then) => {
                    self.lower_expr_into(cond, dest);

                    let mut state = RegsState::new(self);
                    let br_if = self
                        .bc
                        .emit(Instruction::br_if(OpCode::BrIf, dest, Label(0)));

                    state = state.scope(self, |c| c.lower_expr_into(false_then, dest));

                    let jump = self.bc.emit(Instruction::jump(OpCode::Jump, Label(0)));
                    let label = Label(self.bc.len());

                    state.scope_hwm(self, |c| c.lower_expr_into(true_then, dest));

                    self.bc.nth(br_if).args.br_if.1 = label;
                    self.bc.nth(jump).args.jump = Label(self.bc.len());
                }
                _ => todo!(),
            },
        }
        Hint::None
    }

    fn alloc_reg(&mut self) -> Reg {
        self.free_regs.pop().unwrap_or_else(|| {
            let current = self.reg_pointer;
            self.reg_pointer += 1;
            Reg(current)
        })
    }

    fn free_reg(&mut self, reg: LinearReg) {
        self.free_regs.push(reg.into_inner());
    }

    fn register_const(&mut self, value: Value) -> NonLocal {
        NonLocal(self.consts.insert_full(value).0 as u16)
    }
}

#[derive(Debug, Default, Clone)]
struct Bytecode {
    code: Vec<Instruction>,
}

#[derive(Clone, Debug)]
struct RegsState {
    reg_pointer: u16,
    n_free_regs: usize,
}

impl Bytecode {
    fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    fn emit(&mut self, code: Instruction) -> Label {
        self.code.push(code);
        Label((self.code.len() - 1) as u16)
    }

    fn len(&self) -> u16 {
        self.code.len() as u16
    }

    fn nth(&mut self, label: Label) -> &mut Instruction {
        &mut self.code[label.0 as usize]
    }
}

impl RegsState {
    fn new(code: &Code) -> Self {
        Self {
            reg_pointer: code.reg_pointer,
            n_free_regs: code.free_regs.len(),
        }
    }
    fn scope<T>(self, code: &mut Code, f: impl FnOnce(&mut Code) -> T) -> Self {
        f(code);
        let old = code.reg_pointer;
        code.reg_pointer = self.reg_pointer;
        code.free_regs.truncate(self.n_free_regs);
        Self {
            reg_pointer: old,
            n_free_regs: self.n_free_regs,
        }
    }
    fn scope_hwm<T>(self, code: &mut Code, f: impl FnOnce(&mut Code) -> T) {
        f(code);
        code.reg_pointer = code.reg_pointer.max(self.reg_pointer);
        code.free_regs.truncate(self.n_free_regs);
    }
}

pub fn test_interpreter(expr: &Expr<'_>) -> impl Display {
    let mut c = Code {
        bc: Bytecode::new(),
        consts: IndexSet::new(),
        reg_pointer: 0,
        free_regs: Vec::new(),
    };
    let result = c.lower_expr(expr);
    forget(result);
    c
}

impl From<UnaryOperator> for OpCode {
    fn from(value: UnaryOperator) -> Self {
        match value {
            UnaryOperator::Record => Self::Record,
            UnaryOperator::Negation => Self::Negation,
            UnaryOperator::ToInt => Self::ToInt,
            UnaryOperator::Negative => Self::Negative,
        }
    }
}

impl From<BinaryOperator> for OpCode {
    fn from(value: BinaryOperator) -> Self {
        match value {
            BinaryOperator::Concat => Self::Concat,
            BinaryOperator::Eq => Self::Eq,
            BinaryOperator::NEq => Self::NEq,
            BinaryOperator::Gt => Self::Gt,
            BinaryOperator::Lt => Self::Lt,
            BinaryOperator::LtE => Self::LtE,
            BinaryOperator::GtE => Self::GtE,
            BinaryOperator::And => Self::And,
            BinaryOperator::Or => Self::Or,
            BinaryOperator::Matches => Self::Matches,
            BinaryOperator::MatchesNot => Self::MatchesNot,
            BinaryOperator::Add => Self::Add,
            BinaryOperator::Subtract => Self::Subtract,
            BinaryOperator::Multiply => Self::Multiply,
            BinaryOperator::Divide => Self::Divide,
            BinaryOperator::Raise => Self::Raise,
            BinaryOperator::Modulo => Self::Modulo,
        }
    }
}

impl Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write(&self.0.to_ne_bytes());
    }
}

impl Eq for Value {}

// trait Fold<T> {
//     type Args;
//     fn fold(&self, args: Self::Args) -> T;
// }

impl Display for Bytecode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let n = self.code.len() / 10 + 1;
        for (i, e) in self.code.iter().enumerate() {
            write!(f, "{i:n$}: {e}")?;
            if i + 1 < self.code.len() as _ {
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

impl Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Bytecode:\n{}\n", self.bc)?;
        writeln!(f, "Consts:")?;
        for (i, e) in self.consts.iter().enumerate() {
            write!(f, "mem[{i}] = {}", e.0)?;
            if i + 1 < self.consts.len() as _ {
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

impl LinearReg {
    fn into_inner(self) -> Reg {
        let inner = self.0;
        forget(self);
        inner
    }
}

impl HintedReg for LinearReg {
    fn reg(&self) -> Reg {
        self.0
    }

    fn hint(&self) -> Hint {
        self.1
    }
}

// impl Deref for Reg {
//     type Target = Self;

//     fn deref(&self) -> &Self::Target {
//         self
//     }
// }

#[cfg(debug_assertions)]
impl Drop for LinearReg {
    fn drop(&mut self) {
        debug_assert!(false, "Leaked register {}!", self.0);
    }
}
