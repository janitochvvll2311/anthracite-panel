//! Shell resolution and capability detection for the browser terminal.

use std::path::{Path, PathBuf};

/// Shells with an interactive line editor: history, cursor editing and the
/// usual Tab completion. Unknown shells fall back to the component's local
/// history so a minimal `dash` still has usable Up/Down.
const LINE_EDITING_SHELLS: &[&str] = &[
    "bash", "zsh", "fish", "ash", "ksh", "mksh", "busybox", "elvish", "nu", "xonsh",
];

/// Resolves `[terminal] shell`.
///
/// `auto` follows the v0.1 order: `$SHELL`, the account's login shell, then
/// `/bin/sh`. An explicit value must be an absolute, executable file.
pub fn resolve_shell(configured: &str) -> Result<String, String> {
    if configured != "auto" {
        if !Path::new(configured).is_absolute() {
            return Err(format!(
                "`terminal.shell` must be `auto` or an absolute path, got `{configured}`"
            ));
        }
        if !is_executable(Path::new(configured)) {
            return Err(format!(
                "`terminal.shell` {configured} is not an executable file"
            ));
        }
        return Ok(configured.to_string());
    }

    if let Some(shell) = std::env::var_os("SHELL").map(PathBuf::from)
        && shell.is_absolute()
        && is_executable(&shell)
    {
        return Ok(shell.to_string_lossy().into_owned());
    }

    if let Some(shell) = account_shell()
        && is_executable(&shell)
    {
        return Ok(shell.to_string_lossy().into_owned());
    }

    Ok("/bin/sh".to_string())
}

fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// The login shell of the uid the daemon runs as, from `/etc/passwd`.
fn account_shell() -> Option<PathBuf> {
    let uid = unsafe { libc::getuid() };
    let text = std::fs::read_to_string("/etc/passwd").ok()?;
    for line in text.lines() {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() >= 7 && fields[2].parse::<u32>().ok() == Some(uid) {
            let shell = PathBuf::from(fields[6]);
            if shell.is_absolute() {
                return Some(shell);
            }
        }
    }
    None
}

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

    #[test]
    fn explicit_shells_must_be_absolute_and_executable() {
        assert_eq!(resolve_shell("/bin/sh").unwrap(), "/bin/sh");
        assert!(resolve_shell("sh").is_err());
        assert!(resolve_shell("/definitely/not/a/shell").is_err());
    }

    #[test]
    fn auto_resolves_to_an_executable() {
        let shell = resolve_shell("auto").expect("auto shell");
        assert!(std::path::Path::new(&shell).is_absolute());
        assert!(is_executable(std::path::Path::new(&shell)));
    }
}
