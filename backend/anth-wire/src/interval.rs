//! One interval parser shared by the CLI and the HTTP SSE route.
//!
//! `--watch 500ms` and `?interval=500ms` are one setting in two spellings. The
//! parser lives here so the minimum, maximum and accepted suffixes cannot drift
//! between the two transports.

use std::time::Duration;

/// Below this a watch is a busy loop wearing a flag's name.
pub const MIN_INTERVAL: Duration = Duration::from_millis(10);

/// Long enough for a snapshot nobody is looking at, short enough that a typo is
/// a refusal rather than a silent no-op.
pub const MAX_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// What a watch repeats on when the caller names no interval.
pub const DEFAULT_INTERVAL: &str = "2s";

/// Parses an interval such as `500ms`, `2s`, `1.5s`, `1m` or `1h`.
///
/// The unit is required: a bare `2` could mean seconds or milliseconds, and a
/// wrong guess is a panel that updates a thousand times too often.
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
