// This file is part of the uutils awk package.
//
// For the full copyright and license information, please view the LICENSE
// files that was distributed with this source code.

use std::{borrow::Cow, mem::forget, ops::Deref};

use bumpalo::{Bump, collections::Vec};
use parser::{
    Atom, BinaryOperator, BinaryPlaceOperator, Body, Expr, ExprNode, Place, SimpleStatement,
    Statement, UnaryOperator, UnaryPlaceOperator, Variable,
};

use crate::{
    ir::{BinaryArg, Instruction, Label, NonLocal, Reg, UnaryArg},
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
#[repr(transparent)]
struct LinearReg(Reg);

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
                let condition = self.lower_expr(condition);
                let label_then = self.following_instr(1);
                let if_label =
                    self.bc
                        .emit(Instruction::Branch((*condition, label_then, Label(0))));
                self.free_reg(condition);
                self.lower_body(then_body);
                let next = self.following_instr(0);
                self.bc.nth(if_label).set_label(next);

                if let Some(else_body) = else_body {
                    state.reg_pointer += 1;
                    self.bc.nth(if_label).push_start_label();
                    let end_label = self.bc.emit(Instruction::Jump(Label(0)));
                    state.scope_hwm(self, |c| c.lower_body(else_body));
                    let next = self.following_instr(0);
                    self.bc.nth(end_label).set_label(next);
                }
            }
            Statement::While { condition, then_body } => {
                let cond_label = self.following_instr(0);
                let condition = self.lower_expr(condition);
                let while_label = self.bc.emit(Instruction::Branch((
                    *condition,
                    self.following_instr(1),
                    Label(0),
                )));
                self.free_reg(condition);
                self.lower_body(then_body);
                self.bc.emit(Instruction::Jump(cond_label));
                let next = self.following_instr(0);
                self.bc.nth(while_label).set_label(next);
            }
            Statement::DoWhile { then_body, condition } => {
                let do_label = self.following_instr(0);
                self.lower_body(then_body);
                let condition = self.lower_expr(condition);
                self.bc.emit(Instruction::Branch((
                    *condition,
                    do_label,
                    self.following_instr(1),
                )));
                self.free_reg(condition);
            }
            Statement::For { init, condition, update, body } => {
                if let Some(SimpleStatement::Expression(expr)) = init {
                    let reg = self.lower_expr(expr);
                    self.free_reg(reg);
                }
                let cond_label = self.following_instr(0);
                if let Some(condition) = condition {
                    let condition = self.lower_expr(condition);
                    let body_label = self.following_instr(1);
                    let while_label =
                        self.bc
                            .emit(Instruction::Branch((*condition, body_label, Label(0))));
                    self.free_reg(condition);
                    self.lower_body(body);
                    if let Some(SimpleStatement::Expression(expr)) = update {
                        let reg = self.lower_expr(expr);
                        self.free_reg(reg);
                    }
                    self.bc.emit(Instruction::Jump(cond_label));
                    let next = self.following_instr(0);
                    self.bc.nth(while_label).set_label(next);
                } else {
                    self.lower_body(body);
                    if let Some(SimpleStatement::Expression(expr)) = update {
                        let reg = self.lower_expr(expr);
                        self.free_reg(reg);
                    }
                    self.bc.emit(Instruction::Jump(cond_label));
                }
            }
            Statement::Simple(SimpleStatement::Expression(expr)) => {
                let reg = self.lower_expr(expr);
                self.free_reg(reg);
            }
            _ => todo!(),
        }
    }

    fn lower_expr(&mut self, expr: &Expr) -> LinearReg {
        let dest = self.alloc_reg();
        self.lower_expr_into(expr, *dest);
        dest
    }

    fn lower_expr_into(&mut self, expr: &Expr, dest: Reg) {
        match expr {
            Expr::Leaf(atom) => match atom {
                Atom::Variable(Variable::User(ident)) => {
                    let src = self.symbols.register_user_var(ident, self.arena);
                    self.bc.emit(Instruction::LoadUserScalar((dest, src)));
                }
                &Atom::Number(n) => {
                    let src = self.register_const(Value::Float(n));
                    self.bc.emit(Instruction::LoadConst((dest, src)));
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
                    self.bc.emit(Instruction::LoadConst((dest, src)));
                }
                Atom::Regex(r) => {
                    let src = self.register_const(Value::Regex(Cow::Borrowed(
                        self.arena.alloc_slice_copy(r.as_ref()),
                    )));
                    self.bc.emit(Instruction::LoadConst((dest, src)));
                    let rec = self.lower_expr(&Expr::Leaf(Atom::Number(0.)));
                    self.bc.emit(Instruction::Matches((dest, *rec, dest)));
                    self.free_reg(rec);
                }
                _ => todo!(),
            },
            Expr::Node(node) => match node.as_ref() {
                ExprNode::UnaryOperation(op, expr) => {
                    let src = self.lower_expr(expr);
                    self.bc.emit(Instruction::from((*op, (dest, *src))));
                    self.free_reg(src);
                }
                ExprNode::BinaryOperation(op, lhs, rhs) => {
                    let lhs = self.lower_expr(lhs);
                    let rhs = self.lower_expr(rhs);
                    self.bc.emit(Instruction::from((*op, (dest, *lhs, *rhs))));
                    self.free_reg(lhs);
                    self.free_reg(rhs);
                }
                ExprNode::Ternary(cond, true_then, false_then) => {
                    self.lower_expr_into(cond, dest);

                    let mut state = RegsState::new(self);
                    let branch = self.bc.emit(Instruction::Branch((
                        dest,
                        self.following_instr(1),
                        Label(0),
                    )));

                    state = state.scope(self, |c| {
                        c.lower_expr_into(true_then, dest);
                    });

                    let jump = self.bc.emit(Instruction::Jump(Label(0)));
                    let label = self.following_instr(0);

                    state.scope_hwm(self, |c| {
                        c.lower_expr_into(false_then, dest);
                    });

                    self.bc.nth(branch).set_label(label);
                    let next = self.following_instr(0);
                    self.bc.nth(jump).set_label(next);
                }
                ExprNode::BinaryPlaceOperation(op, place, expr) => {
                    self.lower_expr_into(expr, dest);

                    let second_op = match op {
                        BinaryPlaceOperator::Assignment => {
                            self.store_place(place, dest);
                            return;
                        }
                        BinaryPlaceOperator::AddAssign => Instruction::Add,
                        BinaryPlaceOperator::SubAssign => Instruction::Subtract,
                        BinaryPlaceOperator::MulAssign => Instruction::Multiply,
                        BinaryPlaceOperator::DivAssign => Instruction::Divide,
                        BinaryPlaceOperator::PowAssign => Instruction::Raise,
                        BinaryPlaceOperator::ModAssign => Instruction::Modulo,
                    };
                    let lhs = self.alloc_reg();

                    self.load_place(*lhs, place);
                    self.bc.emit(second_op((dest, *lhs, dest)));
                    self.store_place(place, dest);

                    self.free_reg(lhs);
                }
                ExprNode::UnaryPlaceOperation(op, place) => {
                    let rhs = self.alloc_reg();
                    let one = self.register_const(Value::Float(1.));
                    self.bc.emit(Instruction::LoadConst((*rhs, one)));
                    self.load_place(dest, place);

                    let second_op = match op {
                        UnaryPlaceOperator::IncrementL | UnaryPlaceOperator::IncrementR => {
                            Instruction::Add
                        }
                        UnaryPlaceOperator::DecrementL | UnaryPlaceOperator::DecrementR => {
                            Instruction::Subtract
                        }
                    };
                    match op {
                        UnaryPlaceOperator::IncrementL | UnaryPlaceOperator::DecrementL => {
                            self.bc.emit(second_op((dest, dest, *rhs)));
                            self.store_place(place, dest);
                        }
                        UnaryPlaceOperator::IncrementR | UnaryPlaceOperator::DecrementR => {
                            // force dest to concrete num:
                            let tmp = self.alloc_reg();
                            let zero = self.register_const(Value::Float(0.));
                            self.bc.emit(Instruction::LoadConst((*tmp, zero)));
                            self.bc.emit(Instruction::Add((dest, dest, *tmp)));

                            self.bc.emit(second_op((*tmp, dest, *rhs)));
                            self.store_place(place, *tmp);
                            self.free_reg(tmp);
                        }
                    }
                    self.free_reg(rhs);
                }
                _ => todo!(),
            },
        }
    }

    fn load_place(&mut self, dest: Reg, place: &Place<'_>) {
        match place {
            Place::Record(_) => {
                todo!()
            }
            Place::Variable(Variable::User(ident)) => {
                let src = self.symbols.register_user_var(ident, self.arena);
                self.bc.emit(Instruction::LoadUserScalar((dest, src)));
            }
            Place::Variable(var) => {
                self.bc
                    .emit(Instruction::LoadBuiltinScalar((dest, var_index(var))));
            }
            Place::Index(Variable::User(ident), index) => {
                self.subsep_index(dest, index);
                let src = self.symbols.register_user_var(ident, self.arena);
                self.bc.emit(Instruction::LoadUserArray((dest, dest, src)));
            }
            Place::Index(var, index) => {
                self.subsep_index(dest, index);
                self.bc
                    .emit(Instruction::LoadBuiltinArray((dest, dest, var_index(var))));
            }
            Place::ChainedIndex(_, _) => todo!(),
        }
    }

    fn store_place(&mut self, place: &Place<'_>, src: Reg) {
        match place {
            Place::Record(_) => todo!(),
            Place::Variable(Variable::User(ident)) => {
                self.bc.emit(Instruction::StoreUserScalar((
                    src,
                    self.symbols.register_user_var(ident, self.arena),
                )));
            }
            Place::Variable(var) => {
                self.bc
                    .emit(Instruction::StoreBuiltinScalar((src, var_index(var))));
            }
            Place::Index(Variable::User(ident), index) => {
                let rhs = self.alloc_reg();
                self.subsep_index(*rhs, index);
                let place = self.symbols.register_user_var(ident, self.arena);
                self.bc
                    .emit(Instruction::StoreUserArray((src, *rhs, place)));
                self.free_reg(rhs);
            }
            Place::Index(var, index) => {
                let rhs = self.alloc_reg();
                self.subsep_index(*rhs, index);
                self.bc
                    .emit(Instruction::StoreBuiltinArray((src, *rhs, var_index(var))));
                self.free_reg(rhs);
            }
            Place::ChainedIndex(_, _) => todo!(),
        }
    }

    fn subsep_index(&mut self, dest: Reg, index: &[Expr<'_>]) {
        let rhs = self.alloc_reg();
        self.lower_expr_into(&index[0], dest);
        for i in &index[1..] {
            self.lower_expr_into(i, *rhs);
            self.bc.emit(Instruction::Concat((dest, dest, *rhs)));
        }
        self.free_reg(rhs);
    }

    fn alloc_reg(&mut self) -> LinearReg {
        self.free_regs.pop().map(LinearReg).unwrap_or_else(|| {
            let current = self.reg_pointer;
            self.reg_pointer += 1;
            LinearReg(Reg(current))
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

impl From<(UnaryOperator, UnaryArg)> for Instruction {
    fn from((op, args): (UnaryOperator, UnaryArg)) -> Self {
        match op {
            UnaryOperator::Record => Self::Record(args),
            UnaryOperator::Negation => Self::Negation(args),
            UnaryOperator::ToInt => Self::ToInt(args),
            UnaryOperator::Negative => Self::Negative(args),
        }
    }
}

impl From<(BinaryOperator, BinaryArg)> for Instruction {
    fn from((op, args): (BinaryOperator, BinaryArg)) -> Self {
        match op {
            BinaryOperator::Concat => Self::Concat(args),
            BinaryOperator::Eq => Self::Eq(args),
            BinaryOperator::NEq => Self::NEq(args),
            BinaryOperator::Gt => Self::Gt(args),
            BinaryOperator::Lt => Self::Lt(args),
            BinaryOperator::LtE => Self::LtE(args),
            BinaryOperator::GtE => Self::GtE(args),
            BinaryOperator::And => Self::And(args),
            BinaryOperator::Or => Self::Or(args),
            BinaryOperator::Matches => Self::Matches(args),
            BinaryOperator::MatchesNot => Self::MatchesNot(args),
            BinaryOperator::Add => Self::Add(args),
            BinaryOperator::Subtract => Self::Subtract(args),
            BinaryOperator::Multiply => Self::Multiply(args),
            BinaryOperator::Divide => Self::Divide(args),
            BinaryOperator::Raise => Self::Raise(args),
            BinaryOperator::Modulo => Self::Modulo(args),
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

impl Deref for LinearReg {
    type Target = Reg;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

fn var_index(var: &Variable<'_>) -> NonLocal {
    // SAFETY: it is repr(u16).
    unsafe { *<*const Variable>::from(var).cast::<NonLocal>() }
}

#[cfg(debug_assertions)]
impl Drop for LinearReg {
    fn drop(&mut self) {
        debug_assert!(false, "Leaked register {}!", self.0);
    }
}
