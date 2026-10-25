//! What the daemon does with a request.
//!
//! v0.1.0 required a new operation to appear in two protocol enums plus three
//! hand-written `match` arms (50 in `handler.rs`, 43 in `client.rs`, 9 in
//! `daemon.rs`), with nothing tying them together. Here there is one `match`:
//! the socket loop, the CLI client and the HTTP route all call [`Daemon::handle`],
//! so a new operation is one arm rather than a variant repeated across three
//! files.

use std::sync::Arc;

use anth_wire::{ErrorKind, PluginSummary, Request, Response, WireError};

use crate::log::{Level, Logger};
use crate::plugins::registry::Registry;

/// Everything a request may need from the daemon.
///
/// The registry is loaded once at startup and shared, because compiling a wasm
/// module per request would put that cost on the request path and re-verify a
/// digest that cannot have changed while the process runs.
pub struct Daemon {
    plugins: Arc<Registry>,
    log: Arc<Logger>,
    /// The only tree `Request::FileRead` may reach.
    ///
    /// Held here rather than passed per call so no transport can widen it: the
    /// CLI, the HTTP route and a future caller all go through this one value.
    readable_root: Arc<std::path::PathBuf>,
}

impl Daemon {
    pub fn new(
        plugins: Registry,
        log: Arc<Logger>,
        readable_root: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self {
            plugins: Arc::new(plugins),
            log,
            readable_root: Arc::new(readable_root.into()),
        }
    }

    /// A daemon whose readable root is the temp directory.
    ///
    /// Tests that do not exercise `FileRead` should not have to name a root, and
    /// one helper is what keeps `Daemon::new` from growing an argument at each
    /// call site.
    #[cfg(test)]
    pub fn for_test(plugins: Registry, log: Arc<Logger>) -> Self {
        Self::new(plugins, log, std::env::temp_dir())
    }

    /// One plugin's own anchor, resolved by id or alias rather than by scanning.
    ///
    /// Scoped because two plugins may legitimately declare the same path: a
    /// caller that names its plugin is never ambiguous, so this is the form the
    /// browser uses and the only anchor lookup the router performs.
    pub fn anchor_for(
        &self,
        id: &str,
        method: &str,
        path: &str,
    ) -> Result<
        Option<(
            &crate::plugins::registry::Entry,
            std::sync::Arc<crate::plugins::runtime::Loaded>,
        )>,
        WireError,
    > {
        self.plugins.anchor_for(id, method, path)
    }

    /// Whether any plugin answers to this id or alias.
    ///
    /// Separate from [`Self::ui_module`] so a `404` can say which of the two
    /// things is missing. One message for both would tell a caller who typed
    /// `/p/greeting/x` that a plugin called `greeting` declares no such
    /// module, when no plugin called `greeting` exists at all.
    pub fn has_plugin(&self, id_or_alias: &str) -> Result<bool, WireError> {
        Ok(self.plugins.resolve(id_or_alias)?.is_some())
    }

    /// One plugin's declared UI module, resolved by id or alias.
    pub fn ui_module(
        &self,
        id_or_alias: &str,
        module: &str,
    ) -> Result<Option<&crate::plugins::registry::Entry>, WireError> {
        self.plugins.ui_module(id_or_alias, module)
    }

    /// Handles one request.
    /// Answers one request and records what it was and what it cost.
    ///
    /// The timings start here, at the one place every transport funnels through,
    /// so the socket, the HTTP route and a direct call are all measured the same
    /// way. A failure is logged by its kind and status, never by the plugin's own
    /// payload, which is the guest's text and is logged where it is produced.
    pub async fn handle(&self, request: Request) -> Response {
        // The least important line this function can emit is the failure at
        // `warn`, so that is the level the shortcut tests. Guarding on `info`
        // would skip the whole path — including the failure — whenever the
        // operator asked for `--log-level warn`, which is exactly when a refusal
        // is worth reading.
        if !self.log.enabled(Level::Warn) {
            return match dispatch(self, request).await {
                Ok(response) => response,
                Err(error) => Response::Error(error),
            };
        }

        let started = std::time::Instant::now();
        let operation = operation_of(&request);
        if self.log.enabled(Level::Debug) {
            self.log
                .debug("request", "received")
                .field("op", operation)
                .emit();
        }

        let response = match dispatch(self, request).await {
            Ok(response) => response,
            Err(error) => Response::Error(error),
        };
        let ms = started.elapsed().as_millis() as u64;
        let (status, failed) = status_of(&response);

        let event = if failed {
            self.log.warn("request", "failed")
        } else {
            self.log.info("request", "answered")
        };
        event
            .field("op", operation)
            .field("status", status)
            .field("ms", ms)
            .emit();

        response
    }
}

async fn dispatch(daemon: &Daemon, request: Request) -> Result<Response, WireError> {
    match request {
        Request::Hello { protocol_version } => hello(protocol_version),
        Request::HostInfo => Ok(Response::HostInfo(anth_host::collect())),
        Request::FileRead {
            path,
            offset,
            len,
            wait_ms,
        } => {
            // Disk I/O, like a plugin call: a slow or stalled filesystem must not
            // hold a runtime worker that could be answering `/healthz`.
            let root = Arc::clone(&daemon.readable_root);
            let asked = path.clone();
            let slice = tokio::task::spawn_blocking(move || {
                crate::files::read(&root, &asked, offset, len, wait_ms)
            })
            .await
            .map_err(|e| WireError::internal(format!("the read task failed: {e}")))??;
            Ok(Response::FileSlice {
                path,
                offset,
                bytes: slice.bytes,
                next: slice.next,
                eof: slice.eof,
                waited: slice.waited,
            })
        }
        Request::PluginList => Ok(Response::PluginList {
            plugins: daemon
                .plugins
                .entries()
                .map(|entry| PluginSummary {
                    id: entry.id.clone(),
                    alias: entry.alias.clone(),
                    name: entry.name.clone(),
                    version: entry.version.clone(),
                    anchors: entry
                        .anchors
                        .iter()
                        .map(|(method, path)| format!("{method} {path}"))
                        .collect(),
                    ui: entry.ui.clone(),
                    disabled_reason: entry.enabled.as_ref().err().cloned(),
                })
                .collect(),
        }),
        Request::PluginCall { id, pipe, payload } => {
            // Resolved by id or alias, but reported back by canonical id: a
            // caller that typed `hello` should still see `org.anth.hello` in the
            // reply, so a script that logs it records something unambiguous.
            let entry = daemon
                .plugins
                .resolve(&id)?
                .ok_or_else(|| WireError::new(ErrorKind::NotFound, format!("no plugin `{id}`")))?;
            let canonical = entry.id.clone();
            let loaded = entry.usable()?;
            let entry_name = pipe.entry().to_string();
            let encoded = encode_payload(pipe, &payload);
            let log = Arc::clone(&daemon.log);

            // A plugin call is CPU-bound wasm behind a wall-clock deadline, so it
            // must not run on a tokio worker: an invocation that sits at the
            // 30-second limit would hold the thread that could be answering
            // `/healthz`. Blocking here is what keeps the pool free.
            let reply =
                tokio::task::spawn_blocking(move || loaded.call(&entry_name, &encoded, log))
                    .await
                    .map_err(|e| WireError::internal(format!("the plugin task failed: {e}")))??;

            // A plugin's own failure is a *successful* call whose status says
            // what happened, so the caller can show the plugin's message. Only a
            // transport failure — no such plugin, disabled, trapped, out of
            // budget — becomes a `Response::Error`.
            Ok(Response::PluginReply {
                id: canonical,
                status: reply.status,
                value: reply.value,
            })
        }
    }
}

/// Puts a payload into the shape the chosen entry parses.
///
/// The CLI entry reads a JSON *string*, not a bare line, so the host encodes it
/// here rather than asking every caller to remember the quoting. A `plugin call`
/// user types `greet --name "Ana Sol"`; the guest receives
/// `"greet --name \"Ana Sol\""`, which is the same line with its boundary
/// intact. HTTP and direct payloads are already JSON, so they pass through.
pub(crate) fn encode_payload(pipe: anth_wire::PluginPipe, payload: &str) -> String {
    match pipe {
        anth_wire::PluginPipe::Cli => serde_json::Value::String(payload.to_string()).to_string(),
        anth_wire::PluginPipe::Http | anth_wire::PluginPipe::Direct => payload.to_string(),
    }
}

fn hello(protocol_version: u16) -> Result<Response, WireError> {
    if protocol_version != anth_wire::PROTOCOL_VERSION {
        return Err(WireError::invalid(format!(
            "this daemon speaks protocol {}, not {protocol_version}",
            anth_wire::PROTOCOL_VERSION
        )));
    }
    Ok(Response::Welcome {
        protocol_version: anth_wire::PROTOCOL_VERSION,
    })
}

/// A short name for what a request asked, for the log line.
///
/// Deliberately not `Debug`: a plugin call's payload can be a whole command line
/// or body, and a log that prints it would be quoting the guest into the host's
/// own line. The plugin id and the pipe are what an operator filters on.
fn operation_of(request: &Request) -> &'static str {
    match request {
        Request::Hello { .. } => "hello",
        Request::HostInfo => "host-info",
        Request::PluginList => "plugin-list",
        Request::FileRead { .. } => "file-read",
        Request::PluginCall { .. } => "plugin-call",
    }
}

/// The status a client would see, and whether it counts as a failure.
///
/// A plugin answering `4xx` is a successful *call* — the daemon did its job and
/// the status travels with the answer — but a `5xx` is the plugin failing, and an
/// operator triaging a broken plugin wants that line at `warn` without turning on
/// `debug`. A `Response::Error` is the daemon failing, and its status comes from
/// the error kind rather than being written here.
fn status_of(response: &Response) -> (u16, bool) {
    match response {
        Response::Welcome { .. }
        | Response::HostInfo(_)
        | Response::PluginList { .. }
        | Response::FileSlice { .. } => (200, false),
        Response::PluginReply { status, .. } => (*status, *status >= 500),
        Response::Error(error) => (error.kind.http_status(), true),
    }
}
