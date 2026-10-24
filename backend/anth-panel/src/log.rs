//! One line per event, in one of two shapes, or nothing.
//!
//! Three decisions shape this module.
//!
//! **The logger is passed, never global.** `tracing` would put a subscriber in a
//! process-wide slot, and `with_default` only scopes it to one thread, so a
//! `tokio::spawn`ed server task would log somewhere a test cannot see. Here the
//! `Arc<Logger>` travels inside whatever owns it, so a test hands over a sink of
//! its own and asserts on the bytes.
//!
//! **A sink receives text, not a formatted line.** The REPL has a prompt painted
//! on the screen; it cannot let a logger write behind its back. It installs a
//! sink that queues, and repaints around each line. The logger has no opinion
//! about terminal state.
//!
//! **A guest's message is data, not output.** Every field is escaped, so a plugin
//! that logs `"\n"` cannot forge a line of the host's own log. In the `json` shape
//! the encoder does that on its own; in `human` [`escape`] does.

use std::io::Write;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// How much of an event's importance survives the filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
}

impl Level {
    pub const ALL: [Level; 5] = [
        Level::Error,
        Level::Warn,
        Level::Info,
        Level::Debug,
        Level::Trace,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
            Level::Debug => "debug",
            Level::Trace => "trace",
        }
    }

    /// The upper-case form `human` prints, so the column width is fixed.
    fn label(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Level::ALL
            .into_iter()
            .find(|level| level.as_str().eq_ignore_ascii_case(text.trim()))
    }
}

/// What a line looks like, including the choice of no line at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Human,
    Json,
    Off,
}

impl Format {
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "human" | "text" => Some(Format::Human),
            "json" => Some(Format::Json),
            "off" | "none" | "silent" => Some(Format::Off),
            _ => None,
        }
    }
}

/// Where a stream's lines go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Out {
    Stdout,
    Stderr,
}

impl Out {
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "stdout" | "out" => Some(Out::Stdout),
            "stderr" | "err" => Some(Out::Stderr),
            _ => None,
        }
    }
}

/// How the operator asked for logging, as the flags and environment gave it.
///
/// Separate from [`Logger`] so the decision is testable without a stream: the
/// fallback order and the rejection of a typo live in [`Settings::resolve`],
/// a pure function of four `Option<String>`s.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub level: Level,
    pub format: Format,
    pub out: Out,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            level: Level::Info,
            format: Format::Human,
            out: Out::Stdout,
        }
    }
}

impl Settings {
    /// Resolves the three settings from the command line and an environment.
    ///
    /// `env` is passed rather than read so a test can supply its own: a suite
    /// that read the real environment would pass on one machine and fail on the
    /// next. [`Settings::from_flags`] is the thin caller that reads it.
    ///
    /// The flag wins over the variable, and the variable over the default. A
    /// rejected value is reported instead of silently falling back: an operator
    /// who typed `--log-format jsom` must not spend an hour reading `human`
    /// output and wondering why nothing changed.
    pub fn resolve(
        format: Option<&str>,
        level: Option<&str>,
        out: Option<&str>,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> (Self, Option<String>) {
        let mut settings = Settings::default();
        let mut problem = None;
        let mut complain = |message: String| {
            if problem.is_none() {
                problem = Some(message);
            }
        };

        let chosen = |flag: Option<&str>, name: &str| -> Option<String> {
            match flag {
                Some(text) => Some(text.to_string()),
                None => env(name).filter(|text| !text.trim().is_empty()),
            }
        };

        if let Some(text) = chosen(format, "ANTH_LOG_FORMAT") {
            match Format::parse(&text) {
                Some(parsed) => settings.format = parsed,
                None => complain(format!(
                    "`{text}` is not a log format: use human, json or off"
                )),
            }
        }

        if let Some(text) = chosen(level, "ANTH_LOG_LEVEL") {
            match Level::parse(&text) {
                Some(parsed) => settings.level = parsed,
                None => complain(format!(
                    "`{text}` is not a log level: use error, warn, info, debug or trace"
                )),
            }
        }

        if let Some(text) = chosen(out, "ANTH_LOG_OUT") {
            match Out::parse(&text) {
                Some(parsed) => settings.out = parsed,
                None => complain(format!(
                    "`{text}` is not a log stream: use stdout or stderr"
                )),
            }
        }

        (settings, problem)
    }

    /// [`Settings::resolve`] over this process's environment.
    pub fn from_flags(
        format: Option<&str>,
        level: Option<&str>,
        out: Option<&str>,
    ) -> (Self, Option<String>) {
        Self::resolve(format, level, out, &|name| std::env::var(name).ok())
    }

    /// These settings with a sink of the caller's choosing.
    ///
    /// Used by the REPL, which replaces the stream with a queue so it can paint
    /// around its own prompt. `off` still wins: a silent logger stays silent
    /// whatever sink it is given.
    pub fn logger_via(&self, sink: Arc<dyn Sink>) -> Logger {
        let sink: Arc<dyn Sink> = match self.format {
            Format::Off => Arc::new(Nowhere),
            _ => sink,
        };
        Logger {
            level: self.level,
            format: self.format,
            out: self.out,
            sink,
        }
    }

    /// The logger these settings describe, writing to this process's stream.
    pub fn logger(&self) -> Logger {
        let sink: Arc<dyn Sink> = match self.format {
            Format::Off => Arc::new(Nowhere),
            _ => Arc::new(Stream(self.out)),
        };
        Logger {
            level: self.level,
            format: self.format,
            out: self.out,
            sink,
        }
    }
}

/// A value attached to an event.
///
/// Typed rather than stringly, because `json` has to print `3` and not `"3"`:
/// a consumer filtering on `ms > 100` would otherwise compare strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field<'a> {
    Str(&'a str),
    U64(u64),
    I64(i64),
    Bool(bool),
}

impl<'a> Field<'a> {
    fn human(self) -> String {
        match self {
            Field::Str(text) => escape(text),
            Field::U64(number) => number.to_string(),
            Field::I64(number) => number.to_string(),
            Field::Bool(flag) => flag.to_string(),
        }
    }

    fn json(self) -> String {
        match self {
            Field::Str(text) => encode_string(text),
            Field::U64(number) => number.to_string(),
            Field::I64(number) => number.to_string(),
            Field::Bool(flag) => flag.to_string(),
        }
    }
}

/// Where finished lines go. Text arrives without a trailing newline.
pub trait Sink: Send + Sync {
    fn line(&self, text: &str);
}

/// The process's own stream, locked per line so two tasks cannot interleave.
pub struct Stream(pub Out);

impl Sink for Stream {
    fn line(&self, text: &str) {
        match self.0 {
            Out::Stdout => {
                let mut out = std::io::stdout().lock();
                let _ = writeln!(out, "{text}");
            }
            Out::Stderr => {
                let mut out = std::io::stderr().lock();
                let _ = writeln!(out, "{text}");
            }
        }
    }
}

/// Queues lines for whoever owns the terminal.
///
/// The REPL needs this: a prompt is painted on the screen, so a logger writing
/// straight to the stream would land on top of it. The sink hands the finished
/// line to the session, and the session decides where on the screen it goes.
pub struct Queue(tokio::sync::mpsc::UnboundedSender<String>);

impl Queue {
    /// A sink and the receiver its lines arrive on.
    pub fn new() -> (Self, tokio::sync::mpsc::UnboundedReceiver<String>) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        (Self(tx), rx)
    }
}

impl Sink for Queue {
    fn line(&self, text: &str) {
        // A closed receiver means the session ended; dropping the line is right,
        // because there is nowhere left to show it.
        let _ = self.0.send(text.to_string());
    }
}

/// Drops every line. What `--log-format off` installs, so a call site never has
/// to ask whether logging is on.
pub struct Nowhere;

impl Sink for Nowhere {
    fn line(&self, _text: &str) {}
}

#[derive(Clone)]
pub struct Logger {
    level: Level,
    format: Format,
    out: Out,
    sink: Arc<dyn Sink>,
}

impl std::fmt::Debug for Logger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Logger")
            .field("level", &self.level)
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

impl Logger {
    /// A logger with an explicit sink, which in production is always [`Stream`]
    /// or [`Nowhere`]: this exists for the tests that read lines back.
    #[cfg(test)]
    pub fn new(level: Level, format: Format, sink: Arc<dyn Sink>) -> Self {
        Self {
            level,
            format,
            out: Out::Stdout,
            sink,
        }
    }

    /// The settings that produced this logger, so a caller can build a second one
    /// with a different sink and the same level, format and stream.
    pub fn settings(&self) -> Settings {
        Settings {
            level: self.level,
            format: self.format,
            out: self.out,
        }
    }

    /// A logger that prints nothing.
    ///
    /// What a test gets when it does not care, so an assertion failure is not
    /// buried under log lines — and what the load-time ABI check uses, because it
    /// runs before any operator output exists.
    pub fn silent() -> Self {
        Self {
            level: Level::Error,
            format: Format::Off,
            out: Out::Stdout,
            sink: Arc::new(Nowhere),
        }
    }

    #[cfg(test)]
    pub fn format(&self) -> Format {
        self.format
    }

    /// Whether an event at `level` would be printed.
    ///
    /// Exposed so a call site can skip building an expensive field — formatting a
    /// list of plugins, say — rather than paying for a line nobody reads.
    pub fn enabled(&self, level: Level) -> bool {
        self.format != Format::Off && level <= self.level
    }

    pub fn event<'a>(&'a self, level: Level, target: &'a str, message: &'a str) -> Event<'a> {
        Event {
            logger: self,
            level,
            target,
            message,
            fields: Vec::new(),
        }
    }

    /// Only the tests call this: production never logs below `warn` yet, so the
    /// method would otherwise be dead code in the binary.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn error<'a>(&'a self, target: &'a str, message: &'a str) -> Event<'a> {
        self.event(Level::Error, target, message)
    }

    pub fn warn<'a>(&'a self, target: &'a str, message: &'a str) -> Event<'a> {
        self.event(Level::Warn, target, message)
    }

    pub fn info<'a>(&'a self, target: &'a str, message: &'a str) -> Event<'a> {
        self.event(Level::Info, target, message)
    }

    pub fn debug<'a>(&'a self, target: &'a str, message: &'a str) -> Event<'a> {
        self.event(Level::Debug, target, message)
    }

    /// The whole line for `event`, or `None` when the filter drops it.
    ///
    /// Split out from [`Event::emit`] so a test asserts on a returned string
    /// instead of parsing a stream.
    fn render(
        &self,
        level: Level,
        target: &str,
        message: &str,
        fields: &[(&str, Field<'_>)],
    ) -> Option<String> {
        if !self.enabled(level) {
            return None;
        }
        Some(match self.format {
            Format::Off => return None,
            Format::Human => human_line(level, target, message, fields, now()),
            Format::Json => json_line(level, target, message, fields, now()),
        })
    }
}

/// A log event under construction.
///
/// `#[must_use]` because a builder that is dropped without [`Event::emit`] would
/// silently log nothing, which is the failure mode hardest to notice.
#[must_use = "a log event does nothing until `emit` is called"]
pub struct Event<'a> {
    logger: &'a Logger,
    level: Level,
    target: &'a str,
    message: &'a str,
    fields: Vec<(&'a str, Field<'a>)>,
}

impl<'a> Event<'a> {
    pub fn field(mut self, key: &'a str, value: impl Into<Field<'a>>) -> Self {
        self.fields.push((key, value.into()));
        self
    }

    /// Attaches a field only when `condition` holds.
    ///
    /// Keeps an optional fact out of the line entirely rather than as `null`: in
    /// `human` a `plugin=null` reads as a real plugin named "null".
    pub fn field_if(mut self, condition: bool, key: &'a str, value: impl Into<Field<'a>>) -> Self {
        if condition {
            self.fields.push((key, value.into()));
        }
        self
    }

    pub fn emit(self) {
        if let Some(line) = self
            .logger
            .render(self.level, self.target, self.message, &self.fields)
        {
            self.logger.sink.line(&line);
        }
    }
}

impl<'a> From<&'a str> for Field<'a> {
    fn from(text: &'a str) -> Self {
        Field::Str(text)
    }
}

impl<'a> From<&'a String> for Field<'a> {
    fn from(text: &'a String) -> Self {
        Field::Str(text.as_str())
    }
}

impl From<u64> for Field<'_> {
    fn from(number: u64) -> Self {
        Field::U64(number)
    }
}

impl From<u32> for Field<'_> {
    fn from(number: u32) -> Self {
        Field::U64(u64::from(number))
    }
}

impl From<usize> for Field<'_> {
    fn from(number: usize) -> Self {
        Field::U64(number as u64)
    }
}

impl From<i64> for Field<'_> {
    fn from(number: i64) -> Self {
        Field::I64(number)
    }
}

impl From<u16> for Field<'_> {
    fn from(number: u16) -> Self {
        Field::U64(u64::from(number))
    }
}

impl From<u128> for Field<'_> {
    fn from(number: u128) -> Self {
        Field::U64(number as u64)
    }
}

impl From<bool> for Field<'_> {
    fn from(flag: bool) -> Self {
        Field::Bool(flag)
    }
}

/// `2026-10-23T18:42:11.038Z`, from the clock.
fn now() -> String {
    let since = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    timestamp(since.as_secs() as i64, since.subsec_millis())
}

/// RFC 3339 in UTC, always with milliseconds and `Z`.
///
/// Hand-written because the alternative is `chrono`/`time` for one call: the
/// civil-date conversion below is Hinnant's, which is exact for every year this
/// clock can produce.
pub fn timestamp(seconds: i64, millis: u32) -> String {
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = rest / 3_600;
    let minute = (rest % 3_600) / 60;
    let second = rest % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Days since the Unix epoch to a civil date, proleptic Gregorian.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    } / 146_097;
    let day_of_era = (shifted - era * 146_097) as u64;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

/// One line: `ts  LEVEL  target: message key=value`.
fn human_line(
    level: Level,
    target: &str,
    message: &str,
    fields: &[(&str, Field<'_>)],
    stamp: String,
) -> String {
    let mut line = format!(
        "{stamp}  {:<5}  {}: {}",
        level.label(),
        escape(target),
        escape(message)
    );
    for (key, value) in fields {
        line.push(' ');
        line.push_str(&escape(key));
        line.push('=');
        line.push_str(&value.human());
    }
    line
}

/// One line, one JSON object.
///
/// Keys come out sorted, which is `serde_json`'s default map: a log that diffs
/// cleanly is worth more than a fixed field order.
fn json_line(
    level: Level,
    target: &str,
    message: &str,
    fields: &[(&str, Field<'_>)],
    stamp: String,
) -> String {
    let mut object = String::from("{");
    object.push_str(&format!("\"level\":{}", encode_string(level.as_str())));
    object.push_str(&format!(",\"msg\":{}", encode_string(message)));
    for (key, value) in fields {
        object.push(',');
        object.push_str(&encode_string(key));
        object.push(':');
        object.push_str(&value.json());
    }
    object.push_str(&format!(",\"target\":{}", encode_string(target)));
    object.push_str(&format!(",\"ts\":{}", encode_string(&stamp)));
    object.push('}');
    object
}

/// A JSON string literal, with every control character escaped.
///
/// `\n` becomes `\\n`, so a guest's message can never end the line early — which
/// is the whole of the log-injection defence in this shape.
fn encode_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 || ch as u32 == 0x7f => {
                out.push_str(&format!("\\u{:04x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Text safe to put on one line of a shared terminal.
///
/// Control characters are the danger: a raw `\x1b` would move the cursor, and a
/// raw `\n` would let a guest forge a line that looks like the host's.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 || ch as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out
}

/// A sink that keeps what it was given, for tests that assert on bytes.
#[cfg(test)]
#[derive(Clone)]
pub struct Capture(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

#[cfg(test)]
impl Capture {
    pub fn lines(&self) -> Vec<String> {
        self.0.lock().expect("the capture lock").clone()
    }

    pub fn one(&self) -> String {
        let lines = self.lines();
        assert_eq!(lines.len(), 1, "expected exactly one line, got {lines:?}");
        lines[0].clone()
    }
}

#[cfg(test)]
impl Default for Capture {
    fn default() -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(Vec::new())))
    }
}

#[cfg(test)]
impl Sink for Capture {
    fn line(&self, text: &str) {
        self.0
            .lock()
            .expect("the capture lock")
            .push(text.to_string());
    }
}

#[cfg(test)]
mod tests;
