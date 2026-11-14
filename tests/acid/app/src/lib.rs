//! The acid-test app: authored outside `apps/`, one imported capability, one
//! exported function, one route and one panel module.

anth_guest::wit_bindgen::generate!({
    path: "wit",
    world: "app",
    runtime_path: "anth_guest::wit_bindgen::rt",
    with: {
        "anth:env/time@1.0.0": generate,
    },
});

use exports::acid::ping::ping::Guest;

struct Ping;

impl Guest for Ping {
    fn now() -> String {
        let now = anth::env::time::now_ms();
        format!("pong at {now} ms")
    }
}

export!(Ping);
