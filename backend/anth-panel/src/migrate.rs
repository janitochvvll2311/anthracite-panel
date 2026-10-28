//! One-way migration from the v0.2 settings shape.
//!
//! v0.2 kept paths, the terminal switch, the account and its signing key in one
//! `config.toml`. v0.3 splits policy (`config.toml`) from credentials
//! (`secrets.toml`) and adds the feature sections. The migration is deliberately
//! narrow:
//!
//! - known v0.2 keys are mapped by name and type;
//! - everything else stays at the v0.3 release-safe default;
//! - a dry run is the default, and `--write` is explicit;
//! - unknown keys and old sessions are reported, never silently carried.
//!
//! The result is parsed and validated before anything is written. A bad fixture
//! leaves both the source and the target untouched.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use toml::Value;

use crate::config::Config;
use crate::state;

/// The policy half of the migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationPlan {
    pub config: Config,
    /// `None` when v0.2 had no account or signing key.
    pub secrets: Option<SecretsPlan>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretsPlan {
    pub password_hash: String,
    pub signing_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Written {
    pub config: PathBuf,
    pub secrets: Option<PathBuf>,
}

/// Reads and plans one v0.2 file without writing anything.
pub fn plan(text: &str) -> Result<MigrationPlan, String> {
    let document: Value =
        toml::from_str(text).map_err(|error| format!("the v0.2 file does not parse: {error}"))?;
    let root = document
        .as_table()
        .ok_or_else(|| "the v0.2 file is not a TOML table".to_string())?;

    let mut warnings = Vec::new();
    let mut config = Config::default();

    if let Some(value) = root.get("socket").and_then(Value::as_str) {
        config.socket_path = value.to_string();
    }
    if let Some(value) = root.get("plugins").and_then(Value::as_str) {
        config.plugins.root = value.to_string();
    }
    if let Some(value) = root.get("readable").and_then(Value::as_str) {
        config.files.root = value.to_string();
    }
    if let Some(terminal) = root.get("terminal").and_then(Value::as_table) {
        if let Some(enabled) = terminal.get("enabled").and_then(Value::as_bool) {
            config.terminal.enabled = enabled;
        }
        warn_unknown(terminal, &["enabled"], "terminal", &mut warnings);
    }

    let mut secrets = None;
    if let Some(auth) = root.get("auth").and_then(Value::as_table) {
        if let Some(enabled) = auth.get("enabled").and_then(Value::as_bool) {
            config.auth.enabled = enabled;
        }
        if let Some(username) = auth.get("username").and_then(Value::as_str) {
            config.auth.username = username.to_string();
        }
        if let Some(seconds) = auth.get("access_token_secs").and_then(Value::as_integer) {
            config.auth.access_token_secs =
                u64::try_from(seconds).map_err(|_| "`auth.access_token_secs` is negative")?;
        }
        if let Some(seconds) = auth.get("refresh_token_secs").and_then(Value::as_integer) {
            config.auth.refresh_token_secs =
                u64::try_from(seconds).map_err(|_| "`auth.refresh_token_secs` is negative")?;
        }
        let password_hash = auth
            .get("password_hash")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let signing_key = auth
            .get("signing_key")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        if !signing_key.is_empty()
            && (signing_key.len() != 64 || !signing_key.chars().all(|c| c.is_ascii_hexdigit()))
        {
            return Err("`auth.signing_key` is not 64 hex characters".to_string());
        }
        if !password_hash.is_empty() || !signing_key.is_empty() {
            secrets = Some(SecretsPlan {
                password_hash,
                signing_key,
            });
        }
        warn_unknown(
            auth,
            &[
                "enabled",
                "username",
                "password_hash",
                "signing_key",
                "access_token_secs",
                "refresh_token_secs",
            ],
            "auth",
            &mut warnings,
        );
    }

    warn_unknown(
        root,
        &["socket", "plugins", "readable", "terminal", "auth"],
        "the v0.2 config",
        &mut warnings,
    );
    if root.contains_key("session") || root.contains_key("sessions") {
        warnings
            .push("v0.2 sessions are not migrated; existing browsers sign in again".to_string());
    }
    if root.contains_key("plugin_grants") {
        warnings.push(
            "v0.2 plugin grants are not migrated; reinstall the bundles and approve them again"
                .to_string(),
        );
    }

    config
        .validate()
        .map_err(|error| format!("the migrated policy is invalid: {error}"))?;
    Ok(MigrationPlan {
        config,
        secrets,
        warnings,
    })
}

fn warn_unknown(
    table: &toml::map::Map<String, Value>,
    known: &[&str],
    where_: &str,
    warnings: &mut Vec<String>,
) {
    for key in table.keys() {
        if !known.contains(&key.as_str()) {
            warnings.push(format!("ignoring `{where_}.{key}`; v0.3 has no equivalent"));
        }
    }
}

/// Writes a plan to `config_path`, with secrets beside it.
///
/// The target must not exist unless `force` is set. Both files are written
/// atomically; the secrets file is `0600`.
pub fn write(
    plan: &MigrationPlan,
    config_path: impl AsRef<Path>,
    force: bool,
) -> Result<Written, String> {
    let config_path = config_path.as_ref();
    if config_path.is_dir() {
        return Err(format!("{} is not a file path", config_path.display()));
    }
    if config_path.exists() && !force {
        return Err(format!(
            "{} already exists; pass --force to overwrite it",
            config_path.display()
        ));
    }
    let config_dir = config_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let secrets_path = config_dir.join(crate::config::SECRETS_FILE);
    if secrets_path.exists() && !force {
        return Err(format!(
            "{} already exists; pass --force to overwrite it",
            secrets_path.display()
        ));
    }

    state::write_atomic(config_path, plan.config.template().as_bytes(), 0o644)?;
    let secrets = plan.secrets.as_ref().map(|secrets| {
        let text = format!(
            "# Anthracite Panel credentials migrated from v0.2. Machine-owned.\n\
             version = 1\n\n\
             [auth]\n\
             password_hash = {:?}\n\
             signing_key = {:?}\n",
            secrets.password_hash, secrets.signing_key
        );
        state::write_atomic(&secrets_path, text.as_bytes(), 0o600).map(|()| secrets_path.clone())
    });
    let secrets = match secrets {
        Some(Ok(path)) => Some(path),
        Some(Err(error)) => {
            let _ = fs::remove_file(config_path);
            return Err(error);
        }
        None => None,
    };
    Ok(Written {
        config: config_path.to_path_buf(),
        secrets,
    })
}

/// Dry-run text: the policy only. Credentials are counted, never printed.
pub fn render(plan: &MigrationPlan) -> String {
    let mut out = plan.config.template();
    if let Some(secrets) = &plan.secrets {
        out.push_str(&format!(
            "\n# secrets.toml (0600): password hash {}, signing key {}\n",
            if secrets.password_hash.is_empty() {
                "absent"
            } else {
                "present"
            },
            if secrets.signing_key.is_empty() {
                "absent"
            } else {
                "present"
            }
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> &'static str {
        r#"
socket = "/run/anth.sock"
plugins = "/var/lib/anth/plugins"
readable = "/srv/www"
sudo = false

[terminal]
enabled = true

[auth]
enabled = true
username = "operator"
password_hash = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA"
signing_key = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"

[[session]]
digest = "dd"
"#
    }

    #[test]
    fn known_values_are_mapped_and_unknown_ones_warn() {
        let plan = plan(fixture()).expect("plan");
        assert_eq!(plan.config.socket_path, "/run/anth.sock");
        assert_eq!(plan.config.plugins.root, "/var/lib/anth/plugins");
        assert_eq!(plan.config.files.root, "/srv/www");
        assert!(plan.config.terminal.enabled);
        assert!(plan.config.auth.enabled);
        assert_eq!(plan.config.auth.username, "operator");
        let secrets = plan.secrets.as_ref().expect("secrets");
        assert!(secrets.password_hash.starts_with("$argon2id$"));
        assert_eq!(secrets.signing_key.len(), 64);
        assert!(plan.warnings.iter().any(|warning| warning.contains("sudo")));
        assert!(
            plan.warnings
                .iter()
                .any(|warning| warning.contains("sessions"))
        );
        assert!(!render(&plan).contains(&secrets.password_hash));
    }

    #[test]
    fn write_is_atomic_and_secrets_are_0600() {
        use std::os::unix::fs::PermissionsExt;

        let root = scratch("migrate");
        let config = root.join("config.toml");
        let plan = plan(fixture()).expect("plan");
        let written = write(&plan, &config, false).expect("write");
        assert_eq!(written.config, config);
        assert!(written.secrets.is_some());
        assert!(config.exists());
        assert!(root.join("secrets.toml").exists());
        let mode = fs::metadata(root.join("secrets.toml"))
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
        let reloaded = Config::load(&config).expect("reload");
        assert_eq!(reloaded.auth.username, "operator");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn an_existing_target_needs_force() {
        let root = scratch("migrate-existing");
        let config = root.join("config.toml");
        fs::write(&config, "x").expect("seed");
        let plan = plan(fixture()).expect("plan");
        let error = write(&plan, &config, false).expect_err("refused");
        assert!(error.contains("--force"));
        write(&plan, &config, true).expect("forced");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn a_bad_signing_key_stops_the_plan() {
        let text = fixture().replace(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "xyz",
        );
        assert!(plan(&text).is_err());
    }

    #[test]
    fn a_malformed_file_is_refused_without_writing() {
        assert!(plan("not = [").is_err());
    }

    fn scratch(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = PathBuf::from("/tmp/anthracite-panel/tests")
            .join(format!("{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&root).expect("scratch");
        root
    }
}
