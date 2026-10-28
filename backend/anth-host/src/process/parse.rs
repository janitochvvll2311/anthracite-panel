//! Pure `/proc` parsers.
//!
//! Everything here takes text or bytes and returns values; no file is opened.
//! That is what lets the fixtures below cover kernel formats without a live
//! process table, and what keeps the scanner in `mod.rs` free of field offsets.

use std::collections::HashMap;

/// The fields the process table needs from `/proc/<pid>/stat`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatFields {
    /// One-letter state.
    pub state: String,
    pub ppid: u32,
    /// Process group id. Equal to `pid` only for a group leader.
    pub pgrp: u32,
    /// `utime + stime` in clock ticks.
    pub cpu_ticks: u64,
    /// Thread count from the stat field; `/proc/<pid>/status` wins when present.
    pub threads: u32,
    pub start_ticks: u64,
}

/// The fields the process table needs from `/proc/<pid>/status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StatusFields {
    pub uid: Option<u32>,
    pub rss_kib: u64,
    pub threads: Option<u32>,
}

/// Parses `/proc/<pid>/stat`.
///
/// The command name sits between parentheses and may itself contain spaces and
/// parentheses, so the fields are read from the **last** `)` onward.
pub fn parse_stat(text: &str) -> Option<StatFields> {
    let end = text.rfind(')')?;
    let fields: Vec<&str> = text.get(end + 1..)?.split_whitespace().collect();
    // Field 22 (starttime) is index 19 once the first three fields are gone.
    if fields.len() < 20 {
        return None;
    }
    Some(StatFields {
        state: fields[0].to_string(),
        ppid: fields[1].parse().ok()?,
        pgrp: fields[2].parse().ok()?,
        cpu_ticks: fields[11].parse::<u64>().ok()? + fields[12].parse::<u64>().ok()?,
        threads: fields[17].parse().ok()?,
        start_ticks: fields[19].parse().ok()?,
    })
}

/// Parses the three `/proc/<pid>/status` fields that are not in `stat`.
pub fn parse_status(text: &str) -> StatusFields {
    let mut fields = StatusFields::default();
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key {
            "Uid" => {
                fields.uid = value
                    .split_whitespace()
                    .next()
                    .and_then(|uid| uid.parse().ok());
            }
            "VmRSS" => {
                fields.rss_kib = value
                    .split_whitespace()
                    .next()
                    .and_then(|kib| kib.parse().ok())
                    .unwrap_or(0);
            }
            "Threads" => fields.threads = value.parse().ok(),
            _ => {}
        }
    }
    fields
}

/// Parses `/proc/<pid>/comm`, removing the trailing newline.
pub fn parse_comm(text: &str) -> String {
    text.trim_end_matches(['\n', '\r']).to_string()
}

/// Longest command line the table reports, in characters. A process can put
/// megabytes here; a row is a summary, and the page contract bounds the count.
pub const MAX_COMMAND_CHARS: usize = 2048;

/// Parses `/proc/<pid>/cmdline`: NUL-separated arguments become one line.
pub fn parse_cmdline(bytes: &[u8]) -> Option<String> {
    let args: Vec<String> = bytes
        .split(|byte| *byte == 0)
        .filter(|arg| !arg.is_empty())
        .map(|arg| String::from_utf8_lossy(arg).into_owned())
        .collect();
    if args.is_empty() {
        return None;
    }
    let joined = args.join(" ");
    Some(joined.chars().take(MAX_COMMAND_CHARS).collect())
}

/// Reads `/etc/passwd` once into a uid → name map.
pub fn read_users(text: &str) -> HashMap<u32, String> {
    let mut users = HashMap::new();
    for line in text.lines() {
        let mut fields = line.split(':');
        let Some(name) = fields.next() else {
            continue;
        };
        let _password = fields.next();
        let Some(uid) = fields.next().and_then(|uid| uid.parse::<u32>().ok()) else {
            continue;
        };
        if !name.is_empty() {
            users.entry(uid).or_insert_with(|| name.to_string());
        }
    }
    users
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAT: &str = "1234 (a name (with) parens) S 1 1234 1234 0 -1 4194304 100 0 0 0 12 34 0 0 20 0 4 0 98765 123456 789 18446744073709551615";

    #[test]
    fn stat_fields_survive_a_parenthesised_name() {
        let fields = parse_stat(STAT).expect("stat");
        assert_eq!(fields.state, "S");
        assert_eq!(fields.ppid, 1);
        assert_eq!(fields.pgrp, 1234);
        assert_eq!(fields.cpu_ticks, 46);
        assert_eq!(fields.threads, 4);
        assert_eq!(fields.start_ticks, 98765);
    }

    #[test]
    fn a_truncated_stat_is_refused() {
        assert!(parse_stat("1234 (sh) S 1").is_none());
        assert!(parse_stat("not a stat line").is_none());
    }

    #[test]
    fn status_reads_uid_rss_and_threads() {
        let status = "Name:\tsh\nUid:\t1000\t1000\t1000\t1000\nVmRSS:\t  1234 kB\nThreads:\t7\n";
        let fields = parse_status(status);
        assert_eq!(fields.uid, Some(1000));
        assert_eq!(fields.rss_kib, 1234);
        assert_eq!(fields.threads, Some(7));
    }

    #[test]
    fn a_kernel_thread_without_vmrss_reports_zero() {
        let fields = parse_status("Name:\tkthreadd\nUid:\t0\t0\t0\t0\n");
        assert_eq!(fields.rss_kib, 0);
        assert_eq!(fields.threads, None);
    }

    #[test]
    fn cmdline_joins_nul_separated_arguments() {
        let bytes = b"/bin/sh\0-c\0echo hi\0";
        assert_eq!(parse_cmdline(bytes).as_deref(), Some("/bin/sh -c echo hi"));
        assert_eq!(parse_cmdline(b"\0\0"), None);
    }

    #[test]
    fn very_long_command_lines_are_bounded() {
        let bytes = vec![b'x'; MAX_COMMAND_CHARS * 2];
        let command = parse_cmdline(&bytes).expect("command");
        assert_eq!(command.chars().count(), MAX_COMMAND_CHARS);
    }

    #[test]
    fn users_map_uid_to_name() {
        let users =
            read_users("root:x:0:0:root:/root:/bin/sh\nadmin:x:1000:1000::/home/admin:/bin/sh\n");
        assert_eq!(users.get(&0).map(String::as_str), Some("root"));
        assert_eq!(users.get(&1000).map(String::as_str), Some("admin"));
        assert_eq!(users.get(&4242), None);
    }

    #[test]
    fn comm_loses_its_newline_only() {
        assert_eq!(parse_comm("bash\n"), "bash");
        assert_eq!(parse_comm("a b"), "a b");
    }
}
