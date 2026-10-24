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
}

pub async fn run(socket: &std::path::Path, root: &std::path::Path) -> ExitCode {
    let target = Target { socket, root };
    let history_path = history::path();
    let mut history = history_path
        .as_deref()
        .map(history::load)
        .unwrap_or_default();

    let code = if term::RawMode::is_interactive() {
        eprintln!("anth-panel interactive: Tab completes, Up recalls, `help` lists.");
        session(target, &mut history).await
    } else {
        eprintln!("anth-panel interactive: `help`, `quit`, or a subcommand.");
        plain_session(target, &mut history).await
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
async fn session(target: Target<'_>, history: &mut editor::History) -> ExitCode {
    // The guard restores the terminal however this function returns, including
    // through a panic. A terminal stuck in raw mode is worse than no prompt.
    let raw = match term::RawMode::enable() {
        Ok(raw) => raw,
        Err(error) => {
            eprintln!("anth-panel: using plain input: {error}");
            return plain_session(target, history).await;
        }
    };

    let mut prompt = Prompt::new(std::mem::take(history));
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let code = editor_session(target, &mut prompt, &mut out).await;

    // Leave the cursor on a clean line for whatever writes next.
    let _ = out.write_all(b"\r\n");
    let _ = out.flush();
    drop(raw);

    *history = prompt.into_history();
    code
}

/// The byte-at-a-time loop.
async fn editor_session(target: Target<'_>, prompt: &mut Prompt, out: &mut impl Write) -> ExitCode {
    use std::io::Read;

    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut buffer = [0u8; 256];

    let _ = out.write_all(prompt.paint_line().as_bytes());
    let _ = out.flush();

    loop {
        let read = match input.read(&mut buffer) {
            Ok(0) => {
                let effects = prompt.finish();
                return apply(target, effects, prompt, out)
                    .await
                    .unwrap_or(ExitCode::SUCCESS);
            }
            Ok(read) => read,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                eprintln!("anth-panel: reading stdin: {error}");
                return ExitCode::from(1);
            }
        };

        let effects = prompt.feed(&buffer[..read]);
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
    dispatch_command(&socket, &root, cli.command).await;
    None
}

/// The pipe path: whole lines, no editor and no escape sequences in the output.
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
