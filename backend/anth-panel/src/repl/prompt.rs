//! The prompt state machine: keys in, effects out.
//!
//! This is the seam that makes the whole prompt testable. [`Prompt::feed`] takes
//! the bytes stdin delivered and returns what should happen — repaint, submit,
//! offer completions — without touching a file descriptor. [`run`](super::run)
//! is the only part that reads and writes a terminal, so a test can drive a whole
//! editing session from a byte slice and assert on the submitted line.

use super::completion::{self, Candidate};
use super::editor::{History, Key, Line};
use super::keys::Decoder;

/// What the prompt wants done next.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    /// Re-draw the line. Carries the bytes so the caller writes once.
    Repaint(String),
    /// Submit this line to the dispatcher.
    Submit(String),
    /// Ctrl-C: throw the line away and start a new one.
    Cancel,
    /// Ctrl-D on an empty line, or end of input.
    Exit,
    /// Several candidates matched: draw them under the line.
    Offer {
        candidates: Vec<Candidate>,
        bytes: String,
    },
}

/// The interactive prompt's state.
pub struct Prompt {
    line: Line,
    history: History,
    decoder: Decoder,
    frame: super::render::Frame,
    /// True when the last Tab matched several candidates and nothing has been
    /// typed since: a second Tab then takes the common prefix or lists them.
    after_tab: bool,
}

impl Prompt {
    pub fn new(history: History) -> Self {
        Self {
            line: Line::new(),
            history,
            decoder: Decoder::new(),
            frame: super::render::Frame::default(),
            after_tab: false,
        }
    }

    #[cfg(test)]
    pub fn line(&self) -> &Line {
        &self.line
    }

    pub fn history_mut(&mut self) -> &mut History {
        &mut self.history
    }

    /// Hands the history back when the session ends, so the caller can save it.
    pub fn into_history(self) -> History {
        self.history
    }

    /// Paints the line from scratch, with no previous frame to climb over.
    ///
    /// The session calls this after a submitted command and after a cancel,
    /// when the screen state is unknown because a command wrote to it.
    pub fn paint_line(&mut self) -> String {
        self.frame = super::render::Frame::default();
        self.repaint_bytes()
    }

    /// Feeds raw bytes and returns everything that should happen.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Effect> {
        let keys = self.decoder.feed(bytes);
        // A partial sequence — an arrow split across two reads — yields no key
        // here and is finished by the next `feed`.
        keys.into_iter().flat_map(|key| self.key(key)).collect()
    }

    /// Called when stdin closes: an unfinished escape is dropped, and a partial
    /// line is still submitted so a piped last line without a newline is not
    /// silently lost.
    pub fn finish(&mut self) -> Vec<Effect> {
        let mut effects: Vec<Effect> = self
            .decoder
            .flush()
            .into_iter()
            .flat_map(|key| self.key(key))
            .collect();
        if !self.line.is_empty() {
            effects.push(Effect::Submit(self.line.text().to_string()));
        } else {
            effects.push(Effect::Exit);
        }
        effects
    }

    fn key(&mut self, key: Key) -> Vec<Effect> {
        // Any key other than Tab ends the "just tabbed" state, so a second Tab
        // is only special when nothing was typed in between.
        if key != Key::Tab {
            self.after_tab = false;
        }

        match key {
            Key::Char(character) => {
                self.line.insert(character);
                self.history.reset();
                vec![self.repaint()]
            }
            Key::Enter => {
                // The line is trimmed before it is submitted: a completion
                // leaves a trailing space so the next Tab starts a new word,
                // and that space is not part of the command. A blank line
                // submits nothing at all, because a stray keypress must not
                // re-run the previous command or reach the parser as an error.
                let text = self.line.text().trim().to_string();
                self.line.clear();
                self.frame = super::render::Frame::default();
                if text.is_empty() {
                    return vec![self.repaint()];
                }
                self.history.push(&text);
                vec![Effect::Submit(text)]
            }
            Key::Interrupt => {
                // The terminal keeps `ISIG`, so Ctrl-C normally never reaches
                // here; when it does, the line is abandoned and the session goes
                // on rather than dying with a half-typed command.
                self.line.clear();
                self.history.reset();
                self.frame = super::render::Frame::default();
                vec![Effect::Cancel]
            }
            Key::EndOfInput => {
                if self.line.is_empty() {
                    vec![Effect::Exit]
                } else {
                    self.line.delete();
                    vec![self.repaint()]
                }
            }
            Key::Backspace => {
                self.line.backspace();
                self.history.reset();
                vec![self.repaint()]
            }
            Key::Delete => {
                self.line.delete();
                self.history.reset();
                vec![self.repaint()]
            }
            Key::Left => {
                self.line.left();
                vec![self.repaint()]
            }
            Key::Right => {
                self.line.right();
                vec![self.repaint()]
            }
            Key::Home => {
                self.line.home();
                vec![self.repaint()]
            }
            Key::End => {
                self.line.end();
                vec![self.repaint()]
            }
            Key::Up => {
                if let Some(entry) = self.history.older(self.line.text()).map(str::to_string) {
                    self.line.replace(entry);
                }
                vec![self.repaint()]
            }
            Key::Down => {
                if let Some(entry) = self.history.newer().map(str::to_string) {
                    self.line.replace(entry);
                }
                vec![self.repaint()]
            }
            Key::KillToEnd => {
                self.line.kill_to_end();
                self.history.reset();
                vec![self.repaint()]
            }
            Key::KillToStart => {
                self.line.kill_to_start();
                self.history.reset();
                vec![self.repaint()]
            }
            Key::KillWord => {
                self.line.kill_word();
                self.history.reset();
                vec![self.repaint()]
            }
            Key::ClearScreen => {
                // A cleared screen has no previous frame to climb over.
                self.frame = super::render::Frame::default();
                vec![Effect::Repaint(format!(
                    "{}{}",
                    super::render::clear_screen(),
                    self.repaint_bytes()
                ))]
            }
            Key::SearchHistory => self.search_history(),
            Key::Tab => self.complete(),
            Key::Ignored => vec![],
        }
    }

    /// Ctrl-R inserts the newest history entry containing what is typed.
    ///
    /// A full incremental search would need its own prompt and its own tests;
    /// this is the part that earns its keep — recover a command you half
    /// remember — without pretending to be readline.
    fn search_history(&mut self) -> Vec<Effect> {
        let needle = self.line.word().to_string();
        match self.history.search(&needle) {
            Some(entry) => {
                let entry = entry.to_string();
                self.line.replace(entry);
                vec![self.repaint()]
            }
            None => vec![self.repaint()],
        }
    }

    /// Tab: finish the word when one candidate matches, otherwise show them.
    fn complete(&mut self) -> Vec<Effect> {
        // The word under the cursor is the last word before it, which is empty
        // after a space — that is what makes `plugin <Tab>` list the nested
        // subcommands rather than complete `plugin` again.
        let prefix = self.line.prefix().to_string();
        let mut words = self.line.words();
        if words.last() != Some(&prefix.as_str()) {
            words.push(&prefix);
        }
        let found = completion::candidates(&words);
        if found.is_empty() {
            return vec![self.repaint()];
        }

        if found.len() == 1 {
            let replacement = found[0].text.clone();
            self.line.replace_word(&replacement);
            self.history.reset();
            return vec![self.repaint()];
        }

        // Several matches: the first Tab lists them, and a second Tab with no
        // typing in between inserts the prefix they share, which is what a
        // shell user expects instead of the same list twice.
        if self.after_tab
            && let Some(shared) = common_prefix(&found)
            && shared.chars().count() > prefix.chars().count()
        {
            self.line.replace_word(&shared);
        }
        self.after_tab = true;

        // Listing moved the cursor below the line, so the frame restarts and the
        // line is painted fresh underneath the list.
        self.frame = super::render::Frame::default();
        let mut bytes = String::from(super::render::erase_line());
        bytes.push_str(&super::render::completions(
            &found,
            super::render::terminal_width(),
        ));
        bytes.push_str(&self.repaint_bytes());

        vec![Effect::Offer {
            candidates: found,
            bytes,
        }]
    }

    fn repaint(&mut self) -> Effect {
        Effect::Repaint(self.repaint_bytes())
    }

    fn repaint_bytes(&mut self) -> String {
        let width = super::render::terminal_width();
        let (bytes, frame) =
            super::render::paint(self.line.text(), self.line.cursor(), width, self.frame);
        self.frame = frame;
        bytes
    }
}

/// The longest prefix every candidate shares.
fn common_prefix(found: &[Candidate]) -> Option<String> {
    let first = found.first()?.text.clone();
    let mut shared = first.chars().collect::<Vec<char>>();
    for candidate in &found[1..] {
        let other: Vec<char> = candidate.text.chars().collect();
        let keep = shared
            .iter()
            .zip(other.iter())
            .take_while(|(a, b)| a == b)
            .count();
        shared.truncate(keep);
        if shared.is_empty() {
            break;
        }
    }
    Some(shared.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(names, vec!["call", "list"]);
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
