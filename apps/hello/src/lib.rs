//! The smallest app: one imported host interface, one exported function, one
//! route. It is built for `wasm32-unknown-unknown` and wrapped by
//! `scripts/build-app.sh`.

// The guest SDK re-exports the generator; `runtime_path` points the generated
// code at it instead of requiring a direct `wit-bindgen` dependency.

anth_guest::wit_bindgen::generate!({
    path: "wit",
    world: "app",
    runtime_path: "anth_guest::wit_bindgen::rt",
    with: {
        "anth:env/time@1.0.0": generate,
    },
});

use exports::anthracite::hello::greeting::Guest;

struct Hello;

impl Guest for Hello {
    fn greet(name: String) -> String {
        let now = anth::env::time::now_ms();
        format!("hello, {name} (at {now} ms)")
    }
}

export!(Hello);
