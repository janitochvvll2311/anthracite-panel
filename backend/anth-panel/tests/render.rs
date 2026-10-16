use anth_panel::render::{format_kib, format_uptime, render_host_info};
use anth_proto::{HostInfo, LoadAverage, MemoryInfo, OsInfo};

fn sample() -> HostInfo {
    HostInfo {
        hostname: "node-1".to_string(),
        os: OsInfo {
            name: "Debian GNU/Linux".to_string(),
            version: "13".to_string(),
            kernel: "6.12.0-amd64".to_string(),
            arch: "x86_64".to_string(),
        },
        uptime_seconds: 90_061,
        cpu_model: "Intel Xeon".to_string(),
        logical_cores: 4,
        load: LoadAverage {
            one: 0.5,
            five: 0.25,
            fifteen: 0.1,
        },
        memory: MemoryInfo {
            total_kib: 8_388_608,
            available_kib: 4_194_304,
            swap_total_kib: 1_048_576,
            swap_free_kib: 524_288,
        },
    }
}

#[test]
fn render_draws_full_box_borders() {
    let out = render_host_info(&sample());

    let first = out.lines().next().unwrap();
    let last = out.lines().last().unwrap();

    assert!(first.starts_with('┌'), "missing top-left corner:\n{out}");
    assert!(first.ends_with('┐'), "missing top-right corner:\n{out}");
    assert!(last.starts_with('└'), "missing bottom-left corner:\n{out}");
    assert!(last.ends_with('┘'), "missing bottom-right corner:\n{out}");
    assert!(
        out.contains('┬') && out.contains('┴'),
        "missing joints:\n{out}"
    );
    assert!(
        out.contains('├') && out.contains('┤'),
        "missing tees:\n{out}"
    );

    // Corners belong to the outer border only, never to an inner rule.
    assert_eq!(out.matches('┐').count(), 1, "duplicated corner:\n{out}");
    assert_eq!(out.matches('┘').count(), 1, "duplicated corner:\n{out}");
}

#[test]
fn every_rendered_line_has_the_same_width() {
    let out = render_host_info(&sample());
    let widths: Vec<usize> = out.lines().map(|line| line.chars().count()).collect();

    assert!(widths.len() > 5, "expected a multi-line table:\n{out}");
    assert!(
        widths.iter().all(|w| *w == widths[0]),
        "ragged table, widths were {widths:?}:\n{out}"
    );
}

#[test]
fn every_line_starts_and_ends_with_a_vertical_border() {
    let out = render_host_info(&sample());

    for line in out.lines() {
        assert!(
            line.starts_with('│')
                || line.starts_with('┌')
                || line.starts_with('├')
                || line.starts_with('└'),
            "line does not open with a border: {line}"
        );
        assert!(
            line.ends_with('│')
                || line.ends_with('┐')
                || line.ends_with('┤')
                || line.ends_with('┘'),
            "line does not close with a border: {line}"
        );
    }
}

#[test]
fn renders_the_expected_fields() {
    let out = render_host_info(&sample());

    for expected in [
        "FIELD",
        "VALUE",
        "Hostname",
        "node-1",
        "Operating system",
        "Debian GNU/Linux 13",
        "Kernel",
        "6.12.0-amd64",
        "Architecture",
        "x86_64",
        "CPU",
        "Intel Xeon",
        "Logical cores",
        "4",
        "Uptime",
        "1d 1h 1m 1s",
        "Load average",
        "0.50 / 0.25 / 0.10",
        "Memory total",
        "8.0 GiB",
        "Swap total",
    ] {
        assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
    }
}

#[test]
fn reports_memory_percentages() {
    let out = render_host_info(&sample());

    assert!(
        out.contains("4.0 GiB (50%)"),
        "missing used percentage:\n{out}"
    );
}

#[test]
fn omits_swap_lines_when_there_is_no_swap() {
    let mut info = sample();
    info.memory.swap_total_kib = 0;
    info.memory.swap_free_kib = 0;

    let out = render_host_info(&info);

    assert!(
        out.contains("Swap") && out.contains("not configured"),
        "got:\n{out}"
    );
    assert!(!out.contains("Swap used"), "got:\n{out}");
}

#[test]
fn omits_os_version_when_it_is_unknown() {
    let mut info = sample();
    info.os.version = "unknown".to_string();

    let out = render_host_info(&info);

    assert!(out.contains("Debian GNU/Linux unknown"), "got:\n{out}");
}

#[test]
fn survives_an_empty_snapshot_without_breaking_the_box() {
    let out = render_host_info(&HostInfo::default());
    let widths: Vec<usize> = out.lines().map(|line| line.chars().count()).collect();

    assert!(widths.iter().all(|w| *w == widths[0]), "ragged:\n{out}");
    assert!(out.contains("not configured"), "got:\n{out}");
}

#[test]
fn aligns_rows_containing_multi_byte_text() {
    let mut info = sample();
    info.hostname = "servidor-ñ-🚀".to_string();

    let out = render_host_info(&info);
    let widths: Vec<usize> = out.lines().map(|line| line.chars().count()).collect();

    assert!(widths.iter().all(|w| *w == widths[0]), "ragged:\n{out}");
}

#[test]
fn format_kib_picks_the_largest_unit() {
    assert_eq!(format_kib(0), "0 B");
    assert_eq!(format_kib(1), "1 KiB");
    assert_eq!(format_kib(2), "2 KiB");
    assert_eq!(format_kib(1024), "1.0 MiB");
    assert_eq!(format_kib(1024 * 1024), "1.0 GiB");
    assert_eq!(format_kib(1024 * 1024 * 1024), "1.0 TiB");
}

#[test]
fn format_uptime_omits_zero_components() {
    assert_eq!(format_uptime(0), "0s");
    assert_eq!(format_uptime(59), "59s");
    assert_eq!(format_uptime(60), "1m");
    assert_eq!(format_uptime(3661), "1h 1m 1s");
    assert_eq!(format_uptime(86_400), "1d");
    assert_eq!(format_uptime(90_061), "1d 1h 1m 1s");
}
