//! The WIT-facing admin surface: `snapshot`, `apply`, `status`, `logs`,
//! `certificates`, `reload-certificates`, `hosts`, `routes` and
//! `test-upstream`, plus the audit entry an apply leaves behind.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

use anth_contract::Error;
use anth_service::{ChunkStream, Handler};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::config::Config;
use crate::service::Service;

pub const ADMIN: &str = "anthracite:proxy/admin@1.0.0";
const AUDIT: &str = "anth:audit/record@1.0.0";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProxyStatus {
    pub running: bool,
    pub listeners: Vec<String>,
    pub last_reload: u64,
    pub last_error: Option<String>,
    pub version: u64,
    pub dropped: u64,
    pub handshake_errors: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub struct UpstreamCheck {
    pub id: String,
    pub ok: bool,
    pub status: Option<u16>,
    pub latency_ms: u64,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct ApplyInput {
    config: Config,
    expected_version: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct UpstreamInput {
    id: String,
}

impl Service {
    fn audit(&self, action: &str, detail: Value) {
        let Some(path) = self.broker.as_ref() else {
            return;
        };
        let Ok(body) = serde_json::to_vec(&json!({
            "interface": AUDIT,
            "function": "record",
            "input": {"action": action, "detail": detail},
        })) else {
            return;
        };
        let Ok(mut stream) = UnixStream::connect(path) else {
            return;
        };
        let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
        let head = format!(
            "POST /__anth/call HTTP/1.1\r\nHost: anthracite\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
        let mut answer = Vec::new();
        let _ = stream.read_to_end(&mut answer);
    }

    fn apply_input(&self, input: Value) -> Result<Value, Error> {
        let args: ApplyInput = serde_json::from_value(input)
            .map_err(|error| Error::invalid(format!("apply: {error}")))?;
        let detail = json!({
            "version": args.expected_version + 1,
            "listeners": args.config.listeners.len(),
            "hosts": args.config.hosts.len(),
            "upstreams": args.config.upstreams.len(),
        });
        match self.apply(args.config, args.expected_version) {
            Ok(config) => {
                self.audit("proxy.apply", detail);
                Ok(serde_json::to_value(config)
                    .map_err(|error| Error::internal(format!("serializing: {error}")))?)
            }
            Err(error) => {
                self.audit(
                    "proxy.apply.rejected",
                    json!({"version": args.expected_version + 1, "error": error.message()}),
                );
                Err(error)
            }
        }
    }
}

impl Handler for Service {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
        if interface != ADMIN {
            return Err(Error::not_found(format!(
                "`{interface}` is not exported by the proxy"
            )));
        }
        match function {
            "snapshot" => json_value(&self.snapshot().config),
            "apply" => self.apply_input(input),
            "status" => json_value(&self.status()),
            "certificates" => Ok(json!(self.certificates())),
            "reload-certificates" => {
                self.reload_certificates().map_err(|message| {
                    self.audit("proxy.reload.rejected", json!({"error": message}));
                    Error::invalid(format!("certificates: {message}"))
                })?;
                self.audit("proxy.reload", json!({}));
                json_value(&self.status())
            }
            "hosts" => json_value(&self.snapshot().config.hosts),
            "routes" => {
                let snapshot = self.snapshot();
                let routes: Vec<Value> = snapshot
                    .config
                    .hosts
                    .iter()
                    .flat_map(|host| host.routes.iter())
                    .filter_map(|route| serde_json::to_value(route).ok())
                    .collect();
                Ok(Value::Array(routes))
            }
            "test-upstream" => {
                let args: UpstreamInput = serde_json::from_value(input)
                    .map_err(|error| Error::invalid(format!("test-upstream: {error}")))?;
                json_value(&self.test_upstream_blocking(&args.id)?)
            }
            other => Err(Error::not_found(format!(
                "`{other}` is not a function of `{ADMIN}`"
            ))),
        }
    }

    fn stream(&self, interface: &str, function: &str, _input: Value) -> Result<ChunkStream, Error> {
        if interface != ADMIN || function != "logs" {
            return Err(Error::not_found(format!(
                "`{interface}#{function}` is not a stream of the proxy"
            )));
        }
        let receiver = self.logs.subscribe();
        let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|chunk| (Ok(chunk), receiver))
        });
        Ok(Box::pin(stream))
    }
}

fn json_value<T: Serialize>(value: &T) -> Result<Value, Error> {
    serde_json::to_value(value).map_err(|error| Error::internal(format!("serializing: {error}")))
}
