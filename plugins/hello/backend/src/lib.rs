//! The hello plugin's wasm entry point.
//!
//! Behaviour lives in [`logic`], which is plain Rust: it names a capability
//! through [`anth_plugin::Executor`] and never touches an import, a pointer or
//! the allocator, so it runs as a normal test on the host. This file is only the
//! wiring between that logic and the ABI the daemon requires, and the ABI itself
//! comes from `anth-plugin`.

pub mod logic;

use anth_plugin::{Command, Guest, Reply, Router};

/// The guest the host calls.
pub struct Hello {
    router: Router,
}

impl Default for Hello {
    fn default() -> Self {
        Self {
            router: Router::new()
                .get(logic::GREETING_PATH, logic::greeting)
                .get(logic::STATUS_PATH, logic::status),
        }
    }
}

impl Guest for Hello {
    fn handle(&mut self, command: Command) -> Reply {
        // One path, whichever transport delivered the call: the crate has already
        // folded an HTTP anchor and a `plugin call` into the same `Command`, so
        // every route is reachable from the CLI without a second table here.
        self.router
            .dispatch(&mut anth_plugin::HostExecutor, &command)
    }
}

anth_plugin::anth_plugin!(Hello);
