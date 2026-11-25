//! `anth` / `cargo-anth`: create, build, package, sign and verify apps.

mod config;
mod project;
mod template;

use std::path::PathBuf;

use anyhow::{bail, Context, Result};

use crate::config::Project;

pub fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let Some(command) = args.next() else {
        help();
        return Ok(());
    };
    match command.as_str() {
        "app" => app(args.collect()),
        "help" | "--help" | "-h" => {
            help();
            Ok(())
        }
        "--version" | "-V" => {
            println!("anth {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        other => bail!("unknown command `{other}`; run `anth --help`"),
    }
}

fn app(args: Vec<String>) -> Result<()> {
    let Some(command) = args.first().map(String::as_str) else {
        bail!("`anth app` needs a command: new, build, package, sign, verify, check");
    };
    let rest = &args[1..];
    match command {
        "new" => new_app(rest),
        "build" => {
            let project = load_project(rest)?;
            project::build(&project)?;
            Ok(())
        }
        "package" => {
            let project = load_project(rest)?;
            let key = match key_arg(rest)? {
                Some(key) => Some(key),
                None => project::configured_key(&project)?,
            };
            project::package(&project, key.as_deref())?;
            Ok(())
        }
        "sign" => {
            let project = load_project(rest)?;
            let key = match key_arg(rest)? {
                Some(key) => Some(key),
                None => project::configured_key(&project)?,
            };
            let Some(key) = key else {
                bail!("`anth app sign` needs --key <path> or [sign].key in anth.toml");
            };
            project::sign(&project, &key)?;
            Ok(())
        }
        "verify" => {
            let project = load_project(rest)?;
            let public = value_arg(rest, "--public-key");
            project::verify(&project, public.as_deref())?;
            Ok(())
        }
        "check" => {
            let project = load_project(rest)?;
            project::check(&project)?;
            Ok(())
        }
        "help" | "--help" | "-h" => {
            help();
            Ok(())
        }
        other => bail!("unknown `anth app` command `{other}`"),
    }
}

fn new_app(args: &[String]) -> Result<()> {
    let id = positional(args, &["--lang", "--dir"])
        .context("`anth app new` needs an app id, for example org.example.notes")?;
    let lang = value_arg(args, "--lang").unwrap_or_else(|| "rust".to_string());
    let dir = value_arg(args, "--dir").map(PathBuf::from);
    template::create(id, &lang, dir.as_deref())?;
    Ok(())
}

/// The first argument that is not a flag or the value of a value-taking flag.
fn positional<'a>(args: &'a [String], value_flags: &[&str]) -> Option<&'a str> {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if value_flags.contains(&arg) {
            index += 2;
            continue;
        }
        if arg.starts_with("--") {
            index += 1;
            continue;
        }
        return Some(arg);
    }
    None
}

fn load_project(args: &[String]) -> Result<Project> {
    let path = project_arg(args)?;
    Project::load(path.as_deref())
}

fn project_arg(args: &[String]) -> Result<Option<PathBuf>> {
    Ok(value_arg(args, "--project").map(PathBuf::from))
}

fn key_arg(args: &[String]) -> Result<Option<PathBuf>> {
    Ok(value_arg(args, "--key").map(PathBuf::from))
}

fn value_arg(args: &[String], name: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == name {
            return iter.next().cloned();
        }
        if let Some(value) = arg.strip_prefix(&format!("{name}=")) {
            return Some(value.to_string());
        }
    }
    None
}

fn help() {
    println!(
        r#"anth — create, build, package, sign and verify Anthracite apps

Usage:
  anth app new <id> [--lang rust|bun] [--dir <path>]
  anth app build [--project <dir>]
  anth app package [--project <dir>] [--key <path>]
  anth app sign [--project <dir>] [--key <path>]
  anth app verify [--project <dir>] [--public-key <hex>]
  anth app check [--project <dir>]

The same commands are available as `cargo anth ...`.

Environment:
  CARGO, BUN, WASM_TOOLS, ANTH_SIGN, ANTH_CONFORMANCE override the tools.
"#
    );
}
