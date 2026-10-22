//! Host inspection for Linux, built on `/proc` and `/etc/os-release`.
//!
//! Ported from v0.1.0 because it was already the right shape: standard library
//! only (no `sysinfo`-style dependency, no `unsafe`, so the static musl build
//! stays trivial), and every parser is a pure function over file contents so it
//! can be tested against fixtures. [`collect_from`] takes a filesystem root for
//! exactly that reason.
//!
//! Per-process inspection is deliberately out of scope; this describes the host.

mod parse;

pub use parse::{
    parse_cpu_cores, parse_cpu_model, parse_loadavg, parse_meminfo, parse_os_release, parse_uptime,
};

use std::path::Path;

use anth_wire::{HostInfo, OsInfo};

const HOSTNAME: &str = "/proc/sys/kernel/hostname";
const OS_RELEASE: &str = "/etc/os-release";
const OS_RELEASE_FALLBACK: &str = "/usr/lib/os-release";
const OSRELEASE: &str = "/proc/sys/kernel/osrelease";
const UPTIME: &str = "/proc/uptime";
const CPUINFO: &str = "/proc/cpuinfo";
const LOADAVG: &str = "/proc/loadavg";
const MEMINFO: &str = "/proc/meminfo";

pub const UNKNOWN: &str = "unknown";

/// Reads the first readable path, returning `None` if none is.
///
/// A missing or non-UTF-8 source degrades to `None` rather than failing the
/// whole snapshot: the panel should still say what it can.
fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn read_first(paths: &[&str]) -> Option<String> {
    paths.iter().find_map(|path| read_trimmed(*path))
}

/// Collects a snapshot of the host the daemon runs on.
pub fn collect() -> HostInfo {
    let (os_name, os_version) = match read_first(&[OS_RELEASE, OS_RELEASE_FALLBACK]) {
        Some(text) => parse_os_release(&text),
        None => (UNKNOWN.to_string(), UNKNOWN.to_string()),
    };

    let cpuinfo = read_trimmed(CPUINFO).unwrap_or_default();
    let cores = match parse_cpu_cores(&cpuinfo) {
        0 => std::thread::available_parallelism().map_or(0, |n| n.get()),
        count => count,
    };

    HostInfo {
        hostname: read_first(&[HOSTNAME])
            .or_else(|| std::env::var("HOSTNAME").ok())
            .unwrap_or_else(|| UNKNOWN.to_string()),
        os: OsInfo {
            name: os_name,
            version: os_version,
            kernel: read_first(&[OSRELEASE]).unwrap_or_else(|| UNKNOWN.to_string()),
            arch: std::env::consts::ARCH.to_string(),
        },
        uptime_seconds: read_trimmed(UPTIME)
            .map(|text| parse_uptime(&text))
            .unwrap_or(0),
        cpu_model: parse_cpu_model(&cpuinfo).unwrap_or_else(|| UNKNOWN.to_string()),
        logical_cores: cores,
        load: read_trimmed(LOADAVG)
            .map(|text| parse_loadavg(&text))
            .unwrap_or_default(),
        memory: read_trimmed(MEMINFO)
            .map(|text| parse_meminfo(&text))
            .unwrap_or_default(),
    }
}

/// Reads a snapshot from an alternative filesystem root.
///
/// This is how the parsers are exercised against fixture files instead of the
/// machine running the tests.
pub fn collect_from(root: impl AsRef<Path>) -> HostInfo {
    let root = root.as_ref();
    let read = |relative: &str| read_trimmed(root.join(relative));

    let (os_name, os_version) = read("etc/os-release")
        .map(|text| parse_os_release(&text))
        .unwrap_or_else(|| (UNKNOWN.to_string(), UNKNOWN.to_string()));
    let cpuinfo = read("proc/cpuinfo").unwrap_or_default();

    HostInfo {
        hostname: read("proc/sys/kernel/hostname").unwrap_or_else(|| UNKNOWN.to_string()),
        os: OsInfo {
            name: os_name,
            version: os_version,
            kernel: read("proc/sys/kernel/osrelease").unwrap_or_else(|| UNKNOWN.to_string()),
            arch: std::env::consts::ARCH.to_string(),
        },
        uptime_seconds: read("proc/uptime")
            .map(|text| parse_uptime(&text))
            .unwrap_or(0),
        cpu_model: parse_cpu_model(&cpuinfo).unwrap_or_else(|| UNKNOWN.to_string()),
        logical_cores: parse_cpu_cores(&cpuinfo),
        load: read("proc/loadavg")
            .map(|text| parse_loadavg(&text))
            .unwrap_or_default(),
        memory: read("proc/meminfo")
            .map(|text| parse_meminfo(&text))
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Writes a fixture tree.
    ///
    /// Panics if a file cannot be written: v0.1.0 shipped a `fixture!` that
    /// returned early when its input was missing, so seven tests asserted
    /// nothing while the suite reported success (lesson 4).
    fn fixture(files: &[(&str, &str)]) -> tempdir::TempDir {
        let dir = tempdir::TempDir::new();
        for (relative, contents) in files {
            let path = dir.path().join(relative);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .unwrap_or_else(|e| panic!("creating {}: {e}", parent.display()));
            }
            let mut file = std::fs::File::create(&path)
                .unwrap_or_else(|e| panic!("creating {}: {e}", path.display()));
            file.write_all(contents.as_bytes())
                .unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
        }
        dir
    }

    /// A throwaway directory unique to this process, removed on drop.
    mod tempdir {
        use std::path::{Path, PathBuf};

        pub struct TempDir(PathBuf);

        impl TempDir {
            pub fn new() -> Self {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0);
                let path =
                    std::env::temp_dir().join(format!("anth-host-{}-{nanos}", std::process::id()));
                std::fs::create_dir_all(&path).expect("creating the fixture root");
                Self(path)
            }

            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }

    #[test]
    fn a_complete_fixture_tree_is_read_in_full() {
        let dir = fixture(&[
            ("proc/sys/kernel/hostname", "anth\n"),
            ("proc/sys/kernel/osrelease", "6.6.0-test\n"),
            ("etc/os-release", "NAME=\"Alpine Linux\"\nVERSION_ID=3.20\n"),
            ("proc/uptime", "1234.56 999.0\n"),
            ("proc/loadavg", "0.50 0.25 0.12 1/100 1234\n"),
            (
                "proc/meminfo",
                "MemTotal:       2048000 kB\nMemAvailable:    512000 kB\n\
                 SwapTotal:       102400 kB\nSwapFree:        102400 kB\n",
            ),
            (
                "proc/cpuinfo",
                "processor\t: 0\nmodel name\t: Example CPU\n\n\
                 processor\t: 1\nmodel name\t: Example CPU\n\n",
            ),
        ]);

        let info = collect_from(dir.path());

        assert_eq!(info.hostname, "anth");
        assert_eq!(info.os.name, "Alpine Linux");
        assert_eq!(info.os.version, "3.20");
        assert_eq!(info.os.kernel, "6.6.0-test");
        assert_eq!(info.uptime_seconds, 1234);
        assert_eq!(info.cpu_model, "Example CPU");
        assert_eq!(info.logical_cores, 2);
        assert_eq!(info.load.one, 0.5);
        assert_eq!(info.memory.total_kib, 2_048_000);
        assert_eq!(info.memory.available_kib, 512_000);
        assert_eq!(info.memory.used_kib(), 1_536_000);
        assert_eq!(info.memory.swap_used_kib(), 0);
    }

    #[test]
    fn an_empty_root_degrades_every_field_instead_of_failing() {
        let dir = fixture(&[]);
        let info = collect_from(dir.path());

        assert_eq!(info.hostname, UNKNOWN);
        assert_eq!(info.os.name, UNKNOWN);
        assert_eq!(info.os.version, UNKNOWN);
        assert_eq!(info.os.kernel, UNKNOWN);
        assert_eq!(info.cpu_model, UNKNOWN);
        assert_eq!(info.uptime_seconds, 0);
        assert_eq!(info.logical_cores, 0);
        assert_eq!(info.memory.total_kib, 0);
    }

    #[test]
    fn a_partial_root_keeps_the_fields_it_can_read() {
        let dir = fixture(&[
            ("proc/sys/kernel/hostname", "partial\n"),
            ("proc/loadavg", "1.00 2.00 3.00\n"),
        ]);

        let info = collect_from(dir.path());

        assert_eq!(info.hostname, "partial");
        assert_eq!(info.load.fifteen, 3.0);
        assert_eq!(info.os.name, UNKNOWN);
    }

    #[test]
    fn a_blank_source_is_treated_as_absent() {
        // A whitespace-only file must not become an empty-string field: the
        // panel renders `unknown`, not a blank cell.
        let dir = fixture(&[("proc/sys/kernel/hostname", "   \n\n")]);
        assert_eq!(collect_from(dir.path()).hostname, UNKNOWN);
    }

    #[test]
    fn the_os_release_fallback_path_is_not_used_when_the_primary_exists() {
        let dir = fixture(&[
            ("etc/os-release", "NAME=Primary\nVERSION_ID=1\n"),
            ("usr/lib/os-release", "NAME=Fallback\nVERSION_ID=2\n"),
        ]);
        assert_eq!(collect_from(dir.path()).os.name, "Primary");
    }

    #[test]
    fn collect_reads_this_machine_without_panicking() {
        // A smoke test against the real sources: every field must be populated
        // enough to render, and a missing source must not abort.
        let info = collect();
        assert!(!info.hostname.is_empty());
        assert!(!info.os.arch.is_empty());
        assert!(!info.os.kernel.is_empty());
    }
}
