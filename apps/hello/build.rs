//! Mirror the SDK's host catalog into the app's WIT directory before the
//! bindings are generated. Apps do not vendor `wit/deps`; the SDK owns it.
use std::fs;
use std::path::Path;

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let from = manifest.join("../../sdk/rust/guest/wit/deps");
    let to = manifest.join("wit/deps");
    println!("cargo:rerun-if-changed={}", from.display());
    println!("cargo:rerun-if-changed=wit");
    let _ = fs::remove_dir_all(&to);
    copy(&from, &to);
}

fn copy(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap_or_else(|error| panic!("creating {}: {error}", to.display()));
    for entry in
        fs::read_dir(from).unwrap_or_else(|error| panic!("reading {}: {error}", from.display()))
    {
        let entry = entry.expect("entry");
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy(&source, &target);
        } else {
            fs::copy(&source, &target)
                .unwrap_or_else(|error| panic!("copying {}: {error}", source.display()));
        }
    }
}
