//! Tests for every module in this crate.
//!
//! One file per level: `lib.rs` finds a host, `parse.rs` turns file contents
//! into values, and this asserts both. A fixture tree lives here because a
//! parser test and an assembly test need the same throwaway directory.

use super::*;

/// The host assembly tests, which read a fixture tree instead of `/proc`.
mod collect {
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

/// The pure parsers, over file contents passed as strings.
mod parse {
    use super::*;
    use anth_wire::MemoryInfo;

    #[test]
    fn os_release_reads_name_and_version_id() {
        let (name, version) = parse_os_release("NAME=Alpine Linux\nVERSION_ID=3.20\n");
        assert_eq!(name, "Alpine Linux");
        assert_eq!(version, "3.20");
    }

    #[test]
    fn os_release_falls_back_to_version_when_version_id_is_absent() {
        let (_, version) = parse_os_release("NAME=Debian\nVERSION=\"12 (bookworm)\"\n");
        assert_eq!(version, "12 (bookworm)");
    }

    #[test]
    fn os_release_prefers_version_id_over_version() {
        let (_, version) = parse_os_release("NAME=X\nVERSION_ID=1\nVERSION=ignored\n");
        assert_eq!(version, "1");
    }

    #[test]
    fn os_release_resolves_the_permitted_escapes() {
        let (name, _) = parse_os_release(r#"NAME="a\"b\\c\$d\`e""#);
        assert_eq!(name, r#"a"b\c$d`e"#);
    }

    #[test]
    fn os_release_keeps_an_unknown_escape_literal() {
        let (name, _) = parse_os_release(r#"NAME="a\qb""#);
        assert_eq!(name, r"a\qb");
    }

    #[test]
    fn os_release_ignores_comments_and_malformed_lines() {
        let (name, version) = parse_os_release("# NAME=Nope\nnonsense\nNAME=Real\n");
        assert_eq!(name, "Real");
        assert_eq!(version, super::super::UNKNOWN);
    }

    #[test]
    fn os_release_answers_unknown_when_nothing_is_present() {
        let (name, version) = parse_os_release("");
        assert_eq!(name, super::super::UNKNOWN);
        assert_eq!(version, super::super::UNKNOWN);
    }

    #[test]
    fn os_release_treats_an_empty_value_as_absent() {
        let (name, _) = parse_os_release("NAME=\n");
        assert_eq!(name, super::super::UNKNOWN);
    }

    #[test]
    fn cpu_model_reads_the_x86_key() {
        let text = "processor\t: 0\nmodel name\t: Intel Example\n";
        assert_eq!(parse_cpu_model(text).as_deref(), Some("Intel Example"));
    }

    #[test]
    fn cpu_model_reads_the_arm_keys() {
        assert_eq!(
            parse_cpu_model("Hardware\t: BCM2711\n").as_deref(),
            Some("BCM2711")
        );
        assert_eq!(
            parse_cpu_model("Processor\t: ARMv7\n").as_deref(),
            Some("ARMv7")
        );
    }

    #[test]
    fn cpu_model_is_none_when_absent_or_blank() {
        assert!(parse_cpu_model("processor\t: 0\n").is_none());
        assert!(parse_cpu_model("model name\t:   \n").is_none());
    }

    #[test]
    fn cpu_cores_counts_processor_records_only() {
        let text = "processor\t: 0\nmodel name\t: X\n\nprocessor\t: 1\n";
        assert_eq!(parse_cpu_cores(text), 2);
        assert_eq!(parse_cpu_cores("model name\t: X\n"), 0);
    }

    #[test]
    fn meminfo_reads_kibibytes() {
        let memory = parse_meminfo(
            "MemTotal:       1000 kB\nMemAvailable:    400 kB\n\
             SwapTotal:       200 kB\nSwapFree:        50 kB\n",
        );
        assert_eq!(memory.total_kib, 1000);
        assert_eq!(memory.available_kib, 400);
        assert_eq!(memory.swap_total_kib, 200);
        assert_eq!(memory.swap_free_kib, 50);
        assert_eq!(memory.used_kib(), 600);
        assert_eq!(memory.swap_used_kib(), 150);
    }

    #[test]
    fn meminfo_converts_the_larger_units() {
        let memory = parse_meminfo("MemTotal: 1 mB\nSwapTotal: 1 gB\n");
        assert_eq!(memory.total_kib, 1024);
        assert_eq!(memory.swap_total_kib, 1024 * 1024);
    }

    #[test]
    fn meminfo_skips_an_unrecognized_unit_instead_of_guessing() {
        let memory = parse_meminfo("MemTotal: 1 MiB\n");
        assert_eq!(memory.total_kib, 0);
    }

    #[test]
    fn meminfo_ignores_unrelated_keys() {
        let memory = parse_meminfo("Buffers: 10 kB\nCached: 20 kB\n");
        assert_eq!(memory, MemoryInfo::default());
    }

    #[test]
    fn loadavg_reads_three_fields_and_stops() {
        let load = parse_loadavg("0.5 0.25 0.125 2/300 4242\n");
        assert_eq!(load.one, 0.5);
        assert_eq!(load.five, 0.25);
        assert_eq!(load.fifteen, 0.125);
    }

    #[test]
    fn loadavg_zeroes_the_missing_fields() {
        let load = parse_loadavg("1.5\n");
        assert_eq!(load.one, 1.5);
        assert_eq!(load.five, 0.0);
        assert_eq!(load.fifteen, 0.0);
    }

    #[test]
    fn uptime_truncates_to_whole_seconds() {
        assert_eq!(parse_uptime("1234.99 999.0\n"), 1234);
        assert_eq!(parse_uptime(""), 0);
        assert_eq!(parse_uptime("not a number"), 0);
    }
}
