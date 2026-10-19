use anth_panel::cli::TerminalArgs;
use anth_panel::terminal_client::resolve_size_from;

#[test]
fn explicit_flags_win_over_detection() {
    let args = TerminalArgs {
        cols: Some(120),
        rows: Some(40),
    };
    let size = resolve_size_from(&args, Some((80, 24))).unwrap();
    assert_eq!((size.cols, size.rows), (120, 40));
}

#[test]
fn detection_is_used_when_no_flag_is_given() {
    let args = TerminalArgs::default();
    let size = resolve_size_from(&args, Some((132, 43))).unwrap();
    assert_eq!((size.cols, size.rows), (132, 43));
}

#[test]
fn a_missing_detected_dimension_falls_back_to_the_default() {
    let args = TerminalArgs::default();
    let size = resolve_size_from(&args, None).unwrap();
    assert_eq!((size.cols, size.rows), (80, 24));
}

#[test]
fn one_flag_fills_the_other_from_detection() {
    let args = TerminalArgs {
        cols: Some(100),
        rows: None,
    };
    let size = resolve_size_from(&args, Some((80, 24))).unwrap();
    assert_eq!((size.cols, size.rows), (100, 24));

    let args = TerminalArgs {
        cols: None,
        rows: Some(50),
    };
    let size = resolve_size_from(&args, Some((80, 24))).unwrap();
    assert_eq!((size.cols, size.rows), (80, 50));
}

#[test]
fn an_absurd_size_is_rejected() {
    let args = TerminalArgs {
        cols: Some(0),
        rows: Some(24),
    };
    assert!(resolve_size_from(&args, None).is_err());
}
