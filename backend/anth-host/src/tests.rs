use std::fs;
use std::path::PathBuf;

use super::*;

fn scratch(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = PathBuf::from("/tmp/anthracite-panel/tests")
        .join(format!("{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&root).expect("create scratch");
    root
}

fn write(root: &std::path::Path, relative: &str, text: &str) {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent");
    }
    fs::write(path, text).expect("write fixture");
}

#[test]
fn collect_from_reads_fixture_files() {
    let root = scratch("host-info");
    write(
        &root,
        "etc/os-release",
        "NAME=\"Debian GNU/Linux\"\nVERSION_ID=\"13\"\n",
    );
    write(&root, "proc/sys/kernel/hostname", "example\n");
    write(&root, "proc/sys/kernel/osrelease", "6.12.0-test\n");
    write(&root, "proc/uptime", "1234.56 999.00\n");
    write(
        &root,
        "proc/cpuinfo",
        "processor : 0\nmodel name : Example CPU\n\nprocessor : 1\nmodel name : Example CPU\n",
    );
    write(&root, "proc/loadavg", "0.12 0.20 0.31 1/10 123\n");
    write(
        &root,
        "proc/meminfo",
        "MemTotal: 16384000 kB\nMemAvailable: 8192000 kB\nSwapTotal: 2097152 kB\nSwapFree: 1048576 kB\n",
    );

    let info = collect_from(&root);
    assert_eq!(info.hostname, "example");
    assert_eq!(info.os.name, "Debian GNU/Linux");
    assert_eq!(info.os.version, "13");
    assert_eq!(info.os.kernel, "6.12.0-test");
    assert_eq!(info.uptime_seconds, 1234);
    assert_eq!(info.cpu_model, "Example CPU");
    assert_eq!(info.logical_cores, 2);
    assert_eq!(info.load.one, 0.12);
    assert_eq!(info.memory.total_kib, 16_384_000);
    assert!(info.collected_at_ms > 0);

    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn unreadable_fields_degrade_instead_of_failing() {
    let root = scratch("host-empty");
    let info = collect_from(&root);
    assert_eq!(info.hostname, UNKNOWN);
    assert_eq!(info.os.name, UNKNOWN);
    assert_eq!(info.os.version, UNKNOWN);
    assert_eq!(info.os.kernel, UNKNOWN);
    assert_eq!(info.uptime_seconds, 0);
    assert_eq!(info.cpu_model, UNKNOWN);
    assert_eq!(info.logical_cores, 0);
    assert_eq!(info.load.one, 0.0);
    assert_eq!(info.memory.total_kib, 0);
    assert!(info.collected_at_ms > 0);

    fs::remove_dir_all(root).expect("cleanup");
}
