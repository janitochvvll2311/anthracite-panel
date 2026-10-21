//! The `auth` and `config` subcommands.
//!
//! Both edit the configuration file directly rather than talking to the socket:
//! the daemon holds its own copy and a login credential is not a host operation.
//! `config set` is the one place a key is named as text, so the allowlist in
//! [`set_key`] is what keeps a typo from silently creating a key the loader then
//! rejects.

use anth_proto::SecretString;

use crate::auth::password;
use crate::auth::session::AuthService;
use crate::cli::{AuthSetPasswordArgs, AuthToggleArgs, ConfigSetArgs};
use crate::config::Config;

/// `auth enable` / `auth enable --disable`.
pub fn toggle_auth(mut config: Config, args: &AuthToggleArgs) -> Result<Config, String> {
    config.auth.enabled = !args.disable;
    if args.disable {
        // Turning the login off keeps the credential so turning it back on does
        // not force a new password, but drops every live session through the
        // new secret on the next start.
        println!("Panel authentication disabled.");
        return Ok(config);
    }

    if !config.auth.is_configured() {
        // Enabling without a credential would leave an open panel that claims to
        // be locked. Generate one and say so.
        config.bootstrap_auth();
        if let Some(password) = config.take_bootstrap_password() {
            println!("Panel authentication enabled.");
            println!("Username: {}", config.auth.username);
            println!("Password: {password}");
            println!("This password is shown once and is stored only as a hash.");
        }
    } else {
        println!("Panel authentication enabled.");
    }
    Ok(config)
}

/// `auth set-password`.
pub fn set_password(mut config: Config, password: &str) -> Result<Config, String> {
    if password.chars().count() < 8 {
        return Err("the password must be at least 8 characters".to_string());
    }
    let hash = password::hash_password_with_iterations(password, config.auth.iterations.max(1000))?;
    config.auth.password_hash = hash;
    // A new password must not leave a token signed with the old secret valid.
    config.auth.jwt_secret = String::new();
    config.bootstrap_auth();
    config.auth.enabled = true;
    println!("Password updated for account '{}'.", config.auth.username);
    println!("Restart the panel for the change to take effect.");
    Ok(config)
}

/// `auth status`.
pub fn report_status(config: &Config) {
    let auth = AuthService::from_config(&config.auth);
    println!("enabled:      {}", config.auth.enabled);
    println!("username:     {}", config.auth.username);
    println!("password set: {}", !config.auth.password_hash.is_empty());
    println!("sessions:     {}", auth.session_count());
    if config.auth.enabled && !config.auth.is_configured() {
        println!("warning: the login is enabled but no password is set");
    }
}

/// `config set KEY VALUE`.
///
/// The value is parsed as TOML, so `true`, `42` and `"text"` mean what they say
/// in the file. Every accepted key is listed here on purpose: a free-form
/// `toml_edit` would let a typo break the file in a way the next start only
/// reports as a parse error.
pub fn set_key(config: &mut Config, args: &ConfigSetArgs) -> Result<(), String> {
    let value = args.value.trim();
    match args.key.as_str() {
        "socket_path" => config.socket_path = parse_path("socket_path", value)?,
        "sudo" => config.sudo = parse_bool(value)?,
        "terminal.shell" => config.terminal.shell = parse_string(value)?,
        "terminal.max_sessions" => config.terminal.max_sessions = parse_usize(value)?,
        "process.control" => config.process.control = parse_bool(value)?,
        "network.control" => config.network.control = parse_bool(value)?,
        "firewall.control" => config.firewall.control = parse_bool(value)?,
        "volumes.control" => config.volumes.control = parse_bool(value)?,
        "volumes.mount_root" => {
            config.volumes.mount_root = parse_path("volumes.mount_root", value)?
        }
        "accounts.control" => config.accounts.control = parse_bool(value)?,
        "accounts.manage_passwords" => config.accounts.manage_passwords = parse_bool(value)?,
        "accounts.allow_system" => config.accounts.allow_system = parse_bool(value)?,
        "accounts.allow_privileged_groups" => {
            config.accounts.allow_privileged_groups = parse_bool(value)?
        }
        "accounts.home_root" => {
            config.accounts.home_root = parse_path("accounts.home_root", value)?
        }
        "accounts.default_shell" => config.accounts.default_shell = parse_string(value)?,
        "files.root" => config.files.root = parse_path("files.root", value)?,
        "files.write" => config.files.write = parse_bool(value)?,
        "files.max_entries" => config.files.max_entries = parse_usize(value)?,
        "files.max_preview_bytes" => config.files.max_preview_bytes = parse_u64(value)?,
        "files.max_upload_bytes" => config.files.max_upload_bytes = parse_u64(value)?,
        "auth.enabled" => config.auth.enabled = parse_bool(value)?,
        "auth.username" => config.auth.username = parse_string(value)?,
        "auth.access_token_secs" => config.auth.access_token_secs = parse_u64(value)?,
        "auth.refresh_token_secs" => config.auth.refresh_token_secs = parse_u64(value)?,
        "auth.iterations" => {
            let iterations = parse_u64(value)?;
            let iterations = u32::try_from(iterations)
                .map_err(|_| "iterations must fit in 32 bits".to_string())?;
            if iterations == 0 {
                return Err("iterations must be positive".to_string());
            }
            config.auth.iterations = iterations;
        }
        "auth.cookie_secure" => config.auth.cookie_secure = parse_bool(value)?,
        "auth.password_hash" => {
            return Err(
                "use `anth-panel auth set-password` instead of editing the hash".to_string(),
            );
        }
        other => return Err(format!("unknown configuration key: {other}")),
    }
    Ok(())
}

fn parse_bool(value: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        other => Err(format!("expected true or false, got {other}")),
    }
}

fn parse_string(value: &str) -> Result<String, String> {
    let unquoted = if value.len() >= 2 && value.starts_with('"') && value.ends_with('"') {
        &value[1..value.len() - 1]
    } else {
        value
    };
    if unquoted.is_empty() {
        return Err("expected a non-empty string".to_string());
    }
    Ok(unquoted.to_string())
}

/// A path key must stay absolute, because a relative one is resolved against
/// the daemon's working directory and silently points somewhere unexpected.
fn parse_path(key: &str, value: &str) -> Result<String, String> {
    let path = parse_string(value)?;
    if !path.starts_with('/') {
        return Err(format!("{key} must be an absolute path, got {path}"));
    }
    Ok(path)
}

fn parse_usize(value: &str) -> Result<usize, String> {
    value
        .parse()
        .map_err(|_| format!("expected a non-negative integer, got {value}"))
}

fn parse_u64(value: &str) -> Result<u64, String> {
    value
        .parse()
        .map_err(|_| format!("expected a non-negative integer, got {value}"))
}

/// Reads a password for `auth set-password`.
///
/// Reuses the terminal helper the account subcommand already uses, so echo is
/// off and the password never reaches an argument vector.
pub fn read_password(args: &AuthSetPasswordArgs) -> Result<SecretString, String> {
    crate::password_prompt::read_password(args.stdin)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(key: &str, value: &str) -> ConfigSetArgs {
        ConfigSetArgs {
            key: key.to_string(),
            value: value.to_string(),
        }
    }

    #[test]
    fn a_boolean_key_is_set() {
        let mut config = Config::default();
        set_key(&mut config, &args("files.write", "true")).unwrap();
        assert!(config.files.write);
    }

    #[test]
    fn a_string_key_accepts_quoted_and_bare_values() {
        let mut config = Config::default();
        set_key(&mut config, &args("files.root", "/srv")).unwrap();
        assert_eq!(config.files.root, "/srv");
        set_key(&mut config, &args("files.root", "\"/var\"")).unwrap();
        assert_eq!(config.files.root, "/var");
    }

    #[test]
    fn an_unknown_key_is_refused() {
        let mut config = Config::default();
        let error = set_key(&mut config, &args("files.nope", "1")).unwrap_err();
        assert!(error.contains("unknown configuration key"));
    }

    #[test]
    fn the_password_hash_cannot_be_edited_by_hand() {
        let mut config = Config::default();
        let error = set_key(&mut config, &args("auth.password_hash", "x")).unwrap_err();
        assert!(error.contains("auth set-password"));
    }

    #[test]
    fn a_relative_path_is_refused() {
        let mut config = Config::default();
        let root = config.files.root.clone();
        let error = set_key(&mut config, &args("files.root", "srv")).unwrap_err();
        assert!(error.contains("absolute"));
        assert_eq!(config.files.root, root, "a rejected value must not stick");
    }

    #[test]
    fn a_mistyped_boolean_is_refused() {
        let mut config = Config::default();
        assert!(set_key(&mut config, &args("files.write", "yes")).is_err());
    }

    #[test]
    fn zero_iterations_are_refused() {
        let mut config = Config::default();
        assert!(set_key(&mut config, &args("auth.iterations", "0")).is_err());
    }

    #[test]
    fn enabling_without_a_credential_bootstraps_one() {
        let config = Config::default();
        let enabled = toggle_auth(config, &AuthToggleArgs { disable: false }).unwrap();
        assert!(enabled.auth.enabled);
        assert!(enabled.auth.is_configured());
    }

    #[test]
    fn disabling_keeps_the_credential() {
        let mut config = Config::default();
        config.bootstrap_auth();
        let hash = config.auth.password_hash.clone();
        let disabled = toggle_auth(config, &AuthToggleArgs { disable: true }).unwrap();
        assert!(!disabled.auth.enabled);
        assert_eq!(disabled.auth.password_hash, hash);
    }

    #[test]
    fn a_short_password_is_refused() {
        let config = Config::default();
        assert!(set_password(config, "short").is_err());
    }

    #[test]
    fn a_new_password_replaces_the_hash_and_the_secret() {
        let mut config = Config::default();
        config.bootstrap_auth();
        let old_hash = config.auth.password_hash.clone();
        let old_secret = config.auth.jwt_secret.clone();
        config.auth.iterations = password::DEV_ITERATIONS;

        let updated = set_password(config, "a-longer-password").unwrap();
        assert_ne!(updated.auth.password_hash, old_hash);
        assert_ne!(updated.auth.jwt_secret, old_secret);
        assert!(password::verify_password(
            "a-longer-password",
            &updated.auth.password_hash
        ));
    }
}
