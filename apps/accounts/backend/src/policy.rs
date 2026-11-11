//! The operator-approved policy the accounts service runs with.
//!
//! The policy travels in the bundle's argv, which the engine freezes in the
//! grant and shows at approval. Without `--control` the service is read-only.

use std::path::PathBuf;

use anth_contract::Error;

/// Fallback when neither `--shell` nor `/etc/shells` yields an entry.
const BUILTIN_SHELLS: &[&str] = &["/bin/sh", "/bin/bash", "/bin/ash", "/bin/zsh", "/bin/dash"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policy {
    pub control: bool,
    pub allow_system: bool,
    pub allow_privileged_groups: bool,
    pub home_root: PathBuf,
    pub shells: Vec<String>,
    pub default_shell: String,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            control: false,
            allow_system: false,
            allow_privileged_groups: false,
            home_root: PathBuf::from("/home"),
            shells: Vec::new(),
            default_shell: "/bin/sh".to_string(),
        }
    }
}

impl Policy {
    pub fn from_args<I>(args: I) -> Result<Self, Error>
    where
        I: IntoIterator<Item = String>,
    {
        let mut policy = Self::default();
        let mut args = args.into_iter();
        while let Some(argument) = args.next() {
            match argument.as_str() {
                "--socket" => {
                    args.next()
                        .ok_or_else(|| Error::invalid("`--socket` needs a path"))?;
                }
                "--control" => policy.control = true,
                "--allow-system" => policy.allow_system = true,
                "--allow-privileged-groups" => policy.allow_privileged_groups = true,
                "--home-root" => {
                    policy.home_root = PathBuf::from(
                        args.next()
                            .ok_or_else(|| Error::invalid("`--home-root` needs a path"))?,
                    );
                }
                "--shell" => {
                    policy.shells.push(
                        args.next()
                            .ok_or_else(|| Error::invalid("`--shell` needs a path"))?,
                    );
                }
                "--default-shell" => {
                    policy.default_shell = args
                        .next()
                        .ok_or_else(|| Error::invalid("`--default-shell` needs a path"))?;
                }
                other => {
                    return Err(Error::invalid(format!(
                        "unknown accounts-service argument `{other}`"
                    )));
                }
            }
        }
        if !policy.home_root.is_absolute() {
            return Err(Error::invalid("`--home-root` must be an absolute path"));
        }
        if !policy.default_shell.starts_with('/') {
            return Err(Error::invalid("`--default-shell` must be an absolute path"));
        }
        Ok(policy)
    }

    /// Configured shells first, then `/etc/shells`, then a built-in set. An
    /// empty configuration never means "anything goes".
    pub fn allowed_shells(&self) -> Vec<String> {
        if !self.shells.is_empty() {
            return self.shells.clone();
        }
        if let Ok(text) = std::fs::read_to_string("/etc/shells") {
            let listed: Vec<String> = text
                .lines()
                .map(str::trim)
                .filter(|line| line.starts_with('/'))
                .map(str::to_string)
                .collect();
            if !listed.is_empty() {
                return listed;
            }
        }
        BUILTIN_SHELLS
            .iter()
            .map(|shell| (*shell).to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn policy_flags_are_explicit_and_default_to_read_only() {
        let policy = Policy::from_args(args(&["--socket", "/run/a.sock"])).expect("parse");
        assert!(!policy.control);
        assert!(!policy.allow_system);
        assert!(!policy.allow_privileged_groups);
        assert_eq!(policy.home_root, PathBuf::from("/home"));
        assert_eq!(policy.default_shell, "/bin/sh");
        assert!(policy.shells.is_empty());

        let policy = Policy::from_args(args(&[
            "--socket",
            "/run/a.sock",
            "--control",
            "--allow-system",
            "--allow-privileged-groups",
            "--home-root",
            "/srv/home",
            "--shell",
            "/bin/zsh",
            "--default-shell",
            "/bin/bash",
        ]))
        .expect("parse");
        assert!(policy.control);
        assert!(policy.allow_system);
        assert!(policy.allow_privileged_groups);
        assert_eq!(policy.home_root, PathBuf::from("/srv/home"));
        assert_eq!(policy.shells, vec!["/bin/zsh".to_string()]);
        assert_eq!(policy.default_shell, "/bin/bash");
    }

    #[test]
    fn policy_rejects_unknown_flags_and_relative_paths() {
        assert!(Policy::from_args(args(&["--nope"])).is_err());
        assert!(Policy::from_args(args(&["--home-root", "home"])).is_err());
        assert!(Policy::from_args(args(&["--default-shell", "sh"])).is_err());
        assert!(Policy::from_args(args(&["--shell"])).is_err());
    }

    #[test]
    fn configured_shells_win_over_the_host_list() {
        let policy = Policy {
            shells: vec!["/bin/custom".to_string()],
            ..Policy::default()
        };
        assert_eq!(policy.allowed_shells(), vec!["/bin/custom".to_string()]);
    }
}
