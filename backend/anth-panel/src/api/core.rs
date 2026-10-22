use super::prelude::*;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{get, patch, post};
use rust_embed::RustEmbed;

use crate::auth;
use crate::config_api;

use super::accounts::*;
use super::archive::*;
use super::echo::*;
use super::files::*;
use super::firewall::*;
use super::host::*;
use super::network::*;
use super::plugins::*;
use super::process::*;
use super::terminal::ws_terminal;
use super::trash::*;
use super::volumes::*;

pub fn router(app: AppState) -> Router {
    let upload_limit = usize::try_from(app.max_upload_bytes())
        .unwrap_or(usize::MAX)
        .max(1);

    Router::new()
        .route("/healthz", get(get_healthz))
        .route("/api/auth/login", post(auth::routes::post_login))
        .route("/api/auth/refresh", post(auth::routes::post_refresh))
        .route("/api/auth/logout", post(auth::routes::post_logout))
        .route("/api/auth/session", get(auth::routes::get_session))
        .route("/api/auth/password", post(auth::routes::post_password))
        .route(
            "/api/config",
            get(config_api::get_config).patch(config_api::patch_config),
        )
        .route("/api/host-info", get(get_host_info))
        .route("/api/host-info/watch", get(watch_host_info))
        .route("/api/echo", post(post_echo))
        .route("/api/terminal", get(ws_terminal))
        .route("/api/processes", get(get_processes))
        .route("/api/processes/watch", get(watch_processes))
        .route("/api/processes/{pid}/signal", post(post_process_signal))
        .route("/api/processes/spawn", post(post_process_spawn))
        .route("/api/network", get(get_network))
        .route("/api/network/watch", get(watch_network))
        .route("/api/network/{name}/state", post(post_network_state))
        .route("/api/network", post(post_network_create))
        .route("/api/network/{name}", patch(patch_network))
        .route("/api/network/{name}/rename", post(post_network_rename))
        .route("/api/network/{name}/delete", post(post_network_delete))
        .route("/api/firewall", get(get_firewall))
        .route("/api/firewall/rules", post(post_firewall_rule))
        .route(
            "/api/firewall/rules/{handle}/delete",
            post(post_firewall_delete),
        )
        .route("/api/firewall/flush", post(post_firewall_flush))
        .route("/api/accounts", get(get_accounts))
        .route("/api/accounts/users", post(post_account_create))
        .route("/api/accounts/users/{name}", post(post_account_modify))
        .route(
            "/api/accounts/users/{name}/delete",
            post(post_account_delete),
        )
        .route(
            "/api/accounts/users/{name}/password",
            post(post_account_password),
        )
        .route("/api/accounts/groups", post(post_account_group_create))
        .route(
            "/api/accounts/groups/{name}",
            post(post_account_group_modify),
        )
        .route(
            "/api/accounts/groups/{name}/delete",
            post(post_account_group_delete),
        )
        .route("/api/plugins", get(get_plugins))
        // Static assets first: axum picks the most specific match, but the
        // intent is worth reading in order. Assets never reach a guest.
        .route(
            "/api/plugins/{id}/web/{*path}",
            get(crate::plugins::web::asset),
        )
        // One wildcard, not one route per declared anchor: the manifest is a
        // file inside the bundle, so authorization reads the grant, never it.
        .route(
            "/api/plugins/{id}/{*path}",
            get(crate::plugins::api::anchor)
                .post(crate::plugins::api::anchor)
                .put(crate::plugins::api::anchor)
                .patch(crate::plugins::api::anchor)
                .delete(crate::plugins::api::anchor),
        )
        .route("/api/volumes", get(get_volumes))
        .route("/api/volumes/mount", post(post_volume_mount))
        .route("/api/volumes/unmount", post(post_volume_unmount))
        .route(
            "/api/volumes/partitions",
            get(get_partitions).post(post_partition_create),
        )
        .route("/api/volumes/partitions/edit", post(post_partition_edit))
        .route("/api/files", get(get_files))
        .route("/api/files/content", get(get_file_content))
        .route("/api/files/download", get(download_file))
        .route("/api/files/mkdir", post(post_file_mkdir))
        .route("/api/files/rename", post(post_file_rename))
        .route("/api/files/delete", post(post_file_delete))
        .route("/api/files/trash", post(post_file_trash))
        .route("/api/files/trash/list", get(get_trash))
        .route("/api/files/trash/restore", post(post_trash_restore))
        .route("/api/files/trash/empty", post(post_trash_empty))
        .route("/api/files/archive", post(post_archive_create))
        .route("/api/files/archive/list", get(get_archive_listing))
        .route("/api/files/archive/extract", post(post_archive_extract))
        .route("/api/files/write", post(post_file_write))
        .route("/api/files/upload", post(post_file_upload))
        .layer(DefaultBodyLimit::max(upload_limit))
        .fallback(static_handler)
        .layer(axum::middleware::from_fn_with_state(
            app.clone(),
            crate::auth::middleware::require_auth,
        ))
        .with_state(app)
}

pub async fn get_healthz() -> &'static str {
    "ok"
}

#[derive(RustEmbed)]
#[folder = "www"]
struct Asset;

/// Serves the embedded SPA, and answers an unknown `/api` path with JSON.
///
/// Falling through to `index.html` would answer `200 text/html` for a typo in a
/// client, which is indistinguishable from a working route until the JSON parse
/// fails. The SPA fallback stays for browser routes only.
async fn static_handler(uri: Uri) -> impl IntoResponse {
    if uri.path() == "/api" || uri.path().starts_with("/api/") {
        return ApiError::new(StatusCode::NOT_FOUND, "no such API route").into_response();
    }

    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Asset::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            Response::builder()
                .header("content-type", mime.as_ref())
                .body(Body::from(content.data))
                .unwrap()
        }
        None => {
            if let Some(content) = Asset::get("index.html") {
                Response::builder()
                    .header("content-type", "text/html")
                    .body(Body::from(content.data))
                    .unwrap()
            } else {
                StatusCode::NOT_FOUND.into_response()
            }
        }
    }
}
