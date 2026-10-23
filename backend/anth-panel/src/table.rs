//! A grid drawn with box-drawing characters.
//!
//! The host snapshot and the plugin list are both tables, and v0.1.0 drew each
//! with its own `+`/`-`/`|` arithmetic. Two copies of that arithmetic is two
//! places for a column to drift, so it lives here once: callers supply cells,
//! the border geometry is this module's problem.
//!
//! Widths are counted in characters rather than bytes, matching the rest of the
//! crate, so accented text does not shift a column. A wide CJK glyph still takes
//! two terminal columns and will drift; measuring display width properly needs
//! an East Asian width table, which the panel does not earn as a dependency.
//!
//! The junctions are the point: a corner, a tee and a cross each say something
//! about where the line goes, and `+` at every meeting says nothing.

const TOP_LEFT: char = '┌';
const TOP_MID: char = '┬';
const TOP_RIGHT: char = '┐';
const MID_LEFT: char = '├';
const MID_MID: char = '┼';
const MID_RIGHT: char = '┤';
const BOTTOM_LEFT: char = '└';
const BOTTOM_MID: char = '┴';
const BOTTOM_RIGHT: char = '┘';
const HORIZONTAL: char = '─';
const VERTICAL: char = '│';

/// A table under construction.
///
/// Cells are copied into strings because a caller usually builds them on the
/// fly, and a table that outlived a borrowed row would be a lifetime leak in
/// every signature for no gain.
#[derive(Debug, Clone, Default)]
pub struct Table {
    headers: Option<Vec<String>>,
    rows: Vec<Vec<String>>,
    separate_rows: bool,
}

impl Table {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets a header row, drawn between two rules.
    pub fn with_headers(mut self, headers: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.headers = Some(headers.into_iter().map(Into::into).collect());
        self
    }

    /// Adds many rows at once, for a caller that already holds them.
    pub fn rows<R, C>(mut self, rows: impl IntoIterator<Item = R>) -> Self
    where
        R: IntoIterator<Item = C>,
        C: Into<String>,
    {
        for row in rows {
            self.rows.push(row.into_iter().map(Into::into).collect());
        }
        self
    }

    /// Draws a rule between data rows.
    ///
    /// Right for a label/value table, where each row is a separate fact. Wrong
    /// for a list, where rules between every item are noise.
    pub fn separate_rows(mut self, separate: bool) -> Self {
        self.separate_rows = separate;
        self
    }

    pub fn render(&self) -> String {
        let columns = self.columns();
        if columns == 0 {
            return String::new();
        }
        let widths = self.widths(columns);

        let rule = |left: char, middle: char, right: char| {
            let mut line = String::new();
            line.push(left);
            for (index, width) in widths.iter().enumerate() {
                if index > 0 {
                    line.push(middle);
                }
                // One space of padding on each side of the cell.
                line.extend(std::iter::repeat_n(HORIZONTAL, width + 2));
            }
            line.push(right);
            line
        };

        let mut lines = vec![rule(TOP_LEFT, TOP_MID, TOP_RIGHT)];

        if let Some(headers) = &self.headers {
            lines.push(self.line(headers, &widths));
            lines.push(rule(MID_LEFT, MID_MID, MID_RIGHT));
        }

        for (index, row) in self.rows.iter().enumerate() {
            if index > 0 && self.separate_rows {
                lines.push(rule(MID_LEFT, MID_MID, MID_RIGHT));
            }
            lines.push(self.line(row, &widths));
        }

        lines.push(rule(BOTTOM_LEFT, BOTTOM_MID, BOTTOM_RIGHT));
        lines.join("\n")
    }

    /// The number of columns, taken from the widest row so a short row cannot
    /// silently drop a column from the border.
    fn columns(&self) -> usize {
        let headers = self.headers.as_ref().map_or(0, Vec::len);
        self.rows.iter().map(Vec::len).fold(headers, usize::max)
    }

    fn widths(&self, columns: usize) -> Vec<usize> {
        let mut widths = vec![0; columns];
        for row in self.headers.iter().chain(self.rows.iter()) {
            for (index, cell) in row.iter().enumerate() {
                widths[index] = widths[index].max(width(cell));
            }
        }
        widths
    }

    /// One row of cells, padded to the column widths and closed by the border.
    ///
    /// A row with fewer cells than the table has columns is padded with empty
    /// ones, so the right edge stays a straight line.
    fn line(&self, row: &[String], widths: &[usize]) -> String {
        let mut line = String::new();
        line.push(VERTICAL);
        for (index, column_width) in widths.iter().enumerate() {
            let cell = row.get(index).map(String::as_str).unwrap_or("");
            line.push(' ');
            line.push_str(cell);
            line.extend(std::iter::repeat_n(' ', column_width - width(cell)));
            line.push(' ');
            line.push(VERTICAL);
        }
        line
    }
}

/// Width in characters, not bytes.
pub fn width(text: &str) -> usize {
    text.chars().count()
}
