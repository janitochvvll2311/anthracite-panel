//! One line per event, in a human or JSON shape, or nothing.
//!
//! The logger is passed as an `Arc`, never installed globally: a test can own a
//! buffer and assert on the exact bytes, and the REPL can install a queue sink
//! without fighting a process-wide subscriber.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// How much of an event's importance survives the filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
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

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|level| level.as_str().eq_ignore_ascii_case(text.trim()))
    }
}

/// What a line looks like, including no line at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Out {
    #[default]
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

/// How the operator asked for logging, after flags and environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// The flag wins over the variable, and the variable over the default. A bad
    /// value is reported instead of silently falling back.
    pub fn resolve(
        format: Option<&str>,
        level: Option<&str>,
        out: Option<&str>,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> (Self, Vec<String>) {
        Self::resolve_with(Settings::default(), format, level, out, env)
    }

    /// Resolves against an explicit default, usually `[log]` from the config.
    pub fn resolve_with(
        default: Settings,
        format: Option<&str>,
        level: Option<&str>,
        out: Option<&str>,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> (Self, Vec<String>) {
        let mut settings = default;
        let mut problems = Vec::new();

        let chosen = |flag: Option<&str>, name: &str| -> Option<String> {
            match flag {
                Some(text) => Some(text.to_string()),
                None => env(name).filter(|text| !text.trim().is_empty()),
            }
        };

        if let Some(text) = chosen(format, "ANTH_LOG_FORMAT") {
            match Format::parse(&text) {
                Some(parsed) => settings.format = parsed,
                None => problems.push(format!(
                    "`{text}` is not a log format: use human, json or off"
                )),
            }
        }
        if let Some(text) = chosen(level, "ANTH_LOG_LEVEL") {
            match Level::parse(&text) {
                Some(parsed) => settings.level = parsed,
                None => problems.push(format!(
                    "`{text}` is not a log level: use error, warn, info, debug or trace"
                )),
            }
        }
        if let Some(text) = chosen(out, "ANTH_LOG_OUT") {
            match Out::parse(&text) {
                Some(parsed) => settings.out = parsed,
                None => problems.push(format!(
                    "`{text}` is not a log destination: use stdout or stderr"
                )),
            }
        }

        (settings, problems)
    }

    pub fn from_flags(
        format: Option<&str>,
        level: Option<&str>,
        out: Option<&str>,
    ) -> (Self, Vec<String>) {
        Self::resolve(format, level, out, &|name| std::env::var(name).ok())
    }
}

/// A destination for a fully formatted line.
pub trait Sink: Send + Sync {
    fn emit(&self, line: &str);
}

/// Writes to stdout.
pub struct StdoutSink;

impl Sink for StdoutSink {
    fn emit(&self, line: &str) {
        println!("{line}");
    }
}

/// Writes to stderr.
pub struct StderrSink;

impl Sink for StderrSink {
    fn emit(&self, line: &str) {
        eprintln!("{line}");
    }
}

/// Keeps lines in memory, for tests and the REPL queue.
#[derive(Default)]
pub struct BufferSink {
    lines: Mutex<Vec<String>>,
}

impl BufferSink {
    pub fn lines(&self) -> Vec<String> {
        self.lines.lock().expect("buffer lock").clone()
    }
}

impl Sink for BufferSink {
    fn emit(&self, line: &str) {
        self.lines
            .lock()
            .expect("buffer lock")
            .push(line.to_string());
    }
}

/// The logger handed to a feature or a request.
pub struct Logger {
    settings: Settings,
    sink: Arc<dyn Sink>,
}

impl Logger {
    pub fn new(settings: Settings, sink: Arc<dyn Sink>) -> Self {
        Self { settings, sink }
    }

    pub fn stdout(settings: Settings) -> Self {
        Self::new(settings, Arc::new(StdoutSink))
    }

    pub fn stderr(settings: Settings) -> Self {
        Self::new(settings, Arc::new(StderrSink))
    }

    pub fn buffer(settings: Settings) -> (Self, Arc<BufferSink>) {
        let sink = Arc::new(BufferSink::default());
        (Self::new(settings, sink.clone()), sink)
    }

    /// A logger that emits nothing. Extension load-time probes use it.
    pub fn silent() -> Self {
        Self::stdout(Settings {
            format: Format::Off,
            ..Settings::default()
        })
    }

    pub fn with_out(settings: Settings) -> Self {
        match settings.out {
            Out::Stdout => Self::stdout(settings),
            Out::Stderr => Self::stderr(settings),
        }
    }

    pub fn enabled(&self, level: Level) -> bool {
        self.settings.format != Format::Off && level <= self.settings.level
    }

    pub fn event(&self, level: Level, target: &str, message: &str, fields: &[(&str, &str)]) {
        if !self.enabled(level) {
            return;
        }
        let line = match self.settings.format {
            Format::Off => return,
            Format::Human => human_line(level, target, message, fields),
            Format::Json => json_line(level, target, message, fields),
        };
        self.sink.emit(&line);
    }

    pub fn error(&self, target: &str, message: &str) {
        self.event(Level::Error, target, message, &[]);
    }

    pub fn warn(&self, target: &str, message: &str) {
        self.event(Level::Warn, target, message, &[]);
    }

    pub fn info(&self, target: &str, message: &str) {
        self.event(Level::Info, target, message, &[]);
    }

    pub fn debug(&self, target: &str, message: &str) {
        self.event(Level::Debug, target, message, &[]);
    }

    pub fn trace(&self, target: &str, message: &str) {
        self.event(Level::Trace, target, message, &[]);
    }
}

fn human_line(level: Level, target: &str, message: &str, fields: &[(&str, &str)]) -> String {
    let mut line = format!(
        "{} {:5} {target}: {}",
        timestamp_rfc3339(),
        level.as_str().to_uppercase(),
        escape(message)
    );
    for (key, value) in fields {
        let _ = write!(line, " {key}={}", escape(value));
    }
    line
}

fn json_line(level: Level, target: &str, message: &str, fields: &[(&str, &str)]) -> String {
    let mut object = serde_json::Map::new();
    object.insert(
        "ts".to_string(),
        serde_json::Value::String(timestamp_rfc3339()),
    );
    object.insert(
        "level".to_string(),
        serde_json::Value::String(level.as_str().to_string()),
    );
    object.insert(
        "target".to_string(),
        serde_json::Value::String(target.to_string()),
    );
    object.insert(
        "msg".to_string(),
        serde_json::Value::String(message.to_string()),
    );
    for (key, value) in fields {
        object.insert(
            (*key).to_string(),
            serde_json::Value::String((*value).to_string()),
        );
    }
    serde_json::Value::Object(object).to_string()
}

/// Escapes control characters so a field cannot forge a log line.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character.is_control() => {
                let _ = write!(out, "\\x{:02x}", character as u32);
            }
            character => out.push(character),
        }
    }
    out
}

/// RFC 3339 UTC with milliseconds, from the system clock.
pub fn timestamp_rfc3339() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seconds = duration.as_secs();
    let millis = duration.subsec_millis();
    let (year, month, day, hour, minute, second) = civil_from_unix(seconds);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

/// Converts Unix seconds to a UTC calendar date and time.
///
/// The algorithm is the standard days-from-civil inverse; it avoids pulling in
/// a date-time dependency for one log line.
fn civil_from_unix(seconds: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (seconds / 86_400) as i64;
    let remainder = seconds % 86_400;
    let hour = (remainder / 3_600) as u32;
    let minute = ((remainder % 3_600) / 60) as u32;
    let second = (remainder % 60) as u32;

    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let year = if month <= 2 { year + 1 } else { year };

    (year, month, day, hour, minute, second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_beat_environment_and_bad_values_are_reported() {
        let env = |name: &str| match name {
            "ANTH_LOG_LEVEL" => Some("debug".to_string()),
            "ANTH_LOG_FORMAT" => Some("json".to_string()),
            _ => None,
        };
        let (settings, problems) = Settings::resolve(Some("human"), None, None, &env);
        assert_eq!(settings.format, Format::Human);
        assert_eq!(settings.level, Level::Debug);
        assert!(problems.is_empty());

        let (_, problems) = Settings::resolve(Some("jsom"), Some("loud"), Some("nowhere"), &env);
        assert_eq!(problems.len(), 3);
    }

    #[test]
    fn human_lines_escape_control_characters() {
        let line = human_line(Level::Warn, "auth", "bad\nline", &[("code", "x\ty")]);
        assert!(line.contains("bad\\nline"));
        assert!(line.contains("code=x\\ty"));
        assert!(!line.contains('\n'));
    }

    #[test]
    fn json_lines_keep_fields_typed_as_strings_and_escape_newlines() {
        let line = json_line(Level::Info, "request", "answered", &[("op", "host-info")]);
        let value: serde_json::Value = serde_json::from_str(&line).expect("json");
        assert_eq!(value["level"], "info");
        assert_eq!(value["target"], "request");
        assert!(value["fields"].is_null());
        assert_eq!(value["op"], "host-info");
    }

    #[test]
    fn off_silences_everything() {
        let (logger, sink) = Logger::buffer(Settings {
            format: Format::Off,
            ..Settings::default()
        });
        logger.info("test", "hidden");
        assert!(sink.lines().is_empty());
    }

    #[test]
    fn the_level_filter_drops_lower_priority_lines() {
        let (logger, sink) = Logger::buffer(Settings {
            level: Level::Warn,
            ..Settings::default()
        });
        logger.info("test", "hidden");
        logger.warn("test", "shown");
        let lines = sink.lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("shown"));
    }
}
