//! Turns the bytes a terminal sends into [`Key`]s.
//!
//! Two things make this more than a `match` on a byte. An arrow key is an
//! *escape sequence* — `ESC [ A` — and a read may deliver it in pieces, so the
//! decoder keeps an incomplete sequence until the rest arrives instead of
//! misreading `ESC` as a key of its own. And input is UTF-8, so a multi-byte
//! character can be split across reads for the same reason.
//!
//! `ESC [ A` and `ESC O A` both mean Up: the first is what a terminal sends in
//! normal cursor mode and the second in application mode, which full-screen
//! programs switch on. The editor should not see the difference.

use super::editor::Key;

/// Decodes a byte stream into keys, holding partial sequences between reads.
#[derive(Debug, Default)]
pub struct Decoder {
    pending: Vec<u8>,
}

impl Decoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// True when a sequence was started but not finished.
    #[cfg(test)]
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Consumes `bytes`, returning every key that is now complete.
    ///
    /// A trailing partial sequence stays buffered and produces no key. The
    /// caller waits briefly for more bytes and then calls
    /// [`flush`](Self::flush), which is what turns a lone `ESC` press into a
    /// key instead of a hang.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Key> {
        self.pending.extend_from_slice(bytes);
        let mut keys = Vec::new();
        let mut index = 0;

        while index < self.pending.len() {
            match decode_at(&self.pending[index..]) {
                Decoded::Key(key, length) => {
                    keys.push(key);
                    index += length;
                }
                Decoded::NeedMore => break,
                Decoded::Invalid => {
                    // A byte that is neither a control code nor UTF-8: skipping
                    // one keeps the rest of the buffer decodable.
                    index += 1;
                }
            }
        }

        self.pending.drain(..index);
        keys
    }

    /// Gives up on an incomplete sequence, so `ESC` alone is not lost forever.
    ///
    /// An unfinished escape is reported as [`Key::Ignored`]: the user probably
    /// pressed Escape or an unbound Alt chord, and neither should reach the line.
    pub fn flush(&mut self) -> Vec<Key> {
        let mut keys = Vec::new();
        if !self.pending.is_empty() {
            self.pending.clear();
            keys.push(Key::Ignored);
        }
        keys
    }
}

enum Decoded {
    Key(Key, usize),
    /// A valid prefix of something longer.
    NeedMore,
    /// Not decodable here; the caller skips one byte.
    Invalid,
}

/// Decodes one key at the start of `bytes`.
fn decode_at(bytes: &[u8]) -> Decoded {
    let Some(&first) = bytes.first() else {
        return Decoded::NeedMore;
    };

    if first == 0x1b {
        return decode_escape(bytes);
    }

    if let Some(key) = control(first) {
        return Decoded::Key(key, 1);
    }

    decode_char(bytes)
}

/// The single-byte control codes, in raw mode.
///
/// `0x0d` is what Enter sends; terminals that send `0x0a` are handled too
/// because `ICRNL` is off. `0x7f` is Backspace on every terminal that matters,
/// and `0x08` is what some send for Ctrl-H.
fn control(byte: u8) -> Option<Key> {
    Some(match byte {
        0x01 => Key::Home,
        0x02 => Key::Left,
        0x03 => Key::Interrupt,
        0x04 => Key::EndOfInput,
        0x05 => Key::End,
        0x06 => Key::Right,
        0x08 => Key::Backspace,
        0x09 => Key::Tab,
        0x0a | 0x0d => Key::Enter,
        0x0b => Key::KillToEnd,
        0x0c => Key::ClearScreen,
        0x0e => Key::Down,
        0x10 => Key::Up,
        0x12 => Key::SearchHistory,
        0x15 => Key::KillToStart,
        0x17 => Key::KillWord,
        0x7f => Key::Backspace,
        _ => return None,
    })
}

/// Decodes `ESC`, `ESC [ …`, `ESC O x` and Alt chords.
fn decode_escape(bytes: &[u8]) -> Decoded {
    // A lone ESC, or an ESC at the end of a read: wait for the rest.
    let Some(&second) = bytes.get(1) else {
        return Decoded::NeedMore;
    };

    match second {
        b'[' => decode_csi(bytes),
        b'O' => {
            let Some(&third) = bytes.get(2) else {
                return Decoded::NeedMore;
            };
            let key = match third {
                b'A' => Key::Up,
                b'B' => Key::Down,
                b'C' => Key::Right,
                b'D' => Key::Left,
                b'H' => Key::Home,
                b'F' => Key::End,
                _ => Key::Ignored,
            };
            Decoded::Key(key, 3)
        }
        // Alt-<key> and anything else starting with ESC: ignored rather than
        // inserted, so a stray chord cannot type `[A` into the line.
        _ => Decoded::Key(Key::Ignored, 2),
    }
}

/// Decodes `ESC [ …`. Parameters run until a final byte in `0x40..=0x7e`.
fn decode_csi(bytes: &[u8]) -> Decoded {
    let mut index = 2;
    let mut parameters = String::new();
    while index < bytes.len() {
        let byte = bytes[index];
        if (0x40..=0x7e).contains(&byte) {
            let key = match (byte, parameters.as_str()) {
                (b'A', _) => Key::Up,
                (b'B', _) => Key::Down,
                (b'C', _) => Key::Right,
                (b'D', _) => Key::Left,
                (b'H', _) => Key::Home,
                (b'F', _) => Key::End,
                (b'~', "1") | (b'~', "7") => Key::Home,
                (b'~', "4") | (b'~', "8") => Key::End,
                (b'~', "3") => Key::Delete,
                // Insert, PageUp/PageDown, function keys, bracketed paste: all
                // decoded so their bytes do not leak into the line.
                _ => Key::Ignored,
            };
            return Decoded::Key(key, index + 1);
        }
        parameters.push(byte as char);
        index += 1;
    }
    Decoded::NeedMore
}

/// Decodes one UTF-8 character, waiting when the bytes are a partial char.
fn decode_char(bytes: &[u8]) -> Decoded {
    match std::str::from_utf8(bytes) {
        Ok(text) => match text.chars().next() {
            Some(character) => Decoded::Key(Key::Char(character), character.len_utf8()),
            None => Decoded::NeedMore,
        },
        Err(error) => {
            // `error_len() == None` means the buffer ends mid-character, which
            // more bytes will finish. A `Some(_)` is genuinely invalid UTF-8.
            match error.error_len() {
                Some(_) => Decoded::Invalid,
                None => {
                    // Only wait if the head really is a valid prefix.
                    let valid = error.valid_up_to();
                    if valid > 0 {
                        let text = std::str::from_utf8(&bytes[..valid]).unwrap_or("");
                        match text.chars().next() {
                            Some(character) => {
                                Decoded::Key(Key::Char(character), character.len_utf8())
                            }
                            None => Decoded::Invalid,
                        }
                    } else {
                        Decoded::NeedMore
                    }
                }
            }
        }
    }
}
