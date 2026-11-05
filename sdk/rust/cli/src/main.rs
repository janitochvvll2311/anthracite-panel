use std::process::ExitCode;

use anth_cli::{error_text, execute, parse};

fn main() -> ExitCode {
    match parse(std::env::args_os().skip(1)).and_then(execute) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{}", error_text(&error));
            ExitCode::from(error.exit_code())
        }
    }
}
