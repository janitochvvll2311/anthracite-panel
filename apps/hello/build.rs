//! Mirror the SDK's host catalog into the app's WIT directory before the
//! bindings are generated. Apps do not vendor `wit/deps`; the SDK owns it.
use std::fs;
use std::path::Path;

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let from = std::env::var_os("ANTH_GUEST_WIT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| manifest.join("../../sdk/rust/guest/wit/deps"));
    let to = manifest.join("wit/deps");
    println!("cargo:rerun-if-changed={}", from.display());
    if same(&from, &to) {
        return;
    }
    let _ = fs::remove_dir_all(&to);
    copy(&from, &to);
}

/// The catalog is small; mirror only when the bytes differ, so a normal build
/// does not touch the source tree.
fn same(from: &Path, to: &Path) -> bool {
    let Ok(entries) = fs::read_dir(from) else {
        return false;
    };
    let mut expected = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            return false;
        };
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            if !same(&path, &target) {
                return false;
            }
        } else {
            expected.push((path, target));
        }
    }
    expected
        .into_iter()
        .all(|(source, target)| fs::read(&source).ok() == fs::read(&target).ok() && target.exists())
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
