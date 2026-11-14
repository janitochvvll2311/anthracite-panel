//! The app is outside the repository's app workspace, so it carries the SDK's
//! host catalog into its own `wit/deps`: the bundle is packaged without it and
//! the engine injects the catalog it actually runs (§7.3).
use std::fs;
use std::path::Path;

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let from = manifest.join("../../../sdk/rust/guest/wit/deps");
    let to = manifest.join("wit/deps");
    println!("cargo:rerun-if-changed={}", from.display());
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
