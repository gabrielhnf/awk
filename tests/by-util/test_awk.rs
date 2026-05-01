use uutests::new_ucmd;

#[test]
fn empty_program_succeeds() {
    new_ucmd!().arg("").succeeds();
}

#[test]
#[ignore = "parser does not yet support print/field expressions"]
fn print_first_field() {
    new_ucmd!().arg("{ print $1 }").succeeds();
}
