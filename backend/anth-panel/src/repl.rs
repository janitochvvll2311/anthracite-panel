//! Interactive console for the daemon.
//!
//! `daemon --interactive` serves the socket as usual and additionally reads
//! commands from stdin, which spares an operator from a second terminal and a
//! separate `anth-panel` invocation while debugging a policy decision.
//!
//! The console deliberately reuses the client instead of growing a grammar of
//! its own: a line is parsed by the same clap definition the binary uses, the
//! request comes from the same `request_for`, and the output comes from the
//! same `render_response`. A command therefore behaves identically whether it
//! is typed here or run as a subcommand, and a new subcommand needs no work in
//! this module.
//!
//! Two consequences are worth stating. Lines that describe a local action
//! (`daemon`, `api`, `run`) are refused rather than silently ignored, because
//! a nested daemon would fight the running one for the socket. And a command
//! that asks for confirmation on a terminal asks here too, since the console
//! owns the terminal.

use std::path::PathBuf;

use clap::Parser;
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::cli::{self, Cli};
use crate::client::request_for;
use crate::render::render_response;
use crate::stream;

/// How often a terminal session's queued output is drained while a command is
/// running, so a `terminal open` does not look frozen.
const INTERACTIVE_HINT: &str = "Type a command without the `anth-panel` prefix, or `help`.";

/// Runs the console until stdin closes or `quit` is read.
pub async fn run(socket_path: PathBuf) -> std::io::Result<()> {
    let stdin = tokio::io::stdin();
    let mut lines = BufReader::new(stdin).lines();

    println!("{INTERACTIVE_HINT}");

    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if matches!(line, "quit" | "exit") {
            println!("bye");
            return Ok(());
        }
        if matches!(line, "help" | "?") {
            print!("{}", cli::command_parser().render_help());
            continue;
        }

        match parse_line(line) {
            Outcome::Request(request) => {
                // One frame per line: a watch would hold the console until the
                // operator interrupts it, which a typed command should not do.
                stream::follow(&socket_path, &request, None, render_response).await;
            }
            Outcome::Terminal => {
                println!("[terminal] interactive terminal is not wired into the console yet");
            }
            Outcome::Local(reason) => eprintln!("[ERROR] {reason}"),
            Outcome::Invalid(error) => {
                eprintln!("[ERROR] {error}");
                eprintln!("Type `help` for the command list.");
            }
        }
    }

    Ok(())
}

/// What one line means.
enum Outcome {
    /// A request can be sent to the running daemon on this connection.
    Request(anth_proto::Request),
    /// The subcommand would start or own a server, which is already running.
    Local(String),
    /// The line did not parse.
    Invalid(String),
    /// A terminal session, which needs a raw-mode loop the console cannot take.
    Terminal,
}

fn parse_line(line: &str) -> Outcome {
    let words = match shell_words(line) {
        Ok(words) => words,
        Err(error) => return Outcome::Invalid(error),
    };
    if words.is_empty() {
        return Outcome::Invalid("empty command".to_string());
    }

    // The first element of the iterator is the program name, not an argument.
    let argv = std::iter::once("anth-panel").chain(words.iter().map(String::as_str));
    match Cli::try_parse_from(argv) {
        Ok(cli) => match request_for(&cli.command) {
            Some(request) => Outcome::Request(request),
            None => match &cli.command {
                crate::cli::Command::Terminal(_) => Outcome::Terminal,
                crate::cli::Command::Daemon(_)
                | crate::cli::Command::Api(_)
                | crate::cli::Command::Run(_) => Outcome::Local(
                    "this process already serves the socket; start another server separately"
                        .to_string(),
                ),
                crate::cli::Command::Auth(_) => Outcome::Local(
                    "authentication commands edit the config; run them as `anth-panel auth ...`"
                        .to_string(),
                ),
                crate::cli::Command::Config(_) => Outcome::Local(
                    "config commands edit the config; run them as `anth-panel config ...`"
                        .to_string(),
                ),
                _ => Outcome::Invalid("this subcommand cannot be sent to the daemon".to_string()),
            },
        },
        Err(error) => Outcome::Invalid(error.to_string()),
    }
}

/// Splits a line into words, honoring single and double quotes.
///
/// `shlex` would be the obvious dependency, but the console only needs quoting
/// and escaping for arguments such as a command line with spaces, and the
/// static build is worth more than the extra crate.
fn shell_words(line: &str) -> Result<Vec<String>, String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut has_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();

    while let Some(ch) = chars.next() {
        match quote {
            Some(open) if ch == open => quote = None,
            Some(_) if ch == '\\' => match chars.next() {
                Some(escaped) => current.push(escaped),
                None => return Err("trailing backslash".to_string()),
            },
            Some(_) => current.push(ch),
            None if ch == '"' || ch == '\'' => {
                quote = Some(ch);
                has_word = true;
            }
            None if ch.is_whitespace() => {
                if has_word {
                    words.push(std::mem::take(&mut current));
                    has_word = false;
                }
            }
            None if ch == '\\' => match chars.next() {
                Some(escaped) => {
                    current.push(escaped);
                    has_word = true;
                }
                None => return Err("trailing backslash".to_string()),
            },
            None => {
                current.push(ch);
                has_word = true;
            }
        }
    }

    if quote.is_some() {
        return Err("unterminated quote".to_string());
    }
    if has_word {
        words.push(current);
    }
    Ok(words)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_words_splits_plain_arguments() {
        assert_eq!(
            shell_words("process signal 42 term").unwrap(),
            vec!["process", "signal", "42", "term"]
        );
    }

    #[test]
    fn shell_words_keeps_quoted_spaces_together() {
        assert_eq!(
            shell_words(r#"echo "hola mundo""#).unwrap(),
            vec!["echo", "hola mundo"]
        );
        assert_eq!(
            shell_words("process spawn 'sleep 5'").unwrap(),
            vec!["process", "spawn", "sleep 5"]
        );
    }

    #[test]
    fn shell_words_supports_escapes_and_empty_arguments() {
        assert_eq!(
            shell_words(r#"echo hola\ mundo"#).unwrap(),
            vec!["echo", "hola mundo"]
        );
        assert_eq!(shell_words(r#"echo """#).unwrap(), vec!["echo", ""]);
    }

    #[test]
    fn shell_words_rejects_unterminated_quotes() {
        assert!(shell_words(r#"echo "hola"#).is_err());
        assert!(shell_words("echo hola\\").is_err());
    }

    #[test]
    fn an_echo_with_spaces_survives_the_round_trip() {
        match parse_line(r#"echo "hola mundo""#) {
            Outcome::Request(anth_proto::Request::Echo { message }) => {
                assert_eq!(message, "hola mundo");
            }
            _ => panic!("echo should become Request::Echo"),
        }
    }

    #[test]
    fn a_known_subcommand_becomes_a_request() {
        match parse_line("host-info") {
            Outcome::Request(anth_proto::Request::HostInfo) => {}
            _ => panic!("host-info should become Request::HostInfo"),
        }
    }

    #[test]
    fn a_server_subcommand_is_refused_with_a_reason() {
        match parse_line("daemon") {
            Outcome::Local(reason) => assert!(reason.contains("already serves")),
            _ => panic!("daemon must not be sent to the daemon"),
        }
    }

    #[test]
    fn an_unknown_subcommand_reports_the_parse_error() {
        match parse_line("nope") {
            Outcome::Invalid(error) => assert!(error.contains("nope")),
            _ => panic!("an unknown subcommand must be reported"),
        }
    }
}
