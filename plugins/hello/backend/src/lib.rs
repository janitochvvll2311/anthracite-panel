//! The hello plugin's wasm entry point.
//!
//! Behaviour lives in [`logic`], which is plain Rust: it names a capability
//! through [`anth_plugin::Executor`] and never touches an import, a pointer or
//! the allocator, so it runs as a normal test on the host. This file is only the
//! wiring between that logic and the ABI the daemon requires, and the ABI itself
//! comes from `anth-plugin`.

pub mod logic;

use anth_plugin::{Guest, Reply, Request, Router};

/// The guest the host calls.
pub struct Hello {
    router: Router,
}

impl Default for Hello {
    fn default() -> Self {
        Self {
            router: Router::new().get(logic::GREETING_PATH, logic::greeting),
        }
    }
}

impl Guest for Hello {
    fn handle(&mut self, request: Request) -> Reply {
        match request {
            Request::Http(request) => self
                .router
                .dispatch(&mut anth_plugin::HostExecutor, &request),
            // `plugin call <id>` carries no method or path, so it answers the
            // plugin's default anchor: the same greeting a browser gets.
            Request::Cli(_) => self
                .router
                .dispatch(&mut anth_plugin::HostExecutor, &logic::greeting_request()),
        }
    }
}

anth_plugin::anth_plugin!(Hello);
