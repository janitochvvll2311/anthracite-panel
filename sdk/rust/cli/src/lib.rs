//! The `anth` client.
//!
//! The CLI is thin on purpose: it speaks HTTP to a running engine, and when no
//! engine is running it asks the engine binary to validate a configuration. It
//! does not link the daemon, so the client stays small and the validation keeps
//! one implementation (§8.5).
//!
//! `doctor` is the offline half of the pair: `status` asks "is it healthy now?",
//! `doctor` asks "would it boot?" without a daemon (§7.9).

use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anth_contract::Error;

pub const USAGE: &str = "\
usage: anth <command> [options]

commands:
  doctor [--config <path>] [--debug|--release]
                        validate a configuration without a daemon

options:
  -h, --help            show this message
  -V, --version         show the version";

/// The engine binary this client runs for offline checks. Overridable so a
/// development build and a distribution can point somewhere other than the
/// directory next to `anth`.
const ENGINE_ENV: &str = "ANTH_ENGINE";
const ENGINE_BINARY: &str = "anth-engine";

/// Which set of defaults the configuration is validated against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Debug,
    Release,
}

/// What the command line asked for, fully parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    Doctor {
        config: Option<PathBuf>,
        profile: Option<Profile>,
    },
    Help,
    Version,
}

/// Parses arguments without touching the filesystem.
///
/// Flags are compared as text, but a path is kept as [`OsString`]: a state
/// directory with non-UTF-8 bytes is unusual, not invalid.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Invocation, Error> {
    let mut args = args.into_iter();
    let Some(command) = args.next() else {
        return Ok(Invocation::Help);
    };
    match as_text(&command)? {
        "doctor" => parse_doctor(args),
        "help" | "--help" | "-h" => Ok(Invocation::Help),
        "--version" | "-V" | "version" => Ok(Invocation::Version),
        other => Err(usage(format!("`{other}` is not a command"))),
    }
}

fn as_text(arg: &OsString) -> Result<&str, Error> {
    arg.to_str()
        .ok_or_else(|| usage("an option is not valid UTF-8".to_string()))
}

fn parse_doctor(args: impl Iterator<Item = OsString>) -> Result<Invocation, Error> {
    let mut config = None;
    let mut profile = None;
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match as_text(&arg)? {
            "--config" => {
                let path = args
                    .next()
                    .ok_or_else(|| usage("`--config` needs a path".to_string()))?;
                config = Some(PathBuf::from(path));
            }
            "--debug" => profile = set_profile(profile, Profile::Debug)?,
            "--release" => profile = set_profile(profile, Profile::Release)?,
            "--help" | "-h" => return Ok(Invocation::Help),
            other => return Err(usage(format!("`{other}` is not a doctor option"))),
        }
    }
    Ok(Invocation::Doctor { config, profile })
}

fn set_profile(current: Option<Profile>, next: Profile) -> Result<Option<Profile>, Error> {
    match current {
        Some(previous) if previous != next => Err(usage(
            "`--debug` and `--release` cannot be used together".to_string(),
        )),
        _ => Ok(Some(next)),
    }
}

fn usage(message: String) -> Error {
    Error::invalid(format!("{message}\n\n{USAGE}"))
}

/// Runs the invocation and returns the process exit code.
pub fn execute(invocation: Invocation) -> Result<u8, Error> {
    match invocation {
        Invocation::Help => {
            println!("{USAGE}");
            Ok(0)
        }
        Invocation::Version => {
            println!("anth {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Invocation::Doctor { config, profile } => doctor(config, profile),
    }
}

/// Asks the engine to validate without booting it.
///
/// The engine does the reading and the deciding; the CLI forwards the flags,
/// inherits the terminal and returns the engine's exit code unchanged. A
/// wrapper that reinterprets the result would be a second opinion, and there is
/// only one check.
fn doctor(config: Option<PathBuf>, profile: Option<Profile>) -> Result<u8, Error> {
    let engine = locate_engine()?;
    let mut command = Command::new(&engine);
    command.arg("--check");
    if let Some(config) = config {
        command.arg("--config").arg(config);
    }
    match profile {
        Some(Profile::Debug) => {
            command.arg("--debug");
        }
        Some(Profile::Release) => {
            command.arg("--release");
        }
        None => {}
    }
    let status = command
        .status()
        .map_err(|error| Error::invalid(format!("cannot run {}: {error}", engine.display())))?;
    Ok(status.code().unwrap_or(1) as u8)
}

fn locate_engine() -> Result<PathBuf, Error> {
    locate(
        env::var_os(ENGINE_ENV),
        env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf)),
        env::var_os("PATH"),
    )
}

/// The search order: an explicit override, then the directory `anth` lives in
/// (both binaries come from the same build), then `PATH`.
fn locate(
    override_path: Option<OsString>,
    exe_dir: Option<PathBuf>,
    path: Option<OsString>,
) -> Result<PathBuf, Error> {
    if let Some(path) = override_path {
        let path = PathBuf::from(path);
        if !path.is_file() {
            return Err(Error::not_found(format!(
                "{ENGINE_ENV} points to {}, which is not a file",
                path.display()
            )));
        }
        if !is_executable(&path) {
            return Err(Error::invalid(format!(
                "{ENGINE_ENV} points to {}, which is not executable",
                path.display()
            )));
        }
        return Ok(path);
    }
    if let Some(candidate) = exe_dir.map(|dir| dir.join(ENGINE_BINARY)) {
        if is_executable(&candidate) {
            return Ok(candidate);
        }
    }
    if let Some(path) = path {
        for dir in env::split_paths(&path) {
            let candidate = dir.join(ENGINE_BINARY);
            if is_executable(&candidate) {
                return Ok(candidate);
            }
        }
    }
    Err(Error::not_found(format!(
        "no {ENGINE_BINARY} found: put it next to `anth`, on PATH, or set {ENGINE_ENV}"
    )))
}

/// A candidate that exists but cannot run would fail later with a confusing
/// permission error; the search skips it and the override says so plainly.
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.metadata()
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::PermissionsExt;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("anth-cli-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    /// An executable stand-in: `locate` only checks the mode.
    fn engine_file(path: &Path) {
        fs::write(path, b"").expect("write");
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod");
    }

    #[test]
    fn no_arguments_shows_the_usage() {
        assert_eq!(parse(args(&[])).expect("parse"), Invocation::Help);
    }

    #[test]
    fn version_is_recognised() {
        for flag in ["--version", "-V", "version"] {
            assert_eq!(parse(args(&[flag])).expect("parse"), Invocation::Version);
        }
    }

    #[test]
    fn an_unknown_command_says_so_and_shows_the_usage() {
        let error = parse(args(&["nope"])).expect_err("unknown");
        assert!(error.message().contains("not a command"), "{error}");
        assert!(error.message().contains("usage: anth"), "{error}");
    }

    #[test]
    fn doctor_takes_a_config_and_a_profile() {
        let parsed =
            parse(args(&["doctor", "--config", "/etc/anth.toml", "--debug"])).expect("parse");
        assert_eq!(
            parsed,
            Invocation::Doctor {
                config: Some(PathBuf::from("/etc/anth.toml")),
                profile: Some(Profile::Debug),
            }
        );
    }

    #[test]
    fn doctor_without_flags_lets_the_engine_choose() {
        assert_eq!(
            parse(args(&["doctor"])).expect("parse"),
            Invocation::Doctor {
                config: None,
                profile: None,
            }
        );
    }

    #[test]
    fn doctor_refuses_two_profiles() {
        let error = parse(args(&["doctor", "--debug", "--release"])).expect_err("profiles");
        assert!(
            error.message().contains("cannot be used together"),
            "{error}"
        );
    }

    #[test]
    fn doctor_refuses_an_unknown_option() {
        assert!(parse(args(&["doctor", "--all"])).is_err());
    }

    #[test]
    fn config_needs_a_value() {
        assert!(parse(args(&["doctor", "--config"])).is_err());
    }

    #[test]
    fn a_non_utf8_config_path_is_kept_as_bytes() {
        let mut path = b"/tmp/".to_vec();
        path.push(0xff);
        path.extend_from_slice(b"/anth.toml");
        let parsed = parse(vec![
            OsString::from("doctor"),
            OsString::from("--config"),
            OsString::from_vec(path.clone()),
        ])
        .expect("parse");
        assert_eq!(
            parsed,
            Invocation::Doctor {
                config: Some(PathBuf::from(OsString::from_vec(path))),
                profile: None,
            }
        );
    }

    #[test]
    fn a_non_utf8_option_is_refused_instead_of_panicking() {
        let mut flag = b"--".to_vec();
        flag.push(0xff);
        let error =
            parse(vec![OsString::from("doctor"), OsString::from_vec(flag)]).expect_err("non-utf8");
        assert!(error.message().contains("UTF-8"), "{error}");
    }

    #[test]
    fn the_override_wins_over_the_sibling() {
        let dir = scratch("override");
        let engine = dir.join("custom-engine");
        engine_file(&engine);
        let sibling = dir.join(ENGINE_BINARY);
        engine_file(&sibling);
        let found = locate(Some(engine.clone().into_os_string()), Some(dir), None).expect("find");
        assert_eq!(found, engine);
    }

    #[test]
    fn the_sibling_is_found_without_an_override() {
        let dir = scratch("sibling");
        let sibling = dir.join(ENGINE_BINARY);
        engine_file(&sibling);
        let found = locate(None, Some(dir), None).expect("find");
        assert_eq!(found, sibling);
    }

    #[test]
    fn a_path_entry_with_the_engine_is_used() {
        let dir = scratch("path");
        let engine = dir.join(ENGINE_BINARY);
        engine_file(&engine);
        let found = locate(None, None, Some(dir.into_os_string())).expect("find");
        assert_eq!(found, engine);
    }

    #[test]
    fn a_non_executable_candidate_is_skipped_and_the_next_one_wins() {
        let first = scratch("skip-first");
        let second = scratch("skip-second");
        fs::write(first.join(ENGINE_BINARY), b"").expect("write");
        let engine = second.join(ENGINE_BINARY);
        engine_file(&engine);
        let found = locate(None, Some(first), Some(second.into_os_string())).expect("find");
        assert_eq!(found, engine);
    }

    #[test]
    fn a_missing_override_is_reported_as_not_found() {
        let error = locate(Some("/nonexistent/engine".into()), None, None).expect_err("missing");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
        assert!(error.message().contains(ENGINE_ENV), "{error}");
    }

    #[test]
    fn a_non_executable_override_is_reported_as_invalid() {
        let dir = scratch("not-executable");
        let engine = dir.join("engine");
        fs::write(&engine, b"").expect("write");
        let error = locate(Some(engine.into_os_string()), None, None).expect_err("not executable");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
        assert!(error.message().contains("not executable"), "{error}");
    }

    #[test]
    fn no_engine_anywhere_is_reported_as_not_found() {
        let dir = scratch("nowhere");
        let error =
            locate(None, Some(dir.clone()), Some(dir.into_os_string())).expect_err("missing");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
        assert!(error.message().contains("no anth-engine found"), "{error}");
    }
}
