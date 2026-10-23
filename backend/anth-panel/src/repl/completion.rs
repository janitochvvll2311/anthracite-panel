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

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(words: &[&str]) -> Vec<String> {
        candidates(words)
            .into_iter()
            .map(|candidate| candidate.text)
            .collect()
    }

    /// The root grammar is read from `Cli`, so this asserts the wiring: a
    /// subcommand that exists must be offered.
    #[test]
    fn an_empty_line_offers_every_subcommand() {
        let offered = texts(&[""]);

        for expected in ["daemon", "host-info", "plugin", "run"] {
            assert!(offered.contains(&expected.to_string()), "{offered:?}");
        }
    }

    #[test]
    fn a_prefix_narrows_the_subcommands() {
        assert_eq!(texts(&["ho"]), vec!["host-info"]);
        assert_eq!(texts(&["p"]), vec!["plugin"]);
    }

    #[test]
    fn a_deeper_prefix_reaches_the_nested_subcommands() {
        assert_eq!(texts(&["plugin", ""]), vec!["call", "list"]);
        assert_eq!(texts(&["plugin", "ca"]), vec!["call"]);
    }

    /// The nested grammar is the `plugin` subcommand's, not the root's: offering
    /// `host-info` after `plugin` would be a list the parser rejects.
    #[test]
    fn a_nested_prefix_does_not_offer_root_subcommands() {
        let offered = texts(&["plugin", "l"]);

        assert_eq!(offered, vec!["list"]);
        assert!(!offered.contains(&"host-info".to_string()));
    }

    /// A flag is only offered once the word starts with `-`, or every Tab after
    /// a space would list them beside the commands.
    #[test]
    fn a_leading_dash_offers_flags_instead_of_commands() {
        let offered = texts(&["--"]);

        assert!(
            offered.iter().all(|text| text.starts_with("--")),
            "{offered:?}"
        );
        assert!(offered.contains(&"--socket".to_string()), "{offered:?}");
        assert!(
            offered.contains(&"--interactive".to_string()),
            "{offered:?}"
        );
    }

    #[test]
    fn a_flag_prefix_narrows_the_flags() {
        assert_eq!(texts(&["--soc"]), vec!["--socket"]);
    }

    /// `--socket` is global, so it must be completable after a subcommand too.
    #[test]
    fn a_global_flag_is_offered_after_a_subcommand() {
        assert_eq!(texts(&["host-info", "--soc"]), vec!["--socket"]);
    }

    /// A word that names nothing is not a path into the grammar; guessing what
    /// the user meant would offer completions for the wrong command.
    #[test]
    fn an_unknown_path_offers_nothing() {
        assert!(texts(&["nonsense", ""]).is_empty());
    }

    #[test]
    fn a_word_that_matches_nothing_offers_nothing() {
        assert!(texts(&["zzz"]).is_empty());
    }

    /// The kind travels with the candidate, because the picker shows it and a
    /// flag offered as a command would read as a grammar mistake.
    #[test]
    fn a_candidate_reports_whether_it_is_a_command_or_a_flag() {
        let commands = candidates(&["ho"]);
        assert_eq!(commands[0].kind, Kind::Subcommand);

        let flags = candidates(&["--soc"]);
        assert_eq!(flags[0].kind, Kind::Flag);
    }
}
