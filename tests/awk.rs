use std::env;

pub const TESTS_BINARY: &str = env!("CARGO_BIN_EXE_awk");

#[ctor::ctor]
fn init() {
    unsafe {
        env::set_var("UUTESTS_BINARY_PATH", TESTS_BINARY);
        env::remove_var("UUTESTS_UTIL_NAME");
        env::set_var("UUTESTS_UTIL_NAME", "");
        env::set_var("UUTILS_MULTICALL", "0");
    }
}

#[path = "by-util/test_awk.rs"]
mod test_awk;
