//! Tests for every module under `repl/`.
//!
//! One file per level, with an inner module per source file. The first module
//! covers this file: a line has to split like a shell's, and `quit`, `help` and
//! a refused line have to behave the same from the editor and a pipe. The rest
//! assert each neighbour's own contract — the decoder, the line editor, raw
//! mode, completion, the prompt state machine, the repaint arithmetic and the
//! history file.
//!
//! `keys` and `editor` share `Key`, so `keys` reaches into `editor` explicitly
//! rather than glob-importing it; a module named `editor` inside this file would
//! otherwise shadow the one being tested.

use super::{run_words, split};

use std::process::ExitCode;

use crate::repl::editor::History;

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
    let mut history = History::new();
    let socket = std::path::Path::new("/tmp/anth-unused.sock");

    let code = run_words(socket, "quit", &mut history).await;

    assert_eq!(code, Some(ExitCode::SUCCESS));
}

#[tokio::test]
async fn exit_ends_the_session() {
    let mut history = History::new();
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
    let mut history = History::new();
    let socket = std::path::Path::new("/tmp/anth-unused.sock");

    let code = run_words(socket, "help", &mut history).await;

    assert_eq!(code, None);
    assert!(history.entries().is_empty());
}

/// A blank line is not a command: it must not run the previous one again.
#[tokio::test]
async fn a_blank_line_does_nothing() {
    let mut history = History::new();
    let socket = std::path::Path::new("/tmp/anth-unused.sock");

    assert_eq!(run_words(socket, "   ", &mut history).await, None);
    assert!(history.entries().is_empty());
}

/// A line clap refuses is still remembered, because fixing it with Up is the
/// whole reason to recall a mistake.
#[tokio::test]
async fn a_refused_line_is_remembered() {
    let mut history = History::new();
    let socket = std::path::Path::new("/tmp/anth-unused.sock");

    let code = run_words(socket, "nonsense-command", &mut history).await;

    assert_eq!(code, None);
    assert_eq!(history.entries(), ["nonsense-command"]);
}

/// A command that fails at the daemon does not end the session either.
#[tokio::test]
async fn a_failed_command_keeps_the_session() {
    let mut history = History::new();
    // Nothing listens here, so the client reports a connection failure.
    let socket = std::path::Path::new("/tmp/anth-absent-for-the-test.sock");

    let code = run_words(socket, "host-info", &mut history).await;

    assert_eq!(code, None);
    assert_eq!(history.entries(), ["host-info"]);
}

/// `run` and `daemon` never return, so accepting them inside the prompt froze
/// the session. They are refused, the session survives, and Up reaches the line.
#[tokio::test]
async fn a_server_command_is_refused_instead_of_freezing_the_prompt() {
    for line in ["run", "run --addr 127.0.0.1:0", "daemon"] {
        let mut history = History::new();
        let socket = std::path::Path::new("/tmp/anth-unused.sock");

        assert_eq!(run_words(socket, line, &mut history).await, None);
        assert_eq!(history.entries(), [line]);
    }
}

mod keys {
    use crate::repl::editor::Key;
    use crate::repl::keys::*;

    fn decode(bytes: &[u8]) -> Vec<Key> {
        let mut decoder = Decoder::new();
        decoder.feed(bytes)
    }

    #[test]
    fn printable_bytes_become_characters() {
        assert_eq!(decode(b"hi"), vec![Key::Char('h'), Key::Char('i')]);
    }

    #[test]
    fn enter_is_recognised_in_both_encodings() {
        assert_eq!(decode(b"\r"), vec![Key::Enter]);
        assert_eq!(decode(b"\n"), vec![Key::Enter]);
    }

    #[test]
    fn backspace_is_recognised_in_both_encodings() {
        assert_eq!(decode(&[0x7f]), vec![Key::Backspace]);
        assert_eq!(decode(&[0x08]), vec![Key::Backspace]);
    }

    /// Both cursor modes must land on the same key, or arrows work in one
    /// terminal and insert `[A` in another.
    #[test]
    fn arrows_decode_in_normal_and_application_mode() {
        assert_eq!(decode(b"\x1b[A"), vec![Key::Up]);
        assert_eq!(decode(b"\x1bOA"), vec![Key::Up]);
        assert_eq!(decode(b"\x1b[D"), vec![Key::Left]);
        assert_eq!(decode(b"\x1bOD"), vec![Key::Left]);
    }

    #[test]
    fn home_end_and_delete_decode_from_csi_sequences() {
        assert_eq!(decode(b"\x1b[H"), vec![Key::Home]);
        assert_eq!(decode(b"\x1b[F"), vec![Key::End]);
        assert_eq!(decode(b"\x1b[3~"), vec![Key::Delete]);
        assert_eq!(decode(b"\x1b[1~"), vec![Key::Home]);
        assert_eq!(decode(b"\x1b[4~"), vec![Key::End]);
    }

    /// The read of an arrow key can arrive as `ESC` and `[A` separately. The
    /// decoder must not emit a key for the first half.
    #[test]
    fn a_sequence_split_across_reads_waits_for_the_rest() {
        let mut decoder = Decoder::new();

        assert!(decoder.feed(b"\x1b").is_empty());
        assert!(decoder.has_pending());
        assert!(decoder.feed(b"[").is_empty());

        assert_eq!(decoder.feed(b"A"), vec![Key::Up]);
        assert!(!decoder.has_pending());
    }

    /// A lone Escape press is never followed by more bytes, so the caller
    /// flushes it rather than waiting forever.
    #[test]
    fn a_lone_escape_is_flushed_as_ignored() {
        let mut decoder = Decoder::new();
        assert!(decoder.feed(b"\x1b").is_empty());

        assert_eq!(decoder.flush(), vec![Key::Ignored]);
        assert!(!decoder.has_pending());
    }

    /// Flushing an empty buffer must not invent a keypress.
    #[test]
    fn flushing_nothing_produces_nothing() {
        let mut decoder = Decoder::new();

        assert!(decoder.flush().is_empty());
    }

    /// An unbound escape — here Alt-b — is dropped instead of typing `b` or
    /// `[A` by accident.
    #[test]
    fn an_unbound_escape_chord_is_ignored() {
        assert_eq!(decode(b"\x1bb"), vec![Key::Ignored]);
    }

    #[test]
    fn a_multi_byte_character_split_across_reads_waits() {
        let mut decoder = Decoder::new();
        let encoded = "ñ".as_bytes();

        assert!(decoder.feed(&encoded[..1]).is_empty());
        assert_eq!(decoder.feed(&encoded[1..]), vec![Key::Char('ñ')]);
    }

    #[test]
    fn a_multi_byte_character_in_one_read_decodes() {
        assert_eq!(
            decode("año".as_bytes()),
            vec![Key::Char('a'), Key::Char('ñ'), Key::Char('o')]
        );
    }

    #[test]
    fn control_keys_map_to_their_intent() {
        assert_eq!(decode(&[0x03]), vec![Key::Interrupt]);
        assert_eq!(decode(&[0x04]), vec![Key::EndOfInput]);
        assert_eq!(decode(&[0x01]), vec![Key::Home]);
        assert_eq!(decode(&[0x05]), vec![Key::End]);
        assert_eq!(decode(&[0x0b]), vec![Key::KillToEnd]);
        assert_eq!(decode(&[0x15]), vec![Key::KillToStart]);
        assert_eq!(decode(&[0x17]), vec![Key::KillWord]);
        assert_eq!(decode(&[0x0c]), vec![Key::ClearScreen]);
        assert_eq!(decode(&[0x12]), vec![Key::SearchHistory]);
    }

    /// One read can carry a whole pasted line. Every character has to come out,
    /// not just the first.
    #[test]
    fn a_whole_line_in_one_read_decodes_every_character() {
        let keys = decode(b"plugin list");

        assert_eq!(keys.len(), 11);
        assert_eq!(keys[0], Key::Char('p'));
        assert_eq!(keys[10], Key::Char('t'));
    }

    /// A bracketed paste arrives wrapped in `ESC [ 200 ~` … `ESC [ 201 ~`. The
    /// markers must vanish; pasting a command should not insert sequences.
    #[test]
    fn bracketed_paste_markers_are_ignored() {
        let keys = decode(b"\x1b[200~ls\x1b[201~");

        assert_eq!(
            keys,
            vec![Key::Ignored, Key::Char('l'), Key::Char('s'), Key::Ignored]
        );
    }

    #[test]
    fn an_invalid_byte_is_skipped_without_losing_the_next_key() {
        let keys = decode(&[0xff, b'a']);

        assert_eq!(keys, vec![Key::Char('a')]);
    }
}

mod editor {
    use crate::repl::editor::*;

    fn line(text: &str) -> Line {
        Line::with_text(text)
    }

    #[test]
    fn typing_appends_at_the_cursor() {
        let mut line = Line::new();
        for c in "abc".chars() {
            line.insert(c);
        }

        assert_eq!(line.text(), "abc");
        assert_eq!(line.cursor(), 3);
    }

    #[test]
    fn typing_in_the_middle_does_not_jump_to_the_end() {
        let mut line = line("ac");
        line.left();
        line.insert('b');

        assert_eq!(line.text(), "abc");
        assert_eq!(line.cursor(), 2);
    }

    /// A multi-byte character is one editing unit. Deleting half of `ñ` would
    /// panic on the char boundary, and moving into it would corrupt the text.
    #[test]
    fn a_multi_byte_character_is_one_editing_unit() {
        let mut line = line("años");
        line.backspace();

        assert_eq!(line.text(), "año");

        line.left();
        line.left();
        line.backspace();

        assert_eq!(line.text(), "ño");
        assert_eq!(line.cursor(), 0);
    }

    #[test]
    fn backspace_at_the_start_is_a_no_op() {
        let mut line = line("abc");
        line.home();
        line.backspace();

        assert_eq!(line.text(), "abc");
        assert_eq!(line.cursor(), 0);
    }

    #[test]
    fn delete_removes_under_the_cursor_only() {
        let mut line = line("abc");
        line.home();
        line.delete();

        assert_eq!(line.text(), "bc");
        assert_eq!(line.cursor(), 0);
    }

    #[test]
    fn arrows_stop_at_both_ends() {
        let mut line = line("ab");
        line.right();
        assert_eq!(line.cursor(), 2);
        line.left();
        line.left();
        line.left();
        assert_eq!(line.cursor(), 0);
    }

    #[test]
    fn home_and_end_move_to_the_edges() {
        let mut line = line("abc");
        line.home();
        assert_eq!(line.cursor(), 0);
        line.end();
        assert_eq!(line.cursor(), 3);
    }

    #[test]
    fn ctrl_k_and_ctrl_u_cut_around_the_cursor() {
        let mut cut = line("abcdef");
        cut.left();
        cut.left();
        cut.kill_to_end();
        assert_eq!(cut.text(), "abcd");

        let mut line = line("abcdef");
        line.left();
        line.left();
        line.kill_to_start();
        assert_eq!(line.text(), "ef");
        assert_eq!(line.cursor(), 0);
    }

    #[test]
    fn ctrl_w_removes_the_word_and_the_space_before_it() {
        let mut line = line("plugin call hello ");

        line.kill_word();

        assert_eq!(line.text(), "plugin call");
    }

    #[test]
    fn ctrl_w_on_one_word_leaves_nothing() {
        let mut line = line("hello");
        line.kill_word();

        assert_eq!(line.text(), "");
        assert_eq!(line.cursor(), 0);
    }

    #[test]
    fn the_word_under_the_cursor_starts_after_the_last_space() {
        assert_eq!(line("plugin li").word(), "li");
        assert_eq!(line("plugin ").word(), "");
        assert_eq!(line("").word(), "");
    }

    /// A cursor in the middle of a line completes the word it is inside, not
    /// the tail of the line.
    #[test]
    fn a_cursor_inside_a_word_completes_that_word() {
        let mut line = line("plugin list --json");
        line.set_cursor(9);

        assert_eq!(line.word(), "list");
    }

    #[test]
    fn a_completion_replaces_the_word_and_adds_a_space() {
        let mut line = line("plugin li");

        line.replace_word("list");

        assert_eq!(line.text(), "plugin list ");
        assert_eq!(line.cursor(), 12);
    }

    /// Completing mid-line must not eat what followed the cursor.
    #[test]
    fn a_completion_keeps_the_text_after_the_cursor() {
        let mut line = line("plugin li --json");
        line.set_cursor(9);

        line.replace_word("list");

        assert_eq!(line.text(), "plugin list --json");
    }

    #[test]
    fn history_skips_blank_and_repeated_lines() {
        let mut history = History::new();
        history.push("one");
        history.push("one");
        history.push("   ");
        history.push("two");

        assert_eq!(history.entries(), ["two", "one"]);
    }

    #[test]
    fn up_walks_backwards_and_stops_at_the_oldest() {
        let mut history = History::new();
        history.push("one");
        history.push("two");

        assert_eq!(history.older("draft"), Some("two"));
        assert_eq!(history.older("draft"), Some("one"));
        assert_eq!(history.older("draft"), Some("one"));
    }

    /// Down past the newest entry restores what was being typed. Losing it is
    /// the classic readline annoyance: you arrow up to check something and your
    /// half-written command is gone.
    #[test]
    fn down_past_the_newest_restores_the_draft() {
        let mut history = History::new();
        history.push("one");

        assert_eq!(history.older("half typed"), Some("one"));
        assert_eq!(history.newer(), Some("half typed"));
        assert_eq!(history.newer(), None);
    }

    #[test]
    fn down_on_a_fresh_line_does_nothing() {
        let mut history = History::new();
        history.push("one");

        assert_eq!(history.newer(), None);
    }

    /// An edit ends the browsing session, so Up starts from the newest entry
    /// again instead of resuming from wherever it was left.
    #[test]
    fn resetting_makes_up_start_from_the_newest() {
        let mut history = History::new();
        history.push("one");
        history.push("two");
        history.older("draft");
        history.older("draft");

        history.reset();

        assert_eq!(history.older("draft"), Some("two"));
    }

    #[test]
    fn an_empty_history_yields_nothing() {
        let mut history = History::new();

        assert_eq!(history.older("draft"), None);
        assert_eq!(history.search("anything"), None);
    }

    #[test]
    fn search_finds_the_newest_match() {
        let mut history = History::new();
        history.push("plugin list");
        history.push("host-info");
        history.push("plugin call hello");

        assert_eq!(history.search("plugin"), Some("plugin call hello"));
        assert_eq!(history.search("host"), Some("host-info"));
        assert_eq!(history.search("nope"), None);
        assert_eq!(history.search("  "), None);
    }
}

mod term {
    use crate::repl::term::*;

    /// A pipe is not a terminal, so enabling is a no-op rather than the failure
    /// `tcsetattr` would give on a pipe.
    #[test]
    fn a_pipe_needs_no_raw_mode() {
        let mode = RawMode::enable_if(false).expect("enabling on a pipe must not fail");

        assert!(mode.original.is_none());
    }

    /// Dropping a no-op guard must not try to write to a pipe.
    #[test]
    fn dropping_a_no_op_guard_is_safe() {
        let mode = RawMode::enable_if(false).expect("enabling");
        drop(mode);
    }
}

mod completion {
    use crate::repl::completion::*;

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
        assert_eq!(
            texts(&["plugin", ""]),
            vec!["call", "install", "list", "remove"]
        );
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

mod prompt {
    use crate::repl::completion;
    use crate::repl::editor::History;
    use crate::repl::prompt::*;

    fn prompt() -> Prompt {
        Prompt::new(History::new())
    }

    /// Runs a byte script and returns the submitted lines.
    fn submitted(bytes: &[u8]) -> Vec<String> {
        let mut prompt = prompt();
        prompt
            .feed(bytes)
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::Submit(line) => Some(line),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn typing_then_enter_submits_the_line() {
        assert_eq!(submitted(b"host-info\r"), vec!["host-info"]);
    }

    #[test]
    fn several_lines_submit_in_order() {
        assert_eq!(submitted(b"one\r\r two\r"), vec!["one", "two"]);
    }

    /// An empty Enter submits nothing rather than the previous line: a stray
    /// keypress must not re-run a destructive command.
    #[test]
    fn an_empty_enter_submits_nothing() {
        assert_eq!(submitted(b"\r"), Vec::<String>::new());
        assert_eq!(submitted(b"   \r"), Vec::<String>::new());
    }

    /// The whole point of the arrows: recover a command and run it.
    #[test]
    fn up_recalls_the_previous_line_and_enter_runs_it() {
        assert_eq!(
            submitted(b"host-info\r\x1b[A\r"),
            vec!["host-info", "host-info"]
        );
    }

    #[test]
    fn two_ups_go_two_back() {
        assert_eq!(
            submitted(b"one\rtwo\r\x1b[A\x1b[A\r"),
            vec!["one", "two", "one"]
        );
    }

    /// Down returns to the draft that was being typed. Losing it is the classic
    /// annoyance this guards against.
    #[test]
    fn down_restores_what_was_being_typed() {
        assert_eq!(
            submitted(b"older\rdraft\x1b[A\x1b[B\r"),
            vec!["older", "draft"]
        );
    }

    /// Up must not walk into the string that was just submitted from history —
    /// the history already contains it, so the line is cleared and the next Up
    /// starts at the newest entry, which is the line that was just run.
    #[test]
    fn history_browsing_survives_submitting_a_recalled_line() {
        assert_eq!(
            submitted(b"one\rtwo\r\x1b[A\r\x1b[A\r"),
            vec!["one", "two", "two", "two"]
        );
    }

    #[test]
    fn left_and_right_move_the_insertion_point() {
        // Type "ac", move left, insert "b".
        assert_eq!(submitted(b"ac\x1b[Db\r"), vec!["abc"]);
    }

    #[test]
    fn backspace_removes_the_character_before_the_cursor() {
        assert_eq!(submitted(b"abc\x7f\r"), vec!["ab"]);
    }

    #[test]
    fn delete_removes_the_character_under_the_cursor() {
        // `abc` with the cursor before `c`: Delete takes the character it sits
        // on, which is `c`, leaving `ab` with the cursor still before nothing.
        assert_eq!(submitted(b"abc\x1b[D\x1b[3~\r"), vec!["ab"]);
    }

    #[test]
    fn ctrl_k_clears_the_tail() {
        assert_eq!(submitted(b"abcdef\x1b[D\x1b[D\x0b\r"), vec!["abcd"]);
    }

    #[test]
    fn ctrl_u_clears_the_head() {
        assert_eq!(submitted(b"abcdef\x1b[D\x1b[D\x15\r"), vec!["ef"]);
    }

    #[test]
    fn ctrl_w_removes_a_word() {
        assert_eq!(submitted(b"plugin call hello \x17\r"), vec!["plugin call"]);
    }

    /// Tab finishes an unambiguous word, including the subcommand grammar.
    #[test]
    fn tab_completes_a_unique_subcommand() {
        assert_eq!(submitted(b"ho\t\r"), vec!["host-info"]);
    }

    /// Tab must keep the rest of the line: `plugin li --json` completes the
    /// word, not the tail.
    #[test]
    fn tab_completes_only_the_word_under_the_cursor() {
        assert_eq!(submitted(b"plugin l\t\r"), vec!["plugin list"]);
    }

    #[test]
    fn tab_completes_a_nested_subcommand() {
        assert_eq!(submitted(b"plugin ca\t\r"), vec!["plugin call"]);
    }

    #[test]
    fn tab_completes_a_flag() {
        assert_eq!(
            submitted(b"host-info --soc\t\r"),
            vec!["host-info --socket"]
        );
    }

    /// Two candidates list instead of inserting, and the effect carries them so
    /// the caller can draw them.
    #[test]
    fn tab_on_an_ambiguous_word_offers_the_candidates() {
        let mut prompt = prompt();
        let effects = prompt.feed(b"plugin \t");

        let offered = effects
            .into_iter()
            .find_map(|effect| match effect {
                Effect::Offer { candidates, .. } => Some(candidates),
                _ => None,
            })
            .expect("a list of candidates");

        let names: Vec<String> = offered.into_iter().map(|c| c.text).collect();
        assert_eq!(names, vec!["call", "install", "list", "remove"]);
    }

    /// A second Tab with nothing typed between inserts the shared prefix rather
    /// than listing again forever.
    #[test]
    fn a_second_tab_inserts_the_shared_prefix() {
        let mut prompt = prompt();
        prompt.feed(b"plugin \t");
        prompt.feed(b"\t");

        assert_eq!(prompt.line().text(), "plugin ");
    }

    #[test]
    fn tab_that_matches_nothing_changes_nothing() {
        let mut prompt = prompt();
        prompt.feed(b"zzz\t");

        assert_eq!(prompt.line().text(), "zzz");
    }

    /// Ctrl-R finds a command by a fragment, which is how you recover one you
    /// only half remember.
    #[test]
    fn ctrl_r_recalls_the_newest_matching_line() {
        assert_eq!(
            submitted(b"host-info\rplugin list\rplug\x12\r"),
            vec!["host-info", "plugin list", "plugin list"]
        );
    }

    #[test]
    fn ctrl_d_on_an_empty_line_exits() {
        let mut prompt = prompt();

        assert_eq!(prompt.feed(&[0x04]), vec![Effect::Exit]);
    }

    /// Ctrl-D on a non-empty line is delete-under-cursor, not exit: exiting with
    /// a half-typed command would lose it.
    #[test]
    fn ctrl_d_on_a_line_deletes_instead_of_exiting() {
        let mut prompt = prompt();
        prompt.feed(b"ab");
        prompt.feed(b"\x1b[D");

        let effects = prompt.feed(&[0x04]);

        assert!(matches!(effects.as_slice(), [Effect::Repaint(_)]));
        assert_eq!(prompt.line().text(), "a");
        assert_eq!(prompt.line().cursor(), 1);
    }

    #[test]
    fn ctrl_c_cancels_and_clears_the_line() {
        let mut prompt = prompt();
        prompt.feed(b"half typed");

        assert_eq!(prompt.feed(&[0x03]), vec![Effect::Cancel]);
        assert!(prompt.line().is_empty());
    }

    /// `ISIG` keeps Ctrl-C as a signal in a real terminal, so this only guards
    /// the fallback path: the cancelled text is not submitted, only the fresh
    /// line that follows it.
    #[test]
    fn the_line_after_a_cancel_is_fresh() {
        let mut prompt = prompt();
        prompt.feed(b"typo");
        prompt.feed(&[0x03]);

        let submitted: Vec<String> = prompt
            .feed(b"ok\r")
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::Submit(line) => Some(line),
                _ => None,
            })
            .collect();

        assert_eq!(submitted, vec!["ok"]);
        assert!(prompt.line().is_empty());
    }

    /// A piped file whose last line has no newline must still run.
    #[test]
    fn finish_submits_a_line_without_a_newline() {
        let mut prompt = prompt();
        prompt.feed(b"host-info");

        assert_eq!(
            prompt.finish(),
            vec![Effect::Submit("host-info".to_string())]
        );
    }

    #[test]
    fn finish_on_an_empty_line_exits() {
        let mut prompt = prompt();

        assert_eq!(prompt.finish(), vec![Effect::Exit]);
    }

    /// A multi-byte character typed in one read reaches the line intact.
    #[test]
    fn a_multi_byte_character_is_submitted_whole() {
        assert_eq!(submitted("añadir\r".as_bytes()), vec!["añadir"]);
    }

    /// An arrow split across two reads must still work, which is the whole
    /// reason the decoder buffers.
    #[test]
    fn an_arrow_split_across_reads_still_works() {
        let mut prompt = prompt();
        prompt.feed(b"one\r");
        prompt.feed(b"\x1b");
        prompt.feed(b"[A");

        assert_eq!(prompt.line().text(), "one");
    }

    #[test]
    fn edits_are_painted_so_the_terminal_shows_them() {
        let mut prompt = prompt();

        let effects = prompt.feed(b"a");

        assert!(matches!(effects.as_slice(), [Effect::Repaint(_)]));
    }

    #[test]
    fn the_common_prefix_of_several_candidates_is_found() {
        let found = completion::candidates(&["plugin", ""]);

        assert_eq!(common_prefix(&found).as_deref(), Some(""));
    }
}

mod render {
    use crate::repl::render::*;

    #[test]
    fn a_short_line_paints_the_prompt_and_the_text() {
        let (bytes, frame) = paint("hi", 2, 80, Frame::default());

        assert!(bytes.contains("anth> hi"), "{bytes:?}");
        assert_eq!(frame.rows, 1);
    }

    /// The cursor is moved back to its offset, which is the only way a raw
    /// terminal shows an insertion point inside the line.
    #[test]
    fn the_cursor_is_moved_to_its_offset() {
        let (bytes, frame) = paint("abc", 1, 80, Frame::default());

        // Six columns of prompt plus one character is column seven.
        assert_eq!(frame.cursor_column, 7);
        assert!(bytes.ends_with("\r\x1b[7C"), "{bytes:?}");
    }

    #[test]
    fn a_cursor_at_column_zero_emits_no_forward_move() {
        let (bytes, frame) = paint("", 0, 80, Frame::default());

        // Column zero of the line is the prompt's width once it is on screen,
        // so the cursor still lands after `anth> `. What must not happen is a
        // second move past it.
        assert_eq!(frame.cursor_column, PROMPT.chars().count());
        assert!(
            bytes.ends_with(&format!("\r\x1b[{}C", PROMPT.chars().count())),
            "{bytes:?}"
        );
    }

    /// A line longer than the terminal wraps. The frame has to report every row
    /// it used, or the next repaint starts one row too low and the prompt walks
    /// down the screen.
    #[test]
    fn a_wrapped_line_reports_every_row_it_used() {
        // Six columns of prompt plus twenty characters is twenty-six, which is
        // three rows of ten.
        let (_, frame) = paint(&"x".repeat(20), 26, 10, Frame::default());

        assert_eq!(frame.rows, 3);
    }

    #[test]
    fn a_repaint_climbs_to_the_first_row_of_the_previous_frame() {
        let previous = Frame {
            rows: 3,
            cursor_row: 2,
            cursor_column: 4,
        };

        let (bytes, _) = paint("hi", 2, 80, previous);

        assert!(bytes.starts_with("\x1b[2A\r\x1b[J"), "{bytes:?}");
    }

    #[test]
    fn the_first_paint_does_not_climb() {
        let (bytes, _) = paint("hi", 2, 80, Frame::default());

        assert!(bytes.starts_with('\r'), "{bytes:?}");
    }

    /// Cursor arithmetic counts characters, so an accented line does not push
    /// the cursor one column too far.
    #[test]
    fn a_multi_byte_line_places_the_cursor_by_character() {
        let (_, frame) = paint("añ", 4, 80, Frame::default());

        assert_eq!(frame.cursor_column, 8);
    }

    #[test]
    fn a_zero_width_still_paints() {
        let (bytes, _) = paint("hi", 2, 0, Frame::default());

        assert!(bytes.contains("anth> hi"), "{bytes:?}");
    }

    /// A cursor past the end of the text cannot desynchronise the frame, which
    /// would happen if the byte offset were used without clamping.
    #[test]
    fn a_stale_cursor_past_the_text_is_clamped() {
        let (_, frame) = paint("hi", 99, 80, Frame::default());

        assert_eq!(frame.cursor_column, 8);
    }

    /// An exported `COLUMNS` is trusted, and anything unusable falls back to 80
    /// rather than to a zero-width division.
    #[test]
    fn the_width_prefers_columns_and_falls_back() {
        // SAFETY: the variable is only read by this module, and `set_var` is
        // unsafe in edition 2024 because another thread could read it at the
        // same time. These assertions run in one test, in sequence.
        unsafe {
            std::env::set_var("COLUMNS", "132");
        }
        assert_eq!(terminal_width(), 132);

        unsafe {
            std::env::set_var("COLUMNS", "nonsense");
        }
        assert_eq!(terminal_width(), 80);

        unsafe {
            std::env::remove_var("COLUMNS");
        }
        assert_eq!(terminal_width(), 80);
    }
}

mod history {
    use crate::repl::editor::History;
    use crate::repl::history::*;
    use std::path::{Path, PathBuf};

    /// A file inside a directory this fixture owns, both removed on drop.
    ///
    /// The owned directory matters: cleanup must never reach beyond it. An
    /// earlier version kept the file directly in `temp_dir()`, so its parent
    /// was the temp root itself and the drop deleted every other process's
    /// files under `/tmp`.
    struct TempFile {
        root: PathBuf,
        file: PathBuf,
    }

    impl TempFile {
        fn new(label: &str) -> Self {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            let root = std::env::temp_dir().join(format!(
                "anth-history-{label}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&root).expect("creating the fixture directory");
            Self {
                file: root.join("history"),
                root,
            }
        }

        fn path(&self) -> &Path {
            &self.file
        }
    }

    impl Drop for TempFile {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn history(lines: &[&str]) -> History {
        let mut history = History::new();
        for line in lines {
            history.push(line);
        }
        history
    }

    #[test]
    fn a_missing_file_is_an_empty_history() {
        let file = TempFile::new("missing");

        assert!(load(file.path()).entries().is_empty());
    }

    /// The round trip is the whole feature: what you typed yesterday is what Up
    /// finds today.
    #[test]
    fn the_round_trip_preserves_the_order() {
        let file = TempFile::new("round-trip");
        let original = history(&["first", "second", "third"]);

        save(file.path(), &original).expect("writing");
        let loaded = load(file.path());

        assert_eq!(loaded.entries(), ["third", "second", "first"]);
    }

    /// On disk the oldest line comes first, so `tail` shows the newest — the
    /// convention every other history file follows.
    #[test]
    fn the_file_is_written_oldest_first() {
        let file = TempFile::new("oldest-first");
        save(file.path(), &history(&["first", "second"])).expect("writing");

        let text = std::fs::read_to_string(file.path()).expect("reading");

        assert_eq!(text, "first\nsecond\n");
    }

    #[test]
    fn blank_lines_are_not_loaded_as_commands() {
        let file = TempFile::new("blank");
        std::fs::write(file.path(), "one\n\n   \ntwo\n").expect("writing a fixture");

        let loaded = load(file.path());

        assert_eq!(loaded.entries(), ["two", "one"]);
    }

    /// A history written by an older version with no trailing newline must still
    /// load, which is why the reader uses `lines()` and not `split_inclusive`.
    #[test]
    fn a_file_without_a_final_newline_loads() {
        let file = TempFile::new("no-newline");
        std::fs::write(file.path(), "one\ntwo").expect("writing a fixture");

        assert_eq!(load(file.path()).entries(), ["two", "one"]);
    }

    /// The parent directory is created on demand: a fresh account has no
    /// `~/.local/state/anth-panel`.
    #[test]
    fn saving_creates_the_parent_directory() {
        let root = TempFile::new("mkdir");
        let nested = root.path().join("a").join("b").join("history");

        save(&nested, &history(&["one"])).expect("writing into a missing directory");
        assert_eq!(load(&nested).entries(), ["one"]);
    }

    /// Replacing the file rather than truncating it means a failed write cannot
    /// leave a half-history behind.
    #[test]
    fn saving_replaces_a_previous_file() {
        let file = TempFile::new("replace");
        save(file.path(), &history(&["one", "two"])).expect("first write");

        save(file.path(), &history(&["three"])).expect("second write");

        assert_eq!(load(file.path()).entries(), ["three"]);
    }

    /// Nothing is left behind when the rename succeeds.
    #[test]
    fn no_temporary_file_survives_a_successful_save() {
        let file = TempFile::new("no-temp");
        save(file.path(), &history(&["one"])).expect("writing");

        let temporary = file
            .path()
            .with_extension(format!("tmp{}", std::process::id()));
        assert!(!temporary.exists());
    }

    /// `ANTH_HISTORY` wins, which is what lets a test or a user redirect it.
    #[test]
    fn the_path_honours_the_explicit_override() {
        // SAFETY: the variable is read only by `path`, and this test is the only
        // one that sets it.
        unsafe {
            std::env::set_var("ANTH_HISTORY", "/tmp/explicit-history");
        }

        let resolved = path();

        unsafe {
            std::env::remove_var("ANTH_HISTORY");
        }
        assert_eq!(resolved, Some(PathBuf::from("/tmp/explicit-history")));
    }

    /// With no override, the fallback follows XDG rather than dumping state in
    /// the home directory.
    #[test]
    fn the_fallback_is_under_the_state_directory() {
        // SAFETY: these variables are read only by `path`.
        unsafe {
            std::env::remove_var("ANTH_HISTORY");
            std::env::set_var("XDG_STATE_HOME", "/tmp/state-home");
        }

        let resolved = path();

        unsafe {
            std::env::remove_var("XDG_STATE_HOME");
        }
        assert_eq!(
            resolved,
            Some(PathBuf::from("/tmp/state-home/anth-panel/history"))
        );
    }
}
