use std::process::ExitCode;

use anth_cli::{execute, parse};

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)).and_then(execute) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{}: {}", error.kind().code(), error.message());
            ExitCode::from(error.exit_code())
        }
    }
}
