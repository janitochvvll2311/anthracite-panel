//! The host WIT under `engine/wit` is the source of truth for the native set
//! (§7.3, §9.2). This test keeps it and `anth_contract::catalog::NATIVE` from
//! drifting apart.

use std::collections::BTreeSet;
use std::path::Path;

fn interface_ids() -> BTreeSet<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../wit");
    let mut resolve = wit_parser::Resolve::default();
    resolve
        .push_dir(&root)
        .unwrap_or_else(|error| panic!("parsing {}: {error:#}", root.display()));
    let mut ids = BTreeSet::new();
    for (_, package) in resolve.packages.iter() {
        for name in package.interfaces.keys() {
            ids.insert(package.name.interface_id(name));
        }
    }
    ids
}

#[test]
fn the_host_wit_is_exactly_the_native_set_plus_anth_error() {
    let ids = interface_ids();
    let expected: BTreeSet<String> = anth_contract::catalog::NATIVE
        .iter()
        .map(|entry| entry.id.to_string())
        .chain(["anth:error/kinds@1.0.0".to_string()])
        .collect();
    assert_eq!(ids, expected);
}

#[test]
fn every_native_id_resolves_to_an_interface() {
    let ids = interface_ids();
    for entry in anth_contract::catalog::NATIVE {
        assert!(
            ids.contains(entry.id),
            "{} is in NATIVE but not in WIT",
            entry.id
        );
    }
}

#[test]
fn the_host_world_imports_the_native_set() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../wit");
    let mut resolve = wit_parser::Resolve::default();
    let (package, _) = resolve
        .push_dir(&root)
        .unwrap_or_else(|error| panic!("parsing {}: {error:#}", root.display()));
    let package = &resolve.packages[package];
    let world_id = *package.worlds.get("host").expect("the host world");
    let world = &resolve.worlds[world_id];
    let mut imports = BTreeSet::new();
    for key in world.imports.keys() {
        if let wit_parser::WorldKey::Interface(id) = key {
            let interface = &resolve.interfaces[*id];
            let name = interface.name.as_deref().expect("a named interface");
            let owner = interface.package.expect("a named interface has a package");
            let owner = &resolve.packages[owner].name;
            imports.insert(owner.interface_id(name));
        }
    }
    let native: BTreeSet<String> = anth_contract::catalog::NATIVE
        .iter()
        .map(|entry| entry.id.to_string())
        .chain(["anth:error/kinds@1.0.0".to_string()])
        .collect();
    assert_eq!(imports, native);
}
