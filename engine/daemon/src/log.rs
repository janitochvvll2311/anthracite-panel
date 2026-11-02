//! A small logger that honours `[log]` without pulling a framework.
//!
//! Two formats, because the operator reads text and a collector reads JSON.

use std::time::SystemTime;

use crate::config::{LogFormat, LogLevel, LogSection};

pub struct Logger {
    level: LogLevel,
    format: LogFormat,
}

impl Logger {
    pub fn new(section: LogSection) -> Self {
        Self {
            level: section.level,
            format: section.format,
        }
    }

    pub fn info(&self, message: &str) {
        self.log(LogLevel::Info, message);
    }

    pub fn warn(&self, message: &str) {
        self.log(LogLevel::Warn, message);
    }

    fn log(&self, level: LogLevel, message: &str) {
        if rank(level) > rank(self.level) {
            return;
        }
        let at = humantime::format_rfc3339_seconds(SystemTime::now()).to_string();
        match self.format {
            LogFormat::Text => eprintln!("{at} {:<5} {message}", name(level)),
            LogFormat::Json => eprintln!(
                "{{\"at\":\"{at}\",\"level\":\"{}\",\"message\":{}}}",
                name(level).to_ascii_lowercase(),
                serde_json::to_string(message).unwrap_or_else(|_| "\"\"".to_string())
            ),
        }
    }
}

fn rank(level: LogLevel) -> u8 {
    match level {
        LogLevel::Error => 0,
        LogLevel::Warn => 1,
        LogLevel::Info => 2,
        LogLevel::Debug => 3,
    }
}

fn name(level: LogLevel) -> &'static str {
    match level {
        LogLevel::Error => "ERROR",
        LogLevel::Warn => "WARN",
        LogLevel::Info => "INFO",
        LogLevel::Debug => "DEBUG",
    }
}
