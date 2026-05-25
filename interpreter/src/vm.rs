use std::{
    fmt::{self, Display},
    mem::replace,
    vec::Vec as StdVec,
};

use ahash::RandomState;
use bumpalo::{Bump, collections::Vec};
use hashbrown::HashMap;
use indexmap_allocator_api::{IndexMap, IndexSet};
use parser::Identifier;

use crate::{
    ir::{
        NonLocal, OpCode, Reg,
        lower::{Bytecode, Code, ValueContext},
    },
    types::Value,
};

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
    consts: Consts<'a>,
    compat: ExecMode,
}

#[derive(Debug)]
pub struct Registers<'a>(Vec<'a, Value<'a>>);

#[derive(Debug)]
pub struct SymbolTable<'a> {
    user: IndexMap<Identifier<'a>, Value<'a>, RandomState, &'a Bump>,
    // separate table for cheap invalidation. It's an arena _visibly shrugs_.
    records: HashMap<usize, Value<'a>, RandomState, &'a Bump>,
    // etc
}

#[derive(Debug)]
pub struct Consts<'a>(pub IndexSet<Value<'a>, RandomState, &'a Bump>);

impl<'a> Interpreter<'a> {
    pub fn new(compat: ExecMode, code: Code<'a>) -> Self {
        Self {
            arena: code.arena,
            bc: code.bc,
            program_counter: 0,
            registers: Registers(bumpalo::vec![in code.arena; Value::Untyped; 8]),
            symbols: code.symbols,
            consts: code.consts,
            compat,
        }
    }
}

impl<'a> SymbolTable<'a> {
    pub fn new_in(arena: &'a Bump) -> Self {
        Self {
            user: IndexMap::new_in(arena),
            records: HashMap::with_hasher_in(RandomState::new(), arena),
        }
    }

    fn lookup_user_var(&mut self, var: NonLocal, ctx: ValueContext) -> &Value<'a> {
        let v = self.user.get_index_mut(var.0 as _).unwrap().1;
        match ctx {
            ValueContext::Untyped => v,
            ValueContext::Scalar => v.scalar_context(),
            ValueContext::Array => v.array_context(),
        }
    }

    fn write_user_val(&mut self, var: NonLocal, value: Value<'a>) {
        *self.user.get_index_mut(var.0 as _).unwrap().1 = value;
    }

    pub fn register_user_var(&mut self, var: &Identifier, bump: &'a Bump) -> NonLocal {
        if let Some(index) = self.user.get_index_of(var) {
            NonLocal(index as _)
        } else {
            let ident = Identifier {
                namespace: bump.alloc_str(var.namespace),
                literal: bump.alloc_str(var.literal),
            };
            NonLocal(self.user.insert_full(ident, Value::Untyped).0 as _)
        }
    }
}

impl<'a> Consts<'a> {
    pub fn new_in(arena: &'a Bump) -> Self {
        Self(IndexSet::with_capacity_in(4, arena))
    }
}

impl Interpreter<'_> {
    pub fn run(&mut self) {
        while let Some(instr) = self.bc.code.get(self.program_counter) {
            match instr {
                // ix if let Some(&(dest, src)) = ix.get_unary() => {}
                ix if let Some(&(dest, lhs, rhs)) = ix.get_binary() => {
                    let val = {
                        let lhs = self.registers.get(lhs);
                        let rhs = self.registers.get(rhs);
                        match ix.opcode {
                            OpCode::Add => lhs + rhs,
                            OpCode::Subtract => lhs - rhs,
                            OpCode::Multiply => lhs * rhs,
                            OpCode::Divide => lhs / rhs,
                            OpCode::Concat => {
                                let mut buf = StdVec::with_capacity(
                                    lhs.string_size_hint() + rhs.string_size_hint(),
                                );
                                lhs.write_string(&mut buf);
                                rhs.write_string(&mut buf);
                                Value::String(buf.into())
                            }
                            _ => todo!(),
                        }
                    };
                    self.registers.write(dest, val);
                }
                ix if let Some(&(dest, src)) = ix.get_load_store() => match ix.opcode {
                    OpCode::LoadConst => {
                        let val = self.consts.0.get_index(src.0 as _).unwrap().clone();
                        self.registers.write(dest, val);
                    }
                    OpCode::LoadUser => {
                        let val = self.symbols.lookup_user_var(src, ix.hint.into()).clone();
                        self.registers.write(dest, val);
                    }
                    OpCode::StoreUser => {
                        let val = self.registers.get(dest).clone();
                        self.symbols.write_user_val(src, val);
                    }
                    _ => todo!(),
                },
                ix if let Some((cond, true_to, false_to)) = ix.get_branch() => {
                    let label = if self.registers.get(*cond).to_bool() {
                        true_to.0
                    } else {
                        false_to.0
                    };
                    self.program_counter = label as _;

                    continue;
                }
                ix if let Some(label) = ix.get_jump() => {
                    self.program_counter = label.0 as _;
                    continue;
                }
                ix => todo!("{ix:?}"),
            }
            self.program_counter += 1;
        }
    }
}

impl<'a> Registers<'a> {
    fn replace(&mut self, src: Reg, f: impl FnOnce(Value<'a>) -> Value<'a>) {
        let val = replace(self.get_mut(src), Value::Untyped);
        self.write(src, f(val));
    }
    fn get(&self, src: Reg) -> &Value<'a> {
        &self.0[src.0 as usize]
    }
    fn get_mut(&mut self, src: Reg) -> &mut Value<'a> {
        &mut self.0[src.0 as usize]
    }
    fn write(&mut self, dest: Reg, src: Value<'a>) {
        self.0[dest.0 as usize] = src;
    }
}

impl Display for Interpreter<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}\n", self.bc)?;
        writeln!(f, "{}\n", self.registers)?;
        writeln!(f, "{}\n", self.symbols)?;
        write!(f, "{}", self.consts)
    }
}

impl Display for Code<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{}\n", self.bc)?;
        writeln!(f, "{}\n", self.symbols)?;
        write!(f, "{}", self.consts)
    }
}

impl Display for Bytecode<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bytecode:")?;
        let n = self.code.len().checked_ilog10().unwrap_or(0) as usize + 1;
        fmt_list(f, self.code.iter(), |f, i, e| write!(f, "{i:0n$}: {e}"))
    }
}

impl Display for Registers<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Registers:")?;
        let n = self.0.len().checked_ilog10().unwrap_or(0) as usize + 1;
        fmt_list(f, self.0.iter(), |f, i, e| write!(f, "r{i:0n$} = {e}"))
    }
}

impl Display for SymbolTable<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Symbols:")?;
        fmt_list(f, self.user.iter(), |f, i, (k, v)| {
            write!(f, "user[{i}] @ {k} = {v}")
        })
    }
}

impl Display for Consts<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Consts:")?;
        fmt_list(f, self.0.iter(), |f, i, e| write!(f, "mem[{i}] = {e}"))
    }
}

fn fmt_list<'a, T: Copy>(
    f: &mut fmt::Formatter<'a>,
    iter: impl Iterator<Item = T>,
    cb: impl Fn(&mut fmt::Formatter<'a>, usize, T) -> fmt::Result,
) -> fmt::Result {
    for (i, e) in iter.enumerate() {
        write!(f, "\n  ")?;
        cb(f, i, e)?;
    }
    Ok(())
}
