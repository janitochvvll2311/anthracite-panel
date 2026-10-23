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
