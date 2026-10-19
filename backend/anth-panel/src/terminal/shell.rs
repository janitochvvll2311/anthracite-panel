//! Shell capability detection for the browser terminal's keyboard mode.

use std::path::PathBuf;

/// Shells with an interactive line editor: history, cursor editing and the
/// usual Tab completion. Unknown shells fall back to the component's local
/// history so a minimal `dash` still has usable Up/Down.
const LINE_EDITING_SHELLS: &[&str] = &[
    "bash", "zsh", "fish", "ash", "ksh", "mksh", "busybox", "elvish", "nu", "xonsh",
];

/// Whether `program` resolves to a shell with interactive line editing.
///
/// Canonicalisation matters for `/bin/sh`: it is `dash` on Debian (no line
/// editing) but `busybox` on Alpine (line editing).
pub fn supports_line_editing(program: &str) -> bool {
    let resolved = std::fs::canonicalize(program).unwrap_or_else(|_| PathBuf::from(program));
    let name = resolved
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(program);

    shell_name_supports_line_editing(name)
}

fn shell_name_supports_line_editing(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    LINE_EDITING_SHELLS.contains(&name.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_editing_shells_are_recognized() {
        for shell in ["bash", "zsh", "fish", "ash", "ksh", "mksh", "busybox"] {
            assert!(
                shell_name_supports_line_editing(shell),
                "{shell} should support line editing"
            );
        }
    }

    #[test]
    fn plain_sh_and_dash_fall_back_to_local_history() {
        assert!(!shell_name_supports_line_editing("sh"));
        assert!(!shell_name_supports_line_editing("dash"));
        assert!(!shell_name_supports_line_editing("posh"));
    }

    #[test]
    fn a_failed_canonicalisation_uses_the_program_name() {
        assert!(supports_line_editing("/nonexistent/bash"));
        assert!(!supports_line_editing("/nonexistent/dash"));
    }
}
