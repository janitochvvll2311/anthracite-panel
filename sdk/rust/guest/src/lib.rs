//! The guest SDK (§7.1).
//!
//! An app's own `wit/` declares its world; this crate re-exports the generator
//! and carries the host catalog that the build and the engine inject. Keeping
//! the dependency here is what lets the engine change the generator without
//! every app editing its Cargo.toml.

pub use wit_bindgen;
pub use wit_bindgen::*;

/// The host WIT catalog (`wit/deps`) shipped with this crate. A standalone app
/// build script copies it into the app's `wit/` directory; a published crate
/// resolves it inside the registry cache.
pub fn wit_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("wit/deps")
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_catalog_ships_with_the_crate() {
        let catalog = super::wit_path();
        assert!(
            catalog.join("store/store.wit").is_file(),
            "{}",
            catalog.display()
        );
        assert!(catalog.join("env/env.wit").is_file());
    }
}
