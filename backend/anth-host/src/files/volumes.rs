//! Which mount points the trash has to cover.
//!
//! The trash spec is per-volume, so the service needs the list of mounted
//! topdirs. `mountinfo` is the authority — a device that exists but is not
//! mounted has no trash to speak of — and re-reading it on every listing keeps
//! the answer right after a mount happens outside the panel.

use std::path::PathBuf;

/// Parses the mount points out of `/proc/self/mountinfo`.
///
/// Duplicates are collapsed and nested mounts are kept, because a bind mount of
/// a subdirectory is its own volume with its own trash directory.
pub fn topdirs_from_mountinfo(text: &str) -> Vec<PathBuf> {
    let mut topdirs: Vec<PathBuf> = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        // mountpoint is field 5, counting from 1, after the mount id, parent
        // id, major:minor and root.
        let Some(mountpoint) = fields.get(4) else {
            continue;
        };
        let path = PathBuf::from(unescape(mountpoint));
        if !path.is_absolute() {
            continue;
        }
        if !topdirs.contains(&path) {
            topdirs.push(path);
        }
    }
    topdirs
}

/// `/proc/self/mountinfo` escapes space, tab, newline and backslash as octal.
fn unescape(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let octal = &value[index + 1..index + 4];
            if let Ok(byte) = u8::from_str_radix(octal, 8) {
                out.push(byte);
                index += 4;
                continue;
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Reads the live mount table. Unit tests pass their own list instead.
pub fn live_topdirs() -> Vec<PathBuf> {
    match std::fs::read_to_string("/proc/self/mountinfo") {
        Ok(text) => topdirs_from_mountinfo(&text),
        // Without mountinfo the only volume left is the root, which is still
        // better than a trash that silently covers nothing.
        Err(_) => vec![PathBuf::from("/")],
    }
}

/// The home directory and the XDG data directory the spec uses for the home
/// volume's trash.
pub fn home_paths() -> (PathBuf, PathBuf) {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"));
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| home.join(".local/share"));
    (home, data_home)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_mountpoints_and_collapses_duplicates() {
        let text = "\
25 30 0:22 / /sys rw,nosuid - sysfs sysfs rw
26 30 0:23 / /proc rw - proc proc rw
30 1 8:1 / / rw,relatime - ext4 /dev/sda1 rw
31 30 8:1 /home /home rw - ext4 /dev/sda1 rw
";
        assert_eq!(
            topdirs_from_mountinfo(text),
            vec![
                PathBuf::from("/sys"),
                PathBuf::from("/proc"),
                PathBuf::from("/"),
                PathBuf::from("/home"),
            ]
        );
    }

    #[test]
    fn decodes_escaped_spaces_in_mountpoints() {
        let text = "40 30 8:2 / /media/My\\040Disk rw - ext4 /dev/sdb1 rw\n";
        assert_eq!(
            topdirs_from_mountinfo(text),
            vec![PathBuf::from("/media/My Disk")]
        );
    }

    #[test]
    fn ignores_lines_without_a_mountpoint() {
        assert!(topdirs_from_mountinfo("garbage\n\n").is_empty());
    }
}
