//! An interactive prompt over the same dispatcher a one-shot invocation uses.
//!
//! The point of this module is what it *does not* do: it never interprets a
//! command itself. It splits a line the way a shell does, hands the words to
//! clap, and runs the result through `dispatch_command`, which is the same
//! function `main` calls. A typed command therefore cannot drift from a
//! subcommand — the failure this replaces is a REPL that grew its own copy of
//! the flag parsing.

use std::process::ExitCode;

use clap::Parser;

use crate::{Cli, dispatch_command};

/// Runs the prompt until end of input, `quit`, or `exit`.
pub async fn run(socket: &std::path::Path) -> ExitCode {
    let stdin = std::io::stdin();
    let mut line = String::new();

    eprintln!("anth-panel interactive: `help`, `quit`, or a subcommand.");
    loop {
        eprint!("anth> ");
        line.clear();
        match stdin.read_line(&mut line) {
            Ok(0) => break, // end of input
            Ok(_) => {}
            Err(error) => {
                eprintln!("anth-panel: reading stdin: {error}");
                return ExitCode::from(1);
            }
        }

        let words = split(&line);
        // An empty line is not a command: a stray Enter must not repeat the last
        // failure or exit the prompt.
        let Some((first, rest)) = words.split_first() else {
            continue;
        };

        match first.as_str() {
            "quit" | "exit" => break,
            "help" => {
                print_help();
                continue;
            }
            _ => {}
        }

        let args = std::iter::once("anth-panel")
            .chain(std::iter::once(first.as_str()))
            .chain(rest.iter().map(String::as_str));

        let cli = match Cli::try_parse_from(args) {
            Ok(cli) => cli,
            Err(error) => {
                // clap's own message names the flag that was wrong, which is
                // more useful than anything this module could say.
                eprintln!("{error}");
                continue;
            }
        };

        // `--socket` on a line wins over the one the prompt started with, so a
        // session can be pointed at another daemon without restarting.
        let target = cli
            .socket
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| socket.to_path_buf());

        let code = dispatch_command(&target, cli.command).await;
        if code != ExitCode::SUCCESS {
            // The reason is already on stderr, and a prompt that vanished on a
            // typo would lose the session over it.
            continue;
        }
    }
    ExitCode::SUCCESS
}

fn print_help() {
    eprintln!(
        "commands:\n  \
         host-info                      one snapshot of the host\n  \
         plugin list                    every plugin the daemon found\n  \
         plugin call <id> [LINE] [--http] [--direct]\n                                 \
         run one plugin over its own pipe\n  \
         daemon | run [--addr ADDR]     serve\n  \
         help                           this message\n  \
         quit | exit                    leave"
    );
}

/// Splits a line the way a shell does, so `--message "Ana Sol"` survives.
///
/// The rule: single quotes preserve everything literally, double quotes group
/// and honour `\\"`, a backslash outside quotes escapes the next character, and
/// runs of whitespace separate. Single quotes matter more than they look: a
/// `--direct` payload is JSON, and it carries double quotes. Splitting only on
/// `"` turned `'{"action":"x"}'` into `{action:x}`, which is not JSON, so every
/// direct call failed to parse — the same defect the browser prompt had.
pub fn split(line: &str) -> Vec<String> {
    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        Plain,
        Double,
        Single,
    }

    let mut words = Vec::new();
    let mut current = String::new();
    let mut mode = Mode::Plain;
    let mut escaped = false;
    let mut started = false;

    for character in line.chars() {
        if escaped {
            current.push(character);
            escaped = false;
            started = true;
            continue;
        }

        match (mode, character) {
            (Mode::Plain, '"') => {
                mode = Mode::Double;
                started = true;
            }
            (Mode::Double, '"') => mode = Mode::Plain,
            (Mode::Plain, '\'') => {
                mode = Mode::Single;
                started = true;
            }
            (Mode::Single, '\'') => mode = Mode::Plain,
            (Mode::Plain | Mode::Double, '\\') => escaped = true,
            (mode, c) if c.is_whitespace() && mode == Mode::Plain => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            (_, c) => {
                current.push(c);
                started = true;
            }
        }
    }
    // An escape at the very end has nothing to escape, but the empty word it
    // would have started is still a word: `greet \` is one argument.
    if escaped || started {
        words.push(current);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_line_splits_on_spaces() {
        assert_eq!(split("plugin list"), vec!["plugin", "list"]);
    }

    #[test]
    fn a_quoted_value_stays_one_word() {
        assert_eq!(
            split(r#"plugin call org.anth.hello greet --name "Ana Sol""#),
            vec![
                "plugin",
                "call",
                "org.anth.hello",
                "greet",
                "--name",
                "Ana Sol"
            ]
        );
    }

    #[test]
    fn an_empty_line_produces_no_words() {
        assert!(split("   \n").is_empty());
    }

    /// A line of only a quoted empty string is a command with one empty
    /// argument, which is different from an empty line.
    #[test]
    fn a_quoted_empty_string_is_a_word() {
        assert_eq!(split(r#""""#), vec![""]);
    }

    #[test]
    fn extra_spaces_do_not_create_empty_words() {
        assert_eq!(split("  host-info   "), vec!["host-info"]);
    }

    /// A `--direct` payload is JSON, so it carries double quotes. Quoting it
    /// with single quotes is what a user types, and the splitter has to keep the
    /// inner quotes: stripping them turned `{"action":"x"}` into `{action:x}`,
    /// which is not JSON, so every direct call failed to parse.
    #[test]
    fn single_quotes_keep_double_quotes_literal() {
        assert_eq!(
            split(r#"plugin call org.anth.hello '{"action":"x"}' --direct"#),
            vec![
                "plugin",
                "call",
                "org.anth.hello",
                r#"{"action":"x"}"#,
                "--direct"
            ]
        );
    }

    #[test]
    fn a_single_quoted_value_with_a_space_stays_one_word() {
        assert_eq!(
            split(r"greet --name 'Ana Sol'"),
            vec!["greet", "--name", "Ana Sol"]
        );
    }
}
