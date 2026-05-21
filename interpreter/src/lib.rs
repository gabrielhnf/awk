// This file is part of the uutils awk package.
//
// For the full copyright and license information, please view the LICENSE
// files that was distributed with this source code.

mod ir;

use color_eyre::eyre::Result;
use either::Either;

pub use ir::test_interpreter;
use parser::Variable;

pub enum BuiltinCommand {}
pub enum BuiltinVar {}

pub type Command<'a> = Either<BuiltinCommand, &'a str>;
// pub type Variable<'a> = Either<BuiltinVar, &'a str>;

#[derive(Debug)]
pub struct Interpreter;

impl Interpreter {
    #[tracing::instrument]
    pub fn run(self) -> Result<Option<i32>> {
        todo!()
    }
    pub fn eval_expression(&mut self) {}
}

fn index_for_global(_var: &Variable) -> u16 {
    1 // TODO
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
enum InterpreterError {}
