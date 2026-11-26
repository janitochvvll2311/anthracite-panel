//! The wasm component. Replace `ping` with the app's exports.

wit_bindgen::generate!({
    path: "wit",
    world: "app",
});

use exports::example::sdk_rust::api::Guest;

struct App;

impl Guest for App {
    fn ping() -> String {
        "pong".to_string()
    }
}

export!(App);
