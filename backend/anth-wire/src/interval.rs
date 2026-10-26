//! The interval a watch repeats on, written the same way everywhere.
//!
//! `--watch 500ms` on the command line and `?interval=500ms` on the SSE route
//! are one setting with two spellings, so the parser and its bounds live in the
//! contract crate rather than in either caller. Two copies would be two places
//! for the minimum, the maximum and the accepted suffixes to drift, and a client
//! that asked over HTTP for something the CLI would refuse is a bug nobody
//! notices until the two disagree.

use std::time::Duration;

/// Below this a watch is a busy loop wearing a flag's name.
pub const MIN_INTERVAL: Duration = Duration::from_millis(10);

/// Long enough for a snapshot nobody is looking at, short enough that a typo
/// (`2d`) is a refusal rather than a silent no-op.
pub const MAX_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// What a watch repeats on when the caller names no interval.
///
/// A label rather than a `Duration` because clap takes a default as text, and
/// because the HTTP route documents the same default in the same words.
pub const DEFAULT_INTERVAL: &str = "2s";

/// Parses an interval such as `500ms`, `2s`, `1.5s`, `1m` or `1h`.
///
/// The unit is required. A bare `2` could mean seconds to one reader and
/// milliseconds to the next, and a wrong guess here is a panel that updates a
/// thousand times too often rather than an error message.
///
/// Fractional values are accepted only where they round to at least
/// [`MIN_INTERVAL`], so `0.5ms` is refused rather than silently becoming zero.
pub fn parse_interval(raw: &str) -> Result<Duration, String> {
    let raw = raw.trim();
    let (digits, unit_millis) = if let Some(rest) = raw.strip_suffix("ms") {
        (rest, 1.0)
    } else if let Some(rest) = raw.strip_suffix('s') {
        (rest, 1_000.0)
    } else if let Some(rest) = raw.strip_suffix('m') {
        (rest, 60_000.0)
    } else if let Some(rest) = raw.strip_suffix('h') {
        (rest, 3_600_000.0)
    } else {
        return Err(format!(
            "invalid interval `{raw}`: add a unit suffix (ms, s, m or h), e.g. 500ms, 2s, 1m"
        ));
    };

    let value: f64 = digits.trim().parse().map_err(|_| {
        format!(
            "invalid interval `{raw}`: `{}` is not a number",
            digits.trim()
        )
    })?;
    if !value.is_finite() || value <= 0.0 {
        return Err(format!(
            "invalid interval `{raw}`: must be greater than zero"
        ));
    }

    let millis = value * unit_millis;
    if millis < MIN_INTERVAL.as_millis() as f64 {
        return Err(format!(
            "interval too small: `{raw}` is below the {}ms minimum",
            MIN_INTERVAL.as_millis()
        ));
    }
    if millis > MAX_INTERVAL.as_millis() as f64 {
        return Err(format!(
            "interval too large: `{raw}` exceeds the {}-hour maximum",
            MAX_INTERVAL.as_secs() / 3600
        ));
    }

    Ok(Duration::from_millis(millis.round() as u64))
}
