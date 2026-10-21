use serde::{Deserialize, Serialize};
use std::{error::Error, path::Path};

pub const DEFAULT_CONFIG_PATH: &str = "/etc/anth/config.toml";
pub const DEFAULT_SOCKET_PATH: &str = "/var/tmp/anth.sock";

/// Panel authentication settings.
///
/// The password is stored only as a PBKDF2 digest. `enabled` exists so tests and
/// a deliberately open loopback deployment can turn the whole layer off; the
/// bootstrap in `Config::load_or_create` turns it on and creates `admin` the
/// first time, printing the generated password once.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct AuthConfig {
    /// Master switch. When false the API serves every route unauthenticated.
    pub enabled: bool,
    /// Panel account name.
    pub username: String,
    /// `pbkdf2-sha256$<iterations>$<salt>$<digest>`, never the password.
    pub password_hash: String,
    /// HS256 signing key. Generated on bootstrap and never printed.
    pub jwt_secret: String,
    /// Lifetime of an access token.
    pub access_token_secs: u64,
    /// Lifetime of a refresh token, which also bounds one login session.
    pub refresh_token_secs: u64,
    /// PBKDF2 cost for new hashes.
    pub iterations: u32,
    /// Set the `Secure` flag on the auth cookies. A loopback deployment over
    /// plain HTTP needs it off; anything behind TLS needs it on.
    pub cookie_secure: bool,
    /// One-time bootstrap password. Never serialized: it exists so the server
    /// can print it after the first start and is dropped immediately after.
    #[serde(skip)]
    pub bootstrap_password: Option<String>,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            username: "admin".to_string(),
            password_hash: String::new(),
            jwt_secret: String::new(),
            access_token_secs: 15 * 60,
            refresh_token_secs: 12 * 60 * 60,
            iterations: crate::auth::password::default_iterations(),
            cookie_secure: false,
            bootstrap_password: None,
        }
    }
}

impl AuthConfig {
    /// Whether a usable password hash and signing key are present.
    pub fn is_configured(&self) -> bool {
        !self.password_hash.is_empty() && !self.jwt_secret.is_empty()
    }
}

/// Web terminal settings. The token is reserved for the pending auth layer.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalConfig {
    /// Program to spawn for each session. `"auto"` uses `$SHELL` when it points
    /// at an existing file, then falls back to `/bin/sh`.
    pub shell: String,
    /// Shared token reserved for the authentication layer. Not enforced yet.
    pub token: String,
    /// Maximum number of concurrent sessions across the daemon.
    pub max_sessions: usize,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            shell: "auto".to_string(),
            // Reserved for the authentication layer. A terminal is remote code
            // execution, so the token stays empty until that layer exists.
            token: String::new(),
            max_sessions: 4,
        }
    }
}

impl TerminalConfig {
    /// Resolves the configured shell, following `$SHELL` (then the account's
    /// login shell) for `"auto"`.
    pub fn resolved_shell(&self) -> String {
        let login = login_shell();
        resolve_shell(
            &self.shell,
            std::env::var("SHELL").ok().as_deref(),
            login.as_deref(),
        )
    }
}

fn resolve_shell(configured: &str, environment: Option<&str>, login: Option<&str>) -> String {
    if configured != "auto" && !configured.is_empty() {
        return configured.to_string();
    }

    environment
        .filter(|shell| !shell.is_empty() && Path::new(shell).is_file())
        .or_else(|| login.filter(|shell| !shell.is_empty() && Path::new(shell).is_file()))
        .unwrap_or("/bin/sh")
        .to_string()
}

/// The current account's login shell from `/etc/passwd`, if it is usable.
fn login_shell() -> Option<String> {
    let uid = unsafe { libc::getuid() };
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    shell_from_passwd(&passwd, uid)
}

fn shell_from_passwd(text: &str, uid: u32) -> Option<String> {
    text.lines().find_map(|line| {
        let fields: Vec<&str> = line.split(':').collect();
        if fields.len() < 7 || fields[2].parse::<u32>().ok() != Some(uid) {
            return None;
        }

        let shell = fields[6];
        if shell.is_empty() || !Path::new(shell).is_file() {
            return None;
        }
        Some(shell.to_string())
    })
}

/// Process monitoring and control settings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ProcessConfig {
    /// Whether the daemon accepts signal requests. Off by default: killing a
    /// process is destructive and the panel has no authentication yet.
    pub control: bool,
    /// Whether the daemon accepts spawn requests. Separate from `control`
    /// because starting a process and killing one are different risks.
    pub spawn: bool,
    /// Absolute programs `spawn` may start. An empty list denies every spawn,
    /// so enabling `spawn` alone opens nothing.
    pub spawn_allow: Vec<String>,
}

/// Network interface monitoring and control settings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct NetworkConfig {
    /// Whether the daemon accepts up/down requests. Off by default: bringing an
    /// interface down can disconnect the host, and the panel has no
    /// authentication yet.
    pub control: bool,
    /// Whether the daemon accepts create, edit, rename and delete. Separate
    /// from `control` because the panel offers no rollback: creating an
    /// interface is recoverable, but the two switches should not be forced on
    /// an operator who only wanted up/down.
    pub manage: bool,
    /// Old per-section sudo flag. The global top-level `sudo` replaces it;
    /// this is accepted so earlier configs keep loading and is never written.
    #[serde(default, skip_serializing, rename = "sudo")]
    _legacy_sudo: bool,
}

/// Firewall management settings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default, deny_unknown_fields)]
pub struct FirewallConfig {
    /// Whether the daemon accepts rule changes. Off by default: the panel has
    /// no authentication yet and firewall changes can disconnect the host.
    pub control: bool,

    /// Old per-section sudo flag. The global top-level `sudo` replaces it;
    /// this is accepted so earlier configs keep loading and is never written.
    #[serde(default, skip_serializing, rename = "sudo")]
    _legacy_sudo: bool,
    /// Legacy password key. Passwords are no longer stored or used; the key is
    /// accepted so old configs keep loading and is never written back.
    #[serde(default, skip_serializing, rename = "sudo_password")]
    _legacy_sudo_password: Option<String>,
}

/// Filesystem explorer settings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct FilesConfig {
    /// Host directory exposed as `/`. Must be absolute and exist when the
    /// daemon starts.
    pub root: String,
    /// Whether the daemon accepts mkdir, rename, delete and upload requests.
    /// Off by default: the panel has no authentication yet.
    pub write: bool,
    /// Maximum number of entries returned by one listing.
    pub max_entries: usize,
    /// Maximum bytes returned by the inline content endpoint.
    pub max_preview_bytes: u64,
    /// Maximum size of one upload.
    pub max_upload_bytes: u64,
    /// Whether the daemon moves deletions to the freedesktop trash. Off by
    /// default: it writes outside `files.root`, so it is opt-in like `write`.
    pub trash: bool,
    /// Retention ceiling for the trash in bytes. `0` keeps everything until the
    /// trash is emptied by hand.
    pub max_trash_bytes: u64,
    /// Ceiling on the uncompressed bytes one archive may expand to. This is the
    /// zip-bomb guard, not a size limit on the archive itself.
    pub max_archive_bytes: u64,
}

impl Default for FilesConfig {
    fn default() -> Self {
        Self {
            root: "/".to_string(),
            write: false,
            max_entries: 5000,
            max_preview_bytes: 1024 * 1024,
            max_upload_bytes: 64 * 1024 * 1024,
            trash: false,
            max_trash_bytes: 1024 * 1024 * 1024,
            max_archive_bytes: 4 * 1024 * 1024 * 1024,
        }
    }
}

/// Storage volume management settings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct VolumesConfig {
    /// Whether the daemon accepts mount and unmount requests. Off by default:
    /// both change what the host exposes, and the panel has no authentication.
    pub control: bool,
    /// Mount targets must resolve inside this directory. Empty disables the
    /// check and is the documented escape hatch.
    pub mount_root: String,
}

impl Default for VolumesConfig {
    fn default() -> Self {
        Self {
            control: false,
            mount_root: "/mnt".to_string(),
        }
    }
}

/// Account management settings.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct AccountsConfig {
    /// Whether the daemon accepts create, modify and delete requests for local
    /// users and groups. Off by default: this is the first operation that can
    /// hand out root, and the panel has no authentication.
    pub control: bool,
    /// Whether the daemon accepts password changes. A second switch on purpose:
    /// inventory and structure are recoverable, a changed password is not.
    pub manage_passwords: bool,
    /// Allow changes to accounts below `UID_MIN`.
    pub allow_system: bool,
    /// Allow changes to `sudo`, `wheel`, `admin`, `root`, `docker` and `lxd`.
    /// The last two are root-equivalent, which is why they are on this list.
    pub allow_privileged_groups: bool,
    /// New home directories must resolve below this path.
    pub home_root: String,
    /// Login shells a user may be given. Empty means "read `/etc/shells`".
    pub shells: Vec<String>,
    /// Shell used when a create request does not name one.
    pub default_shell: String,
}

impl Default for AccountsConfig {
    fn default() -> Self {
        Self {
            control: false,
            manage_passwords: false,
            allow_system: false,
            allow_privileged_groups: false,
            home_root: "/home".to_string(),
            shells: Vec::new(),
            default_shell: "/bin/sh".to_string(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub socket_path: String,
    /// Run privileged control commands through `sudo -n`. This single switch
    /// covers both `ip link set` and `nft`; each path falls back to its direct
    /// mechanism when sudo cannot elevate.
    pub sudo: bool,
    pub terminal: TerminalConfig,
    pub process: ProcessConfig,
    pub network: NetworkConfig,
    pub firewall: FirewallConfig,
    pub volumes: VolumesConfig,
    pub accounts: AccountsConfig,
    pub files: FilesConfig,
    pub auth: AuthConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            socket_path: DEFAULT_SOCKET_PATH.to_string(),
            sudo: false,
            terminal: TerminalConfig::default(),
            process: ProcessConfig::default(),
            network: NetworkConfig::default(),
            firewall: FirewallConfig::default(),
            volumes: VolumesConfig::default(),
            accounts: AccountsConfig::default(),
            files: FilesConfig::default(),
            auth: AuthConfig::default(),
        }
    }
}

impl Config {
    /// Defaults that make a debug build convenient for local development.
    ///
    /// Production defaults stay safe: process control, network control, firewall
    /// control, volume control and file writes are off unless an existing config
    /// file explicitly enables them. Development also sets the global `sudo` flag so `ip link
    /// set` and `nft` work when the daemon runs unprivileged.
    pub fn dev_default() -> Self {
        let mut config = Self {
            sudo: true,
            ..Self::default()
        };
        config.process.control = true;
        // Spawn is enabled only with a fixed, harmless allowlist. An empty list
        // would deny everything and make the feature untestable locally; a wide
        // list would make a debug build a shell launcher.
        config.process.spawn = true;
        config.process.spawn_allow = vec![
            "/bin/echo".to_string(),
            "/bin/sleep".to_string(),
            "/usr/bin/uptime".to_string(),
        ];
        config.network.control = true;
        config.network.manage = true;
        config.firewall.control = true;
        config.volumes.control = true;
        config.accounts.control = true;
        // Deliberately not enabled even here: a debug build must not be able to
        // change a password by accident.
        config.files.write = true;
        config
    }

    /// Defaults used when a server subcommand has to create a missing config.
    pub fn server_default() -> Self {
        if cfg!(debug_assertions) {
            Self::dev_default()
        } else {
            Self::default()
        }
    }
}

impl Config {
    pub fn from_toml(text: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        match toml::from_str(text) {
            Ok(config) => Ok(config),
            Err(e) => {
                eprintln!("Error parsing TOML: {e}");
                Err(Box::new(e).into())
            }
        }
    }

    pub fn to_toml(&self) -> Result<String, Box<dyn Error + Send + Sync>> {
        match toml::to_string_pretty(self) {
            Ok(body) => Ok(body),
            Err(e) => {
                eprintln!("Error serializing config to TOML: {e}");
                Err(Box::new(e).into())
            }
        }
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();

        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_toml(&text)
                .map_err(|source| format!("Error parsing config at {}: {source}", path.display())),
            Err(source) => Err(format!(
                "Error reading config at {}: {source}",
                path.display()
            )),
        }
    }

    /// Loads the configuration at `path`, falling back to in-memory defaults when
    /// it cannot be read. Never touches the filesystem, so it is the right entry
    /// point for the client subcommands.
    pub fn load_or_default(path: impl AsRef<Path>) -> Self {
        Self::load(path).unwrap_or_else(|e| {
            eprintln!("Failed to load config: {e}");
            Self::default()
        })
    }

    /// Loads the configuration at `path`, or writes a default one first when it
    /// cannot be read, so a first server run bootstraps the configuration file.
    /// Debug builds bootstrap with global sudo, process control, network
    /// control, firewall control, volume control and file writes enabled;
    /// release builds keep them off. A failure to persist the default is reported but not fatal:
    /// the freshly built defaults are still usable.
    pub fn load_or_create(path: impl AsRef<Path>) -> Self {
        let mut config = match Self::load(&path) {
            Ok(config) => config,
            Err(e) => {
                eprintln!("Failed to load config: {e}");
                eprintln!("Using default config.");

                let config = Self::server_default();
                if cfg!(debug_assertions) {
                    eprintln!(
                        "Debug build: global sudo, process control, network control, firewall control, volume control and file writes are enabled in the generated config."
                    );
                }
                config
            }
        };

        if config.bootstrap_auth() {
            let printed = config.print_bootstrap_password();
            if let Err(e) = config.save(&path) {
                eprintln!("Failed to save config: {e}");
            }
            eprintln!("{printed}");
        }

        config
    }

    /// Fills in the auth section the first time a server starts.
    ///
    /// Returns `true` when something was generated, which is the caller's signal
    /// to persist and to print the one-time password. An operator who ran
    /// `auth set-password` before the first start already has a hash, so this
    /// only supplies the signing key and leaves the credential alone.
    pub fn bootstrap_auth(&mut self) -> bool {
        let mut changed = false;

        if self.auth.jwt_secret.is_empty() {
            self.auth.jwt_secret = crate::auth::jwt::random_id()
                + &crate::auth::jwt::random_id()
                + &crate::auth::jwt::random_id();
            changed = true;
        }

        if self.auth.password_hash.is_empty() {
            let password = crate::auth::password::generate_password(20);
            match crate::auth::password::hash_password_with_iterations(
                &password,
                self.auth
                    .iterations
                    .max(crate::auth::password::DEV_ITERATIONS),
            ) {
                Ok(hash) => {
                    self.auth.password_hash = hash;
                    self.auth.bootstrap_password = Some(password);
                    self.auth.enabled = true;
                    changed = true;
                }
                Err(e) => eprintln!("Failed to hash the bootstrap password: {e}"),
            }
        }

        changed
    }

    /// Takes the one-time bootstrap password, if this run generated one.
    ///
    /// It is not serialized: the config holds only the hash.
    pub fn take_bootstrap_password(&mut self) -> Option<String> {
        self.auth.bootstrap_password.take()
    }

    /// The message printed once, after the first start creates the account.
    fn print_bootstrap_password(&mut self) -> String {
        let Some(password) = self.take_bootstrap_password() else {
            return String::new();
        };
        format!(
            "\nPanel authentication is now enabled.\n\
             Username: {}\n\
             Password: {}\n\
             This password is shown once and is stored only as a hash in the config.\n\
             Change it with `anth-panel auth set-password`.\n",
            self.auth.username, password
        )
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();

        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|source| {
                format!(
                    "Error creating config directory at {}: {source}",
                    parent.display()
                )
            })?;
        }

        let body = self.to_toml().map_err(|source| {
            format!("Error serializing config at {}: {source}", path.display())
        })?;
        write_private(path, &body)
            .map_err(|source| format!("Error writing config at {}: {source}", path.display()))
    }
}

/// Writes `body` to `path` with mode `0600`.
///
/// The file holds the PBKDF2 digest and the JWT signing key. With the default
/// umask a plain `fs::write` leaves it world-readable, and any local account
/// that can read the signing key can forge an access token and skip the
/// password. The mode is set before the content is written, so the secret is
/// never briefly readable by others.
fn write_private(path: &Path, body: &str) -> std::io::Result<()> {
    use std::io::Write;

    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        // `mode` only applies when the file is created, so an existing file
        // written by an older build has to be tightened too.
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(body.as_bytes())?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_uses_auto() {
        assert_eq!(TerminalConfig::default().shell, "auto");
    }

    #[test]
    fn global_sudo_is_off_by_default_and_on_in_debug_defaults() {
        assert!(!Config::default().sudo);
        if cfg!(debug_assertions) {
            let dev = Config::dev_default();
            assert!(dev.sudo);
            let text = dev.to_toml().unwrap();
            assert!(text.contains("sudo = true"), "{text}");
        }

        let release = Config::default().to_toml().unwrap();
        assert!(release.contains("sudo = false"), "{release}");
    }

    #[test]
    fn global_sudo_is_loaded_and_legacy_section_keys_are_ignored() {
        let config = Config::from_toml(
            r#"
sudo = true

[network]
control = true
sudo = false

[firewall]
control = true
sudo = false
sudo_password = "secret"
"#,
        )
        .unwrap();

        assert!(config.sudo);
        assert!(config.network.control);
        assert!(config.firewall.control);
        let text = config.to_toml().unwrap();
        assert_eq!(text.matches("sudo").count(), 1, "{text}");
        assert!(text.contains("sudo = true"), "{text}");
        assert!(!text.contains("sudo_password"), "{text}");
    }

    #[test]
    fn legacy_section_sudo_keys_do_not_enable_the_global_flag() {
        let config = Config::from_toml(
            r#"
[network]
sudo = true

[firewall]
sudo = true
"#,
        )
        .unwrap();

        assert!(!config.sudo);
    }

    #[test]
    fn firewall_section_is_loaded_from_toml() {
        let config = Config::from_toml(
            r#"
[firewall]
control = true
"#,
        )
        .unwrap();

        assert!(config.firewall.control);
    }

    #[test]
    fn file_defaults_are_safe() {
        let files = FilesConfig::default();
        assert_eq!(files.root, "/");
        assert!(!files.write);
        assert_eq!(files.max_entries, 5000);
        assert_eq!(files.max_preview_bytes, 1024 * 1024);
        assert_eq!(files.max_upload_bytes, 64 * 1024 * 1024);
    }

    #[test]
    fn network_control_is_off_by_default_and_on_in_debug_defaults() {
        assert!(!NetworkConfig::default().control);
        if cfg!(debug_assertions) {
            assert!(Config::dev_default().network.control);
        }
    }

    #[test]
    fn network_section_is_loaded_from_toml() {
        let config = Config::from_toml(
            r#"
[network]
control = true
"#,
        )
        .unwrap();

        assert!(config.network.control);
    }

    #[test]
    fn files_section_is_loaded_from_toml() {
        let config = Config::from_toml(
            r#"
[files]
root = "/srv"
write = true
max_entries = 10
max_preview_bytes = 20
max_upload_bytes = 30
"#,
        )
        .unwrap();

        assert_eq!(config.files.root, "/srv");
        assert!(config.files.write);
        assert_eq!(config.files.max_entries, 10);
        assert_eq!(config.files.max_preview_bytes, 20);
        assert_eq!(config.files.max_upload_bytes, 30);
    }

    #[test]
    fn an_explicit_shell_wins_over_the_environment() {
        assert_eq!(
            resolve_shell("/bin/bash", Some("/bin/zsh"), Some("/bin/sh")),
            "/bin/bash".to_string()
        );
    }

    #[test]
    fn auto_uses_an_existing_environment_shell() {
        assert_eq!(
            resolve_shell("auto", Some("/bin/sh"), Some("/bin/dash")),
            "/bin/sh".to_string()
        );
    }

    #[test]
    fn auto_falls_back_to_the_login_shell_then_bin_sh() {
        assert_eq!(
            resolve_shell("auto", Some("/nonexistent/shell"), Some("/bin/sh")),
            "/bin/sh".to_string()
        );
        assert_eq!(
            resolve_shell("auto", None, Some("/bin/sh")),
            "/bin/sh".to_string()
        );
        assert_eq!(resolve_shell("auto", None, None), "/bin/sh".to_string());
        assert_eq!(resolve_shell("", Some(""), None), "/bin/sh".to_string());
    }

    #[test]
    fn passwd_entries_are_matched_by_uid() {
        let passwd = concat!(
            "root:x:0:0:root:/root:/bin/sh\n",
            "alice:x:1000:1000:Alice:/home/alice:/bin/sh\n",
            "broken:x:1001:1001:Broken:/home/broken:\n",
        );

        assert_eq!(shell_from_passwd(passwd, 0), Some("/bin/sh".to_string()));
        assert_eq!(shell_from_passwd(passwd, 1000), Some("/bin/sh".to_string()));
        assert_eq!(shell_from_passwd(passwd, 1001), None);
        assert_eq!(shell_from_passwd(passwd, 9999), None);
    }
}
