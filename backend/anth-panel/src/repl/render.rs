//! Repainting the prompt after a key.
//!
//! A raw terminal has no line discipline, so every keystroke has to be answered
//! with escape sequences that put the line and its cursor back where they
//! belong. That is easy to get subtly wrong, so the arithmetic lives in
//! [`paint`], a function of numbers that returns a string: a test asserts the
//! exact bytes for a wrapped line without needing a terminal to run in.
//!
//! The cursor is placed by counting *characters*, matching `render`'s table
//! drawing elsewhere in the crate. A wide CJK glyph still occupies two columns
//! and will drift; measuring display width properly needs a table of East Asian
//! widths, which is a dependency this prompt does not earn. ASCII and accented
//! Latin — what a command line actually contains — are exact.

use super::completion::Candidate;

/// The prompt, kept here so its width and its text cannot disagree.
pub const PROMPT: &str = "anth> ";

/// The rows the previous frame used, so a repaint can climb back over them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Frame {
    pub rows: usize,
    /// The row the cursor was left on, counted from the frame's first row.
    pub cursor_row: usize,
    /// The column the cursor was left on.
    pub cursor_column: usize,
}

impl Frame {
    /// How the next frame must begin so it lands on the first row of this one.
    fn climb_back(&self) -> String {
        let mut out = String::new();
        if self.cursor_row > 0 {
            out.push_str(&format!("\x1b[{}A", self.cursor_row));
        }
        out.push('\r');
        out
    }
}

/// Builds the repaint for `text` with the cursor at `cursor` (a byte offset).
///
/// `previous` is the frame the caller painted last. Repainting climbs to its
/// first row and clears from there down, so a line that got shorter leaves no
/// debris behind.
pub fn paint(text: &str, cursor: usize, width: usize, previous: Frame) -> (String, Frame) {
    let width = width.max(1);
    let prompt_width = PROMPT.chars().count();
    let before_cursor = text[..cursor.min(text.len())].chars().count();

    let mut bytes = previous.climb_back();
    bytes.push_str("\x1b[J");
    bytes.push_str(PROMPT);
    bytes.push_str(text);

    // The terminal leaves the cursor wherever the text ended up, which is the
    // position of the last character. With `\r` already emitted above and the
    // text starting after the prompt, absolute columns are what matter.
    let total = prompt_width + text.chars().count();
    let last_row = total / width;

    let cursor_columns = prompt_width + before_cursor;
    let cursor_row = cursor_columns / width;
    let cursor_column = cursor_columns % width;

    // Move up first, then to column 0, then right. Going up from the last row
    // is the only order that works when the cursor wrapped past the text.
    if cursor_row < last_row {
        bytes.push_str(&format!("\x1b[{}A", last_row - cursor_row));
    }
    bytes.push('\r');
    if cursor_column > 0 {
        bytes.push_str(&format!("\x1b[{cursor_column}C"));
    }

    (
        bytes,
        Frame {
            rows: last_row + 1,
            cursor_row,
            cursor_column,
        },
    )
}

/// A full repaint after Ctrl-L, which clears the screen rather than one row.
pub fn clear_screen() -> &'static str {
    "\x1b[H\x1b[2J"
}

/// Wipes the prompt line before a command writes its own output, so the two do
/// not interleave on one row.
pub fn erase_line() -> &'static str {
    "\r\x1b[K"
}

/// Lays out the candidates a Tab found, in columns that fit the terminal.
///
/// Column count is derived from the widest candidate rather than fixed, so the
/// list stays readable at 40 columns and at 200. A candidate is annotated with
/// its kind only when it is long enough to leave room, because the annotation
/// is a hint and the list is the point.
pub fn completions(found: &[Candidate], width: usize) -> String {
    let width = width.max(1);
    let cell = found
        .iter()
        .map(|candidate| candidate.text.chars().count())
        .max()
        .unwrap_or(0)
        .max(1)
        + 2;
    let columns = (width / cell).max(1);

    let mut out = String::new();
    for (index, candidate) in found.iter().enumerate() {
        if index % columns == 0 {
            out.push_str("\r\n");
        }
        let padding = cell - candidate.text.chars().count();
        out.push_str(&candidate.text);
        out.push_str(&" ".repeat(padding));
    }
    out.push('\n');
    out
}

/// The columns the terminal reports for itself, with a sane fallback.
///
/// `COLUMNS` is what a shell exports; probing with `tput` would mean a process
/// per keystroke, and an `ioctl` would mean another `unsafe` block. Every shell
/// that bothers exports it, and 80 is the conservative answer when nothing does.
pub fn terminal_width() -> usize {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width > 0)
        .unwrap_or(80)
}
