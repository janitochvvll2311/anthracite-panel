//! ACME client for Anthracite: an RFC 8555 account and order service.
//!
//! The service owns the CA conversation, the account keys under `{data}`, the
//! HTTP-01 token directory under `{shared}/acme/http-01` and the DNS-01
//! solvers. Issued material is handed to the certificate store through the
//! broker; this process never publishes TLS material itself.

pub mod account;
pub mod acme;
pub mod config;
pub mod crypto;
pub mod http;
pub mod order;
pub mod renew;
pub mod solver;
pub mod store;
pub mod worker;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anth_contract::Error;
use anth_service::{serve, Broker, ChunkStream, Handler};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::broadcast;

use config::Config;
use crypto::now;
use http::Http;
use order::{Challenge, Order, Orders, Status};
use renew::Backoff;

const CLIENT: &str = "anthracite:acme/client@1.0.0";

pub struct Core {
    pub data: PathBuf,
    pub shared: PathBuf,
    pub http01_root: PathBuf,
    pub config: Config,
    pub orders: Orders,
    pub http: Http,
    pub broker: Option<Broker>,
    pub handle: tokio::runtime::Handle,
    pub renew_backoff: std::sync::Mutex<HashMap<String, Backoff>>,
}

pub struct Service {
    core: Arc<Core>,
}

impl Service {
    fn audit(&self, action: &str, detail: Value) {
        let Some(broker) = self.core.broker.clone() else {
            return;
        };
        let action = action.to_string();
        self.core.handle.spawn(async move {
            let _ = broker.audit(&action, detail).await;
        });
    }
}

#[derive(Debug, Deserialize)]
struct IssueInput {
    account: String,
    names: Vec<String>,
    challenge: String,
    #[serde(default)]
    solver: Option<String>,
    #[serde(rename = "key-type", default)]
    key_type: String,
    #[serde(rename = "auto-renew", default)]
    auto_renew: bool,
    #[serde(default)]
    staging: bool,
    #[serde(default)]
    force: bool,
}

impl Handler for Service {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
        if interface != CLIENT {
            return Err(Error::not_found(format!(
                "`{interface}` is not served by the ACME client"
            )));
        }
        match function {
            "list-accounts" => Ok(Value::Array(account::list(&self.core.data))),
            "create-account" => {
                let directory = text(&input, "directory")?;
                let contact = strings(&input, "contact");
                let key_type = input
                    .get("key-type")
                    .and_then(Value::as_str)
                    .unwrap_or(crypto::EC)
                    .to_string();
                let key_type = crypto::parse_key_type(&key_type)?;
                let eab = input.get("eab").cloned();
                let created = self.core.handle.block_on(account::create(
                    &self.core.data,
                    &self.core.config,
                    &self.core.http,
                    directory,
                    &contact,
                    key_type,
                    eab,
                ))?;
                self.audit(
                    "acme.create-account",
                    json!({"directory": directory, "key-type": key_type}),
                );
                Ok(created)
            }
            "issue" => {
                let order = self.issue(input)?;
                self.audit(
                    "acme.issue",
                    json!({
                        "order": order["id"],
                        "names": order["names"],
                        "challenge": order["challenge"],
                    }),
                );
                Ok(order)
            }
            "list-orders" => Ok(Value::Array(
                self.core.orders.list().iter().map(Order::public).collect(),
            )),
            "capabilities" => Ok(json!([
                {
                    "challenge": "http-01",
                    "solvers": [],
                    "directories": self.core.config.directory_urls(),
                    "staging-directories": self
                        .core
                        .config
                        .directories
                        .iter()
                        .filter(|directory| directory.staging)
                        .map(|directory| directory.url.clone())
                        .collect::<Vec<_>>(),
                },
                {
                    "challenge": "dns-01",
                    "solvers": self
                        .core
                        .config
                        .solvers
                        .iter()
                        .map(|solver| solver.id.clone())
                        .collect::<Vec<_>>(),
                    "directories": self.core.config.directory_urls(),
                    "staging-directories": self
                        .core
                        .config
                        .directories
                        .iter()
                        .filter(|directory| directory.staging)
                        .map(|directory| directory.url.clone())
                        .collect::<Vec<_>>(),
                }
            ])),
            "confirm-challenge" => {
                let id = text(&input, "order")?.to_string();
                let order = self.confirm(&id)?;
                self.audit("acme.confirm-challenge", json!({"order": id}));
                Ok(order)
            }
            "renew" => {
                let certificate = text(&input, "certificate")?.to_string();
                let order = self.renew(&certificate)?;
                self.audit(
                    "acme.renew",
                    json!({"certificate": certificate, "order": order["id"]}),
                );
                Ok(order)
            }
            "revoke" => {
                let certificate = text(&input, "certificate")?.to_string();
                let reason = text(&input, "reason")?;
                let reason = revoke_reason(reason)?;
                self.core
                    .handle
                    .block_on(self.revoke(&certificate, reason))?;
                self.audit(
                    "acme.revoke",
                    json!({"certificate": certificate, "reason": reason}),
                );
                Ok(json!(true))
            }
            other => Err(Error::not_found(format!(
                "`{CLIENT}` has no function `{other}`"
            ))),
        }
    }

    fn stream(&self, interface: &str, function: &str, input: Value) -> Result<ChunkStream, Error> {
        if interface != CLIENT || function != "follow" {
            return Err(Error::not_found(format!(
                "`{interface}#{function}` is not a stream of the ACME client"
            )));
        }
        let id = text(&input, "order")?.to_string();
        // Subscribe first: an event written between the snapshot and the
        // subscription would otherwise be in neither.
        let receiver = self.core.orders.subscribe();
        let snapshot = self.core.orders.snapshot_events(&id)?;
        let last = snapshot.last().map(|event| event.seq).unwrap_or(0);
        let stream = futures_util::stream::unfold(
            (snapshot.into_iter(), receiver, last, id.clone()),
            |(mut snapshot, mut receiver, last, id)| async move {
                loop {
                    if let Some(event) = snapshot.next() {
                        let seq = event.seq;
                        let bytes = serde_json::to_vec(&event.public(&id)).ok()?;
                        return Some((Ok(bytes), (snapshot, receiver, seq, id)));
                    }
                    match receiver.recv().await {
                        Ok(event) if event.order == id && event.seq > last => {
                            let bytes = serde_json::to_vec(&event.public).ok()?;
                            return Some((Ok(bytes), (snapshot, receiver, event.seq, id)));
                        }
                        Ok(_) => continue,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => return None,
                    }
                }
            },
        );
        Ok(Box::pin(stream))
    }
}

impl Service {
    fn issue(&self, input: Value) -> Result<Value, Error> {
        let request: IssueInput = serde_json::from_value(input)
            .map_err(|error| Error::invalid(format!("issue: {error}")))?;
        let (account, _) = account::load(&self.core.data, &request.account)?;
        let directory = self.core.config.directory(&account.directory)?.clone();
        let names = order::normalize_names(&request.names)?;
        let challenge = Challenge::parse(&request.challenge)?;
        if challenge == Challenge::Http01 && names.iter().any(|name| name.starts_with("*.")) {
            return Err(Error::invalid(
                "a wildcard needs DNS-01; HTTP-01 cannot validate `*.` names",
            ));
        }
        if request.staging != directory.staging {
            return Err(Error::invalid(format!(
                "the request says staging = {}, but the account directory is {}",
                request.staging,
                if directory.staging {
                    "staging"
                } else {
                    "production"
                }
            )));
        }
        let key_type = crypto::parse_key_type(if request.key_type.is_empty() {
            crypto::EC
        } else {
            &request.key_type
        })?;
        let solver = match challenge {
            Challenge::Http01 => None,
            Challenge::Dns01 => {
                let id = request
                    .solver
                    .clone()
                    .or_else(|| {
                        self.core
                            .config
                            .solvers
                            .first()
                            .map(|solver| solver.id.clone())
                    })
                    .ok_or_else(|| Error::invalid("DNS-01 needs a solver in the config"))?;
                Some(self.core.config.solver(&id)?.id.clone())
            }
        };
        let duplicate = self.core.orders.list().into_iter().any(|order| {
            order.account == account.id
                && order.names == names
                && order.challenge == challenge
                && matches!(
                    order.status,
                    Status::Pending | Status::Ready | Status::Processing | Status::Valid
                )
        });
        if duplicate && !request.force {
            return Err(Error::conflict(
                "an identical order is already open or valid; revoke it or wait for it",
            ));
        }
        let id = order::random_id();
        let created = now();
        let order = Order {
            id: id.clone(),
            account: account.id.clone(),
            names: names.clone(),
            challenge,
            solver,
            key_type: key_type.to_string(),
            auto_renew: request.auto_renew,
            staging: directory.staging,
            directory: directory.url.clone(),
            status: Status::Pending,
            expires_at: created + 3_600,
            certificate: None,
            last_error: None,
            order_url: None,
            finalize_url: None,
            certificate_url: None,
            renew_of: None,
            confirmed: false,
            events: Vec::new(),
            created_at: created,
            updated_at: created,
        };
        self.core.orders.insert(order);
        self.core.orders.event(
            &id,
            "created",
            Some(format!(
                "requested {} with {}",
                names.join(", "),
                challenge.as_str()
            )),
        )?;
        worker::spawn(&self.core, &id);
        Ok(self.core.orders.get(&id)?.public())
    }

    fn confirm(&self, id: &str) -> Result<Value, Error> {
        let order = self.core.orders.get(id)?;
        if order.challenge != Challenge::Dns01 {
            return Err(Error::invalid(
                "only a DNS-01 order waits for a manual confirmation",
            ));
        }
        let solver = order.solver.as_deref().unwrap_or("manual");
        let solver = self.core.config.solver(solver)?;
        if solver.kind != "manual" {
            return Err(Error::invalid(format!(
                "the `{}` solver confirms itself; this order does not wait",
                solver.id
            )));
        }
        let updated = self.core.orders.update(id, |order| {
            order.confirmed = true;
            Ok(())
        })?;
        Ok(updated.public())
    }

    fn renew(&self, certificate: &str) -> Result<Value, Error> {
        if self.core.orders.list().iter().any(|order| {
            order.renew_of.as_deref() == Some(certificate)
                && !matches!(order.status, Status::Invalid | Status::Revoked)
        }) {
            return Err(Error::conflict(format!(
                "`{certificate}` already has a renewal; only a failed or revoked renewal can be retried"
            )));
        }
        let mapping = self
            .core
            .orders
            .list()
            .into_iter()
            .find(|order| order.certificate.as_deref() == Some(certificate));
        let names = match &mapping {
            Some(order) => order.names.clone(),
            None => {
                let record = self
                    .core
                    .handle
                    .block_on(store::certificate(self.core.broker.as_ref(), certificate))?;
                renew::names(&record)?
            }
        };
        let staging = mapping.as_ref().map(|order| order.staging).unwrap_or(false);
        let wildcard = names.iter().any(|name| name.starts_with("*."));
        let challenge = mapping
            .as_ref()
            .map(|order| order.challenge)
            .unwrap_or(if wildcard {
                Challenge::Dns01
            } else {
                Challenge::Http01
            });
        let solver = mapping.as_ref().and_then(|order| order.solver.clone());
        let key_type = mapping
            .as_ref()
            .map(|order| order.key_type.clone())
            .unwrap_or_else(|| crypto::EC.to_string());
        let account = mapping.as_ref().map(|order| order.account.clone());
        renew::renew(
            &self.core,
            renew::Renewal {
                certificate: certificate.to_string(),
                names,
                account,
                staging,
                challenge,
                solver,
                key_type,
            },
        )
    }

    async fn revoke(&self, certificate: &str, reason: u8) -> Result<(), Error> {
        let order = self
            .core
            .orders
            .list()
            .into_iter()
            .find(|order| order.certificate.as_deref() == Some(certificate))
            .ok_or_else(|| {
                Error::not_found(format!(
                    "no order in this service issued `{certificate}`; its account key is not available"
                ))
            })?;
        let chain = std::fs::read_to_string(store::chain_path(&order.dir(&self.core.data)))
            .map_err(|error| {
                Error::not_found(format!("reading the certificate material: {error}"))
            })?;
        let (account, key) = account::load(&self.core.data, &order.account)?;
        let kid = account
            .kid
            .clone()
            .ok_or_else(|| Error::invalid("the account has no ACME URL"))?;
        let directory = self.core.config.directory(&order.directory)?.clone();
        let client = acme::AcmeClient::connect(&self.core.http, &directory).await?;
        client.revoke(&key, &kid, &chain, reason).await?;
        // A revoked certificate must not be auto-renewed; the store would
        // otherwise keep feeding it to the loop as `acme` + `auto-renew`.
        let _ = store::set_auto_renew(self.core.broker.as_ref(), certificate, false).await;
        self.core.orders.update(&order.id, |order| {
            order.status = Status::Revoked;
            Ok(())
        })?;
        self.core.orders.event(
            &order.id,
            "revoked",
            Some(format!("reason {reason} accepted by the CA")),
        )?;
        Ok(())
    }
}

fn revoke_reason(reason: &str) -> Result<u8, Error> {
    let reason = reason.trim();
    if let Ok(number) = reason.parse::<u8>() {
        if number <= 10 {
            return Ok(number);
        }
    }
    let code = match reason {
        "unspecified" => 0,
        "key-compromise" | "keyCompromise" => 1,
        "ca-compromise" | "cACompromise" => 2,
        "affiliation-changed" | "affiliationChanged" => 3,
        "superseded" => 4,
        "cessation-of-operation" | "cessationOfOperation" => 5,
        other => {
            return Err(Error::invalid(format!(
                "`{other}` is not an RFC 5280 revocation reason"
            )))
        }
    };
    Ok(code)
}

fn text<'a>(input: &'a Value, key: &str) -> Result<&'a str, Error> {
    input
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid(format!("the call needs `{key}`")))
}

fn strings(input: &Value, key: &str) -> Vec<String> {
    input
        .get(key)
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(|value| value.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

pub async fn run() -> Result<(), Error> {
    let socket = anth_service::socket_argument()?;
    let data = argument("--data")?;
    let shared = argument("--shared")?;
    let broker = std::env::args()
        .skip(1)
        .collect::<Vec<_>>()
        .windows(2)
        .find(|pair| pair[0] == "--broker")
        .map(|pair| PathBuf::from(&pair[1]));

    let data = PathBuf::from(data);
    let shared = PathBuf::from(shared);
    let http01_root = shared.join("acme/http-01");
    prepare_dirs(&data, &http01_root)?;
    let config = Config::load(&data)?;
    let http = Http::new(config.ca_bundle.as_deref().map(Path::new))?;
    let orders = Orders::load(&data)?;
    let core = Arc::new(Core {
        data,
        shared,
        http01_root,
        config,
        orders,
        http,
        broker: broker.map(Broker::new),
        handle: tokio::runtime::Handle::current(),
        renew_backoff: std::sync::Mutex::new(HashMap::new()),
    });
    renew::spawn(&core);
    let service = Arc::new(Service {
        core: Arc::clone(&core),
    });
    serve(&socket, service).await
}

/// Reads one `--flag value` argument. A missing flag is exit 2 with a message
/// that names it; unknown extra arguments are ignored for now (the strict
/// parser is tracked in design §30.4).
fn prepare_dirs(data: &Path, http01_root: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(data)
        .map_err(|error| Error::unavailable(format!("creating {}: {error}", data.display())))?;
    let _ = std::fs::set_permissions(data, std::fs::Permissions::from_mode(0o700));
    std::fs::create_dir_all(http01_root).map_err(|error| {
        Error::unavailable(format!("creating {}: {error}", http01_root.display()))
    })?;
    // The proxy runs under another uid (same engine gid) and must traverse
    // `{shared}/acme` and read the token. The token is a public key
    // authorization, not a secret, so 0770/0640 is the intended mode.
    let acme_dir = http01_root.parent().unwrap_or(http01_root);
    for dir in [acme_dir, http01_root] {
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o770));
    }
    Ok(())
}

fn argument(flag: &str) -> Result<String, Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    for pair in args.windows(2) {
        if pair[0] == flag {
            return Ok(pair[1].clone());
        }
    }
    Err(Error::invalid(format!("`{flag} <value>` is required")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_token_directory_is_group_readable_by_the_proxy() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("acme-dirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        let data = dir.join("data");
        let http01 = dir.join("shared/acme/http-01");
        prepare_dirs(&data, &http01).expect("prepare");
        assert_eq!(
            std::fs::metadata(&data).expect("data").permissions().mode() & 0o777,
            0o700
        );
        for path in [http01.parent().expect("acme").to_path_buf(), http01.clone()] {
            assert_eq!(
                std::fs::metadata(&path).expect("dir").permissions().mode() & 0o777,
                0o770
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn revocation_reasons_accept_names_and_codes() {
        assert_eq!(revoke_reason("key-compromise").expect("reason"), 1);
        assert_eq!(revoke_reason("4").expect("reason"), 4);
        assert!(revoke_reason("11").is_err());
        assert!(revoke_reason("because").is_err());
    }
}
