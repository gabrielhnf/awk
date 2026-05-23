use bumpalo::{Bump, collections::Vec};
use hashbrown::{DefaultHashBuilder, HashMap};
use indexmap::{IndexMap, IndexSet};
use parser::Identifier;

use crate::ir::{
    NonLocal, OpCode, Reg,
    lower::{Bytecode, Code},
};

#[derive(Clone, Debug, PartialEq, PartialOrd)]
#[repr(transparent)]
pub struct Value(pub f64); // TODO: use NaN-boxing.

#[derive(Debug)]
pub enum ExecMode {
    Uu,
    Gnu,
    Posix,
}

#[derive(Debug)]
pub struct Interpreter<'a> {
    arena: &'a Bump,
    bc: Bytecode<'a>,
    program_counter: usize,
    registers: Registers<'a>,
    symbols: SymbolTable<'a>,
    consts: IndexSet<Value>,
    compat: ExecMode,
}

#[derive(Debug)]
pub struct Registers<'a>(Vec<'a, Value>);

#[derive(Debug)]
pub struct SymbolTable<'a> {
    user: IndexMap<Identifier<'a>, Value>,
    // separate table for cheap invalidation. It's an arena _visibly shrugs_.
    records: HashMap<usize, Value, DefaultHashBuilder, &'a Bump>,
    // etc
}

impl<'a> Interpreter<'a> {
    pub fn new(compat: ExecMode, code: Code<'a>) -> Self {
        Self {
            arena: code.arena,
            bc: code.bc,
            program_counter: 0,
            registers: Registers(bumpalo::vec![in code.arena; Value(0.); 8]),
            symbols: code.symbols,
            consts: code.consts,
            compat,
        }
    }
}

impl<'a> SymbolTable<'a> {
    pub fn new_in(arena: &'a Bump) -> Self {
        Self {
            user: IndexMap::new(),
            records: HashMap::new_in(arena),
        }
    }
    fn lookup_user_var(&self, var: NonLocal) -> &Value {
        self.user.get_index(var.0 as _).unwrap().1
    }

    fn write_user_val(&mut self, var: NonLocal, value: &Value) {
        *self.user.get_index_mut(var.0 as _).unwrap().1 = Value::clone(value);
    }

    pub fn register_user_var(&mut self, var: &Identifier, bump: &'a Bump) -> NonLocal {
        if let Some(index) = self.user.get_index_of(var) {
            NonLocal(index as _)
        } else {
            let ident = Identifier {
                namespace: bump.alloc_str(var.namespace),
                literal: bump.alloc_str(var.literal),
            };
            NonLocal(self.user.insert_full(ident, Value(0.)).0 as _)
        }
    }
}

impl Interpreter<'_> {
    pub fn run(&mut self) {
        while let Some(instr) = self.bc.code.get(self.program_counter) {
            match instr {
                ix if let Some(&(dest, src)) = ix.get_unary() => {}
                ix if let Some(&(dest, lhs, rhs)) = ix.get_binary() => {
                    let lhs = self.registers.read(lhs);
                    let rhs = self.registers.read(rhs);
                    let val = match ix.opcode {
                        OpCode::Add => Value(lhs.0 + rhs.0),
                        OpCode::Subtract => Value(lhs.0 - rhs.0),
                        OpCode::Multiply => Value(lhs.0 * rhs.0),
                        OpCode::Divide => Value(lhs.0 / rhs.0),
                        _ => todo!(),
                    };
                    self.registers.write(dest, &val);
                }
                ix if let Some(&(dest, src)) = ix.get_load_store() => match ix.opcode {
                    OpCode::LoadConst => self
                        .registers
                        .write(dest, self.consts.get_index(src.0 as _).unwrap()),
                    OpCode::LoadUser => {
                        self.registers
                            .write(dest, self.symbols.lookup_user_var(src));
                    }
                    OpCode::StoreUser => {
                        self.symbols.write_user_val(src, self.registers.read(dest));
                    }
                    _ => todo!(),
                },
                _ => todo!(),
            }
            self.program_counter += 1;
        }
    }
}

impl Registers<'_> {
    fn read(&self, src: Reg) -> &Value {
        &self.0[src.0 as usize]
    }
    fn write(&mut self, dest: Reg, src: &Value) {
        self.0[dest.0 as usize] = Value::clone(src);
    }
}
