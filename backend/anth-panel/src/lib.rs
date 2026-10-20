pub mod accounts;
pub mod accounts_admin;
pub mod api;
pub mod app;
pub mod cli;
pub mod client;
pub mod command;
pub mod config;
pub mod daemon;
pub mod files;
pub mod firewall;
pub mod handler;
pub mod hostinfo;
pub mod network;
pub mod poll;
pub mod process;
pub mod render;
pub mod sse;
pub mod stream;
pub mod terminal;
pub mod terminal_client;
pub(crate) mod text;
pub mod volumes;
pub(crate) mod ws;

pub use accounts::collect as collect_accounts;
pub use api::{ApiError, get_healthz, get_host_info, post_echo, router, watch_host_info};
pub use app::AppState;
pub use cli::Cli;
pub use client::{handle_response, request_for};
pub use config::{Config, DEFAULT_CONFIG_PATH, DEFAULT_SOCKET_PATH};
pub use hostinfo::collect as collect_host_info;
pub use network::collect as collect_network;
pub use render::{
    render_account_snapshot, render_file_listing, render_firewall_snapshot, render_host_info,
    render_network_snapshot, render_process_snapshot, render_response,
};
pub use stream::{DEFAULT_INTERVAL, Frames, parse_interval};
pub use volumes::collect as collect_volumes;
