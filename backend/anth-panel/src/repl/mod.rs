//! An interactive prompt over the same dispatcher a one-shot invocation uses.
//!
//! The point of this module is what it *does not* do: it never interprets a
//! command itself. It splits a line the way a shell does, hands the words to
//! clap, and runs the result through `dispatch_command`, which is the same
//! function `main` calls. A typed command therefore cannot drift from a
//! subcommand — the failure this replaces is a REPL that grew its own copy of
//! the flag parsing.
//!
//! Two input paths, chosen by whether stdin is a terminal:
//!
//! - **A terminal** gets [`prompt`]: raw mode, arrow keys, history and Tab
//!   completion, because there is a cursor to move and a screen to repaint.
//! - **A pipe** gets plain line reading. `echo help | anth-panel -i` must not
//!   receive escape sequences in its output, and there is nothing to edit.
//!
//! The editing behaviour itself is pure and lives in [`editor`], [`keys`],
//! [`completion`] and [`prompt`]; [`term`] is the only module that touches a
//! terminal's modes, and [`history`] the only one that touches a file.

pub mod completion;
pub mod editor;
pub mod history;
pub mod keys;
pub mod prompt;
pub mod render;
pub mod term;

use std::io::{BufRead, Write};
use std::process::ExitCode;

use clap::Parser;

use prompt::{Effect, Prompt};

use crate::{Cli, dispatch_command};

/// Runs the prompt until end of input, `quit`, or `exit`.
/// The daemon socket and the plugin directory a session works against.
///
/// Carried as one value because the two travel together through every level of
/// the prompt, and because a line may override either one: a session started
/// against one registry can edit another without being restarted.
#[derive(Clone, Copy)]
pub struct Target<'a> {
    pub socket: &'a std::path::Path,
    pub root: &'a std::path::Path,
    pub readable: &'a std::path::Path,
    /// What the commands run from the prompt log to. Borrowed rather than owned,
    /// so `Target` stays `Copy` and every helper can take it by value.
    pub log: &'a std::sync::Arc<crate::log::Logger>,
}

/// The logger the prompt writes through, and the queue it drains.
///
/// A daemon sharing the process (`run -i`) builds this first and hands the same
/// logger to its `Daemon`, so its lines arrive here instead of on stdout: there
/// is one terminal and one writer, and it is the prompt.
pub struct Wired {
    pub logger: std::sync::Arc<crate::log::Logger>,
    pub lines: tokio::sync::mpsc::UnboundedReceiver<String>,
}

impl Wired {
    /// A queue-backed logger with `base`'s level and format.
    ///
    /// `off` still wins: the settings are the operator's, only the sink changes.
    pub fn from(base: &crate::log::Logger) -> Self {
        let (sink, lines) = crate::log::Queue::new();
        Self {
            logger: std::sync::Arc::new(base.settings().logger_via(std::sync::Arc::new(sink))),
            lines,
        }
    }
}

pub async fn run(
    socket: &std::path::Path,
    root: &std::path::Path,
    readable: &std::path::Path,
    log: std::sync::Arc<crate::log::Logger>,
    wired: Option<Wired>,
) -> ExitCode {
    let target = Target {
        socket,
        root,
        readable,
        log: &log,
    };
    let history_path = history::path();
    let mut history = history_path
        .as_deref()
        .map(history::load)
        .unwrap_or_default();

    let code = if term::RawMode::is_interactive() {
        eprintln!("anth-panel interactive: Tab completes, Up recalls, `help` lists.");
        session(target, &mut history, wired).await
    } else {
        eprintln!("anth-panel interactive: `help`, `quit`, or a subcommand.");
        // A daemon in this process still queues its lines; without this task they
        // would pile up unread and the operator would see nothing.
        let writer = wired.map(|wired| tokio::spawn(print_lines(wired.lines)));
        let code = plain_session(target, &mut history).await;
        if let Some(writer) = writer {
            writer.abort();
        }
        code
    };

    // A history that cannot be written is worth one line, not a failed session:
    // the commands already ran, and the prompt still works without persistence.
    if let Some(path) = history_path
        && let Err(error) = history::save(&path, &history)
    {
        eprintln!("anth-panel: keeping history in {}: {error}", path.display());
    }

    code
}

/// The editor path, for a real terminal.
async fn session(
    target: Target<'_>,
    history: &mut editor::History,
    wired: Option<Wired>,
) -> ExitCode {
    // The guard restores the terminal however this function returns, including
    // through a panic. A terminal stuck in raw mode is worse than no prompt.
    let raw = match term::RawMode::enable() {
        Ok(raw) => raw,
        Err(error) => {
            eprintln!("anth-panel: using plain input: {error}");
            let writer = wired.map(|wired| tokio::spawn(print_lines(wired.lines)));
            let code = plain_session(target, history).await;
            if let Some(writer) = writer {
                writer.abort();
            }
            return code;
        }
    };

    let mut prompt = Prompt::new(std::mem::take(history));
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    // The prompt, not the logger, owns the terminal: the sink queues and this
    // loop paints. A daemon in this process was given the same logger, so its
    // lines arrive here too instead of landing on top of the prompt.
    let wired = wired.unwrap_or_else(|| Wired::from(target.log));
    let logger = wired.logger;
    let mut logs = wired.lines;
    let target = Target {
        socket: target.socket,
        root: target.root,
        readable: target.readable,
        log: &logger,
    };

    let mut keys = Reader::spawn();
    let code = editor_session(target, &mut prompt, &mut out, &mut keys, &mut logs).await;

    // Leave the cursor on a clean line for whatever writes next.
    let _ = out.write_all(b"\r\n");
    let _ = out.flush();
    drop(raw);

    *history = prompt.into_history();
    code
}

/// The byte-at-a-time loop, with log lines painted above the prompt.
///
/// Two inputs, one owner of the terminal. Keys come from a blocking reader on a
/// thread of its own, logs come from the logger's queue, and this loop is the
/// only thing that writes: it paints a log line above the prompt and repaints the
/// prompt under it, so an arriving line never lands in the middle of a word.
///
/// Reading stdin on a thread rather than in this function is also a fix of its
/// own: `input.read()` blocks, and blocking inside an async task holds a runtime
/// worker hostage for as long as the operator thinks.
async fn editor_session(
    target: Target<'_>,
    prompt: &mut Prompt,
    out: &mut impl Write,
    keys: &mut impl KeySource,
    logs: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
) -> ExitCode {
    let _ = out.write_all(prompt.paint_line().as_bytes());
    let _ = out.flush();

    loop {
        // `select!` rather than draining between keystrokes: a line that arrives
        // while the operator is idle must appear at once, not at the next Enter.
        let batch = tokio::select! {
            key = keys.next() => match key {
                Some(bytes) => Some(bytes),
                // The input ended. Whatever is half-typed is still submitted, so a
                // piped last line without a newline is not lost.
                None => {
                    let effects = prompt.finish();
                    return apply(target, effects, prompt, out)
                        .await
                        .unwrap_or(ExitCode::SUCCESS);
                }
            },
            line = logs.recv() => {
                if let Some(line) = line {
                    let bytes = prompt.paint_above(&line);
                    let _ = out.write_all(bytes.as_bytes());
                    let _ = out.flush();
                }
                // A closed queue means nothing will log again; keys keep working.
                continue;
            }
        };

        let effects = prompt.feed(&batch.expect("a key batch or an exit"));
        if let Some(code) = apply(target, effects, prompt, out).await {
            return code;
        }
    }
}

/// Acts on the prompt's effects, returning an exit code when the session ends.
///
/// Bytes are buffered and flushed once per batch, so a fast typist does not
/// cause a flush per keystroke.
async fn apply(
    target: Target<'_>,
    effects: Vec<Effect>,
    prompt: &mut Prompt,
    out: &mut impl Write,
) -> Option<ExitCode> {
    let mut wrote = false;

    for effect in effects {
        match effect {
            Effect::Repaint(bytes) | Effect::Offer { bytes, .. } => {
                let _ = out.write_all(bytes.as_bytes());
                wrote = true;
            }
            Effect::Cancel => {
                // A fresh line, and the session continues: leaving would close
                // the prompt over one mistyped word.
                let _ = out.write_all(b"^C\r\n");
                let _ = out.write_all(prompt.paint_line().as_bytes());
                wrote = true;
            }
            Effect::Exit => return Some(ExitCode::SUCCESS),
            Effect::Submit(line) => {
                // The command's output starts on its own line.
                let _ = out.write_all(b"\r\n");
                let _ = out.flush();

                if run_line(target, &line, prompt).await.is_some() {
                    return Some(ExitCode::SUCCESS);
                }

                let _ = out.write_all(prompt.paint_line().as_bytes());
                wrote = true;
            }
        }
    }

    if wrote {
        let _ = out.flush();
    }
    None
}

/// Runs one command line, returning an exit code only when it ends the session.
///
/// This is the original REPL body: the same `split`, the same
/// `Cli::try_parse_from`, the same `dispatch_command`. The editor only decides
/// *which* line arrives here.
async fn run_line(target: Target<'_>, line: &str, prompt: &mut Prompt) -> Option<ExitCode> {
    run_words(target, line, prompt.history_mut()).await
}

/// The shared body of both sessions, so a typed line behaves identically
/// whether it came from the editor or a pipe.
async fn run_words(
    target: Target<'_>,
    line: &str,
    history: &mut editor::History,
) -> Option<ExitCode> {
    let words = split(line);
    // An empty line is not a command: a stray Enter must not repeat the last
    // failure or exit the prompt.
    let (first, rest) = words.split_first()?;

    match first.as_str() {
        "quit" | "exit" => return Some(ExitCode::SUCCESS),
        "help" => {
            print_help();
            return None;
        }
        // A prompt cannot host a server: the serve loop never returns, so
        // accepting these froze the session with no way back except a signal.
        // They are start-time commands — `run -i` binds the server and then gives
        // you this same prompt — so the refusal names the spelling that works.
        "daemon" | "run" => {
            // Remembered like any other refused line, so Up reaches the typo.
            history.push(line);
            eprintln!(
                "anth-panel: `{first}` starts a server and cannot be typed inside the prompt; \
                 start the session with `anth-panel {first} -i` instead"
            );
            return None;
        }
        _ => {}
    }

    // Remember the line before running it, so `quit` and `help` are not the only
    // things a later session leaves out. A refused line is kept too: it is what
    // the user will fix with an Up and an edit.
    history.push(line);

    let args = std::iter::once("anth-panel")
        .chain(std::iter::once(first.as_str()))
        .chain(rest.iter().map(String::as_str));

    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) => {
            // clap's own message names the flag that was wrong, which is more
            // useful than anything this module could say.
            eprintln!("{error}");
            return None;
        }
    };

    // `--socket` on a line wins over the one the prompt started with, so a
    // session can be pointed at another daemon without restarting; `--plugins`
    // wins the same way for the directory.
    let socket = cli
        .socket
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| target.socket.to_path_buf());
    let root = cli
        .plugins
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| target.root.to_path_buf());

    // A failed command does not end the session: the reason is already on
    // stderr, and a prompt that vanished on a typo would lose it.
    dispatch_command(
        &socket,
        &root,
        target.readable,
        cli.command,
        std::sync::Arc::clone(target.log),
    )
    .await;
    None
}

/// Where a key batch comes from.
///
/// An interface rather than a concrete stdin, so the session loop can be driven
/// by a test that supplies keys and a log line in a known order. Nothing here
/// reads the environment.
pub trait KeySource {
    /// The next bytes, or `None` at end of input.
    fn next(&mut self) -> impl std::future::Future<Output = Option<Vec<u8>>> + Send;
}

/// Keys from stdin, read on a thread of its own.
///
/// `read` blocks, and blocking inside an async task would stall a runtime worker
/// until the operator types something. A dedicated thread never does that, and
/// the channel is what lets the session `select!` between keys and log lines.
pub struct Reader {
    lines: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
}

impl Reader {
    pub fn spawn() -> Self {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        std::thread::Builder::new()
            .name("anth-stdin".to_string())
            // A thread that cannot start is reported by the read path seeing a
            // closed channel, which ends the session rather than hanging it.
            .spawn(move || {
                use std::io::Read;
                let stdin = std::io::stdin();
                let mut input = stdin.lock();
                let mut buffer = [0u8; 256];
                loop {
                    match input.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(read) => {
                            if tx.send(buffer[..read].to_vec()).is_err() {
                                break;
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
            })
            .ok();
        Self { lines: rx }
    }
}

impl KeySource for Reader {
    async fn next(&mut self) -> Option<Vec<u8>> {
        self.lines.recv().await
    }
}

/// Writes queued log lines to stdout, for the session that has no prompt to
/// protect.
async fn print_lines(mut lines: tokio::sync::mpsc::UnboundedReceiver<String>) {
    use std::io::Write;
    let stdout = std::io::stdout();
    while let Some(line) = lines.recv().await {
        let mut out = stdout.lock();
        let _ = writeln!(out, "{line}");
        let _ = out.flush();
    }
}

/// The pipe path: whole lines, no editor and no escape sequences in the output.
///
/// The prompt goes to stderr and log lines to stdout. Kept apart on purpose: a
/// caller redirecting the two gets a clean transcript on each, and only a caller
/// merging them into one pipe sees them interleave — where there is no cursor to
/// protect, so there is nothing the queue could do about it anyway.
async fn plain_session(target: Target<'_>, history: &mut editor::History) -> ExitCode {
    let stdin = std::io::stdin();
    let mut line = String::new();

    loop {
        eprint!("{}", render::PROMPT);
        line.clear();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => break, // end of input
            Ok(_) => {}
            Err(error) => {
                eprintln!("anth-panel: reading stdin: {error}");
                return ExitCode::from(1);
            }
        }

        if let Some(code) = run_words(target, &line, history).await {
            return code;
        }
    }
    ExitCode::SUCCESS
}

fn print_help() {
    eprintln!(
        "commands:\n  \
         host-info                      one snapshot of the host\n  \
         plugin list                    every plugin the daemon found\n  \
         plugin install <bundle.zip>    add a bundle to the registry\n  \
         plugin remove <id>             delete an installed bundle\n  \
         plugin disable <id>            switch a plugin off for the next start\n  \
         plugin enable <id>             switch it back on\n  \
         plugin call <id> [LINE] [--http] [--direct]\n                                 \
         run one plugin over its own pipe\n  \
         help                           this message\n  \
         quit | exit                    leave\n\
         \n\
         `run` and `daemon` start a server, so they are given on the command line\n\
         (`anth-panel run -i`), not typed here.\n\
         \n\
         editing:\n  \
         Tab                            complete a command, subcommand or flag\n  \
         Up / Down                      walk the history\n  \
         Ctrl-R                         search the history\n  \
         Ctrl-A / Ctrl-E                start / end of line\n  \
         Ctrl-W                         delete the word before the cursor\n  \
         Ctrl-K / Ctrl-U                delete to the end / start\n  \
         Ctrl-L                         repaint the screen\n  \
         Ctrl-C                         abandon the line\n  \
         Ctrl-D                         leave on an empty line"
    );
}

/// Splits a line the way a shell does, so `--message "Ana Sol"` survives.
///
/// The rule: single quotes preserve everything literally, double quotes group
/// and honour `\"`, a backslash outside quotes escapes the next character, and
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
mod tests;
