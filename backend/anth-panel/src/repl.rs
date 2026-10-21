//! The interactive session behind the global `--interactive` / `-i` flag.
//!
//! The session reuses the binary instead of growing a grammar of its own: a
//! line is parsed with the same clap definition, executed by the same code a
//! one-shot invocation runs, and rendered by the same table renderer. A typed
//! command therefore behaves exactly like the binary, flags and confirmation
//! prompts included, and a new subcommand needs no work here.
//!
//! Two modes share [`session`]. A client session (`anth-panel -i`) sends
//! requests to a running daemon. `daemon -i` serves the socket and runs the
//! session beside the accept loop, so a debugging operator needs one terminal;
//! that mode refuses the subcommands that would start a second server, because
//! the process already owns the socket.
//!
//! Execution is injected as a [`Runner`] rather than implemented here: the
//! dispatcher lives in the binary because it owns the confirmation prompts and
//! the chunked file transfers, and duplicating it would let the session and the
//! one-shot path drift apart.

use std::future::Future;
use std::pin::Pin;

use clap::Parser;
use tokio::io::{AsyncBufRead, Lines};

use crate::cli::{self, Cli, Command};
use crate::client::request_for;

/// Shown once when a session starts on a terminal.
const HINT: &str =
    "Type a command without the `anth-panel` prefix, or `help`. Ctrl-D or `quit` exits.";

/// Runs one parsed command and reports its exit code.
///
/// Boxed because the dispatcher is an async `match` over borrowed arguments:
/// the session holds it for the whole loop, so the closure cannot be generic.
/// The future is not `Send` because a chunked file transfer holds a
/// `Box<dyn Write>` across an await; the session runs on the caller's task, so
/// nothing requires it to be.
pub type Runner = Box<dyn FnMut(Command) -> Pin<Box<dyn Future<Output = u8>>>>;

/// Runs the session until stdin closes or `quit` is read.
///
/// `first` runs before the prompt, so `anth-panel -i host-info` prints the
/// snapshot and then stays interactive.
pub async fn session<R>(
    first: Option<Command>,
    lines: &mut Lines<R>,
    banner: bool,
    run: &mut Runner,
) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
{
    if banner {
        println!("{HINT}");
    }
    if let Some(command) = first {
        run_once(command, run).await;
    }

    loop {
        if banner {
            prompt();
        }
        let Some(line) = lines.next_line().await? else {
            if banner {
                println!();
            }
            return Ok(());
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match line {
            "quit" | "exit" => return Ok(()),
            "help" | "?" => {
                print!("{}", cli::command_parser().render_help());
                continue;
            }
            _ => {}
        }
        match parse_line(line) {
            Ok(command) => run_once(command, run).await,
            Err(error) => {
                eprintln!("[ERROR] {error}");
                eprintln!("Type `help` for the command list.");
            }
        }
    }
}

fn prompt() {
    use std::io::Write;
    print!("> ");
    let _ = std::io::stdout().flush();
}

/// Applies the session-only refusals, then runs the command.
///
/// A server subcommand never returns, so running one here would leave the
/// prompt blocked forever with no way back: it is refused rather than started.
async fn run_once(command: Command, run: &mut Runner) {
    if matches!(command, Command::Daemon | Command::Api(_) | Command::Run(_)) {
        eprintln!("[ERROR] this subcommand serves and never returns; run it outside the session");
        return;
    }
    run(command).await;
}

/// Parses one typed line into a command.
///
/// The line omits the binary name, which is the point of a prompt, so a
/// synthetic one is prepended: `Cli::try_parse_from` reads its first element as
/// the program name and would otherwise consume the first typed word.
pub fn parse_line(line: &str) -> Result<Command, String> {
    let words = shell_words(line)?;
    if words.is_empty() {
        return Err("empty command".to_string());
    }

    let argv = std::iter::once("anth-panel").chain(words.iter().map(String::as_str));
    let cli = Cli::try_parse_from(argv).map_err(|error| error.to_string())?;

    let Some(command) = cli.command else {
        return Err("a command is required after the options; try `help`".to_string());
    };
    match refusal_reason(&command) {
        Some(reason) => Err(reason),
        None => Ok(command),
    }
}

/// Commands a session refuses because they cannot work with a line reader.
fn refusal_reason(command: &Command) -> Option<String> {
    match command {
        // It would need to take stdin back from the line reader, and silently
        // reading the session's own commands as terminal input is worse than
        // asking for a separate invocation.
        Command::Terminal(_) => Some(
            "`terminal` needs the terminal's own stdin, so run it as `anth-panel terminal`"
                .to_string(),
        ),
        _ => None,
    }
}

/// Whether a command travels over the socket, run locally, or neither.
///
/// Exposed for tests and for documentation: it is the contract that decides
/// what a session can do.
pub fn outcome_for(command: &Command) -> Outcome {
    if request_for(command).is_some() {
        return Outcome::Request;
    }
    match command {
        Command::Terminal(_) => Outcome::Terminal,
        Command::Daemon | Command::Api(_) | Command::Run(_) => Outcome::Server,
        Command::Auth(_) | Command::Config(_) => Outcome::Local,
        Command::File(args) => match &args.command {
            // Chunked transfers own their socket loop, so `request_for` has no
            // form for them; the dispatcher still runs them.
            cli::FileCommand::Read(_) | cli::FileCommand::Upload(_) => Outcome::Local,
            _ => Outcome::Unsupported,
        },
        _ => Outcome::Unsupported,
    }
}

/// What a session does with a command.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Sent to the daemon and rendered as a table.
    Request,
    /// Runs in this process (`auth`, `config`, chunked file transfers).
    Local,
    /// Starts a server, which a session may not do while one already runs.
    Server,
    /// Needs the process's real stdin.
    Terminal,
    /// No session form.
    Unsupported,
}

/// Splits a line into words, honoring single and double quotes.
///
/// `shlex` would be the obvious dependency, but the session only needs quoting
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
    use tokio::io::{AsyncBufReadExt, BufReader};

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
            Ok(Command::Echo { message }) => assert_eq!(message, "hola mundo"),
            other => panic!("echo should parse, got {other:?}"),
        }
    }

    #[test]
    fn a_known_subcommand_parses() {
        assert!(matches!(parse_line("host-info"), Ok(Command::HostInfo(_))));
    }

    #[test]
    fn an_unknown_subcommand_reports_the_parse_error() {
        let error = parse_line("nope").unwrap_err();
        assert!(error.contains("nope"), "{error}");
    }

    #[test]
    fn the_global_flag_parses_after_the_subcommand() {
        assert!(matches!(
            parse_line("host-info -i"),
            Ok(Command::HostInfo(_))
        ));
    }

    #[test]
    fn terminal_is_refused_because_it_needs_raw_stdin() {
        let error = parse_line("terminal").unwrap_err();
        assert!(error.contains("terminal"), "{error}");
    }

    #[test]
    fn a_line_with_only_options_is_reported() {
        let error = parse_line("-c /tmp/x.toml").unwrap_err();
        assert!(error.contains("command is required"), "{error}");
    }

    #[test]
    fn outcomes_classify_every_family() {
        assert_eq!(
            outcome_for(&parse_line("host-info").unwrap()),
            Outcome::Request
        );
        assert_eq!(outcome_for(&parse_line("daemon").unwrap()), Outcome::Server);
        assert_eq!(
            outcome_for(&parse_line("auth status").unwrap()),
            Outcome::Local
        );
        assert_eq!(
            outcome_for(&Command::Terminal(Default::default())),
            Outcome::Terminal
        );
    }

    #[tokio::test]
    async fn the_session_runs_commands_and_stops_at_quit() {
        let mut input = BufReader::new("help\nquit\n".as_bytes()).lines();
        let seen = std::rc::Rc::new(std::cell::Cell::new(0));
        let counter = std::rc::Rc::clone(&seen);
        let mut run: Runner = Box::new(move |_command| {
            counter.set(counter.get() + 1);
            Box::pin(async { 0 })
        });

        session(None, &mut input, false, &mut run).await.unwrap();
        assert_eq!(seen.get(), 0, "help must not reach the dispatcher");
    }

    #[tokio::test]
    async fn a_server_subcommand_is_refused() {
        let mut input = BufReader::new("".as_bytes()).lines();
        let ran = std::rc::Rc::new(std::cell::Cell::new(false));
        let flag = std::rc::Rc::clone(&ran);
        let mut run: Runner = Box::new(move |_command| {
            flag.set(true);
            Box::pin(async { 0 })
        });

        session(Some(Command::Daemon), &mut input, false, &mut run)
            .await
            .unwrap();
        assert!(
            !ran.get(),
            "a nested daemon must be refused before dispatch"
        );
    }

    #[tokio::test]
    async fn the_first_command_runs_before_the_prompt() {
        let mut input = BufReader::new("".as_bytes()).lines();
        let ran = std::rc::Rc::new(std::cell::Cell::new(false));
        let flag = std::rc::Rc::clone(&ran);
        let mut run: Runner = Box::new(move |_command| {
            flag.set(true);
            Box::pin(async { 0 })
        });

        session(
            Some(parse_line("host-info").unwrap()),
            &mut input,
            false,
            &mut run,
        )
        .await
        .unwrap();
        assert!(ran.get(), "the command given on the command line must run");
    }

    #[tokio::test]
    async fn an_empty_line_is_skipped_and_eof_exits() {
        let mut input = BufReader::new("\n\n".as_bytes()).lines();
        let mut run: Runner = Box::new(|_command| Box::pin(async { 0 }));
        session(None, &mut input, false, &mut run).await.unwrap();
    }
}
