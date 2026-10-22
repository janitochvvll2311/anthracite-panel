//! The subcommand implementations, one module per concern.
//!
//! `main` parses and calls `dispatch`; everything below that is here.

pub mod client;
pub mod confirm;
pub mod dispatch;
pub mod plugin_cmd;
pub mod server;
pub mod settings_cmd;

#[cfg(test)]
mod tests;
