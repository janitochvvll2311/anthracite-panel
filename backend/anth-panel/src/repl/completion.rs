//! Tab completion, derived from the CLI definition rather than a second list.
//!
//! The candidates come from clap's own `Command`, which is the object `Cli`
//! already builds. That is the whole design: a subcommand or flag added to
//! `main.rs` is completable immediately, and cannot be forgotten here. A
//! hand-written table would be the same drift the REPL's dispatcher exists to
//! avoid — a prompt that offers a command `main` does not have, or hides one it
//! does.
//!
//! Completion is static on purpose. The one dynamic candidate worth having is
//! `plugin call <id>`, and answering it means a socket round trip per keystroke;
//! the editor would block on a daemon that may be down. The id is short and
//! `plugin list` prints it.

use clap::{Command, CommandFactory};

use crate::Cli;

/// One thing Tab can insert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The text that replaces the current word.
    pub text: String,
    /// What it is, shown in the picker when several match.
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Subcommand,
    Flag,
}

/// Every candidate for the word being completed.
///
/// `words` is the line split on whitespace with the word under the cursor as
/// its last element, so `["plugin", "li"]` completes the `li`. Only that last
/// word is filtered by; the ones before it select which subcommand's grammar
/// applies.
pub fn candidates(words: &[&str]) -> Vec<Candidate> {
    let (prefix, path) = match words.split_last() {
        Some((prefix, path)) => (*prefix, path),
        None => ("", [].as_slice()),
    };

    let Some(command) = command_at(path) else {
        // Past a word that names no subcommand, there is no grammar to offer.
        return Vec::new();
    };

    let mut found = Vec::new();

    // A word starting with `-` is a flag; anything else is a subcommand or a
    // positional, and only subcommands are completable.
    if prefix.starts_with('-') {
        for flag in long_flags(&command, true) {
            if flag.starts_with(prefix) {
                found.push(Candidate {
                    text: flag,
                    kind: Kind::Flag,
                });
            }
        }
    } else {
        for name in command.get_subcommands().map(Command::get_name) {
            if name.starts_with(prefix) {
                found.push(Candidate {
                    text: name.to_string(),
                    kind: Kind::Subcommand,
                });
            }
        }
        // Only the command's own flags: global ones appear once the word being
        // completed starts with `-`, so a Tab after a space lists commands
        // rather than a wall of flags.
        for flag in long_flags(&command, false) {
            if flag.starts_with(prefix) {
                found.push(Candidate {
                    text: flag,
                    kind: Kind::Flag,
                });
            }
        }
    }

    found.sort_by(|a, b| a.text.cmp(&b.text));
    found.dedup_by(|a, b| a.text == b.text);
    found
}

/// The `clap::Command` a path of words selects, if it selects one.
fn command_at(path: &[&str]) -> Option<Command> {
    let mut command = Cli::command();
    for word in path {
        let next = command
            .get_subcommands()
            .find(|sub| sub.get_name() == *word)
            .cloned()?;
        command = next;
    }
    Some(command)
}

/// The flags declared `global = true` on the root, which every subcommand
/// accepts even though clap does not list them there.
///
/// Calling `Command::build` would propagate them, but it also materialises the
/// implicit `help` subcommand into every child, so `plugin <Tab>` would offer
/// `help` beside `call` and `list`. Reading the root's globals keeps the
/// completion list equal to the grammar a user reads in `--help`.
fn global_flags() -> Vec<String> {
    Cli::command()
        .get_arguments()
        .filter(|arg| arg.is_global_set())
        .filter_map(|arg| arg.get_long().map(|long| format!("--{long}")))
        .collect()
}

/// The long flags a command accepts, spelled `--name`.
///
/// Hidden ones are skipped: completing a flag the help text does not show would
/// advertise an interface the operator is not meant to use.
fn long_flags(command: &Command, include_globals: bool) -> Vec<String> {
    let mut flags: Vec<String> = command
        .get_arguments()
        .filter(|arg| !arg.is_hide_set())
        .filter_map(|arg| arg.get_long().map(|long| format!("--{long}")))
        .collect();

    if include_globals {
        flags.extend(global_flags());
    }

    // `clap` already adds `--help`; `--version` is only on the root. Reporting
    // whatever the definition holds keeps both honest.
    flags.sort();
    flags.dedup();
    flags
}
