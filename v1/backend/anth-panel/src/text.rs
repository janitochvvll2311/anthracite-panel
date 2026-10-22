//! Small shared filesystem helpers used by the read-only inspectors.

use std::path::Path;

/// Reads a file and trims surrounding whitespace, treating an empty result as
/// absent. Inspectors use it for sysfs and `/proc` values that are optional.
pub fn read_trimmed(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}
