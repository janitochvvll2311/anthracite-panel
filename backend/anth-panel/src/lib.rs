//! Anthracite Panel: shared library for the daemon, CLI and HTTP API.

pub mod api;
pub mod app_host;
pub mod apps;
pub mod auth;
pub mod client;
pub mod config;
pub mod dispatch;
pub mod lifecycle;
pub mod log;
pub mod migrate;
pub mod render;
pub mod socket;
pub mod state;
pub mod terminal_client;
pub mod terminal_web;
pub(crate) mod ws;
pub mod www;

pub use anth_host::{accounts, files, firewall, network, process, terminal, volumes};
pub use config::Config;
pub use log::{Logger, Settings};
