// This file is part of the uutils awk package.
//
// For the full copyright and license information, please view the LICENSE
// files that was distributed with this source code.

// static POSIX: bool = false;

mod utils;

use bumpalo::Bump;
use color_eyre::Result;
use parser::{Lexer, Parser};

use crate::utils::{ensure_consistent_panic, exit_err};

fn main() {
    if let Err(e) = ensure_consistent_panic(uu_main) {
        exit_err(e)
    }
}

#[tracing::instrument]
fn uu_main() -> Result<()> {
    let arg = std::env::args().nth(1).unwrap();

    let arena = Bump::with_capacity(4000); // 4KB minus metadata-ish
    let mut parser = Parser::new(&arena);
    let ast = match parser.parse("CLI", arg.as_bytes()) {
        Ok(ast) => dbg!(ast),
        Err((report, source)) => {
            report.eprint(("CLI", source)).unwrap();
            return Ok(());
        }
    };
    println!("{:?}", ast.rules);
    dbg!(arena.chunk_capacity());

    // for token in lex {
    //     let Ok(x) = token else {
    //         return token.map(drop).map_err(color_eyre::Report::from);
    //     };
    //     println!("{x:?}");
    // }
    // exit_with(Interpreter.run())
    Ok(())
}
