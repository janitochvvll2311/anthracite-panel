//! Where the daemon looks for its socket, its bundles and its own settings.
//!
//! Every default is relative to the working directory, so a checkout run with
//! `cargo run` needs no flags and no environment: `./anth.sock`,
//! `./plugins`, `./config.toml`. An installed service overrides them, and the
//! order is the same for each — flag, then environment, then the config file,
//! then the relative default — so there is one rule to remember rather than
//! three.
//!
//! A config file that will not parse is refused rather than ignored. It holds
//! settings an operator wrote down; answering a parse error with defaults would
//! silently run with something else.
//!
//! Resolution is a pure function of the flags, an environment accessor and the
//! parsed file. The caller reads the real environment, so a test asserts the
//! order without depending on the machine it runs on.

use std::path::{Path, PathBuf};

use anth_wire::WireError;

/// The settings file, read from the working directory unless told otherwise.
pub const DEFAULT_CONFIG_FILE: &str = "config.toml";

/// The socket, beside the checkout that runs it.
pub const DEFAULT_SOCKET: &str = "anth.sock";

/// The bundle directory, so `./plugins/<id>/` is what a daemon started here
/// reads.
pub const DEFAULT_PLUGINS: &str = "plugins";

/// The two paths that decide where the daemon finds its things.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub socket: PathBuf,
    pub plugins: PathBuf,
}

/// What `config.toml` may set. An absent key falls through to the default.
#[derive(Debug, Default, serde::Deserialize)]
struct File {
    socket: Option<String>,
    plugins: Option<String>,
}

/// Resolves the socket and the bundle directory.
///
/// `env` is passed rather than read, so the order can be tested against a
/// supply the test controls.
pub fn resolve(
    socket_flag: Option<&str>,
    plugins_flag: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
    file: Option<&Path>,
) -> Result<Paths, WireError> {
    let settings = match file {
        Some(path) => load(path)?,
        None => File::default(),
    };

    let pick = |flag: Option<&str>, name: &str, from_file: Option<&str>, fallback: &str| {
        flag.map(PathBuf::from)
            .or_else(|| {
                env(name)
                    .filter(|text| !text.trim().is_empty())
                    .map(PathBuf::from)
            })
            .or_else(|| from_file.map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from(fallback))
    };

    Ok(Paths {
        socket: pick(
            socket_flag,
            "ANTH_SOCKET",
            settings.socket.as_deref(),
            DEFAULT_SOCKET,
        ),
        plugins: pick(
            plugins_flag,
            "ANTH_PLUGINS",
            settings.plugins.as_deref(),
            DEFAULT_PLUGINS,
        ),
    })
}

/// The config file to read: `--config`, then `$ANTH_CONFIG`, then `./config.toml`.
pub fn path(flag: Option<&str>, env: &dyn Fn(&str) -> Option<String>) -> PathBuf {
    flag.map(PathBuf::from)
        .or_else(|| {
            env("ANTH_CONFIG")
                .filter(|text| !text.trim().is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG_FILE))
}

/// Fills in the environment from the process, for the caller in `main`.
pub fn environment(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// Reads one settings file.
///
/// A missing file is the default settings, because that is the state of a fresh
/// checkout: nothing was written down, so nothing overrides the defaults. A file
/// that exists and does not parse is an error, because it was written on purpose
/// and running without it would discard an operator's decision.
fn load(path: &Path) -> Result<File, WireError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(File::default()),
        Err(error) => {
            return Err(WireError::internal(format!(
                "reading {}: {error}",
                path.display()
            )));
        }
    };
    toml::from_str(&text)
        .map_err(|error| WireError::invalid(format!("{} will not parse: {error}", path.display())))
}
