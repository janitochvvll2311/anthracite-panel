//! One typed HTTP client for the Anthracite Panel API.
//!
//! The same methods work over the local Unix socket and over TCP. Local
//! requests are authenticated by peer credentials in the daemon; remote
//! requests may carry a bearer token. Nothing here knows about the web UI.
//!
//! ```no_run
//! # async fn example() -> Result<(), anth_wire::WireError> {
//! use anth_client::Client;
//! let client = Client::unix("/run/anth/anth.sock");
//! let info = client.host_info().await?;
//! println!("{}", info.hostname);
//! # Ok(())
//! # }
//! ```

mod sse;
mod terminal;
mod transport;

use anth_wire::{
    AccountCreate, AccountDelete, AccountGroupCreate, AccountGroupDelete, AccountSetPassword,
    AccountSnapshot, ArchiveCreated, ArchiveExtracted, ArchiveFormat, ArchiveListing, Change,
    FileChunk, FileDeleted, FileEntry, FileRestored, FileTrashed, FileWritten, FirewallAction,
    FirewallChain, FirewallProtocol, FirewallSnapshot, Hello, HostInfo, NetworkCreate, NetworkEdit,
    NetworkSnapshot, Page, PageRequest, ProcessSignal,
};
use anth_wire::{
    ProcessSignalled, ProcessSnapshot, ProcessStarted, Rebooting, Restarting, TerminalSize,
    TrashEmptied, TrashListing, VolumeFormatRequest, VolumeMountRequest, VolumePartitionCreate,
    VolumePartitionDelete, VolumePartitionResize, VolumeSnapshot, VolumeUnmountRequest, WireError,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use hyper::http::{HeaderMap, Method, StatusCode};
pub use sse::EventStream;
pub use terminal::{TerminalReader, TerminalSession, TerminalWriter};
pub use transport::{Download, Endpoint, RawResponse, encode};

/// A configured client.
#[derive(Clone, Debug)]
pub struct Client {
    endpoint: Endpoint,
    token: Option<String>,
}

impl Client {
    /// A client for the local socket.
    pub fn unix(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            endpoint: Endpoint::Unix(path.into()),
            token: None,
        }
    }

    /// A client for a remote base address such as `http://127.0.0.1:8080`.
    pub fn http(base: impl Into<String>) -> Self {
        Self {
            endpoint: Endpoint::Http(base.into()),
            token: None,
        }
    }

    /// Builds a client from an explicit endpoint.
    pub fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            token: None,
        }
    }

    /// Adds or replaces the bearer token sent with every request.
    pub fn with_bearer(mut self, token: impl Into<String>) -> Self {
        self.token = Some(token.into());
        self
    }

    /// The endpoint this client talks to.
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    // ---- handshake -------------------------------------------------------

    /// Reads the daemon's protocol version.
    pub async fn hello(&self) -> Result<Hello, WireError> {
        self.get("/api/hello").await
    }

    // ---- host ------------------------------------------------------------

    /// One host snapshot.
    pub async fn host_info(&self) -> Result<HostInfo, WireError> {
        self.get("/api/host-info").await
    }

    /// One host snapshot per interval, in milliseconds.
    pub async fn watch_host_info(
        &self,
        interval_ms: u64,
    ) -> Result<EventStream<HostInfo>, WireError> {
        self.stream(&format!("/api/host-info/watch?interval={}ms", interval_ms))
            .await
    }

    // ---- network ---------------------------------------------------------

    /// One network inventory snapshot.
    pub async fn network(&self) -> Result<NetworkSnapshot, WireError> {
        self.get("/api/network").await
    }

    /// One network snapshot per interval, in milliseconds.
    pub async fn watch_network(
        &self,
        interval_ms: u64,
    ) -> Result<EventStream<NetworkSnapshot>, WireError> {
        self.stream(&format!("/api/network/watch?interval={interval_ms}ms"))
            .await
    }

    /// Brings one interface up or down.
    pub async fn network_set_state(&self, name: &str, up: bool) -> Result<Change, WireError> {
        let body = serde_json::json!({ "name": name, "up": up });
        self.post::<_, Change>("/api/network/state", &body).await
    }

    /// Creates one virtual interface.
    pub async fn network_create(&self, body: &NetworkCreate) -> Result<Change, WireError> {
        self.post::<_, Change>("/api/network/create", body).await
    }

    /// Changes an interface's MTU or administrative state.
    pub async fn network_edit(&self, body: &NetworkEdit) -> Result<Change, WireError> {
        self.post::<_, Change>("/api/network/edit", body).await
    }

    /// Renames one interface.
    pub async fn network_rename(&self, name: &str, new_name: &str) -> Result<Change, WireError> {
        let body = serde_json::json!({ "name": name, "new_name": new_name });
        self.post::<_, Change>("/api/network/rename", &body).await
    }

    /// Deletes one interface. The daemon refuses to cut the connection in use.
    pub async fn network_delete(&self, name: &str) -> Result<Change, WireError> {
        let body = serde_json::json!({ "name": name });
        self.post::<_, Change>("/api/network/delete", &body).await
    }

    // ---- firewall --------------------------------------------------------

    /// The managed firewall table.
    pub async fn firewall(&self) -> Result<FirewallSnapshot, WireError> {
        self.get("/api/firewall").await
    }

    /// Adds one allow or deny rule.
    pub async fn firewall_add(
        &self,
        chain: FirewallChain,
        action: FirewallAction,
        protocol: FirewallProtocol,
        port: u16,
        comment: Option<&str>,
    ) -> Result<Change, WireError> {
        let body = serde_json::json!({
            "chain": chain,
            "action": action,
            "protocol": protocol,
            "port": port,
            "comment": comment,
        });
        self.post::<_, Change>("/api/firewall/rule", &body).await
    }

    /// Changes one managed rule.
    #[allow(clippy::too_many_arguments)]
    pub async fn firewall_update(
        &self,
        handle: u64,
        chain: FirewallChain,
        action: FirewallAction,
        protocol: FirewallProtocol,
        port: u16,
        comment: Option<&str>,
    ) -> Result<Change, WireError> {
        let body = serde_json::json!({
            "handle": handle,
            "chain": chain,
            "action": action,
            "protocol": protocol,
            "port": port,
            "comment": comment,
        });
        self.post::<_, Change>("/api/firewall/update", &body).await
    }

    /// Deletes one managed rule by handle.
    pub async fn firewall_delete(&self, handle: u64) -> Result<Change, WireError> {
        let body = serde_json::json!({ "handle": handle });
        self.post::<_, Change>("/api/firewall/delete", &body).await
    }

    /// Removes every rule from one managed chain.
    pub async fn firewall_flush(&self, chain: FirewallChain) -> Result<Change, WireError> {
        let body = serde_json::json!({ "chain": chain });
        self.post::<_, Change>("/api/firewall/flush", &body).await
    }

    /// Sets the default policy of one managed base chain.
    pub async fn firewall_policy(
        &self,
        chain: FirewallChain,
        policy: FirewallAction,
    ) -> Result<Change, WireError> {
        let body = serde_json::json!({ "chain": chain, "policy": policy });
        self.post::<_, Change>("/api/firewall/policy", &body).await
    }

    // ---- storage ---------------------------------------------------------

    /// Block devices, mounts and swap.
    pub async fn volumes(&self) -> Result<VolumeSnapshot, WireError> {
        self.get("/api/volumes").await
    }

    /// Mounts one device.
    pub async fn volume_mount(&self, body: &VolumeMountRequest) -> Result<Change, WireError> {
        self.post::<_, Change>("/api/volumes/mount", body).await
    }

    /// Unmounts one target.
    pub async fn volume_unmount(&self, body: &VolumeUnmountRequest) -> Result<Change, WireError> {
        self.post::<_, Change>("/api/volumes/unmount", body).await
    }

    /// Creates one partition.
    pub async fn volume_partition_create(
        &self,
        body: &VolumePartitionCreate,
    ) -> Result<Change, WireError> {
        self.post::<_, Change>("/api/volumes/partition/create", body)
            .await
    }

    /// Resizes one partition.
    pub async fn volume_partition_resize(
        &self,
        body: &VolumePartitionResize,
    ) -> Result<Change, WireError> {
        self.post::<_, Change>("/api/volumes/partition/resize", body)
            .await
    }

    /// Deletes one partition.
    pub async fn volume_partition_delete(
        &self,
        body: &VolumePartitionDelete,
    ) -> Result<Change, WireError> {
        self.post::<_, Change>("/api/volumes/partition/delete", body)
            .await
    }

    /// Formats one partition.
    pub async fn volume_format(&self, body: &VolumeFormatRequest) -> Result<Change, WireError> {
        self.post::<_, Change>("/api/volumes/format", body).await
    }

    // ---- accounts --------------------------------------------------------

    /// Local and directory-resolved users and groups.
    pub async fn accounts(&self) -> Result<AccountSnapshot, WireError> {
        self.get("/api/accounts").await
    }

    /// Creates one local user.
    pub async fn account_create(&self, body: &AccountCreate) -> Result<Change, WireError> {
        self.post::<_, Change>("/api/accounts/users", body).await
    }

    /// Deletes one local user.
    pub async fn account_delete(&self, body: &AccountDelete) -> Result<Change, WireError> {
        self.post::<_, Change>("/api/accounts/users/delete", body)
            .await
    }

    /// Sets one local user's password.
    pub async fn account_password(&self, body: &AccountSetPassword) -> Result<Change, WireError> {
        self.post::<_, Change>("/api/accounts/users/password", body)
            .await
    }

    /// Creates one local group.
    pub async fn account_group_create(
        &self,
        body: &AccountGroupCreate,
    ) -> Result<Change, WireError> {
        self.post::<_, Change>("/api/accounts/groups", body).await
    }

    /// Deletes one local group.
    pub async fn account_group_delete(
        &self,
        body: &AccountGroupDelete,
    ) -> Result<Change, WireError> {
        self.post::<_, Change>("/api/accounts/groups/delete", body)
            .await
    }

    // ---- processes -------------------------------------------------------

    /// One page of the process table.
    pub async fn processes(
        &self,
        limit: Option<u16>,
        cursor: Option<&str>,
    ) -> Result<ProcessSnapshot, WireError> {
        let mut path = String::from("/api/processes");
        let mut query = Vec::new();
        if let Some(limit) = limit {
            query.push(format!("limit={limit}"));
        }
        if let Some(cursor) = cursor {
            query.push(format!("cursor={}", encode(cursor)));
        }
        if !query.is_empty() {
            path.push('?');
            path.push_str(&query.join("&"));
        }
        self.get(&path).await
    }

    /// One process snapshot per `interval`.
    pub async fn watch_processes(
        &self,
        interval_ms: u64,
        limit: Option<u16>,
    ) -> Result<EventStream<ProcessSnapshot>, WireError> {
        let mut path = format!("/api/processes/watch?interval={}ms", interval_ms);
        if let Some(limit) = limit {
            path.push_str(&format!("&limit={limit}"));
        }
        self.stream(&path).await
    }

    /// Sends one allowlisted signal.
    pub async fn process_signal(
        &self,
        pid: u32,
        start_ticks: u64,
        signal: ProcessSignal,
        group: bool,
    ) -> Result<ProcessSignalled, WireError> {
        let body = serde_json::json!({
            "pid": pid,
            "start_ticks": start_ticks,
            "signal": signal.as_str(),
            "group": group,
        });
        self.post("/api/processes/signal", &body).await
    }

    /// Starts one allowlisted program.
    pub async fn process_spawn(
        &self,
        program: &str,
        args: &[String],
        cwd: Option<&str>,
    ) -> Result<ProcessStarted, WireError> {
        let body = serde_json::json!({
            "program": program,
            "args": args,
            "cwd": cwd,
        });
        self.post("/api/processes/spawn", &body).await
    }

    // ---- files -----------------------------------------------------------

    /// One page of a directory listing.
    pub async fn files_list(
        &self,
        path: &str,
        request: &PageRequest,
    ) -> Result<Page<FileEntry>, WireError> {
        let mut uri = format!("/api/files?path={}", encode(path));
        if let Some(limit) = request.limit {
            uri.push_str(&format!("&limit={limit}"));
        }
        if let Some(cursor) = &request.cursor {
            uri.push_str(&format!("&cursor={}", encode(cursor)));
        }
        self.get(&uri).await
    }

    /// A chunk of a UTF-8 text file.
    pub async fn files_read(
        &self,
        path: &str,
        offset: u64,
        max_bytes: Option<u64>,
    ) -> Result<FileChunk, WireError> {
        let mut uri = format!("/api/files/read?path={}&offset={offset}", encode(path));
        if let Some(max_bytes) = max_bytes {
            uri.push_str(&format!("&max_bytes={max_bytes}"));
        }
        self.get(&uri).await
    }

    /// Streams one file as it is.
    pub async fn files_download(&self, path: &str) -> Result<Download, WireError> {
        let response = self
            .request_raw(
                Method::GET,
                &format!("/api/files/download?path={}", encode(path)),
                None,
                None,
            )
            .await?;
        if response.is_success() {
            Ok(response.body)
        } else {
            Err(response.error().await)
        }
    }

    /// Deletes one file.
    pub async fn files_delete(&self, path: &str) -> Result<FileDeleted, WireError> {
        let response = self
            .request_raw(
                Method::DELETE,
                &format!("/api/files?path={}", encode(path)),
                None,
                None,
            )
            .await?;
        finish(response).await
    }

    /// Writes one UTF-8 text file.
    pub async fn files_write(
        &self,
        path: &str,
        content: &str,
        create: bool,
    ) -> Result<FileWritten, WireError> {
        let body = serde_json::json!({ "path": path, "content": content, "create": create });
        self.post("/api/files/write", &body).await
    }

    /// Uploads bytes, base64 encoded on the wire.
    pub async fn files_upload(
        &self,
        path: &str,
        content_base64: &str,
        create: bool,
    ) -> Result<FileWritten, WireError> {
        let body = serde_json::json!({
            "path": path,
            "content_base64": content_base64,
            "create": create,
        });
        self.post("/api/files/upload", &body).await
    }

    /// Moves one entry to the trash.
    pub async fn files_trash(&self, path: &str) -> Result<FileTrashed, WireError> {
        self.post("/api/files/trash", &serde_json::json!({ "path": path }))
            .await
    }

    /// Every entry the trash holds.
    pub async fn trash_list(&self) -> Result<TrashListing, WireError> {
        self.get("/api/files/trash/list").await
    }

    /// Restores one trash entry.
    pub async fn trash_restore(
        &self,
        id: &str,
        overwrite: bool,
    ) -> Result<FileRestored, WireError> {
        self.post(
            "/api/files/trash/restore",
            &serde_json::json!({ "id": id, "overwrite": overwrite }),
        )
        .await
    }

    /// Deletes everything the trash holds.
    pub async fn trash_empty(&self) -> Result<TrashEmptied, WireError> {
        let response = self
            .request_raw(Method::POST, "/api/files/trash/empty", None, None)
            .await?;
        finish(response).await
    }

    /// Compresses one entry.
    pub async fn archive_create(
        &self,
        path: &str,
        destination: Option<&str>,
        format: Option<ArchiveFormat>,
    ) -> Result<ArchiveCreated, WireError> {
        self.post(
            "/api/files/archive",
            &serde_json::json!({
                "path": path,
                "destination": destination,
                "format": format,
            }),
        )
        .await
    }

    /// Lists one archive.
    pub async fn archive_list(&self, path: &str) -> Result<ArchiveListing, WireError> {
        self.get(&format!("/api/files/archive/list?path={}", encode(path)))
            .await
    }

    /// Extracts one archive.
    pub async fn archive_extract(
        &self,
        path: &str,
        into: Option<&str>,
    ) -> Result<ArchiveExtracted, WireError> {
        self.post(
            "/api/files/archive/extract",
            &serde_json::json!({ "path": path, "into": into }),
        )
        .await
    }

    // ---- lifecycle -------------------------------------------------------

    /// Replaces the daemon with a fresh copy of itself.
    pub async fn restart(&self) -> Result<Restarting, WireError> {
        let response = self
            .request_raw(Method::POST, "/api/restart", None, None)
            .await?;
        finish(response).await
    }

    /// Schedules a reboot.
    pub async fn reboot(&self, delay_s: u32) -> Result<Rebooting, WireError> {
        self.post("/api/reboot", &serde_json::json!({ "delay_s": delay_s }))
            .await
    }

    // ---- terminal --------------------------------------------------------

    /// Opens one terminal session over a WebSocket.
    pub async fn terminal(&self, size: TerminalSize) -> Result<TerminalSession, WireError> {
        TerminalSession::open(&self.endpoint, self.token.as_deref(), size).await
    }

    // ---- raw -------------------------------------------------------------

    /// Sends one request without a typed body, for custom implementations.
    pub async fn request_raw(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        content_type: Option<&str>,
    ) -> Result<RawResponse, WireError> {
        transport::send(
            &self.endpoint,
            self.token.as_deref(),
            method,
            path,
            body,
            content_type,
        )
        .await
    }

    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, WireError> {
        let response = self.request_raw(Method::GET, path, None, None).await?;
        finish(response).await
    }

    async fn post<B: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, WireError> {
        let bytes = transport::json_body(body)?;
        let response = self
            .request_raw(Method::POST, path, Some(bytes), Some("application/json"))
            .await?;
        finish(response).await
    }

    async fn stream<T: DeserializeOwned>(&self, path: &str) -> Result<EventStream<T>, WireError> {
        let response = self.request_raw(Method::GET, path, None, None).await?;
        if response.is_success() {
            Ok(EventStream::new(response.body))
        } else {
            Err(response.error().await)
        }
    }
}

async fn finish<T: DeserializeOwned>(response: RawResponse) -> Result<T, WireError> {
    if response.is_success() {
        response.json().await
    } else {
        Err(response.error().await)
    }
}
