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
fn encode_payload(pipe: anth_wire::PluginPipe, payload: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use anth_wire::PluginPipe;

    fn daemon_without_plugins() -> Daemon {
        Daemon::new(Registry::empty())
    }

    #[tokio::test]
    async fn an_empty_registry_answers_a_list_with_nothing_in_it() {
        let response = daemon_without_plugins().handle(Request::PluginList).await;

        match response {
            Response::PluginList { plugins } => assert!(plugins.is_empty()),
            other => panic!("expected a list, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn calling_an_unknown_plugin_is_a_404() {
        let response = daemon_without_plugins()
            .handle(Request::PluginCall {
                id: "org.nobody".to_string(),
                pipe: PluginPipe::Cli,
                payload: "greet".to_string(),
            })
            .await;

        let error = response.error().expect("must be an error");
        assert_eq!(error.kind, ErrorKind::NotFound);
        assert!(error.message.contains("org.nobody"));
    }

    /// The alias is a way to *name* a plugin, not a second identity: a call that
    /// used it must land on the same plugin, and the refusal it produces names
    /// the canonical id so a caller can tell which one it reached.
    #[tokio::test]
    async fn a_call_by_alias_reaches_the_plugin_that_declared_it() {
        let daemon = Daemon::new(Registry::with_disabled(
            "org.test.aliased",
            Some("short"),
            &[],
            "the bundle was rewritten",
        ));

        let response = daemon
            .handle(Request::PluginCall {
                id: "short".to_string(),
                pipe: PluginPipe::Cli,
                payload: "greet".to_string(),
            })
            .await;

        let error = response.error().expect("a disabled plugin must refuse");
        assert_eq!(error.kind, ErrorKind::Forbidden);
        assert!(
            error.message.contains("org.test.aliased"),
            "the canonical id must be named, got: {}",
            error.message
        );
    }

    /// An alias resolves a *plugin*, so an anchor lookup scoped to it must find
    /// the same one its id would.
    #[tokio::test]
    async fn an_anchor_lookup_accepts_the_alias() {
        let daemon = Daemon::new(Registry::with_disabled(
            "org.test.aliased",
            Some("short"),
            &[("GET", "/greeting")],
            "disabled",
        ));

        // Both forms must reach the same plugin. This entry is disabled, so both
        // report the same refusal naming the canonical id: an alias is a way to
        // name a plugin, not a separate lookup path with its own rules.
        let by_alias = daemon.anchor_for("short", "GET", "/greeting").unwrap_err();
        let by_id = daemon
            .anchor_for("org.test.aliased", "GET", "/greeting")
            .unwrap_err();
        assert_eq!(by_alias.kind, by_id.kind);
        assert_eq!(by_alias.message, by_id.message);
        assert!(
            by_alias.message.contains("org.test.aliased"),
            "{}",
            by_alias.message
        );

        // A path the plugin does not declare is still `None` rather than a
        // refusal, so the alias does not change which paths a plugin owns.
        assert!(
            daemon
                .anchor_for("short", "GET", "/other")
                .unwrap()
                .is_none()
        );
    }

    /// An alias is discoverable from the list, or nobody would know to type it.
    #[tokio::test]
    async fn the_list_reports_the_alias() {
        let daemon = Daemon::new(Registry::with_disabled(
            "org.test.aliased",
            Some("short"),
            &[],
            "disabled",
        ));

        match daemon.handle(Request::PluginList).await {
            Response::PluginList { plugins } => {
                assert_eq!(plugins.len(), 1);
                assert_eq!(plugins[0].alias.as_deref(), Some("short"));
            }
            other => panic!("expected a list, got {other:?}"),
        }
    }

    /// Every pipe names its own ABI entry. A wrong mapping would silently send a
    /// command line down the HTTP entry, which is the confusion the three
    /// entries exist to prevent.
    /// The CLI entry parses a JSON string, so a bare line would be a `400` on
    /// every call. The host owns the encoding — the caller types a line.
    #[test]
    fn a_cli_payload_is_encoded_as_a_json_string() {
        let encoded = encode_payload(anth_wire::PluginPipe::Cli, r#"greet --name "Ana Sol""#);

        let decoded: String = serde_json::from_str(&encoded).expect("must be a JSON string");
        assert_eq!(decoded, r#"greet --name "Ana Sol""#);
    }

    /// The other two pipes already carry JSON, so re-encoding would double it.
    #[test]
    fn an_http_payload_passes_through_untouched() {
        let body = r#"{"message":"hi"}"#;

        assert_eq!(encode_payload(anth_wire::PluginPipe::Http, body), body);
        assert_eq!(encode_payload(anth_wire::PluginPipe::Direct, body), body);
    }

    #[test]
    fn each_pipe_names_its_own_entry() {
        assert_eq!(PluginPipe::Cli.entry(), "anth_cli");
        assert_eq!(PluginPipe::Http.entry(), "anth_http");
        assert_eq!(PluginPipe::Direct.entry(), "anth_direct");
    }
}
