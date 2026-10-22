//! The plugin contract for Anthracite Panel.
//!
//! A plugin is two halves — a wasm module the daemon runs and a browser module
//! the panel mounts — and each half had its contract written out by hand in every
//! plugin. This crate is the daemon-facing half of that contract, so a plugin
//! author writes behaviour and nothing else.
//!
//! ```
//! use anth_plugin::{Guest, Reply, Request, Router, Method, HttpRequest};
//!
//! #[derive(Default)]
//! struct Hello {
//!     router: Router,
//! }
//!
//! impl Hello {
//!     fn greeting(executor: &mut dyn anth_plugin::Executor, _request: &HttpRequest) -> Reply {
//!         match executor.exec_bare("host.hostname") {
//!             Ok(reply) => {
//!                 let name = reply.get("stdout").and_then(|v| v.as_str()).unwrap_or("unknown");
//!                 Reply::message(format!("hola {name}"))
//!             }
//!             Err(detail) => Reply::failed(detail),
//!         }
//!     }
//! }
//!
//! impl Guest for Hello {
//!     fn handle(&mut self, request: Request) -> Reply {
//!         match request {
//!             Request::Http(_) => Reply::ok(serde_json::json!({})),
//!             Request::Cli(_) => Reply::ok(serde_json::json!({})),
//!         }
//!     }
//! }
//! ```
//!
//! What the crate deliberately does *not* do: it never decides which capability a
//! plugin may run (the manifest and the host's grant do), it never reads the
//! filesystem, and it never executes a program. Those are the host's authority,
//! and a library a guest links must not be able to widen it.

#![no_std]

extern crate alloc;

// The library is `no_std` because it is linked into a wasm guest, but its tests
// run on the host and use the standard library's test harness.
#[cfg(test)]
extern crate std;

pub mod abi;
pub mod envelope;
pub mod exec;

pub use abi::{
    ABI_VERSION, Guest, HostExecutor, alloc_string, answer, free_string, read_string, write_string,
};
pub use envelope::{CliRequest, HttpRequest, Reply, Request, decode};
pub use exec::{Executor, Handler, Method, Router};

#[cfg(test)]
mod tests;
