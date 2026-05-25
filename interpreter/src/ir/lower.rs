// This file is part of the uutils awk package.
//
// For the full copyright and license information, please view the LICENSE
// files that was distributed with this source code.

use std::{borrow::Cow, mem::forget};

use bumpalo::{Bump, collections::Vec};
use parser::{
    Atom, BinaryOperator, BinaryPlaceOperator, Body, Expr, ExprNode, Place, SimpleStatement,
    Statement, UnaryOperator, Variable,
};

use crate::{
    ir::{Hint, HintedReg, Instruction, Label, NonLocal, OpCode, Reg},
    types::Value,
    vm::{Consts, ExecMode, Interpreter, SymbolTable},
};

#[derive(Debug)]
pub struct Code<'arena> {
    pub arena: &'arena Bump,
    pub bc: Bytecode<'arena>,
    pub consts: Consts<'arena>,
    pub symbols: SymbolTable<'arena>,
    free_regs: Vec<'arena, Reg>,
    pub reg_pointer: u16,
}

#[must_use]
#[derive(Debug)]
struct LinearReg(Reg, Hint);

#[derive(Clone, Copy)]
pub enum ValueContext {
    Untyped,
    Scalar,
    Array,
}

impl<'a> Code<'a> {
    fn lower_body(&mut self, body: &Body) {
        for stmnt in &body.0 {
            self.lower_statement(stmnt);
        }
    }

    fn lower_statement(&mut self, stmnt: &Statement) {
        match stmnt {
            Statement::If { condition, then_body, else_body } => {
                let mut state = RegsState::new(self);
                let condition = self.lower_expr(condition, ValueContext::Scalar);
                let label_then = self.following_instr(1);
                let if_label =
                    self.bc
                        .emit(Instruction::branch(condition.reg(), label_then, Label(0)));
                self.free_reg(condition);
                self.lower_body(then_body);
                self.bc.nth(if_label).args.branch.2 = self.following_instr(0);

                if let Some(else_body) = else_body {
                    state.reg_pointer += 1;
                    unsafe { self.bc.nth(if_label).args.branch.2.0 += 1 };
                    let end_label = self.bc.emit(Instruction::jump(Label(0)));
                    state.scope_hwm(self, |c| c.lower_body(else_body));
                    self.bc.nth(end_label).args.jump = self.following_instr(0);
                }
            }
            Statement::While { condition, then_body } => {
                let cond_label = self.following_instr(0);
                let condition = self.lower_expr(condition, ValueContext::Scalar);
                let while_label = self.bc.emit(Instruction::branch(
                    condition.reg(),
                    self.following_instr(1),
                    Label(0),
                ));
                self.free_reg(condition);
                self.lower_body(then_body);
                self.bc.emit(Instruction::jump(cond_label));
                self.bc.nth(while_label).args.branch.2 = self.following_instr(0);
            }
            Statement::DoWhile { then_body, condition } => {
                let do_label = self.following_instr(0);
                self.lower_body(then_body);
                let condition = self.lower_expr(condition, ValueContext::Scalar);
                self.bc.emit(Instruction::branch(
                    condition.reg(),
                    do_label,
                    self.following_instr(1),
                ));
                self.free_reg(condition);
            }
            Statement::For { init, condition, update, body } => {
                if let Some(SimpleStatement::Expression(expr)) = init {
                    let reg = self.lower_expr(expr, ValueContext::Untyped);
                    self.free_reg(reg);
                }
                let cond_label = self.following_instr(0);
                if let Some(condition) = condition {
                    let condition = self.lower_expr(condition, ValueContext::Scalar);
                    let body_label = self.following_instr(1);
                    let while_label =
                        self.bc
                            .emit(Instruction::branch(condition.reg(), body_label, Label(0)));
                    self.free_reg(condition);
                    self.lower_body(body);
                    if let Some(SimpleStatement::Expression(expr)) = update {
                        let reg = self.lower_expr(expr, ValueContext::Untyped);
                        self.free_reg(reg);
                    }
                    self.bc.emit(Instruction::jump(cond_label));
                    self.bc.nth(while_label).args.branch.2 = self.following_instr(0);
                } else {
                    self.lower_body(body);
                    if let Some(SimpleStatement::Expression(expr)) = update {
                        let reg = self.lower_expr(expr, ValueContext::Untyped);
                        self.free_reg(reg);
                    }
                    self.bc.emit(Instruction::jump(cond_label));
                }
            }
            Statement::Simple(SimpleStatement::Expression(expr)) => {
                let reg = self.lower_expr(expr, ValueContext::Untyped);
                self.free_reg(reg);
            }
            _ => todo!(),
        }
    }

    fn lower_expr(&mut self, expr: &Expr, ctx: ValueContext) -> LinearReg {
        let mut dest = self.alloc_reg();
        let hint = self.lower_expr_into(expr, dest.reg(), ctx);
        dest.1 = hint;
        dest
    }

    fn lower_expr_into(&mut self, expr: &Expr, dest: Reg, ctx: ValueContext) -> Hint {
        match expr {
            Expr::Leaf(atom) => match atom {
                Atom::Variable(Variable::User(ident)) => {
                    let src = self.symbols.register_user_var(ident, self.arena);
                    self.bc
                        .emit(Instruction::load_store(OpCode::LoadUser, dest, src, ctx));
                }
                &Atom::Number(n) => {
                    let src = self.register_const(Value::Float(n));
                    self.bc.emit(Instruction::load_store(
                        OpCode::LoadConst,
                        dest,
                        src,
                        ValueContext::Scalar,
                    ));
                    return Hint::UnboxedFloat64;
                }
                atom @ (Atom::String(s) | Atom::TypedRegex(s)) => {
                    let val = if matches!(atom, Atom::String(_)) {
                        Value::String
                    } else {
                        Value::Regex
                    };
                    let src = self.register_const(val(Cow::Borrowed(
                        self.arena.alloc_slice_copy(s.as_ref()),
                    )));
                    self.bc.emit(Instruction::load_store(
                        OpCode::LoadConst,
                        dest,
                        src,
                        ValueContext::Scalar,
                    ));
                }
                Atom::Regex(r) => {
                    let src = self.register_const(Value::Regex(Cow::Borrowed(
                        self.arena.alloc_slice_copy(r.as_ref()),
                    )));
                    self.bc.emit(Instruction::load_store(
                        OpCode::LoadConst,
                        dest,
                        src,
                        ValueContext::Scalar,
                    ));
                    let dest = LinearReg(dest, Hint::None);
                    let rec = self.lower_expr(&Expr::Leaf(Atom::Number(0.)), ctx);
                    self.bc.emit(Instruction::binary(
                        OpCode::Matches,
                        dest.reg(),
                        &rec,
                        &dest,
                    ));
                    forget(dest);
                    self.free_reg(rec);
                }
                _ => todo!(),
            },
            Expr::Node(node) => match node.as_ref() {
                ExprNode::UnaryOperation(op, expr) => {
                    let src = self.lower_expr(expr, ValueContext::Scalar);
                    self.bc.emit(Instruction::unary(*op, dest, &src));
                    self.free_reg(src);
                }
                ExprNode::BinaryOperation(op, lhs, rhs) => {
                    let lhs = self.lower_expr(lhs, ValueContext::Scalar);
                    let rhs = self.lower_expr(rhs, ValueContext::Scalar);
                    self.bc.emit(Instruction::binary(*op, dest, &lhs, &rhs));
                    self.free_reg(lhs);
                    self.free_reg(rhs);
                }
                ExprNode::Ternary(cond, true_then, false_then) => {
                    self.lower_expr_into(cond, dest, ValueContext::Scalar);

                    let mut state = RegsState::new(self);
                    let branch =
                        self.bc
                            .emit(Instruction::branch(dest, self.following_instr(1), Label(0)));

                    state = state.scope(self, |c| {
                        c.lower_expr_into(true_then, dest, ValueContext::Scalar)
                    });

                    let jump = self.bc.emit(Instruction::jump(Label(0)));
                    let label = self.following_instr(0);

                    state.scope_hwm(self, |c| {
                        c.lower_expr_into(false_then, dest, ValueContext::Scalar)
                    });

                    self.bc.nth(branch).args.branch.2 = label;
                    self.bc.nth(jump).args.jump = self.following_instr(0);
                }
                ExprNode::BinaryPlaceOperation(BinaryPlaceOperator::Assignment, place, expr) => {
                    self.lower_expr_into(expr, dest, ctx);
                    let Place::Variable(Variable::User(var)) = place else {
                        todo!()
                    };
                    let var = self.symbols.register_user_var(var, self.arena);
                    self.bc
                        .emit(Instruction::load_store(OpCode::StoreUser, dest, var, ctx));
                }
                _ => todo!(),
            },
        }
        Hint::None
    }

    fn alloc_reg(&mut self) -> LinearReg {
        self.free_regs
            .pop()
            .map(|r| LinearReg(r, Hint::None))
            .unwrap_or_else(|| {
                let current = self.reg_pointer;
                self.reg_pointer += 1;
                LinearReg(Reg(current), Hint::None)
            })
    }

    fn free_reg(&mut self, reg: LinearReg) {
        self.free_regs.push(reg.into_inner());
    }

    fn register_const(&mut self, value: Value<'a>) -> NonLocal {
        NonLocal(self.consts.0.insert_full(value).0 as u16)
    }

    fn following_instr(&self, nth: u16) -> Label {
        Label(self.bc.len() + nth)
    }
}

#[derive(Debug, Clone)]
pub struct Bytecode<'a> {
    pub code: Vec<'a, Instruction>,
}

#[derive(Clone, Debug)]
struct RegsState {
    reg_pointer: u16,
    n_free_regs: usize,
}

impl<'a> Bytecode<'a> {
    fn new_in(bump: &'a Bump) -> Self {
        Self { code: Vec::with_capacity_in(64, bump) }
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
        Self { reg_pointer: old, n_free_regs: self.n_free_regs }
    }
    fn scope_hwm<T>(self, code: &mut Code, f: impl FnOnce(&mut Code) -> T) {
        f(code);
        code.reg_pointer = code.reg_pointer.max(self.reg_pointer);
        code.free_regs.truncate(self.n_free_regs);
    }
}

pub fn test_interpreter(stmnt: &Body<'_>) -> String {
    let bump = Bump::with_capacity(16384);
    let mut c = Code {
        arena: &bump,
        bc: Bytecode::new_in(&bump),
        consts: Consts::new_in(&bump),
        symbols: SymbolTable::new_in(&bump),
        reg_pointer: 0,
        free_regs: Vec::new_in(&bump),
    };
    c.lower_body(stmnt);
    let mut vm = Interpreter::new(ExecMode::Uu, c);
    vm.run();
    vm.to_string()
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

// trait Fold<T> {
//     type Args;
//     fn fold(&self, args: Self::Args) -> T;
// }

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
