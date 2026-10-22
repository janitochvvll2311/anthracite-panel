//! HTTP API: one module per command family, plus this router.
//!
//! Every route names a command, so a body carries the command's arguments with
//! no `type`/`data` envelope; the envelope stays an `anth-proto` concern for
//! socket clients. See `docs/api-endpoints.md`.
//!
//! The split is by command family, not by layer, so a change to the network
//! routes touches `network.rs` and nothing else. `prelude` exists only to keep
//! the shared imports in one place; a module that needs something else imports
//! it directly.

pub(crate) mod prelude {
    pub use std::net::SocketAddr;
    pub use std::str::FromStr;
    pub use std::sync::Arc;

    pub use axum::Json;
    pub use axum::body::{Body, Bytes};
    pub use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
    pub use axum::extract::ws::WebSocketUpgrade;
    pub use axum::extract::{ConnectInfo, Path, Query, State};
    pub use axum::http::{Response, StatusCode, Uri};
    pub use axum::response::IntoResponse;
    pub use serde::{Deserialize, Serialize};
    pub use serde_json::Value;

    pub use anth_proto::{
        AccountError, AccountErrorKind, AccountSnapshot, AnthClient, ArchiveFormat, ArchiveListing,
        DEFAULT_FILE_CHUNK, DEFAULT_TERMINAL_SIZE, FileChunk, FileError, FileErrorKind,
        FileListing, FileWritten, FirewallAction, FirewallError, FirewallErrorKind,
        FirewallProtocol, FirewallSnapshot, NetworkCreateArgs, NetworkEditArgs, NetworkError,
        NetworkErrorKind, NetworkInterfaceKind, NetworkSnapshot, ProcessSignal, ProcessSnapshot,
        ProcessSpawnErrorKind, Request, Response as CommandResponse, TerminalSize, TrashListing,
        VolumeError, VolumeErrorKind, VolumeSnapshot,
    };

    pub use crate::app::AppState;
    pub use crate::poll;
    pub use crate::sse::SseStream;
    pub use crate::stream;

    pub(crate) use super::error::{ApiError, gateway, unexpected_response};
    pub(crate) use super::shared::{
        WatchQuery, exchange, get_snapshot, query_command, watch_snapshots,
    };
}

mod accounts;
mod archive;
mod core;
mod echo;
mod error;
mod files;
mod firewall;
mod guards;
mod host;
mod network;
mod plugins;
mod process;
mod shared;
mod terminal;
mod trash;
mod volumes;

pub use core::{get_healthz, router};
pub use error::ApiError;

// Kept at the module root because `lib.rs` re-exports them: an embedder of the
// crate (and the router tests) reach a handler by `anth_panel::api::<name>`.
pub use accounts::*;
pub use archive::*;
pub use echo::*;
pub use files::*;
pub use firewall::*;
pub use host::*;
pub use network::*;
pub use plugins::*;
pub use process::*;
pub use terminal::ws_terminal;
pub use trash::*;
pub use volumes::*;
