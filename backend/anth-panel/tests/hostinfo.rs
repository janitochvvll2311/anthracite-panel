use anth_panel::hostinfo::{
    collect, collect_from, parse_cpu_cores, parse_cpu_model, parse_loadavg, parse_meminfo,
    parse_os_release, parse_uptime,
};

const OS_RELEASE: &str = r#"
PRETTY_NAME="Debian GNU/Linux 13"
NAME="Debian GNU/Linux"
VERSION_ID="13"
VERSION="13 (trixie)"
ID=debian
"#;

const CPUINFO: &str = r#"
processor	: 0
vendor_id	: GenuineIntel
model name	: Intel(R) Xeon(R) CPU E5-2673 v4 @ 2.30GHz
cpu MHz		: 2299.998

processor	: 1
vendor_id	: GenuineIntel
model name	: Intel(R) Xeon(R) CPU E5-2673 v4 @ 2.30GHz
cpu MHz		: 2299.998
"#;

const MEMINFO: &str = r#"
MemTotal:        8134656 kB
MemFree:          123456 kB
MemAvailable:    4067328 kB
SwapTotal:       2097148 kB
SwapFree:        1048574 kB
"#;

#[test]
fn parse_os_release_reads_name_and_version_id() {
    let (name, version) = parse_os_release(OS_RELEASE);
    assert_eq!(name, "Debian GNU/Linux");
    assert_eq!(version, "13");
}

#[test]
fn parse_os_release_uses_version_when_version_id_is_missing() {
    let text = "NAME=\"Alpine Linux\"\nVERSION=\"3.20.3\"\n";
    assert_eq!(
        parse_os_release(text),
        ("Alpine Linux".to_string(), "3.20.3".to_string())
    );
}

#[test]
fn parse_os_release_handles_unquoted_values_and_comments() {
    let text = "# a comment\nNAME=Gentoo\nVERSION_ID=2.14\nEMPTY=\n";
    assert_eq!(
        parse_os_release(text),
        ("Gentoo".to_string(), "2.14".to_string())
    );
}

#[test]
fn parse_os_release_unquotes_escapes() {
    let text = "NAME=\"Quoted \\\"Name\\\"\"\nVERSION_ID=\"1\\\\2\"\n";
    assert_eq!(
        parse_os_release(text),
        ("Quoted \"Name\"".to_string(), "1\\2".to_string())
    );
}

#[test]
fn parse_os_release_falls_back_to_unknown() {
    assert_eq!(
        parse_os_release(""),
        ("unknown".to_string(), "unknown".to_string())
    );
    assert_eq!(
        parse_os_release("ID=debian\n"),
        ("unknown".to_string(), "unknown".to_string())
    );
}

#[test]
fn parse_os_release_ignores_lines_without_a_key_value_pair() {
    let text = "not-a-pair\nNAME=\"X\"\n";
    assert_eq!(parse_os_release(text).0, "X");
}

#[test]
fn parse_cpu_model_reads_the_first_processor_model() {
    assert_eq!(
        parse_cpu_model(CPUINFO).as_deref(),
        Some("Intel(R) Xeon(R) CPU E5-2673 v4 @ 2.30GHz")
    );
}

#[test]
fn parse_cpu_model_understands_arm_keys() {
    let text = "processor\t: 0\nHardware\t: BCM2711\n";
    assert_eq!(parse_cpu_model(text).as_deref(), Some("BCM2711"));

    let single = "processor : 0\nProcessor : ARMv7 Processor rev 3 (v7l)\n";
    assert_eq!(
        parse_cpu_model(single).as_deref(),
        Some("ARMv7 Processor rev 3 (v7l)")
    );
}

#[test]
fn parse_cpu_model_is_none_when_absent() {
    assert_eq!(parse_cpu_model("processor : 0\n"), None);
    assert_eq!(parse_cpu_model(""), None);
}

#[test]
fn parse_cpu_cores_counts_processor_entries() {
    assert_eq!(parse_cpu_cores(CPUINFO), 2);
    assert_eq!(parse_cpu_cores(""), 0);
}

#[test]
fn parse_meminfo_reads_the_four_fields_in_kibibytes() {
    let memory = parse_meminfo(MEMINFO);
    assert_eq!(memory.total_kib, 8_134_656);
    assert_eq!(memory.available_kib, 4_067_328);
    assert_eq!(memory.swap_total_kib, 2_097_148);
    assert_eq!(memory.swap_free_kib, 1_048_574);
}

#[test]
fn parse_meminfo_defaults_missing_fields_to_zero() {
    let memory = parse_meminfo("MemTotal: 100 kB\n");
    assert_eq!(memory.total_kib, 100);
    assert_eq!(memory.available_kib, 0);
    assert_eq!(memory.swap_total_kib, 0);
}

#[test]
fn parse_meminfo_converts_non_kilobyte_units() {
    let memory = parse_meminfo("MemTotal: 2 mB\nMemAvailable: 1 gB\n");
    assert_eq!(memory.total_kib, 2048);
    assert_eq!(memory.available_kib, 1024 * 1024);
}

#[test]
fn parse_meminfo_skips_lines_it_cannot_parse() {
    let memory =
        parse_meminfo("MemTotal: not-a-number kB\nMemFree: 5 unknown\nMemAvailable: 7 kB\n");
    assert_eq!(memory.total_kib, 0);
    assert_eq!(memory.available_kib, 7);
}

#[test]
fn parse_loadavg_reads_three_values() {
    let load = parse_loadavg("0.52 0.58 0.59 1/234 5678\n");
    assert_eq!(load.one, 0.52);
    assert_eq!(load.five, 0.58);
    assert_eq!(load.fifteen, 0.59);
}

#[test]
fn parse_loadavg_defaults_to_zero_when_short() {
    let load = parse_loadavg("0.5\n");
    assert_eq!(load.one, 0.5);
    assert_eq!(load.five, 0.0);
    assert_eq!(load.fifteen, 0.0);
    assert_eq!(parse_loadavg("").fifteen, 0.0);
}

#[test]
fn parse_uptime_truncates_the_fractional_part() {
    assert_eq!(parse_uptime("90061.42 360000.00\n"), 90_061);
    assert_eq!(parse_uptime("0.00 0.00\n"), 0);
    assert_eq!(parse_uptime(""), 0);
}

#[test]
fn collect_reports_a_plausible_consistent_snapshot() {
    let info = collect();

    assert!(!info.hostname.is_empty());
    assert!(!info.os.name.is_empty());
    assert!(!info.os.kernel.is_empty());
    assert_eq!(info.os.arch, std::env::consts::ARCH);
    assert!(info.logical_cores >= 1);
    assert!(info.memory.total_kib > 0);
    assert!(
        info.memory.available_kib <= info.memory.total_kib,
        "available {} exceeds total {}",
        info.memory.available_kib,
        info.memory.total_kib
    );
}

#[test]
fn collect_from_reads_a_fixture_root() {
    let root = std::env::temp_dir().join(format!(
        "anth-hostinfo-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    for dir in ["etc", "proc/sys/kernel"] {
        std::fs::create_dir_all(root.join(dir)).unwrap();
    }
    std::fs::write(root.join("etc/os-release"), OS_RELEASE).unwrap();
    std::fs::write(root.join("proc/cpuinfo"), CPUINFO).unwrap();
    std::fs::write(root.join("proc/meminfo"), MEMINFO).unwrap();
    std::fs::write(root.join("proc/loadavg"), "1.5 1.0 0.5 1/100 42\n").unwrap();
    std::fs::write(root.join("proc/uptime"), "3661.5 1000.0\n").unwrap();
    std::fs::write(root.join("proc/sys/kernel/hostname"), "fixture-box\n").unwrap();
    std::fs::write(root.join("proc/sys/kernel/osrelease"), "6.1.0-fixture\n").unwrap();

    let info = collect_from(&root);

    assert_eq!(info.hostname, "fixture-box");
    assert_eq!(info.os.name, "Debian GNU/Linux");
    assert_eq!(info.os.version, "13");
    assert_eq!(info.os.kernel, "6.1.0-fixture");
    assert_eq!(info.cpu_model, "Intel(R) Xeon(R) CPU E5-2673 v4 @ 2.30GHz");
    assert_eq!(info.logical_cores, 2);
    assert_eq!(info.uptime_seconds, 3661);
    assert_eq!(info.load.one, 1.5);
    assert_eq!(info.memory.total_kib, 8_134_656);
    assert_eq!(info.memory.swap_free_kib, 1_048_574);

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn collect_from_degrades_gracefully_on_an_empty_root() {
    let root = std::env::temp_dir().join(format!(
        "anth-hostinfo-empty-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&root).unwrap();

    let info = collect_from(&root);

    assert_eq!(info.hostname, "unknown");
    assert_eq!(info.os.name, "unknown");
    assert_eq!(info.cpu_model, "unknown");
    assert_eq!(info.logical_cores, 0);
    assert_eq!(info.uptime_seconds, 0);
    assert_eq!(info.memory.total_kib, 0);

    std::fs::remove_dir_all(&root).unwrap();
}
