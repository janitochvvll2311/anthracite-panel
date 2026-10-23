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

use crate::plugins::registry::Registry;

/// Everything a request may need from the daemon.
///
/// The registry is loaded once at startup and shared, because compiling a wasm
/// module per request would put that cost on the request path and re-verify a
/// digest that cannot have changed while the process runs.
pub struct Daemon {
    plugins: Arc<Registry>,
}

impl Daemon {
    pub fn new(plugins: Registry) -> Self {
        Self {
            plugins: Arc::new(plugins),
        }
    }

    /// The plugin that declares an anchor, if exactly one does.
    pub fn anchor_owner(
        &self,
        method: &str,
        path: &str,
    ) -> Result<
        Option<(
            &crate::plugins::registry::Entry,
            &crate::plugins::runtime::Loaded,
        )>,
        WireError,
    > {
        self.plugins.anchor_owner(method, path)
    }

    /// One plugin's own anchor, resolved by id rather than by scanning.
    ///
    /// Scoped because two plugins may legitimately declare the same path: the
    /// unscoped lookup has to answer that as a conflict, but a caller that names
    /// its plugin is never ambiguous, so this is the form the browser uses.
    pub fn anchor_for(
        &self,
        id: &str,
        method: &str,
        path: &str,
    ) -> Result<
        Option<(
            &crate::plugins::registry::Entry,
            &crate::plugins::runtime::Loaded,
        )>,
        WireError,
    > {
        let Some(entry) = self.plugins.resolve(id)? else {
            return Ok(None);
        };
        let declared = entry
            .anchors
            .iter()
            .any(|(m, p)| m.eq_ignore_ascii_case(method) && p == path);
        if !declared {
            return Ok(None);
        }
        Ok(Some((entry, entry.usable()?)))
    }

    /// Handles one request.
    pub async fn handle(&self, request: Request) -> Response {
        match dispatch(self, request) {
            Ok(response) => response,
            Err(error) => Response::Error(error),
        }
    }
}

fn dispatch(daemon: &Daemon, request: Request) -> Result<Response, WireError> {
    match request {
        Request::Hello { protocol_version } => hello(protocol_version),
        Request::HostInfo => Ok(Response::HostInfo(anth_host::collect())),
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
            let reply = loaded.call(pipe.entry(), &encode_payload(pipe, &payload))?;

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
