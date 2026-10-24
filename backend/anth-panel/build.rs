//! Fails a release build that would ship without the shell.
//!
//! `www` embeds its files at compile time, and `www/` is gitignored apart from
//! `.gitkeep`. Nothing else in the build noticed: rust-embed happily embeds an
//! empty directory, `available()` then answers `false`, and the daemon tells the
//! visitor to run a frontend build — in production, where nobody can. A release
//! binary without a UI is not a smaller binary, it is a broken one.
//!
//! A debug build is deliberately exempt: it reads `www/` from disk per request,
//! so a working tree with no staged shell is the normal state before the first
//! `build-shell.sh`.

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
    let www = manifest.join("www");
    let index = www.join("index.html");

    println!("cargo:rerun-if-changed=www/index.html");

    if std::env::var("PROFILE").as_deref() == Ok("release") && !index.is_file() {
        panic!(
            "the shell is missing: {}\n\
             A release build embeds `www/`, so without it the binary serves no UI.\n\
             Run `frontend/build-shell.sh` first, or `ANTH_WEB` for a debug build.",
            index.display(),
        );
    }
}
