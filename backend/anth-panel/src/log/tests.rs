//! The formats, the filter and the escaping.
//!
//! Every test reads the line the logger produced, from a sink it owns. Nothing
//! here touches a real terminal, a global subscriber or the environment.

use super::*;

fn logger(level: Level, format: Format) -> (Logger, Capture) {
    let sink = Capture::default();
    (Logger::new(level, format, Arc::new(sink.clone())), sink)
}

#[test]
fn a_human_line_carries_stamp_level_target_and_message() {
    let (log, sink) = logger(Level::Info, Format::Human);

    log.info("daemon", "listening on /run/anth.sock").emit();

    let line = sink.one();
    assert!(
        line.ends_with("INFO   daemon: listening on /run/anth.sock"),
        "got {line:?}"
    );
    // The stamp is checked by shape, not by value: a test that pinned the clock
    // would fail tomorrow.
    assert_eq!(&line[4..5], "-", "the year comes first, got {line:?}");
    assert_eq!(
        &line[10..11],
        "T",
        "the date ends at index 10, got {line:?}"
    );
    assert!(line.contains("Z  INFO"), "got {line:?}");
}

#[test]
fn fields_follow_the_message_as_key_value() {
    let (log, sink) = logger(Level::Info, Format::Human);

    log.info("request", "plugin call")
        .field("plugin", "org.anth.hello")
        .field("status", 200u16)
        .field("ms", 3u64)
        .emit();

    let line = sink.one();
    assert!(
        line.ends_with("request: plugin call plugin=org.anth.hello status=200 ms=3"),
        "got {line:?}"
    );
}

#[test]
fn a_json_line_is_one_object_with_the_expected_keys() {
    let (log, sink) = logger(Level::Info, Format::Json);

    log.info("request", "plugin call")
        .field("plugin", "org.anth.hello")
        .field("status", 200u16)
        .field("ok", true)
        .emit();

    let line = sink.one();
    let parsed: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");

    assert_eq!(parsed["level"], "info");
    assert_eq!(parsed["target"], "request");
    assert_eq!(parsed["msg"], "plugin call");
    assert_eq!(parsed["plugin"], "org.anth.hello");
    // A number, not a string: a consumer comparing `status > 100` needs it.
    assert_eq!(parsed["status"], 200);
    assert!(parsed["status"].is_number(), "got {line:?}");
    assert_eq!(parsed["ok"], true);
    assert!(parsed["ts"].is_string(), "got {line:?}");
}

/// The bug this module was written for: a plugin message containing a newline
/// used to produce two lines, the second of which looked like the host's own.
#[test]
fn a_message_with_a_newline_stays_one_line_in_both_shapes() {
    let hostile = "benign\nanth-panel: listening on /run/anth.sock";

    let (human, human_sink) = logger(Level::Info, Format::Human);
    human
        .info("plugin", hostile)
        .field("plugin", "org.anth.hello")
        .emit();
    let line = human_sink.one();
    assert!(
        !line.contains('\n'),
        "the newline must be escaped: {line:?}"
    );
    assert!(line.contains("\\nanth-panel: listening"), "got {line:?}");

    let (json, json_sink) = logger(Level::Info, Format::Json);
    json.info("plugin", hostile)
        .field("plugin", "org.anth.hello")
        .emit();
    let line = json_sink.one();
    assert!(
        !line.contains('\n'),
        "the newline must be escaped: {line:?}"
    );
    let parsed: serde_json::Value = serde_json::from_str(&line).expect("still valid JSON");
    assert_eq!(parsed["msg"], hostile, "the message survives intact");
}

/// An escape character would move the cursor on a terminal the prompt is painting.
#[test]
fn control_characters_cannot_move_the_cursor() {
    let (log, sink) = logger(Level::Info, Format::Human);

    log.info("plugin", "clear\x1b[2Jhere")
        .field("x", "a\rb")
        .emit();

    let line = sink.one();
    assert!(!line.contains('\x1b'), "no raw escape: {line:?}");
    assert!(!line.contains('\r'), "no raw carriage return: {line:?}");
    assert!(line.contains("\\x1b[2Jhere"), "got {line:?}");
    assert!(line.contains("x=a\\rb"), "got {line:?}");
}

/// A field name comes from the call site, not from a guest, but the encoder must
/// not special-case it: a key with a quote would break the object otherwise.
#[test]
fn a_quoted_field_name_is_still_valid_json() {
    let (log, sink) = logger(Level::Info, Format::Json);

    log.info("t", "m").field("we\"ird", "v").emit();

    let line = sink.one();
    let parsed: serde_json::Value = serde_json::from_str(&line).expect("valid JSON");
    assert_eq!(parsed["we\"ird"], "v");
}

#[test]
fn the_level_filter_is_inclusive_and_ordered() {
    let (log, sink) = logger(Level::Warn, Format::Human);

    log.error("t", "kept error").emit();
    log.warn("t", "kept warn").emit();
    log.info("t", "dropped info").emit();
    log.debug("t", "dropped debug").emit();

    let lines = sink.lines();
    assert_eq!(lines.len(), 2, "got {lines:?}");
    assert!(lines[0].contains("ERROR"), "got {lines:?}");
    assert!(lines[1].contains("WARN"), "got {lines:?}");
}

#[test]
fn every_level_reports_whether_it_is_enabled() {
    let (log, _sink) = logger(Level::Debug, Format::Human);

    assert!(log.enabled(Level::Error));
    assert!(log.enabled(Level::Warn));
    assert!(log.enabled(Level::Info));
    assert!(log.enabled(Level::Debug));
    assert!(!log.enabled(Level::Trace));
}

/// `off` has to silence even an error, or a supervisor that already logs gets
/// duplicates it cannot turn off.
#[test]
fn the_off_format_prints_nothing_at_any_level() {
    let (log, sink) = logger(Level::Trace, Format::Off);

    for level in Level::ALL {
        log.event(level, "t", "m").field("k", "v").emit();
    }

    assert!(sink.lines().is_empty(), "got {:?}", sink.lines());
}

#[test]
fn a_silent_logger_is_off() {
    let log = Logger::silent();

    log.error("t", "m").emit();

    assert_eq!(log.format(), Format::Off);
    assert!(!log.enabled(Level::Error));
}

/// Without this, a caller that forgot `emit` would look like a logger that
/// dropped its line.
#[test]
fn building_without_emitting_writes_nothing() {
    let (log, sink) = logger(Level::Info, Format::Human);

    drop(log.info("t", "never emitted"));

    assert!(sink.lines().is_empty());
}

#[test]
fn a_conditional_field_is_absent_rather_than_null() {
    let (log, sink) = logger(Level::Info, Format::Json);

    log.info("t", "m")
        .field_if(false, "plugin", "org.anth.hello")
        .field_if(true, "status", 200u16)
        .emit();

    let parsed: serde_json::Value = serde_json::from_str(&sink.one()).expect("valid JSON");
    assert_eq!(parsed["status"], 200);
    assert!(
        parsed.get("plugin").is_none(),
        "an absent field must not appear at all: {parsed}"
    );
}

#[test]
fn level_names_round_trip() {
    for level in Level::ALL {
        assert_eq!(Level::parse(level.as_str()), Some(level));
        assert_eq!(Level::parse(&level.as_str().to_uppercase()), Some(level));
    }
    assert_eq!(Level::parse("verbose"), None);
}

#[test]
fn format_names_accept_the_spellings_an_operator_will_try() {
    assert_eq!(Format::parse("human"), Some(Format::Human));
    assert_eq!(Format::parse("JSON"), Some(Format::Json));
    assert_eq!(Format::parse("off"), Some(Format::Off));
    assert_eq!(Format::parse("none"), Some(Format::Off));
    assert_eq!(Format::parse("yaml"), None);
}

#[test]
fn the_timestamp_is_rfc3339_for_a_known_instant() {
    // 2026-10-23T18:42:11.038Z
    assert_eq!(timestamp(1_792_780_931, 38), "2026-10-23T18:42:11.038Z");
    // The epoch itself, and a leap day, because a hand-written calendar is
    // exactly where an off-by-one hides.
    assert_eq!(timestamp(0, 0), "1970-01-01T00:00:00.000Z");
    assert_eq!(timestamp(1_709_164_800, 0), "2024-02-29T00:00:00.000Z");
}

/// A field value that is a string must not be read as a number, and vice versa:
/// `Field` exists to keep the two apart.
#[test]
fn a_string_field_stays_a_json_string() {
    let (log, sink) = logger(Level::Info, Format::Json);

    log.info("t", "m")
        .field("status", "200")
        .field("code", 200u16)
        .emit();

    let parsed: serde_json::Value = serde_json::from_str(&sink.one()).expect("valid JSON");
    assert!(parsed["status"].is_string());
    assert!(parsed["code"].is_number());
}

/// Dates before the epoch, the 2000 leap day and the 2100 non-leap year, because a
/// hand-written calendar is exactly where an off-by-one hides.
/// Cross-checked against Python's calendar, so this is not the code checking itself.
#[test]
fn the_timestamp_matches_a_reference_calendar_across_dates() {
    assert_eq!(timestamp(3476829624, 0), "2080-03-05T02:00:24.000Z");
    assert_eq!(timestamp(-1561096521, 0), "1920-07-13T18:04:39.000Z");
    assert_eq!(timestamp(586753488, 0), "1988-08-05T03:04:48.000Z");
    assert_eq!(timestamp(2397089971, 0), "2045-12-17T02:19:31.000Z");
    assert_eq!(timestamp(92606891, 0), "1972-12-07T20:08:11.000Z");
    assert_eq!(timestamp(3656600440, 0), "2085-11-14T18:20:40.000Z");
    assert_eq!(timestamp(-29568907, 0), "1969-01-23T18:24:53.000Z");
    assert_eq!(timestamp(-2047946152, 0), "1905-02-07T22:04:08.000Z");
    assert_eq!(timestamp(951782400, 0), "2000-02-29T00:00:00.000Z");
    assert_eq!(timestamp(951868800, 0), "2000-03-01T00:00:00.000Z");
    assert_eq!(timestamp(4102444800, 0), "2100-01-01T00:00:00.000Z");
    assert_eq!(timestamp(1700000000, 0), "2023-11-14T22:13:20.000Z");
}

/// A stand-in environment, so the precedence is tested without the machine's own.
fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let pairs: Vec<(String, String)> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |name| {
        pairs
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    }
}

#[test]
fn the_defaults_are_human_info_and_stdout() {
    let (settings, problem) = Settings::resolve(None, None, None, &env_of(&[]));

    assert_eq!(problem, None);
    assert_eq!(settings.level, Level::Info);
    assert_eq!(settings.format, Format::Human);
    assert_eq!(settings.out, Out::Stdout);
}

#[test]
fn a_flag_wins_over_the_environment() {
    let env = env_of(&[
        ("ANTH_LOG_FORMAT", "human"),
        ("ANTH_LOG_LEVEL", "error"),
        ("ANTH_LOG_OUT", "stderr"),
    ]);

    let (settings, problem) = Settings::resolve(Some("json"), Some("debug"), Some("stdout"), &env);

    assert_eq!(problem, None);
    assert_eq!(settings.format, Format::Json);
    assert_eq!(settings.level, Level::Debug);
    assert_eq!(settings.out, Out::Stdout);
}

#[test]
fn the_environment_is_read_when_no_flag_is_given() {
    let env = env_of(&[
        ("ANTH_LOG_FORMAT", "off"),
        ("ANTH_LOG_LEVEL", "trace"),
        ("ANTH_LOG_OUT", "stderr"),
    ]);

    let (settings, problem) = Settings::resolve(None, None, None, &env);

    assert_eq!(problem, None);
    assert_eq!(settings.format, Format::Off);
    assert_eq!(settings.level, Level::Trace);
    assert_eq!(settings.out, Out::Stderr);
}

/// An empty variable is what an unset one looks like through a shell assignment,
/// and must not be rejected as a typo.
#[test]
fn an_empty_environment_value_falls_back_to_the_default() {
    let env = env_of(&[("ANTH_LOG_FORMAT", ""), ("ANTH_LOG_LEVEL", "   ")]);

    let (settings, problem) = Settings::resolve(None, None, None, &env);

    assert_eq!(problem, None);
    assert_eq!(settings.format, Format::Human);
    assert_eq!(settings.level, Level::Info);
}

/// The failure this guards against is a silent fallback: the operator would see
/// `human` output and never learn the flag was ignored.
#[test]
fn a_mistyped_flag_is_reported_and_keeps_the_default() {
    let (settings, problem) = Settings::resolve(Some("jsom"), None, None, &env_of(&[]));

    assert_eq!(settings.format, Format::Human, "the default survives");
    let problem = problem.expect("a typo must be reported");
    assert!(problem.contains("jsom"), "got {problem:?}");
    assert!(problem.contains("human, json or off"), "got {problem:?}");
}

#[test]
fn a_mistyped_level_and_stream_are_reported_too() {
    let (_, level_problem) = Settings::resolve(None, Some("verbose"), None, &env_of(&[]));
    let (_, out_problem) = Settings::resolve(None, None, Some("tty"), &env_of(&[]));

    assert!(
        level_problem
            .expect("reported")
            .contains("error, warn, info"),
        "the message names what is accepted"
    );
    assert!(
        out_problem.expect("reported").contains("stdout or stderr"),
        "the message names what is accepted"
    );
}
