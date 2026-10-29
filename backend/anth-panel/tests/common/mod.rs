//! Shared helpers for the transport tests.
#![allow(dead_code)]

use std::sync::Arc;

use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::config::{AuthConfig, Paths};
use anth_panel::dispatch::Daemon;
use anth_panel::log::{Logger, Settings};

/// Builds the real router with web authentication off, for the local socket.
///
/// The socket tests exercise the same handlers the browser reaches; only the
/// authentication path differs, and the listener adds that.
pub fn local_router(daemon: Arc<Daemon>, paths: &Paths) -> axum::Router {
    let (auth, _) = AuthService::load(
        &AuthConfig {
            enabled: false,
            ..AuthConfig::default()
        },
        paths,
    )
    .expect("auth");
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    api::router(AppState::new(daemon, auth, logger))
}
