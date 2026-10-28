//! The plugin contract for Anthracite Panel.
//!
//! A plugin declares its own vocabulary — [`Action`]s and the [`Response`]s they
//! produce — and the crate keeps every transport out of it. There are three
//! pipes and each has its own type: `anth_http`, `anth_cli` and `anth_direct`.
//!
//! The crate is `no_std`; the generated exports live in [`abi`].

#![no_std]

extern crate alloc;

pub mod abi;
pub mod exec;
mod message;
pub mod plugin;
pub mod transport;

pub use abi::{
    ABI_VERSION, HostExecutor, LogLevel, alloc_string, answer_cli, answer_direct, answer_http,
    free_string, read_string, write_string,
};
pub use exec::Executor;
pub use message::{Action, Request, Response, Status};
pub use plugin::Plugin;
pub use transport::{CliRequest, HttpRequest, Method};

#[cfg(test)]
mod tests;
