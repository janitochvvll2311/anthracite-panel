//! `bundle-emulator`: renders the demo bundles next to the built binary.
//!
//! ```sh
//! cargo build --manifest-path demo/emulator/Cargo.toml
//! cargo run --manifest-path demo/emulator/Cargo.toml --bin bundle-emulator -- \
//!     --out demo/dist
//! ```

use std::path::PathBuf;

fn main() {
    let mut binary = None;
    let mut out = PathBuf::from("demo/dist");
    let mut args = std::env::args().skip(1);
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--binary" => {
                binary = args.next().map(PathBuf::from);
            }
            "--out" => {
                out = args.next().map(PathBuf::from).unwrap_or(out);
            }
            other => {
                eprintln!("unknown argument `{other}`");
                std::process::exit(2);
            }
        }
    }
    let binary = binary.unwrap_or_else(|| {
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|dir| dir.join("host-emulator")))
            .unwrap_or_else(|| PathBuf::from("target/debug/host-emulator"))
    });
    match anth_host_emulator::bundle::build(&binary, &out) {
        Ok(built) => {
            for bundle in built {
                println!("{} -> {}", bundle.app, bundle.path.display());
            }
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}
