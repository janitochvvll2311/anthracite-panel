//! The line editor behind the interactive prompt.
//!
//! Everything here is a pure function over a [`Line`] and a key, so the
//! behaviour that matters — where the cursor lands, which history entry comes
//! next, what Tab inserts — is tested without a terminal. The bytes that carry a
//! key in are [`keys`](super::keys)'s problem, and the escape sequences that
//! repaint the line are [`render`](super::render)'s.
//!
//! Cursor positions are byte offsets into `text` that always sit on a character
//! boundary. A multi-byte character is one editing unit: Backspace removes a
//! whole `ñ`, not half of one.

/// What a key asks the editor to do.
///
/// This is a decoded intent, not a byte sequence: two terminals may send
/// `ESC [ A` and `ESC O A` for Up, and the editor should never know that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// A character to insert at the cursor.
    Char(char),
    Enter,
    /// Ctrl-C: abandon the line.
    Interrupt,
    /// Ctrl-D: end of input when the line is empty, delete-under-cursor otherwise.
    EndOfInput,
    Backspace,
    Delete,
    Left,
    Right,
    /// Ctrl-A / Home.
    Home,
    /// Ctrl-E / End.
    End,
    Up,
    Down,
    /// Ctrl-K: drop everything from the cursor to the end.
    KillToEnd,
    /// Ctrl-U: drop everything before the cursor.
    KillToStart,
    /// Ctrl-W: drop the word before the cursor.
    KillWord,
    /// Ctrl-L: ask for a full repaint.
    ClearScreen,
    /// Ctrl-R: search the history for the text typed so far.
    SearchHistory,
    Tab,
    /// Anything decoded but not bound, so the editor can ignore it rather than
    /// inserting the escape sequence's bytes into the line.
    Ignored,
}

/// The mutable state of one line being edited.
#[derive(Debug, Default)]
pub struct Line {
    text: String,
    cursor: usize,
}

impl Line {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adopts an existing line, with the cursor at its end.
    #[cfg(test)]
    pub fn with_text(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.len();
        Self { text, cursor }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Moves the cursor, which a scenario setting one up inside a line needs.
    #[cfg(test)]
    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor.min(self.text.len());
    }

    pub fn replace(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
    }

    pub fn clear(&mut self) {
        self.replace("");
    }

    /// The byte offset of the character boundary before `from`.
    fn previous_boundary(&self, from: usize) -> usize {
        self.text[..from]
            .char_indices()
            .next_back()
            .map(|(index, _)| index)
            .unwrap_or(0)
    }

    /// The byte offset one character after `from`.
    fn next_boundary(&self, from: usize) -> usize {
        self.text[from..]
            .chars()
            .next()
            .map(|c| from + c.len_utf8())
            .unwrap_or(self.text.len())
    }

    pub fn insert(&mut self, character: char) {
        self.text.insert(self.cursor, character);
        self.cursor += character.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let start = self.previous_boundary(self.cursor);
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    pub fn delete(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        let end = self.next_boundary(self.cursor);
        self.text.replace_range(self.cursor..end, "");
    }

    pub fn left(&mut self) {
        self.cursor = self.previous_boundary(self.cursor);
    }

    pub fn right(&mut self) {
        self.cursor = self.next_boundary(self.cursor);
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.text.len();
    }

    pub fn kill_to_end(&mut self) {
        self.text.truncate(self.cursor);
    }

    pub fn kill_to_start(&mut self) {
        self.text.replace_range(..self.cursor, "");
        self.cursor = 0;
    }

    /// Deletes the word before the cursor, skipping trailing whitespace first.
    ///
    /// The whitespace skip is what makes Ctrl-W behave like a shell's: after
    /// `plugin call |` one press removes `call `, not the space it left behind.
    pub fn kill_word(&mut self) {
        let mut start = self.cursor;
        while start > 0 {
            let previous = self.previous_boundary(start);
            if !self.text[previous..start].chars().all(char::is_whitespace) {
                break;
            }
            start = previous;
        }
        // The word itself, and the whitespace that separated it from the word
        // before: `plugin call |` loses `call ` so the line ends `plugin `.
        while start > 0 {
            let previous = self.previous_boundary(start);
            if self.text[previous..start].chars().all(char::is_whitespace) {
                start = previous;
                break;
            }
            start = previous;
        }
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
    }

    /// The byte offset where the word under the cursor starts.
    pub fn word_start(&self) -> usize {
        self.text[..self.cursor]
            .char_indices()
            .rev()
            .find(|(_, c)| c.is_whitespace())
            .map(|(index, c)| index + c.len_utf8())
            .unwrap_or(0)
    }

    /// The word the cursor is inside, used to decide what Tab completes.
    ///
    /// The word starts after the last space before the cursor and ends at the
    /// next space after it, so a cursor parked in the middle of `list` names
    /// `list` and not the `li` before it.
    pub fn word(&self) -> &str {
        &self.text[self.word_start()..self.word_end()]
    }

    /// The part of the word before the cursor, which is what a completion
    /// matches against: `plugin li|st` still completes from `li`.
    pub fn prefix(&self) -> &str {
        &self.text[self.word_start()..self.cursor]
    }

    /// Where the word under the cursor ends: the next whitespace, or the end of
    /// the line.
    fn word_end(&self) -> usize {
        self.text[self.cursor..]
            .char_indices()
            .find(|(_, c)| c.is_whitespace())
            .map(|(index, _)| self.cursor + index)
            .unwrap_or(self.text.len())
    }

    /// Replaces the word under the cursor, keeping whatever follows it.
    ///
    /// A completion inserts a space after itself, the way a shell does, so the
    /// next Tab starts a fresh word instead of extending the one just completed.
    pub fn replace_word(&mut self, replacement: &str) {
        let start = self.word_start();
        let end = self.word_end();
        let suffix = self.text[end..].to_string();
        self.text.truncate(start);
        self.text.push_str(replacement);
        self.cursor = self.text.len();
        if suffix.is_empty() {
            self.text.push(' ');
            self.cursor = self.text.len();
        } else {
            self.text.push_str(&suffix);
        }
    }

    /// The words before the cursor, for deciding which grammar a completion is in.
    pub fn words(&self) -> Vec<&str> {
        self.text[..self.cursor].split_whitespace().collect()
    }
}

/// The command history, newest first, plus the position a browse session holds.
#[derive(Debug, Default)]
pub struct History {
    /// Newest first, which is the order Up walks.
    entries: Vec<String>,
    /// The entry shown right now, or `None` when editing a fresh line.
    position: Option<usize>,
    /// What the user had typed before starting to browse, restored on Down past
    /// the newest entry — a shell would otherwise silently lose that line.
    draft: String,
}

impl History {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    /// Records a line, newest first.
    ///
    /// A blank line and a repeat of the newest entry are both skipped: Up should
    /// move, and pressing Enter twice must not need two presses to see the line
    /// before it.
    pub fn push(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        if self.entries.first().map(String::as_str) == Some(line) {
            self.position = None;
            return;
        }
        self.entries.insert(0, line.to_string());
        self.position = None;
    }

    /// The line to show after Up, given the current draft.
    pub fn older(&mut self, current: &str) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        let next = match self.position {
            None => {
                self.draft = current.to_string();
                0
            }
            Some(position) => (position + 1).min(self.entries.len() - 1),
        };
        self.position = Some(next);
        self.entries.get(next).map(String::as_str)
    }

    /// The line to show after Down, restoring the draft past the newest entry.
    pub fn newer(&mut self) -> Option<&str> {
        let position = self.position?;
        if position == 0 {
            self.position = None;
            return Some(&self.draft);
        }
        let next = position - 1;
        self.position = Some(next);
        self.entries.get(next).map(String::as_str)
    }

    /// Stops browsing, so the next Up starts from the newest entry.
    pub fn reset(&mut self) {
        self.position = None;
    }

    /// The newest entry containing `needle`, newest first, for Ctrl-R.
    pub fn search(&self, needle: &str) -> Option<&str> {
        let needle = needle.trim();
        if needle.is_empty() {
            return None;
        }
        self.entries
            .iter()
            .find(|entry| entry.contains(needle))
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
