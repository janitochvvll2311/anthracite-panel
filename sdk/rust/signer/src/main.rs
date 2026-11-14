//! `anth-sign`: sign an app bundle offline with an Ed25519 key.
//!
//! The engine verifies `manifest.sig`; this is the tool that produces it, for
//! the release process and for a third-party author. The private key never
//! leaves the machine that runs this command.

use std::path::PathBuf;
use std::process::ExitCode;

use anth_contract::Error;
use anth_signer::{generate_key, sign};

const USAGE: &str = "\
usage: anth-sign <dir> --key <path>
       anth-sign keygen --out <path>

commands:
  <dir>                 write <dir>/manifest.sig over manifest.toml and wit/**
  keygen                write a new Ed25519 private key (mode 0600)

options:
  --key <path>          private key: 32 bytes of lowercase hex, mode 0600
  --out <path>          where keygen writes the private key
  -h, --help            show this message";

#[derive(Debug)]
enum Command {
    Sign { dir: PathBuf, key: PathBuf },
    Keygen { out: PathBuf },
}

fn main() -> ExitCode {
    let result = parse(std::env::args().skip(1)).and_then(run);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {}", error.kind().code(), error.message());
            ExitCode::from(1)
        }
    }
}

fn run(command: Command) -> Result<(), Error> {
    match command {
        Command::Sign { dir, key } => {
            let public = sign(&dir, &key)?;
            println!("signed {}", dir.join("manifest.sig").display());
            println!("public_key\t{}", public.key);
            println!("fingerprint\t{}", public.fingerprint);
        }
        Command::Keygen { out } => {
            let public = generate_key(&out)?;
            println!("wrote {}", out.display());
            println!("public_key\t{}", public.key);
            println!("fingerprint\t{}", public.fingerprint);
        }
    }
    Ok(())
}

fn parse(args: impl Iterator<Item = String>) -> Result<Command, Error> {
    let mut key = None;
    let mut out = None;
    let mut positional = Vec::new();
    let mut args = args.peekable();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "--key" => key = Some(value(&mut args, "--key")?),
            "--out" => out = Some(value(&mut args, "--out")?),
            other if other.starts_with('-') => {
                return Err(usage(format!("unknown option `{other}`")));
            }
            other => positional.push(PathBuf::from(other)),
        }
    }
    match (positional.as_slice(), key, out) {
        ([dir], Some(key), None) => Ok(Command::Sign {
            dir: dir.clone(),
            key,
        }),
        ([command], None, Some(out)) if command == "keygen" => Ok(Command::Keygen { out }),
        ([command], None, None) if command == "keygen" => Err(usage(
            "`keygen` needs an `--out <path>`; refusing to guess where a key lives".to_string(),
        )),
        ([_], Some(_), Some(_)) => Err(usage(
            "`--out` belongs to `keygen`, not to signing".to_string(),
        )),
        ([_], None, Some(_)) => Err(usage("signing needs a `--key <path>`".to_string())),
        ([], _, _) => Err(usage("no bundle directory".to_string())),
        _ => Err(usage("one bundle directory, `--key <path>`".to_string())),
    }
}

fn value(args: &mut impl Iterator<Item = String>, option: &str) -> Result<PathBuf, Error> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| usage(format!("`{option}` needs a path")))
}

fn usage(message: String) -> Error {
    Error::invalid(format!("{message}\n\n{USAGE}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Command, Error> {
        parse(args.iter().map(|argument| argument.to_string()))
    }

    #[test]
    fn a_directory_and_a_key_sign() {
        let command = parse_args(&["bundle", "--key", "release.key"]).expect("sign");
        assert!(matches!(
            command,
            Command::Sign { ref dir, ref key }
                if dir == std::path::Path::new("bundle") && key == std::path::Path::new("release.key")
        ));
    }

    #[test]
    fn keygen_requires_an_explicit_output() {
        let error = parse_args(&["keygen"]).expect_err("no --out");
        assert!(error.message().contains("--out"), "{error}");
    }

    #[test]
    fn a_missing_key_or_directory_is_refused() {
        assert!(parse_args(&["bundle"]).is_err());
        assert!(parse_args(&["--key", "release.key"]).is_err());
        assert!(parse_args(&["a", "b", "--key", "k"]).is_err());
        assert!(parse_args(&["bundle", "--key"]).is_err());
        assert!(parse_args(&["bundle", "--key", "k", "--out", "o"]).is_err());
        assert!(parse_args(&["--nonsense"]).is_err());
    }
}
