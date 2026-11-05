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

use anth_contract::{Error, ErrorKind};

pub mod client;

pub use client::Client;

pub const USAGE: &str = "\
usage: anth [--socket <path>] <command> [options]

commands:
  doctor [--config <path>] [--debug|--release]
                        validate a configuration without a daemon
  status                health, version and the installed catalogue
  app list              list installed apps
  app show <id>         show one app
  app install <bundle>  install a bundle (zip)
  app upgrade <id> <bundle>
                        replace an installed bundle
  app enable|disable <id> [--acknowledge <token>]
  app remove <id> [--acknowledge <token>]
  app approve <id> [--accept-write|--accept-admin|--accept-host|--accept-package]
  app revoke <id> [--acknowledge <token>]

options:
  --socket <path>       engine socket; then ANTH_SOCKET, then /run/anth/engine.sock
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
    Status {
        socket: Option<PathBuf>,
    },
    App {
        socket: Option<PathBuf>,
        command: AppCommand,
    },
    Help,
    Version,
}

/// The `anth app` subcommands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppCommand {
    List,
    Show(String),
    Install(PathBuf),
    Upgrade {
        app: String,
        bundle: PathBuf,
    },
    Enable {
        app: String,
        acknowledge: Option<String>,
    },
    Disable {
        app: String,
        acknowledge: Option<String>,
    },
    Remove {
        app: String,
        acknowledge: Option<String>,
    },
    Approve {
        app: String,
        accept: Vec<String>,
    },
    Revoke {
        app: String,
        acknowledge: Option<String>,
    },
}

/// Parses arguments without touching the filesystem.
///
/// Flags are compared as text, but a path is kept as [`OsString`]: a state
/// directory with non-UTF-8 bytes is unusual, not invalid.
pub fn parse(args: impl IntoIterator<Item = OsString>) -> Result<Invocation, Error> {
    let mut args = args.into_iter().peekable();
    let mut socket = None;
    let command = loop {
        let Some(arg) = args.next() else {
            return Ok(Invocation::Help);
        };
        match as_text(&arg)? {
            "--socket" => {
                let path = args
                    .next()
                    .ok_or_else(|| usage("`--socket` needs a path".to_string()))?;
                socket = Some(PathBuf::from(path));
            }
            "--help" | "-h" => return Ok(Invocation::Help),
            "--version" | "-V" => return Ok(Invocation::Version),
            other => break other.to_string(),
        }
    };
    match command.as_str() {
        "doctor" => parse_doctor(args),
        "help" => Ok(Invocation::Help),
        "version" => Ok(Invocation::Version),
        "status" => {
            if let Some(extra) = args.next() {
                return Err(usage(format!(
                    "`{}` is not a status option",
                    as_text(&extra)?
                )));
            }
            Ok(Invocation::Status { socket })
        }
        "app" => parse_app(args, socket),
        other => Err(usage(format!("`{other}` is not a command"))),
    }
}

fn parse_app(
    args: impl Iterator<Item = OsString>,
    socket: Option<PathBuf>,
) -> Result<Invocation, Error> {
    let mut args = args.into_iter().peekable();
    let sub = args
        .next()
        .ok_or_else(|| usage("`app` needs a subcommand".to_string()))?;
    let sub = as_text(&sub)?.to_string();
    let mut acknowledge = None;
    let mut accept = Vec::new();
    let mut positional: Vec<String> = Vec::new();
    while let Some(arg) = args.next() {
        match as_text(&arg)? {
            "--acknowledge" => {
                let token = args
                    .next()
                    .ok_or_else(|| usage("`--acknowledge` needs a token".to_string()))?;
                acknowledge = Some(as_text(&token)?.to_string());
            }
            "--accept-write" => accept.push("write".to_string()),
            "--accept-admin" => accept.push("admin".to_string()),
            "--accept-host" => accept.push("host".to_string()),
            "--accept-package" => accept.push("package".to_string()),
            "--help" | "-h" => return Ok(Invocation::Help),
            value => positional.push(value.to_string()),
        }
    }
    let is_approve = sub == "approve";
    if is_approve && acknowledge.is_some() {
        return Err(usage(
            "`app approve` does not take `--acknowledge`".to_string(),
        ));
    }
    if !is_approve && !accept.is_empty() {
        return Err(usage(
            "--accept-* only applies to `app approve`".to_string(),
        ));
    }
    let command = match (sub.as_str(), positional.as_slice()) {
        ("list", []) => AppCommand::List,
        ("show", [app]) => AppCommand::Show(app.clone()),
        ("install", [bundle]) => AppCommand::Install(PathBuf::from(bundle)),
        ("upgrade", [app, bundle]) => AppCommand::Upgrade {
            app: app.clone(),
            bundle: PathBuf::from(bundle),
        },
        ("enable", [app]) => AppCommand::Enable {
            app: app.clone(),
            acknowledge,
        },
        ("disable", [app]) => AppCommand::Disable {
            app: app.clone(),
            acknowledge,
        },
        ("remove", [app]) => AppCommand::Remove {
            app: app.clone(),
            acknowledge,
        },
        ("approve", [app]) => AppCommand::Approve {
            app: app.clone(),
            accept,
        },
        ("revoke", [app]) => AppCommand::Revoke {
            app: app.clone(),
            acknowledge,
        },
        (sub, _) => {
            return Err(usage(format!("`app {sub}` does not take those arguments")));
        }
    };
    Ok(Invocation::App { socket, command })
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
        Invocation::Status { socket } => status(socket),
        Invocation::App { socket, command } => app(socket, command),
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

/// How the terminal shows a refused request: a `409` that names an
/// acknowledgment token becomes an instruction (§7.9); everything else keeps
/// the code and the message.
pub fn error_text(error: &Error) -> String {
    if error.kind() == ErrorKind::Conflict {
        if let Some(tokens) = error
            .envelope()
            .error
            .detail
            .as_ref()
            .and_then(|detail| detail["acknowledge"].as_array())
        {
            let tokens: Vec<&str> = tokens.iter().filter_map(|token| token.as_str()).collect();
            if !tokens.is_empty() {
                return format!(
                    "warning: {}\nrerun with --acknowledge {} to proceed",
                    error.message(),
                    tokens.join(" ")
                );
            }
        }
    }
    format!("{}: {}", error.kind().code(), error.message())
}

/// The socket the engine listens on: explicit, env, then the release default.
fn socket_path(explicit: Option<PathBuf>) -> PathBuf {
    explicit
        .or_else(|| env::var_os("ANTH_SOCKET").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/run/anth/engine.sock"))
}

fn client(socket: Option<PathBuf>) -> Client {
    Client::new(socket_path(socket))
}

/// Readiness, the version and the catalogue in one answer.
fn status(socket: Option<PathBuf>) -> Result<u8, Error> {
    let client = client(socket);
    let health = client.get_json("/api/health")?;
    let version = client.get_json("/api/engine/version")?;
    let apps = client.get_json("/api/engine/apps")?;
    println!(
        "engine {} (protocol {})",
        version["engine"].as_str().unwrap_or("?"),
        version["protocol"].as_u64().unwrap_or(0)
    );
    println!("health {}", health["status"].as_str().unwrap_or("?"));
    println!("apps {}", apps["apps"].as_array().map_or(0, Vec::len));
    for problem in apps["problems"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
    {
        eprintln!(
            "warning: {}: {}",
            problem["app"].as_str().unwrap_or("?"),
            problem["message"].as_str().unwrap_or("?")
        );
    }
    Ok(0)
}

fn app(socket: Option<PathBuf>, command: AppCommand) -> Result<u8, Error> {
    let client = client(socket);
    match command {
        AppCommand::List => {
            let value = client.get_json("/api/engine/apps")?;
            for app in value["apps"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    app["id"].as_str().unwrap_or("?"),
                    app["version"].as_str().unwrap_or("?"),
                    if app["enabled"].as_bool().unwrap_or(false) {
                        "enabled"
                    } else {
                        "disabled"
                    },
                    app["alias"].as_str().unwrap_or("-"),
                    app["grant_state"].as_str().unwrap_or("none")
                );
            }
            for problem in value["problems"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or(&[])
            {
                eprintln!(
                    "warning: {}: {}",
                    problem["app"].as_str().unwrap_or("?"),
                    problem["message"].as_str().unwrap_or("?")
                );
            }
            Ok(0)
        }
        AppCommand::Show(app) => {
            let value = client.get_json(&format!("/api/engine/apps/{app}"))?;
            print_value(
                &serde_json::to_vec(&value)
                    .map_err(|error| Error::internal(format!("rendering the app: {error}")))?,
            )
        }
        AppCommand::Install(bundle) => {
            let body = std::fs::read(&bundle).map_err(|error| {
                Error::not_found(format!("reading {}: {error}", bundle.display()))
            })?;
            let response = client.request("POST", "/api/engine/apps", &body, &[])?;
            print_value(&response.body)
        }
        AppCommand::Upgrade { app, bundle } => {
            let body = std::fs::read(&bundle).map_err(|error| {
                Error::not_found(format!("reading {}: {error}", bundle.display()))
            })?;
            let response = client.request("PUT", &format!("/api/engine/apps/{app}"), &body, &[])?;
            print_value(&response.body)
        }
        AppCommand::Enable { app, acknowledge } => patch_enabled(&client, &app, true, acknowledge),
        AppCommand::Disable { app, acknowledge } => {
            patch_enabled(&client, &app, false, acknowledge)
        }
        AppCommand::Remove { app, acknowledge } => {
            delete(&client, &format!("/api/engine/apps/{app}"), acknowledge)
        }
        AppCommand::Approve { app, accept } => {
            let body = serde_json::json!({ "accept": accept }).to_string();
            let response = client.request(
                "PUT",
                &format!("/api/engine/apps/{app}/grant"),
                body.as_bytes(),
                &[],
            )?;
            print_value(&response.body)
        }
        AppCommand::Revoke { app, acknowledge } => delete(
            &client,
            &format!("/api/engine/apps/{app}/grant"),
            acknowledge,
        ),
    }
}

fn patch_enabled(
    client: &Client,
    app: &str,
    enabled: bool,
    acknowledge: Option<String>,
) -> Result<u8, Error> {
    let body = serde_json::json!({ "enabled": enabled }).to_string();
    let headers: Vec<(&str, &str)> = acknowledge
        .as_deref()
        .map(|token| vec![("x-anth-acknowledge", token)])
        .unwrap_or_default();
    let response = client.request(
        "PATCH",
        &format!("/api/engine/apps/{app}"),
        body.as_bytes(),
        &headers,
    )?;
    print_value(&response.body)
}

fn delete(client: &Client, path: &str, acknowledge: Option<String>) -> Result<u8, Error> {
    let headers: Vec<(&str, &str)> = acknowledge
        .as_deref()
        .map(|token| vec![("x-anth-acknowledge", token)])
        .unwrap_or_default();
    client.request("DELETE", path, &[], &headers)?;
    Ok(0)
}

fn print_value(body: &[u8]) -> Result<u8, Error> {
    if body.is_empty() {
        return Ok(0);
    }
    let value: serde_json::Value = serde_json::from_slice(body).map_err(|error| {
        Error::internal(format!("the engine answered with invalid JSON: {error}"))
    })?;
    println!(
        "{}",
        serde_json::to_string_pretty(&value)
            .map_err(|error| Error::internal(format!("rendering the answer: {error}")))?
    );
    Ok(0)
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
    use std::io::{Read, Write};
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;
    use std::sync::mpsc;
    use std::thread;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    /// A listener that answers `responses` in order, one per connection, and
    /// sends back every request it received.
    fn serve(path: &Path, responses: Vec<Vec<u8>>) -> mpsc::Receiver<Vec<u8>> {
        let listener = UnixListener::bind(path).expect("bind");
        let (send, receive) = mpsc::channel();
        thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().expect("accept");
                let mut request = Vec::new();
                let mut buffer = [0u8; 1024];
                loop {
                    let read = stream.read(&mut buffer).expect("read");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if let Some(split) = request.windows(4).position(|window| window == b"\r\n\r\n")
                    {
                        let head = String::from_utf8_lossy(&request[..split]).to_lowercase();
                        let length = head
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length:"))
                            .and_then(|value| value.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        if request.len() >= split + 4 + length {
                            break;
                        }
                    }
                }
                let _ = send.send(request);
                stream.write_all(&response).expect("write");
            }
        });
        receive
    }

    fn answer(status: &str, body: &str) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
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
    fn status_takes_the_socket() {
        assert_eq!(
            parse(args(&["--socket", "/tmp/engine.sock", "status"])).expect("parse"),
            Invocation::Status {
                socket: Some(PathBuf::from("/tmp/engine.sock")),
            }
        );
        assert_eq!(
            parse(args(&["status"])).expect("parse"),
            Invocation::Status { socket: None }
        );
    }

    #[test]
    fn status_refuses_positional_arguments() {
        assert!(parse(args(&["status", "now"])).is_err());
    }

    #[test]
    fn app_list_and_show_parse() {
        assert_eq!(
            parse(args(&["app", "list"])).expect("parse"),
            Invocation::App {
                socket: None,
                command: AppCommand::List,
            }
        );
        assert_eq!(
            parse(args(&["app", "show", "org.example.notes"])).expect("parse"),
            Invocation::App {
                socket: None,
                command: AppCommand::Show("org.example.notes".to_string()),
            }
        );
    }

    #[test]
    fn install_and_upgrade_take_a_bundle() {
        assert_eq!(
            parse(args(&["app", "install", "/tmp/notes.zip"])).expect("parse"),
            Invocation::App {
                socket: None,
                command: AppCommand::Install(PathBuf::from("/tmp/notes.zip")),
            }
        );
        assert_eq!(
            parse(args(&[
                "app",
                "upgrade",
                "org.example.notes",
                "/tmp/notes.zip"
            ]))
            .expect("parse"),
            Invocation::App {
                socket: None,
                command: AppCommand::Upgrade {
                    app: "org.example.notes".to_string(),
                    bundle: PathBuf::from("/tmp/notes.zip"),
                },
            }
        );
    }

    #[test]
    fn approve_takes_the_accept_flags() {
        let parsed = parse(args(&[
            "app",
            "approve",
            "org.example.notes",
            "--accept-write",
            "--accept-host",
        ]))
        .expect("parse");
        assert_eq!(
            parsed,
            Invocation::App {
                socket: None,
                command: AppCommand::Approve {
                    app: "org.example.notes".to_string(),
                    accept: vec!["write".to_string(), "host".to_string()],
                },
            }
        );
    }

    #[test]
    fn approve_refuses_acknowledge() {
        let error = parse(args(&[
            "app",
            "approve",
            "org.example.notes",
            "--acknowledge",
            "dependents",
        ]))
        .expect_err("acknowledge");
        assert!(error.message().contains("--acknowledge"), "{error}");
    }

    #[test]
    fn accept_flags_only_apply_to_approve() {
        let error = parse(args(&[
            "app",
            "disable",
            "org.example.notes",
            "--accept-write",
        ]))
        .expect_err("accept");
        assert!(error.message().contains("--accept-*"), "{error}");
    }

    #[test]
    fn enable_takes_an_acknowledgment() {
        assert_eq!(
            parse(args(&[
                "app",
                "enable",
                "org.example.notes",
                "--acknowledge",
                "dependents",
            ]))
            .expect("parse"),
            Invocation::App {
                socket: None,
                command: AppCommand::Enable {
                    app: "org.example.notes".to_string(),
                    acknowledge: Some("dependents".to_string()),
                },
            }
        );
    }

    #[test]
    fn app_needs_a_known_subcommand_and_its_arguments() {
        assert!(parse(args(&["app"])).is_err());
        assert!(parse(args(&["app", "nope"])).is_err());
        assert!(parse(args(&["app", "show"])).is_err());
        assert!(parse(args(&["app", "list", "extra"])).is_err());
        assert!(parse(args(&["app", "approve"])).is_err());
    }

    #[test]
    fn a_conflict_becomes_an_acknowledgment_instruction() {
        let error = Error::conflict("`org.anth.fs` is required by org.example.notes").with_detail(
            serde_json::json!({
                "dependents": ["org.example.notes"],
                "acknowledge": ["dependents"],
            }),
        );
        let text = error_text(&error);
        assert!(
            text.starts_with("warning: `org.anth.fs` is required by"),
            "{text}"
        );
        assert!(
            text.ends_with("rerun with --acknowledge dependents to proceed"),
            "{text}"
        );
    }

    #[test]
    fn other_errors_keep_the_code() {
        let text = error_text(&Error::not_found("`x` is not installed"));
        assert_eq!(text, "not-found: `x` is not installed");
    }

    #[test]
    fn status_asks_health_version_and_apps() {
        let dir = scratch("status");
        let socket = dir.join("engine.sock");
        let receive = serve(
            &socket,
            vec![
                answer("200 OK", r#"{"status":"ok"}"#),
                answer("200 OK", r#"{"engine":"0.5.0","protocol":1}"#),
                answer("200 OK", r#"{"apps":[]}"#),
            ],
        );
        assert_eq!(status(Some(socket)).expect("status"), 0);
        let lines: Vec<String> = (0..3)
            .map(|_| {
                let request = receive.recv().expect("request");
                String::from_utf8_lossy(&request)
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_string()
            })
            .collect();
        assert_eq!(
            lines,
            [
                "GET /api/health HTTP/1.1",
                "GET /api/engine/version HTTP/1.1",
                "GET /api/engine/apps HTTP/1.1",
            ]
        );
    }

    #[test]
    fn a_revoke_sends_the_acknowledgment_header() {
        let dir = scratch("revoke");
        let socket = dir.join("engine.sock");
        let receive = serve(&socket, vec![answer("204 No Content", "")]);
        let code = app(
            Some(socket),
            AppCommand::Revoke {
                app: "org.example.notes".to_string(),
                acknowledge: Some("dependents".to_string()),
            },
        )
        .expect("revoke");
        assert_eq!(code, 0);
        let request = String::from_utf8_lossy(&receive.recv().expect("request")).to_string();
        assert!(
            request.starts_with("DELETE /api/engine/apps/org.example.notes/grant HTTP/1.1"),
            "{request}"
        );
        assert!(
            request
                .to_lowercase()
                .contains("x-anth-acknowledge: dependents"),
            "{request}"
        );
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
