use std::process::ExitCode;

use anth_cli::{error_text, execute, parse};

fn main() -> ExitCode {
    // `anth app new|build|package|sign|verify|check` is the local developer
    // workflow. `anth app install|approve|...` remains the engine client below.
    if is_dev() {
        return match cargo_anth::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("error: {error:#}");
                ExitCode::from(1)
            }
        };
    }
    match parse(std::env::args_os().skip(1)).and_then(execute) {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{}", error_text(&error));
            ExitCode::from(error.exit_code())
        }
    }
}

fn is_dev() -> bool {
    let mut args = std::env::args().skip(1);
    args.next().as_deref() == Some("app")
        && matches!(
            args.next().as_deref(),
            Some(
                "new"
                    | "build"
                    | "package"
                    | "sign"
                    | "verify"
                    | "check"
                    | "help"
                    | "--help"
                    | "-h"
            )
        )
}
