//! `anth-conformance`: validate one bundle offline.

use std::fs;
use std::process::ExitCode;

use anth_conformance::{validate, Request};
use anth_contract::Error;

const USAGE: &str = "\
usage: anth-conformance [options] <bundle.anth>

options:
  --require-signature       refuse a bundle without manifest.sig
  --allow-third-party-service
                            accept a third-party service bundle
  --known <namespace:package>
                            allow an imported provider package (repeatable)
  --key <hex-public-key>    trust an Ed25519 signer (repeatable)
  --max-bytes <n>           compressed bundle cap (default 33554432)
  --json                    print the summary as JSON
  -h, --help                show this message";

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)).and_then(|(path, request, json)| {
        let bytes = fs::read(&path)
            .map_err(|error| Error::not_found(format!("reading {}: {error}", path)))?;
        let report = validate(&bytes, &request)?;
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&report.to_json())
                    .map_err(|error| Error::internal(error.to_string()))?
            );
        } else {
            println!("{} {}", report.id, report.version);
            println!("package {}", report.package);
            println!("signer {}", report.signer.as_deref().unwrap_or("unsigned"));
            for export in &report.exports {
                println!("export {export}");
            }
            for import in &report.imports {
                println!("import {import}");
            }
            if let Some(service) = &report.service {
                println!("service {service}");
            }
        }
        Ok(())
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{}: {}", error.kind().code(), error.message());
            ExitCode::from(1)
        }
    }
}

fn parse(args: impl Iterator<Item = String>) -> Result<(String, Request, bool), Error> {
    let mut request = Request::default();
    let mut json = false;
    let mut path = None;
    let mut args = args.peekable();
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            "--require-signature" => request.require_signature = true,
            "--allow-third-party-service" => request.allow_third_party_service = true,
            "--json" => json = true,
            "--known" => {
                let value = args
                    .next()
                    .ok_or_else(|| usage("`--known` needs a package".to_string()))?;
                if !value.contains(':') {
                    return Err(usage(format!("`{value}` is not a `namespace:package`")));
                }
                request.known_packages.push(value);
            }
            "--key" => {
                let value = args
                    .next()
                    .ok_or_else(|| usage("`--key` needs a public key".to_string()))?;
                if value.len() != 64 || !value.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(usage(
                        "`--key` needs 64 lowercase hex characters".to_string(),
                    ));
                }
                request.keys.push(value.to_ascii_lowercase());
            }
            "--max-bytes" => {
                let value = args
                    .next()
                    .ok_or_else(|| usage("`--max-bytes` needs a number".to_string()))?;
                request.max_bundle_bytes = value
                    .parse::<u64>()
                    .map_err(|_| usage("`--max-bytes` needs a positive number".to_string()))?;
                if request.max_bundle_bytes == 0 {
                    return Err(usage("`--max-bytes` needs a positive number".to_string()));
                }
            }
            value if value.starts_with('-') => {
                return Err(usage(format!("`{value}` is not an option")));
            }
            value => {
                if path.replace(value.to_string()).is_some() {
                    return Err(usage(
                        "only one bundle can be checked at a time".to_string(),
                    ));
                }
            }
        }
    }
    let path = path.ok_or_else(|| usage("a bundle path is required".to_string()))?;
    Ok((path, request, json))
}

fn usage(message: String) -> Error {
    Error::invalid(format!("{message}\n\n{USAGE}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn flags_are_parsed() {
        let (path, request, json) = parse(
            args(&[
                "--require-signature",
                "--known",
                "example:other",
                "--key",
                &"a".repeat(64),
                "--max-bytes",
                "1024",
                "--json",
                "app.anth",
            ])
            .into_iter(),
        )
        .expect("parse");
        assert_eq!(path, "app.anth");
        assert!(request.require_signature);
        assert_eq!(request.known_packages, ["example:other"]);
        assert_eq!(request.keys, ["a".repeat(64)]);
        assert_eq!(request.max_bundle_bytes, 1024);
        assert!(json);
    }

    #[test]
    fn a_bundle_is_required() {
        assert!(parse(args(&[]).into_iter()).is_err());
        assert!(parse(args(&["--unknown"]).into_iter()).is_err());
    }
}
