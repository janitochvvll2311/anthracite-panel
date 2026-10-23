//! The prompt's own tests: line splitting, and the shared command body.
//!
//! The editing behaviour is tested where it lives — `editor`, `keys`,
//! `completion`, `prompt` and `render` each carry their own module tests, because
//! that is the module whose contract they assert. What is left here is the part
//! that belongs to this file: a line has to split like a shell's, and `quit`,
//! `help` and a refused line have to behave the same from the editor and a pipe.

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

/// A bare `quit` and a `quit` typed through the editor must both end the
/// session, which is what sharing `run_words` guarantees.
#[tokio::test]
async fn quit_ends_the_session() {
    let mut history = editor::History::new();
    let socket = std::path::Path::new("/tmp/anth-unused.sock");

    let code = run_words(socket, "quit", &mut history).await;

    assert_eq!(code, Some(ExitCode::SUCCESS));
}

#[tokio::test]
async fn exit_ends_the_session() {
    let mut history = editor::History::new();
    let socket = std::path::Path::new("/tmp/anth-unused.sock");

    assert_eq!(
        run_words(socket, "exit", &mut history).await,
        Some(ExitCode::SUCCESS)
    );
}

/// `help` prints and keeps the session open, and is not remembered: recalling
/// it later would be noise.
#[tokio::test]
async fn help_does_not_end_the_session_and_is_not_history() {
    let mut history = editor::History::new();
    let socket = std::path::Path::new("/tmp/anth-unused.sock");

    let code = run_words(socket, "help", &mut history).await;

    assert_eq!(code, None);
    assert!(history.entries().is_empty());
}

/// A blank line is not a command: it must not run the previous one again.
#[tokio::test]
async fn a_blank_line_does_nothing() {
    let mut history = editor::History::new();
    let socket = std::path::Path::new("/tmp/anth-unused.sock");

    assert_eq!(run_words(socket, "   ", &mut history).await, None);
    assert!(history.entries().is_empty());
}

/// A line clap refuses is still remembered, because fixing it with Up is the
/// whole reason to recall a mistake.
#[tokio::test]
async fn a_refused_line_is_remembered() {
    let mut history = editor::History::new();
    let socket = std::path::Path::new("/tmp/anth-unused.sock");

    let code = run_words(socket, "nonsense-command", &mut history).await;

    assert_eq!(code, None);
    assert_eq!(history.entries(), ["nonsense-command"]);
}

/// A command that fails at the daemon does not end the session either.
#[tokio::test]
async fn a_failed_command_keeps_the_session() {
    let mut history = editor::History::new();
    // Nothing listens here, so the client reports a connection failure.
    let socket = std::path::Path::new("/tmp/anth-absent-for-the-test.sock");

    let code = run_words(socket, "host-info", &mut history).await;

    assert_eq!(code, None);
    assert_eq!(history.entries(), ["host-info"]);
}
