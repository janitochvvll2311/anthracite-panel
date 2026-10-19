use anth_panel::render::{
    format_kib, format_uptime, render_file_listing, render_host_info, render_network_snapshot,
    render_process_snapshot, render_response,
};
use anth_proto::{
    HostInfo, LoadAverage, MemoryInfo, NetworkAddress, NetworkAddressFamily, NetworkError,
    NetworkErrorKind, NetworkInterface, NetworkSnapshot, OsInfo, ProcessInfo, ProcessSnapshot,
    Response,
};

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
fn format_kib_never_renders_a_value_of_1024_or_more() {
    // A quantity that rounds to 1024 at its own precision belongs to the next
    // unit up, otherwise it prints as e.g. "1024.0 MiB".
    assert_eq!(format_kib(1024 * 1024 - 1), "1.0 GiB");
    assert_eq!(format_kib(1024 * 1024 * 1024 - 1), "1.0 TiB");

    // Sweep both sides of every unit boundary and assert the invariant directly,
    // rather than pinning the exact largest value that stays in the lower unit.
    let mut candidates = vec![0_u64, 1, 1023, 1024, 1024 * 1024, 1024 * 1024 * 1024];
    for base in [1024_u64, 1024 * 1024, 1024 * 1024 * 1024] {
        for delta in 1..64 {
            candidates.push(base - delta);
            candidates.push(base + delta);
        }
    }

    for kib in candidates {
        let rendered = format_kib(kib);
        let value: f64 = rendered
            .split_whitespace()
            .next()
            .unwrap_or_else(|| panic!("format_kib({kib}) rendered nothing"))
            .parse()
            .unwrap_or_else(|_| panic!("format_kib({kib}) rendered {rendered:?}"));

        assert!(
            value < 1024.0,
            "format_kib({kib}) rendered {rendered:?}, whose value is not below 1024"
        );
    }
}

#[test]
fn format_kib_does_not_overflow_on_extreme_input() {
    // saturating_mul keeps the byte count from wrapping into a bogus unit.
    let rendered = format_kib(u64::MAX);
    assert!(rendered.ends_with("TiB"), "got {rendered:?}");
    assert_ne!(rendered, "0 B", "got {rendered:?}");
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

#[test]
fn renders_process_rows_and_a_truncation_warning() {
    let snapshot = ProcessSnapshot {
        processes: vec![ProcessInfo {
            pid: 42,
            ppid: 1,
            uid: 1000,
            user: "alice".to_string(),
            name: "sleep".to_string(),
            command: "sleep 30".to_string(),
            state: "S".to_string(),
            cpu_ticks: 250,
            rss_kib: 2048,
            threads: 1,
            start_ticks: 12345,
        }],
        clock_ticks: 100,
        timestamp_ms: 1_700_000_000_000,
        control_enabled: false,
        total: 2500,
        truncated: true,
        warning: Some("showing 2000 of 2500 processes".to_string()),
    };

    let out = render_process_snapshot(&snapshot);
    for expected in ["PID", "alice", "sleep 30", "2.5s", "2.0 MiB"] {
        assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
    }
    assert!(out.contains("Warning: showing 2000 of 2500 processes"));
}

#[test]
fn render_file_listing_shows_types_sizes_and_warning() {
    let listing = anth_proto::FileListing {
        path: "/".to_string(),
        parent: None,
        entries: vec![
            anth_proto::FileEntry {
                name: "docs".to_string(),
                path: "/docs".to_string(),
                kind: anth_proto::FileKind::Directory,
                size: 0,
                modified_ms: Some(0),
                hidden: false,
                target: None,
            },
            anth_proto::FileEntry {
                name: "note.txt".to_string(),
                path: "/note.txt".to_string(),
                kind: anth_proto::FileKind::File,
                size: 2048,
                modified_ms: Some(0),
                hidden: false,
                target: None,
            },
        ],
        total: 2,
        truncated: false,
        warning: Some("showing 2 of 3 entries".to_string()),
        write_enabled: false,
    };

    let out = render_file_listing(&listing);
    assert!(out.contains("TYPE"));
    assert!(out.contains("docs"));
    assert!(out.contains("dir"));
    assert!(out.contains("note.txt"));
    assert!(out.contains("2.0 KiB"));
    assert!(out.contains("1970-01-01 00:00 UTC"));
    assert!(out.contains("Warning: showing 2 of 3 entries"));
}

#[test]
fn render_network_snapshot_shows_addresses_speed_and_traffic() {
    let snapshot = NetworkSnapshot {
        interfaces: vec![
            NetworkInterface {
                name: "eth0".to_string(),
                index: 2,
                state: "up".to_string(),
                up: true,
                running: true,
                loopback: false,
                mac: "aa:bb:cc:dd:ee:ff".to_string(),
                mtu: 1500,
                speed_mbps: Some(1000),
                duplex: Some("full".to_string()),
                addresses: vec![NetworkAddress {
                    family: NetworkAddressFamily::Ipv4,
                    address: "192.0.2.10".to_string(),
                    prefix_len: 24,
                }],
                rx_bytes: 2048,
                tx_bytes: 1024,
                rx_packets: 10,
                tx_packets: 5,
                rx_errors: 1,
                tx_errors: 2,
                rx_dropped: 3,
                tx_dropped: 4,
            },
            NetworkInterface {
                name: "lo".to_string(),
                index: 1,
                state: "unknown".to_string(),
                up: true,
                running: true,
                mac: "00:00:00:00:00:00".to_string(),
                mtu: 65536,
                speed_mbps: None,
                duplex: None,
                addresses: Vec::new(),
                ..NetworkInterface::default()
            },
        ],
        timestamp_ms: 1_700_000_000_000,
        control_enabled: true,
        total: 3,
        truncated: true,
        warning: Some("showing 2 of 3 interfaces".to_string()),
    };

    let out = render_network_snapshot(&snapshot);
    for expected in [
        "IFACE",
        "eth0",
        "UP",
        "192.0.2.10/24",
        "1000 Mb/s",
        "2.0 KiB",
        "1.0 KiB",
        "lo",
        "00:00:00:00:00:00",
    ] {
        assert!(out.contains(expected), "missing {expected:?} in:\n{out}");
    }
    assert!(out.contains("Warning: showing 2 of 3 interfaces"));
}

#[test]
fn render_response_uses_the_shared_cli_prefixes() {
    assert_eq!(
        render_response(&Response::Ok {
            message: "done".to_string(),
        }),
        "[OK] done"
    );
    assert_eq!(
        render_response(&Response::NetworkStateChanged {
            name: "eth0".to_string(),
            up: false,
        }),
        "[OK] brought eth0 down"
    );
    assert_eq!(
        render_response(&Response::NetworkError(NetworkError::new(
            NetworkErrorKind::Protected,
            "loopback is protected",
        ))),
        "[ERROR] loopback is protected"
    );
}
