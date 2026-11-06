//! The host WIT under `engine/wit` is the source of truth for the native set
//! (§7.3, §9.2). This test keeps it and `anth_contract::catalog::NATIVE` from
//! drifting apart.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    let mut dir = std::env::current_dir().expect("current directory");
    loop {
        if dir.join("tests/fixtures").is_dir() {
            return dir;
        }
        if !dir.pop() {
            panic!("no tests/fixtures above the current directory");
        }
    }
}

fn interface_ids() -> BTreeSet<String> {
    let root = repo_root().join("engine/wit");
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
fn the_hello_app_resolves_against_its_packaged_catalog() {
    let root = repo_root().join("apps/hello/wit");
    let mut resolve = wit_parser::Resolve::default();
    let (package, _) = resolve
        .push_dir(&root)
        .unwrap_or_else(|error| panic!("parsing {}: {error:#}", root.display()));
    let name = &resolve.packages[package].name;
    assert_eq!(name.namespace, "anthracite");
    assert_eq!(name.name, "hello");
    assert_eq!(
        name.interface_id("greeting"),
        "anthracite:hello/greeting@1.0.0"
    );
    let imports = world_imports(&resolve, package, "app");
    assert!(imports.contains("anth:env/time@1.0.0"), "{imports:?}");
}

#[test]
fn the_hello_catalog_matches_the_engine_catalog() {
    use std::fs;
    let engine = repo_root().join("engine/wit/deps");
    let hello = repo_root().join("apps/hello/wit/deps");
    for package in ["error", "env", "store"] {
        let from = engine.join(package);
        let to = hello.join(package);
        let mut expected = fs::read_dir(&from)
            .expect("engine deps")
            .map(|entry| entry.expect("entry").file_name())
            .collect::<Vec<_>>();
        let mut actual = fs::read_dir(&to)
            .expect("hello deps")
            .map(|entry| entry.expect("entry").file_name())
            .collect::<Vec<_>>();
        expected.sort();
        actual.sort();
        assert_eq!(expected, actual, "package {package}");
        for file in expected {
            assert_eq!(
                fs::read(from.join(&file)).expect("engine file"),
                fs::read(to.join(&file)).expect("hello file"),
                "{} drifted",
                file.to_string_lossy()
            );
        }
    }
}

#[test]
fn an_app_package_resolves_against_the_catalog() {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = repo_root().join("engine/wit");
    let dir = std::env::temp_dir().join("anthracite-panel").join(format!(
        "anth-wit-app-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    for package in ["error", "env", "store"] {
        let from = root.join("deps").join(package);
        let to = dir.join("deps").join(package);
        fs::create_dir_all(&to).expect("mkdir");
        for entry in fs::read_dir(&from).expect("read_dir") {
            let entry = entry.expect("entry");
            fs::copy(entry.path(), to.join(entry.file_name())).expect("copy");
        }
    }
    fs::write(
        dir.join("app.wit"),
        "package example:notes@1.0.0;\n\n\
         interface notes {\n\
         \x20   use anth:error/kinds@1.0.0.{error};\n\
         \x20   all: func() -> result<list<string>, error>;\n\
         }\n\n\
         world app {\n\
         \x20   import anth:store/kv@1.0.0;\n\
         \x20   export notes;\n\
         }\n",
    )
    .expect("app.wit");
    let mut resolve = wit_parser::Resolve::default();
    let (package, _) = resolve
        .push_dir(&dir)
        .unwrap_or_else(|error| panic!("resolving {}: {error:#}", dir.display()));
    let package = &resolve.packages[package];
    assert_eq!(
        package.name.interface_id("notes"),
        "example:notes/notes@1.0.0"
    );
    assert!(package.interfaces.contains_key("notes"));
    let world = *package.worlds.get("app").expect("the app world");
    let imports: Vec<String> = resolve.worlds[world]
        .imports
        .keys()
        .filter_map(|key| match key {
            wit_parser::WorldKey::Interface(id) => {
                let interface = &resolve.interfaces[*id];
                let name = interface.name.as_deref().expect("named");
                let owner = interface.package.expect("owned");
                Some(resolve.packages[owner].name.interface_id(name))
            }
            wit_parser::WorldKey::Name(_) => None,
        })
        .collect();
    assert!(
        imports.iter().any(|id| id == "anth:store/kv@1.0.0"),
        "{imports:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

fn world_imports(
    resolve: &wit_parser::Resolve,
    package: wit_parser::PackageId,
    name: &str,
) -> BTreeSet<String> {
    let package = &resolve.packages[package];
    let world = *package.worlds.get(name).expect("the world");
    let world = &resolve.worlds[world];
    let mut imports = BTreeSet::new();
    for key in world.imports.keys() {
        if let wit_parser::WorldKey::Interface(id) = key {
            let interface = &resolve.interfaces[*id];
            let name = interface.name.as_deref().expect("a named interface");
            let owner = interface.package.expect("a named interface has a package");
            imports.insert(resolve.packages[owner].name.interface_id(name));
        }
    }
    imports
}

#[test]
fn the_host_world_imports_the_native_set() {
    let root = repo_root().join("engine/wit");
    let mut resolve = wit_parser::Resolve::default();
    let (package, _) = resolve
        .push_dir(&root)
        .unwrap_or_else(|error| panic!("parsing {}: {error:#}", root.display()));
    let imports = world_imports(&resolve, package, "host");
    let native: BTreeSet<String> = anth_contract::catalog::NATIVE
        .iter()
        .map(|entry| entry.id.to_string())
        .chain(["anth:error/kinds@1.0.0".to_string()])
        .collect();
    assert_eq!(imports, native);
}

#[test]
fn host_runtime_is_the_same_catalog_as_host() {
    let root = repo_root().join("engine/wit");
    let mut resolve = wit_parser::Resolve::default();
    let (package, _) = resolve
        .push_dir(&root)
        .unwrap_or_else(|error| panic!("parsing {}: {error:#}", root.display()));
    let host = world_imports(&resolve, package, "host");
    let runtime = world_imports(&resolve, package, "host-runtime");
    assert_eq!(host, runtime);
    assert!(runtime.contains("anth:store/kv@1.0.0"));
}
