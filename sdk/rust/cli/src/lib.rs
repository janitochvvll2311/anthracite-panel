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

use anth_contract::{
    AppProblem, AppView, CompositionView, Error, ErrorKind, GrantState, KeyView, SurfaceView,
};
use serde::Deserialize;

pub mod client;

pub use client::Client;

#[derive(Deserialize)]
struct HealthView {
    status: String,
}

#[derive(Deserialize)]
struct VersionView {
    engine: String,
    protocol: u32,
}

#[derive(Deserialize)]
struct CatalogueView {
    apps: Vec<AppView>,
    #[serde(default)]
    problems: Vec<AppProblem>,
}

#[derive(Deserialize)]
struct KeysView {
    keys: Vec<KeyView>,
}

#[derive(Deserialize)]
struct AuditPage {
    entries: Vec<AuditRow>,
    #[serde(default)]
    total: usize,
    next: Option<usize>,
    #[serde(default)]
    problems: Vec<String>,
}

#[derive(Deserialize)]
struct AuditRow {
    at: String,
    actor: String,
    action: String,
    result: String,
    app: Option<String>,
    detail: Option<serde_json::Value>,
}

pub const USAGE: &str = "\
usage: anth [--socket <path>] <command> [options]

commands:
  doctor [--config <path>] [--debug|--release]
                        validate a configuration without a daemon
  status                health, version and the installed catalogue
  compose [--show] [--add <id>] [--remove <id>] [--now]
                        read or edit the composition
  audit [--limit <n>] [--before <n>]
                        read the audit log, newest first
  key list              trusted signers
  key add <public-key> [--label <label>]
                        trust a third-party signer
  key remove <fingerprint>
                        stop trusting a signer
  app list              list installed apps
  app show <id>         show one app
  app install <bundle>  install a bundle (zip)
  app upgrade <id> <bundle>
                        replace an installed bundle
  app enable|disable <id> [--acknowledge <token>]
  app remove <id> [--acknowledge <token>]
  app approve <id> [--accept-write|--accept-admin|--accept-host|--accept-package]
  app revoke <id> [--acknowledge <token>]
  app <alias|id> <verb> [--flag <value>] [--flag] [--no-flag]
                        invoke a generated app verb

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
    Compose {
        socket: Option<PathBuf>,
        show: bool,
        add: Vec<String>,
        remove: Vec<String>,
        now: bool,
    },
    Audit {
        socket: Option<PathBuf>,
        limit: usize,
        before: Option<usize>,
    },
    Key {
        socket: Option<PathBuf>,
        command: KeyCommand,
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
    Invoke {
        app: String,
        verb: String,
        flags: Vec<(String, Option<String>)>,
    },
}

/// The `anth key` subcommands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyCommand {
    List,
    Add {
        public_key: String,
        label: Option<String>,
    },
    Remove(String),
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
        "compose" => parse_compose(args, socket),
        "audit" => parse_audit(args, socket),
        "key" => parse_key(args, socket),
        other => Err(usage(format!("`{other}` is not a command"))),
    }
}

/// The fixed `anth app` verbs. `parse_invoke` handles everything else.
const APP_SUBCOMMANDS: &[&str] = &[
    "list", "show", "install", "upgrade", "enable", "disable", "remove", "approve", "revoke",
];

fn parse_app(
    args: impl Iterator<Item = OsString>,
    socket: Option<PathBuf>,
) -> Result<Invocation, Error> {
    let mut args = args.into_iter().peekable();
    let sub = args
        .next()
        .ok_or_else(|| usage("`app` needs a subcommand".to_string()))?;
    let sub = as_text(&sub)?.to_string();
    if !APP_SUBCOMMANDS.contains(&sub.as_str()) {
        return parse_invoke(sub, args, socket);
    }
    let mut acknowledge = None;
    let mut accept = Vec::new();
    let mut positional: Vec<OsString> = Vec::new();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--acknowledge") => {
                let token = args
                    .next()
                    .ok_or_else(|| usage("`--acknowledge` needs a token".to_string()))?;
                acknowledge = Some(as_text(&token)?.to_string());
            }
            Some("--accept-write") => accept.push("write".to_string()),
            Some("--accept-admin") => accept.push("admin".to_string()),
            Some("--accept-host") => accept.push("host".to_string()),
            Some("--accept-package") => accept.push("package".to_string()),
            Some("--help" | "-h") => return Ok(Invocation::Help),
            Some(value) if value.starts_with('-') => {
                return Err(usage(format!("`{value}` is not an app option")));
            }
            _ => positional.push(arg),
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
        ("show", [app]) => AppCommand::Show(as_text(app)?.to_string()),
        ("install", [bundle]) => AppCommand::Install(PathBuf::from(bundle.clone())),
        ("upgrade", [app, bundle]) => AppCommand::Upgrade {
            app: as_text(app)?.to_string(),
            bundle: PathBuf::from(bundle.clone()),
        },
        ("enable", [app]) => AppCommand::Enable {
            app: as_text(app)?.to_string(),
            acknowledge,
        },
        ("disable", [app]) => AppCommand::Disable {
            app: as_text(app)?.to_string(),
            acknowledge,
        },
        ("remove", [app]) => AppCommand::Remove {
            app: as_text(app)?.to_string(),
            acknowledge,
        },
        ("approve", [app]) => AppCommand::Approve {
            app: as_text(app)?.to_string(),
            accept,
        },
        ("revoke", [app]) => AppCommand::Revoke {
            app: as_text(app)?.to_string(),
            acknowledge,
        },
        (sub, _) => {
            return Err(usage(format!("`app {sub}` does not take those arguments")));
        }
    };
    Ok(Invocation::App { socket, command })
}

fn parse_invoke(
    app: String,
    args: impl Iterator<Item = OsString>,
    socket: Option<PathBuf>,
) -> Result<Invocation, Error> {
    let mut tokens = Vec::new();
    for arg in args {
        if matches!(arg.to_str(), Some("--help" | "-h")) {
            return Ok(Invocation::Help);
        }
        tokens.push(as_text(&arg)?.to_string());
    }
    let mut tokens = tokens.into_iter().peekable();
    let verb = tokens
        .next()
        .ok_or_else(|| usage(format!("`app {app}` needs a verb")))?;
    let mut flags = Vec::new();
    while let Some(token) = tokens.next() {
        let Some(rest) = token.strip_prefix("--") else {
            return Err(usage(format!("`{token}` is not a flag")));
        };
        if rest.is_empty() {
            return Err(usage("`--` needs a flag name".to_string()));
        }
        if let Some((key, value)) = rest.split_once('=') {
            flags.push((key.to_string(), Some(value.to_string())));
        } else if let Some(key) = rest.strip_prefix("no-") {
            flags.push((key.to_string(), Some("false".to_string())));
        } else if let Some(value) = tokens.next_if(|value| !value.starts_with("--")) {
            flags.push((rest.to_string(), Some(value)));
        } else {
            flags.push((rest.to_string(), None));
        }
    }
    Ok(Invocation::App {
        socket,
        command: AppCommand::Invoke { app, verb, flags },
    })
}

fn parse_compose(
    args: impl Iterator<Item = OsString>,
    socket: Option<PathBuf>,
) -> Result<Invocation, Error> {
    let mut args = args.into_iter();
    let mut show = false;
    let mut now = false;
    let mut add = Vec::new();
    let mut remove = Vec::new();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--show") => show = true,
            Some("--now") => now = true,
            Some("--add") => {
                let id = args
                    .next()
                    .ok_or_else(|| usage("`--add` needs an app id".to_string()))?;
                add.push(as_text(&id)?.to_string());
            }
            Some("--remove") => {
                let id = args
                    .next()
                    .ok_or_else(|| usage("`--remove` needs an app id".to_string()))?;
                remove.push(as_text(&id)?.to_string());
            }
            Some("--help" | "-h") => return Ok(Invocation::Help),
            Some(value) => {
                return Err(usage(format!("`{value}` is not a compose option")));
            }
            None => return Err(usage("a compose option is not valid UTF-8".to_string())),
        }
    }
    if show && (!add.is_empty() || !remove.is_empty()) {
        return Err(usage(
            "`--show` cannot be combined with `--add` or `--remove`".to_string(),
        ));
    }
    Ok(Invocation::Compose {
        socket,
        show,
        add,
        remove,
        now,
    })
}

fn parse_audit(
    args: impl Iterator<Item = OsString>,
    socket: Option<PathBuf>,
) -> Result<Invocation, Error> {
    let mut args = args.into_iter();
    let mut limit = 100usize;
    let mut before = None;
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--limit") => {
                let value = args
                    .next()
                    .ok_or_else(|| usage("`--limit` needs a number".to_string()))?;
                limit = as_text(&value)?
                    .parse::<usize>()
                    .map_err(|_| usage("`--limit` needs a positive number".to_string()))?;
            }
            Some("--before") => {
                let value = args
                    .next()
                    .ok_or_else(|| usage("`--before` needs a number".to_string()))?;
                before = Some(
                    as_text(&value)?
                        .parse::<usize>()
                        .map_err(|_| usage("`--before` needs a positive number".to_string()))?,
                );
            }
            Some("--help" | "-h") => return Ok(Invocation::Help),
            Some(value) => {
                return Err(usage(format!("`{value}` is not an audit option")));
            }
            None => return Err(usage("an audit option is not valid UTF-8".to_string())),
        }
    }
    Ok(Invocation::Audit {
        socket,
        limit,
        before,
    })
}

fn parse_key(
    args: impl Iterator<Item = OsString>,
    socket: Option<PathBuf>,
) -> Result<Invocation, Error> {
    let mut args = args.into_iter();
    let sub = args
        .next()
        .ok_or_else(|| usage("`key` needs a subcommand".to_string()))?;
    let sub = as_text(&sub)?.to_string();
    let mut label = None;
    let mut positional = Vec::new();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some("--label") => {
                let value = args
                    .next()
                    .ok_or_else(|| usage("`--label` needs a value".to_string()))?;
                label = Some(as_text(&value)?.to_string());
            }
            Some("--help" | "-h") => return Ok(Invocation::Help),
            Some(value) if value.starts_with('-') => {
                return Err(usage(format!("`{value}` is not a key option")));
            }
            _ => positional.push(arg),
        }
    }
    let command = match (sub.as_str(), positional.as_slice()) {
        ("list", []) if label.is_none() => KeyCommand::List,
        ("add", [public_key]) => KeyCommand::Add {
            public_key: as_text(public_key)?.to_string(),
            label,
        },
        ("remove", [fingerprint]) if label.is_none() => {
            KeyCommand::Remove(as_text(fingerprint)?.to_string())
        }
        (sub, _) => {
            return Err(usage(format!("`key {sub}` does not take those arguments")));
        }
    };
    Ok(Invocation::Key { socket, command })
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
        Invocation::Compose {
            socket,
            show,
            add,
            remove,
            now,
        } => compose(socket, show, add, remove, now),
        Invocation::Audit {
            socket,
            limit,
            before,
        } => audit(socket, limit, before),
        Invocation::Key { socket, command } => key(socket, command),
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

/// Reads the composition, or edits and replaces it with the validator the
/// engine served (§8.1). Recomposing is a restart in v0.5.0 (§8.6).
fn compose(
    socket: Option<PathBuf>,
    show: bool,
    add: Vec<String>,
    remove: Vec<String>,
    now: bool,
) -> Result<u8, Error> {
    let client = client(socket);
    let response = client.request("GET", "/api/engine/composition", &[], &[])?;
    let etag = response
        .headers
        .get("etag")
        .cloned()
        .ok_or_else(|| Error::internal("the engine answered without an ETag"))?;
    let mut view: serde_json::Value = serde_json::from_slice(&response.body)
        .map_err(|error| Error::internal(format!("the composition is not JSON: {error}")))?;
    if let Some(fatal) = view["surface"]["fatal"].as_object() {
        warn_fatal_fields(
            fatal["code"].as_str().unwrap_or("invalid"),
            fatal["message"].as_str().unwrap_or("it cannot be resolved"),
        );
    }
    if show || (add.is_empty() && remove.is_empty()) {
        return print_value(
            &serde_json::to_vec(&view["composition"])
                .map_err(|error| Error::internal(format!("rendering the composition: {error}")))?,
        );
    }
    let apps = view["composition"]["profile"][0]["apps"]
        .as_array_mut()
        .ok_or_else(|| Error::internal("the composition has no profile"))?;
    edit_apps(apps, &add, &remove);
    let body = serde_json::to_vec(&view["composition"])
        .map_err(|error| Error::internal(format!("rendering the composition: {error}")))?;
    let response = client.request(
        "PUT",
        "/api/engine/composition",
        &body,
        &[
            ("if-match", etag.as_str()),
            ("content-type", "application/json"),
        ],
    )?;
    let updated: serde_json::Value = serde_json::from_slice(&response.body).map_err(|error| {
        Error::internal(format!("the engine answered with invalid JSON: {error}"))
    })?;
    print_value(
        &serde_json::to_vec(&updated["composition"])
            .map_err(|error| Error::internal(format!("rendering the composition: {error}")))?,
    )?;
    if now {
        eprintln!(
            "v0.5.0 recomposes by restarting: the document is saved; running apps change on the next restart"
        );
    } else {
        eprintln!("composition written; running apps change on the next restart");
    }
    Ok(0)
}

/// Edits the profile by app id, not by raw string: `notes` replaces or removes
/// `notes@0.2.0` too, and a pin change replaces the existing entry instead of
/// composing the app twice (§8.1).
fn edit_apps(apps: &mut Vec<serde_json::Value>, add: &[String], remove: &[String]) {
    for id in add {
        let wanted = anth_contract::Profile::app_id(id);
        let found = apps.iter_mut().find(|app| {
            app.as_str()
                .is_some_and(|existing| anth_contract::Profile::app_id(existing) == wanted)
        });
        match found {
            Some(existing) => *existing = serde_json::Value::String(id.clone()),
            None => apps.push(serde_json::Value::String(id.clone())),
        }
    }
    if !remove.is_empty() {
        apps.retain(|app| {
            app.as_str()
                .map(anth_contract::Profile::app_id)
                .is_none_or(|base| {
                    !remove
                        .iter()
                        .any(|id| anth_contract::Profile::app_id(id) == base)
                })
        });
    }
}

/// A composition that cannot boot is still readable; name the reason once.
fn warn_fatal(surface: &SurfaceView) {
    if let Some(fatal) = &surface.fatal {
        warn_fatal_fields(&fatal.code, &fatal.message);
    }
}

fn warn_fatal_fields(code: &str, message: &str) {
    eprintln!("warning: composition {code}: {message}");
}

fn grant_label(state: GrantState) -> &'static str {
    match state {
        GrantState::None => "none",
        GrantState::Current => "current",
        GrantState::Stale => "stale",
        GrantState::Invalid => "invalid",
    }
}

fn audit(socket: Option<PathBuf>, limit: usize, before: Option<usize>) -> Result<u8, Error> {
    let client = client(socket);
    let mut path = format!("/api/engine/audit?limit={limit}");
    if let Some(before) = before {
        path.push_str(&format!("&before={before}"));
    }
    let page: AuditPage = client.get_typed(&path)?;
    let _total = page.total;
    for problem in &page.problems {
        eprintln!("warning: {problem}");
    }
    for entry in &page.entries {
        let app = entry.app.as_deref().unwrap_or("-");
        let detail = entry
            .detail
            .as_ref()
            .and_then(|value| serde_json::to_string(value).ok())
            .unwrap_or_default();
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            entry.at, entry.actor, entry.action, entry.result, app, detail
        );
    }
    if let Some(next) = page.next {
        println!("next: --before {next}");
    }
    Ok(0)
}

fn key(socket: Option<PathBuf>, command: KeyCommand) -> Result<u8, Error> {
    let client = client(socket);
    match command {
        KeyCommand::List => {
            let value: KeysView = client.get_typed("/api/engine/keys")?;
            for key in &value.keys {
                println!(
                    "{}\t{}\t{}",
                    key.fingerprint,
                    key.key.label.as_deref().unwrap_or("-"),
                    key.key.added_at
                );
            }
            Ok(0)
        }
        KeyCommand::Add { public_key, label } => {
            let body = serde_json::json!({"public_key": public_key, "label": label}).to_string();
            let response = client.request(
                "POST",
                "/api/engine/keys",
                body.as_bytes(),
                &[("content-type", "application/json")],
            )?;
            let value: serde_json::Value =
                serde_json::from_slice(&response.body).map_err(|error| {
                    Error::internal(format!("the engine answered with invalid JSON: {error}"))
                })?;
            let fingerprint = value["fingerprint"]
                .as_str()
                .ok_or_else(|| Error::internal("the engine answered without a fingerprint"))?;
            println!("{fingerprint}");
            Ok(0)
        }
        KeyCommand::Remove(fingerprint) => {
            client.request(
                "DELETE",
                &format!("/api/engine/keys/{fingerprint}"),
                &[],
                &[],
            )?;
            Ok(0)
        }
    }
}

/// Readiness, the composition, the version and the catalogue in one answer.
fn status(socket: Option<PathBuf>) -> Result<u8, Error> {
    let client = client(socket);
    let health: HealthView = client.get_typed("/api/health")?;
    let version: VersionView = client.get_typed("/api/engine/version")?;
    let composition: CompositionView = client.get_typed("/api/engine/composition")?;
    let apps: CatalogueView = client.get_typed("/api/engine/apps")?;
    println!("engine {} (protocol {})", version.engine, version.protocol);
    println!("health {}", health.status);
    let composed: usize = composition
        .composition
        .profile
        .iter()
        .map(|profile| profile.apps.len())
        .sum();
    println!(
        "composition {composed} app(s) in {} profile(s)",
        composition.composition.profile.len()
    );
    warn_fatal(&composition.surface);
    for degraded in &composition.surface.degraded {
        eprintln!("warning: degraded {}: {}", degraded.app, degraded.reason);
    }
    println!("apps {}", apps.apps.len());
    for problem in &apps.problems {
        eprintln!("warning: {}: {}", problem.app, problem.message);
    }
    Ok(0)
}

fn app(socket: Option<PathBuf>, command: AppCommand) -> Result<u8, Error> {
    let client = client(socket);
    match command {
        AppCommand::List => {
            let value: CatalogueView = client.get_typed("/api/engine/apps")?;
            for app in &value.apps {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    app.id,
                    app.version,
                    if app.enabled { "enabled" } else { "disabled" },
                    app.alias.as_deref().unwrap_or("-"),
                    grant_label(app.grant_state)
                );
            }
            for problem in &value.problems {
                eprintln!("warning: {}: {}", problem.app, problem.message);
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
            let response = client.request(
                "POST",
                "/api/engine/apps",
                &body,
                &[("content-type", "application/zip")],
            )?;
            print_value(&response.body)
        }
        AppCommand::Upgrade { app, bundle } => {
            let body = std::fs::read(&bundle).map_err(|error| {
                Error::not_found(format!("reading {}: {error}", bundle.display()))
            })?;
            let response = client.request(
                "PUT",
                &format!("/api/engine/apps/{app}"),
                &body,
                &[("content-type", "application/zip")],
            )?;
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
                &[("content-type", "application/json")],
            )?;
            print_value(&response.body)
        }
        AppCommand::Revoke { app, acknowledge } => delete(
            &client,
            &format!("/api/engine/apps/{app}/grant"),
            acknowledge,
        ),
        AppCommand::Invoke { app, verb, flags } => invoke(&client, &app, &verb, &flags),
    }
}

/// A generated CLI verb: resolve alias → id and verb → route in the surface,
/// then call the app route (`§7.9`, `§7.13`). Only composed apps have verbs.
fn invoke(
    client: &Client,
    app: &str,
    verb: &str,
    flags: &[(String, Option<String>)],
) -> Result<u8, Error> {
    let apps: CatalogueView = client.get_typed("/api/engine/apps")?;
    let installed = &apps.apps;
    let chosen = if let Some(exact) = installed
        .iter()
        .find(|candidate| candidate.id.as_str() == app)
    {
        exact
    } else {
        let matches: Vec<&AppView> = installed
            .iter()
            .filter(|candidate| candidate.alias.as_deref() == Some(app))
            .collect();
        match matches.as_slice() {
            [] => return Err(Error::not_found(format!("`{app}` is not installed"))),
            [only] => *only,
            many => {
                let ids: Vec<String> = many
                    .iter()
                    .map(|candidate| candidate.id.to_string())
                    .collect();
                return Err(Error::conflict(format!(
                    "`{app}` names more than one app ({}); use the full id",
                    ids.join(", ")
                ))
                .with_detail(serde_json::json!({"apps": ids})));
            }
        }
    };
    let id = chosen.id.to_string();
    let composition: CompositionView = client.get_typed("/api/engine/composition")?;
    let composed = composition
        .surface
        .apps
        .iter()
        .find(|candidate| candidate.id.as_str() == id)
        .ok_or_else(|| Error::unavailable(format!("`{id}` is not running")))?;
    let call = composed
        .cli
        .iter()
        .find(|entry| entry.verb == verb)
        .ok_or_else(|| Error::not_found(format!("`{id}` has no CLI verb `{verb}`")))?;
    let route = composed
        .routes
        .iter()
        .find(|route| route.interface == call.interface && route.function == call.function)
        .ok_or_else(|| {
            Error::unavailable(format!(
                "`{id}` verb `{verb}` has no route; v0.5.0 invokes through routes"
            ))
        })?;
    let method = route.method.as_str();
    let path = route.path.as_str();
    let no_body = matches!(method, "GET" | "HEAD" | "DELETE");
    let mut body = Vec::new();
    let mut url = format!("/api/apps/{id}{path}");
    let mut headers = Vec::new();
    if no_body {
        let mut query = form_urlencoded::Serializer::new(String::new());
        for (key, value) in flags {
            query.append_pair(key, value.as_deref().unwrap_or("true"));
        }
        let query = query.finish();
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query);
        }
    } else {
        let mut object = serde_json::Map::new();
        for (key, value) in flags {
            let value = flag_value(value);
            match object.get_mut(key) {
                Some(existing) => {
                    if let Some(values) = existing.as_array_mut() {
                        values.push(value);
                    } else {
                        let first = existing.take();
                        *existing = serde_json::Value::Array(vec![first, value]);
                    }
                }
                None => {
                    object.insert(key.clone(), value);
                }
            }
        }
        body = serde_json::to_vec(&serde_json::Value::Object(object))
            .map_err(|error| Error::internal(format!("rendering the input: {error}")))?;
        headers.push(("content-type", "application/json"));
    }
    let response = client.request(method, &url, &body, &headers)?;
    print_value(&response.body)
}

/// A flag has no type information in the surface, so its value stays a string;
/// the runtime coerces it to the WIT parameter type. A bare flag is `true`.
fn flag_value(value: &Option<String>) -> serde_json::Value {
    match value {
        None => serde_json::Value::Bool(true),
        Some(text) => serde_json::Value::String(text.clone()),
    }
}

fn patch_enabled(
    client: &Client,
    app: &str,
    enabled: bool,
    acknowledge: Option<String>,
) -> Result<u8, Error> {
    let body = serde_json::json!({ "enabled": enabled }).to_string();
    let mut headers = vec![("content-type", "application/json")];
    if let Some(token) = acknowledge.as_deref() {
        headers.push(("x-anth-acknowledge", token));
    }
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

    #[test]
    fn a_catalogue_view_reads_contract_fields() {
        let text = r#"{"apps":[{"id":"org.example.notes","version":"0.1.0","enabled":true,"tier":"third-party","installed_at":"2026-01-01T00:00:00Z","alias":"notes","grant_state":"current"}],"problems":[{"app":"broken","message":"bad"}]}"#;
        let view: CatalogueView = serde_json::from_str(text).expect("catalogue");
        assert_eq!(view.apps[0].alias.as_deref(), Some("notes"));
        assert_eq!(grant_label(view.apps[0].grant_state), "current");
        assert_eq!(view.problems[0].app, "broken");
    }

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
    fn audit_takes_paging_options() {
        assert_eq!(
            parse(args(&["audit"])).expect("parse"),
            Invocation::Audit {
                socket: None,
                limit: 100,
                before: None,
            }
        );
        assert_eq!(
            parse(args(&["audit", "--limit", "2", "--before", "7"])).expect("parse"),
            Invocation::Audit {
                socket: None,
                limit: 2,
                before: Some(7),
            }
        );
        assert!(parse(args(&["audit", "--limit", "nope"])).is_err());
        assert!(parse(args(&["audit", "--nope"])).is_err());
    }

    #[test]
    fn audit_requests_a_page_and_a_cursor() {
        let dir = scratch("audit");
        let socket = dir.join("engine.sock");
        let body = r#"{"entries":[],"total":3,"next":1,"limit":2,"before":3,"problems":[]}"#;
        let receive = serve(
            &socket,
            vec![answer("200 OK", body), answer("200 OK", body)],
        );
        audit(Some(socket.clone()), 2, None).expect("latest page");
        let first = String::from_utf8_lossy(&receive.recv().expect("request")).to_string();
        assert!(
            first.starts_with("GET /api/engine/audit?limit=2 HTTP/1.1"),
            "{first}"
        );

        audit(Some(socket), 2, Some(1)).expect("older page");
        let second = String::from_utf8_lossy(&receive.recv().expect("request")).to_string();
        assert!(
            second.starts_with("GET /api/engine/audit?limit=2&before=1 HTTP/1.1"),
            "{second}"
        );
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
    fn compose_defaults_to_showing() {
        assert_eq!(
            parse(args(&["compose"])).expect("parse"),
            Invocation::Compose {
                socket: None,
                show: false,
                add: Vec::new(),
                remove: Vec::new(),
                now: false,
            }
        );
        assert_eq!(
            parse(args(&["compose", "--show", "--now"])).expect("parse"),
            Invocation::Compose {
                socket: None,
                show: true,
                add: Vec::new(),
                remove: Vec::new(),
                now: true,
            }
        );
    }

    #[test]
    fn compose_takes_adds_and_removes() {
        assert_eq!(
            parse(args(&[
                "compose",
                "--add",
                "org.example.notes",
                "--remove",
                "org.example.old",
            ]))
            .expect("parse"),
            Invocation::Compose {
                socket: None,
                show: false,
                add: vec!["org.example.notes".to_string()],
                remove: vec!["org.example.old".to_string()],
                now: false,
            }
        );
    }

    #[test]
    fn compose_show_cannot_be_combined_with_edits() {
        let error = parse(args(&["compose", "--show", "--add", "org.example.notes"]))
            .expect_err("show and add");
        assert!(error.message().contains("--show"), "{error}");
        assert!(parse(args(&["compose", "--add"])).is_err());
        assert!(parse(args(&["compose", "--nope"])).is_err());
        assert!(parse(args(&["compose", "org.example.notes"])).is_err());
    }

    #[test]
    fn a_compose_edit_sends_the_served_validator() {
        let dir = scratch("compose");
        let socket = dir.join("engine.sock");
        let before = r#"{"composition":{"version":1,"profile":[{"id":"default","apps":[]}]},"surface":{"apps":[],"degraded":[],"links":[]}}"#;
        let after = r#"{"composition":{"version":1,"profile":[{"id":"default","apps":["org.example.notes"]}]},"surface":{"apps":[],"degraded":[],"links":[]}}"#;
        let receive = serve(
            &socket,
            vec![
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"abc\"\r\n\r\n{before}",
                    before.len()
                )
                .into_bytes(),
                answer("200 OK", after),
            ],
        );
        let code = compose(
            Some(socket),
            false,
            vec!["org.example.notes".to_string()],
            Vec::new(),
            true,
        )
        .expect("compose");
        assert_eq!(code, 0);
        let get = String::from_utf8_lossy(&receive.recv().expect("get")).to_string();
        assert!(
            get.starts_with("GET /api/engine/composition HTTP/1.1"),
            "{get}"
        );
        let put = String::from_utf8_lossy(&receive.recv().expect("put")).to_string();
        assert!(
            put.starts_with("PUT /api/engine/composition HTTP/1.1"),
            "{put}"
        );
        assert!(put.to_lowercase().contains("if-match: \"abc\""), "{put}");
        assert!(
            put.to_lowercase()
                .contains("content-type: application/json"),
            "{put}"
        );
        assert!(put.contains("org.example.notes"), "{put}");
    }

    #[test]
    fn compose_edits_by_app_id_not_by_pinned_text() {
        let mut apps = vec![serde_json::json!("org.example.notes@0.1.0")];
        edit_apps(&mut apps, &["org.example.notes@0.2.0".to_string()], &[]);
        assert_eq!(apps, [serde_json::json!("org.example.notes@0.2.0")]);
        edit_apps(&mut apps, &["org.example.consumer".to_string()], &[]);
        assert_eq!(apps.len(), 2);
        edit_apps(&mut apps, &[], &["org.example.notes".to_string()]);
        assert_eq!(apps, [serde_json::json!("org.example.consumer")]);
        edit_apps(&mut apps, &[], &["org.example.consumer@9.9.9".to_string()]);
        assert!(apps.is_empty());
    }

    #[test]
    fn key_commands_parse() {
        assert_eq!(
            parse(args(&["key", "list"])).expect("parse"),
            Invocation::Key {
                socket: None,
                command: KeyCommand::List,
            }
        );
        assert_eq!(
            parse(args(&["key", "remove", "abc"])).expect("parse"),
            Invocation::Key {
                socket: None,
                command: KeyCommand::Remove("abc".to_string()),
            }
        );
        assert_eq!(
            parse(args(&["key", "add", "ea4a", "--label", "laptop"])).expect("parse"),
            Invocation::Key {
                socket: None,
                command: KeyCommand::Add {
                    public_key: "ea4a".to_string(),
                    label: Some("laptop".to_string()),
                },
            }
        );
    }

    #[test]
    fn key_refuses_options_that_do_not_apply() {
        assert!(parse(args(&["key", "list", "--label", "x"])).is_err());
        assert!(parse(args(&["key", "remove", "abc", "--label", "x"])).is_err());
        assert!(parse(args(&["key", "add"])).is_err());
        assert!(parse(args(&["key", "add", "ea4a", "extra"])).is_err());
        assert!(parse(args(&["key", "nope"])).is_err());
        assert!(parse(args(&["key"])).is_err());
    }

    #[test]
    fn a_key_add_sends_json_and_a_remove_uses_the_path() {
        let dir = scratch("key");
        let socket = dir.join("engine.sock");
        let body = r#"{"fingerprint":"abc","public_key":"ea4a","label":"laptop"}"#;
        let receive = serve(
            &socket,
            vec![answer("201 Created", body), answer("204 No Content", "")],
        );
        key(
            Some(socket.clone()),
            KeyCommand::Add {
                public_key: "ea4a".to_string(),
                label: Some("laptop".to_string()),
            },
        )
        .expect("add");
        key(Some(socket), KeyCommand::Remove("abc".to_string())).expect("remove");
        let add = String::from_utf8_lossy(&receive.recv().expect("add")).to_string();
        assert!(add.starts_with("POST /api/engine/keys HTTP/1.1"), "{add}");
        assert!(
            add.to_lowercase()
                .contains("content-type: application/json"),
            "{add}"
        );
        assert!(add.contains("\"public_key\":\"ea4a\""), "{add}");
        assert!(add.contains("\"label\":\"laptop\""), "{add}");
        let remove = String::from_utf8_lossy(&receive.recv().expect("remove")).to_string();
        assert!(
            remove.starts_with("DELETE /api/engine/keys/abc HTTP/1.1"),
            "{remove}"
        );
    }

    #[test]
    fn a_generated_verb_parses_flags() {
        assert_eq!(
            parse(args(&["app", "hello", "greet", "--name", "Ada", "--loud"])).expect("parse"),
            Invocation::App {
                socket: None,
                command: AppCommand::Invoke {
                    app: "hello".to_string(),
                    verb: "greet".to_string(),
                    flags: vec![
                        ("name".to_string(), Some("Ada".to_string())),
                        ("loud".to_string(), None),
                    ],
                },
            }
        );
        assert_eq!(
            parse(args(&["app", "hello", "greet", "--no-loud", "--label=hi"])).expect("parse"),
            Invocation::App {
                socket: None,
                command: AppCommand::Invoke {
                    app: "hello".to_string(),
                    verb: "greet".to_string(),
                    flags: vec![
                        ("loud".to_string(), Some("false".to_string())),
                        ("label".to_string(), Some("hi".to_string())),
                    ],
                },
            }
        );
    }

    #[test]
    fn an_app_can_be_invoked_by_alias_or_id() {
        let dir = scratch("invoke");
        let socket = dir.join("engine.sock");
        let apps = r#"{"apps":[{"id":"org.anthracite.hello","version":"0.1.0","enabled":true,"tier":"third-party","installed_at":"2026-01-01T00:00:00Z","alias":"hello","grant_state":"none"}],"problems":[]}"#;
        let composition = r#"{"composition":{"version":1,"profile":[{"id":"default","apps":["org.anthracite.hello"]}]},"surface":{"apps":[{"id":"org.anthracite.hello","version":"0.1.0","risk":"read","routes":[{"method":"GET","path":"/hello","interface":"anthracite:hello/greeting@1.0.0","function":"greet"}],"slots":[],"cli":[{"verb":"greet","interface":"anthracite:hello/greeting@1.0.0","function":"greet"}]}],"degraded":[],"links":[]}}"#;
        let receive = serve(
            &socket,
            vec![
                answer("200 OK", apps),
                answer("200 OK", composition),
                answer("200 OK", r#""hello, Ada""#),
            ],
        );
        app(
            Some(socket),
            AppCommand::Invoke {
                app: "hello".to_string(),
                verb: "greet".to_string(),
                flags: vec![("name".to_string(), Some("Ada".to_string()))],
            },
        )
        .expect("invoke");
        let get_apps = String::from_utf8_lossy(&receive.recv().expect("apps")).to_string();
        assert!(
            get_apps.starts_with("GET /api/engine/apps HTTP/1.1"),
            "{get_apps}"
        );
        let get_composition =
            String::from_utf8_lossy(&receive.recv().expect("composition")).to_string();
        assert!(
            get_composition.starts_with("GET /api/engine/composition HTTP/1.1"),
            "{get_composition}"
        );
        let call = String::from_utf8_lossy(&receive.recv().expect("call")).to_string();
        assert!(
            call.starts_with("GET /api/apps/org.anthracite.hello/hello?name=Ada HTTP/1.1"),
            "{call}"
        );
    }

    #[test]
    fn an_ambiguous_alias_requires_the_full_id() {
        let dir = scratch("ambiguous");
        let socket = dir.join("engine.sock");
        let apps = r#"{"apps":[{"id":"org.example.one","version":"0.1.0","enabled":true,"tier":"third-party","installed_at":"2026-01-01T00:00:00Z","alias":"notes","grant_state":"none"},{"id":"org.example.two","version":"0.1.0","enabled":true,"tier":"third-party","installed_at":"2026-01-01T00:00:00Z","alias":"notes","grant_state":"none"}],"problems":[]}"#;
        let _receive = serve(&socket, vec![answer("200 OK", apps)]);
        let error = app(
            Some(socket),
            AppCommand::Invoke {
                app: "notes".to_string(),
                verb: "list".to_string(),
                flags: Vec::new(),
            },
        )
        .expect_err("ambiguous");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Conflict);
        assert!(error.message().contains("org.example.one"), "{error}");
        assert!(error.message().contains("org.example.two"), "{error}");
    }

    #[test]
    fn a_verb_with_a_body_sends_flag_values() {
        let dir = scratch("invoke-body");
        let socket = dir.join("engine.sock");
        let apps = r#"{"apps":[{"id":"org.anthracite.hello","version":"0.1.0","enabled":true,"tier":"third-party","installed_at":"2026-01-01T00:00:00Z","alias":"hello","grant_state":"none"}],"problems":[]}"#;
        let composition = r#"{"composition":{"version":1,"profile":[{"id":"default","apps":["org.anthracite.hello"]}]},"surface":{"apps":[{"id":"org.anthracite.hello","version":"0.1.0","risk":"read","routes":[{"method":"POST","path":"/echo","interface":"anthracite:hello/greeting@1.0.0","function":"greet"}],"slots":[],"cli":[{"verb":"greet","interface":"anthracite:hello/greeting@1.0.0","function":"greet"}]}],"degraded":[],"links":[]}}"#;
        let receive = serve(
            &socket,
            vec![
                answer("200 OK", apps),
                answer("200 OK", composition),
                answer("200 OK", r#""ok""#),
            ],
        );
        app(
            Some(socket),
            AppCommand::Invoke {
                app: "hello".to_string(),
                verb: "greet".to_string(),
                flags: vec![
                    ("count".to_string(), Some("3".to_string())),
                    ("ok".to_string(), None),
                    ("label".to_string(), Some("Ada".to_string())),
                    ("literal".to_string(), Some("true".to_string())),
                ],
            },
        )
        .expect("invoke");
        let _ = receive.recv().expect("apps");
        let _ = receive.recv().expect("composition");
        let call = String::from_utf8_lossy(&receive.recv().expect("call")).to_string();
        assert!(
            call.starts_with("POST /api/apps/org.anthracite.hello/echo HTTP/1.1"),
            "{call}"
        );
        assert!(call.contains("\"count\":\"3\""), "{call}");
        assert!(call.contains("\"ok\":true"), "{call}");
        assert!(call.contains("\"label\":\"Ada\""), "{call}");
        assert!(call.contains("\"literal\":\"true\""), "{call}");
    }

    #[test]
    fn status_asks_health_version_composition_and_apps() {
        let dir = scratch("status");
        let socket = dir.join("engine.sock");
        let receive = serve(
            &socket,
            vec![
                answer("200 OK", r#"{"status":"ok"}"#),
                answer("200 OK", r#"{"engine":"0.5.0","protocol":1}"#),
                answer(
                    "200 OK",
                    r#"{"composition":{"version":1,"profile":[]},"surface":{"apps":[],"degraded":[],"links":[]}}"#,
                ),
                answer("200 OK", r#"{"apps":[]}"#),
            ],
        );
        assert_eq!(status(Some(socket)).expect("status"), 0);
        let lines: Vec<String> = (0..4)
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
                "GET /api/engine/composition HTTP/1.1",
                "GET /api/engine/apps HTTP/1.1",
            ]
        );
    }

    #[test]
    fn an_approve_and_a_disable_send_json() {
        let dir = scratch("json");
        let socket = dir.join("engine.sock");
        let receive = serve(
            &socket,
            vec![
                answer("201 Created", "{}"),
                answer("200 OK", r#"{"enabled":false}"#),
            ],
        );
        app(
            Some(socket.clone()),
            AppCommand::Approve {
                app: "org.example.notes".to_string(),
                accept: vec!["write".to_string()],
            },
        )
        .expect("approve");
        app(
            Some(socket),
            AppCommand::Disable {
                app: "org.example.notes".to_string(),
                acknowledge: None,
            },
        )
        .expect("disable");
        for _ in 0..2 {
            let request = String::from_utf8_lossy(&receive.recv().expect("request")).to_string();
            assert!(
                request
                    .to_lowercase()
                    .contains("content-type: application/json"),
                "{request}"
            );
        }
    }

    #[test]
    fn an_install_sends_the_bundle_as_a_zip() {
        let dir = scratch("install");
        let socket = dir.join("engine.sock");
        let bundle = dir.join("notes.zip");
        fs::write(&bundle, b"zip").expect("write");
        let receive = serve(&socket, vec![answer("201 Created", "{}")]);
        app(Some(socket), AppCommand::Install(bundle)).expect("install");
        let request = String::from_utf8_lossy(&receive.recv().expect("request")).to_string();
        assert!(
            request
                .to_lowercase()
                .contains("content-type: application/zip"),
            "{request}"
        );
    }

    #[test]
    fn a_non_utf8_bundle_path_is_kept_as_bytes() {
        let mut path = b"/tmp/".to_vec();
        path.push(0xff);
        path.extend_from_slice(b"/notes.zip");
        let parsed = parse(vec![
            OsString::from("app"),
            OsString::from("install"),
            OsString::from_vec(path.clone()),
        ])
        .expect("parse");
        assert_eq!(
            parsed,
            Invocation::App {
                socket: None,
                command: AppCommand::Install(PathBuf::from(OsString::from_vec(path))),
            }
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
