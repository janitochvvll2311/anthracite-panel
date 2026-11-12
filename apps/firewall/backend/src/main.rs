//! Host firewall service for the Firewall app.
//!
//! The service owns exactly one nftables table, `inet anthracite`, with the
//! `input` and `output` base chains. `nft` is invoked with an argument vector,
//! never through a shell. The panel and the engine never read or write nft.

mod nft;

use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_contract::Error;
use anth_service::Handler;
use serde_json::{json, Value};

use nft::{
    default_chains, parse_table, Chain, NftRunner, ParsedTable, Protocol, SystemNft, Verdict,
    FAMILY, TABLE,
};

const FIREWALL: &str = "anthracite:firewall/firewall@1.0.0";
const MAX_COMMENT_CHARS: usize = 32;

struct Service {
    control: bool,
    runner: Arc<dyn NftRunner>,
    /// Serializes read/guard/command sequences so two mutations cannot race.
    mutation_lock: Mutex<()>,
}

impl Service {
    fn new(control: bool, runner: Arc<dyn NftRunner>) -> Self {
        Self {
            control,
            runner,
            mutation_lock: Mutex::new(()),
        }
    }

    fn snapshot(&self) -> Result<ParsedTable, Error> {
        self.read_table().map(|(table, _)| table)
    }

    fn read_table(&self) -> Result<(ParsedTable, String), Error> {
        let args = vec![
            "-j".to_string(),
            "list".to_string(),
            "table".to_string(),
            FAMILY.to_string(),
            TABLE.to_string(),
        ];
        match self.runner.run(&args) {
            Ok(output) if output.ok => {
                let table = parse_table(&output.stdout).map_err(|detail| {
                    Error::unavailable(format!("reading the firewall: {detail}"))
                })?;
                Ok((table, output.stdout))
            }
            Ok(output) if missing_table(&output.stderr) => Ok((
                ParsedTable {
                    initialized: false,
                    rules: Vec::new(),
                    chains: default_chains(),
                },
                String::new(),
            )),
            Ok(output) => Err(nft_failure(&output.stderr)),
            Err(detail) => Err(Error::unavailable(format!(
                "`nft` is not installed or could not be executed: {detail}"
            ))),
        }
    }

    fn require_control(&self) -> Result<(), Error> {
        if self.control {
            Ok(())
        } else {
            Err(Error::denied(
                "firewall changes are disabled; start the service with `--firewall-control`",
            ))
        }
    }

    fn ensure_chain(&self, chain: Chain) -> Result<(), Error> {
        let table = self.snapshot()?;
        if !table.initialized {
            self.run_ok(
                &[
                    "add".to_string(),
                    "table".to_string(),
                    FAMILY.to_string(),
                    TABLE.to_string(),
                ],
                "creating the firewall table",
            )?;
        }
        let initialized = table
            .chains
            .iter()
            .find(|state| state.chain == chain)
            .map(|state| state.initialized)
            .unwrap_or(false);
        if !initialized {
            self.run_ok(&chain_args(chain), "creating the firewall chain")?;
        }
        Ok(())
    }

    fn add_rule(&self, input: &Value) -> Result<(), Error> {
        self.require_control()?;
        let chain = chain(input, "chain")?;
        let action = verdict(input, "action")?;
        let protocol = protocol(input, "protocol")?;
        let port = port(input)?;
        let comment = comment(input)?;
        let _serial = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::internal("the firewall mutation lock is poisoned"))?;
        self.ensure_chain(chain)?;
        let table = self.snapshot()?;
        let mut matches = table.rules.iter().filter(|rule| {
            rule.chain == chain
                && rule.action.is_some()
                && rule.protocol == Some(protocol)
                && rule.port == Some(port)
        });
        if let Some(first) = matches.next() {
            self.run_ok(
                &replace_args(
                    first.handle,
                    chain,
                    action,
                    protocol,
                    port,
                    comment.as_deref(),
                ),
                "replacing the firewall rule",
            )?;
            let extras: Vec<u64> = matches.map(|rule| rule.handle).collect();
            for handle in extras {
                self.run_ok(&delete_args(chain, handle), "removing a duplicate rule")?;
            }
        } else {
            self.run_ok(
                &add_args(chain, action, protocol, port, comment.as_deref()),
                "adding the firewall rule",
            )?;
        }
        Ok(())
    }

    fn update_rule(&self, input: &Value) -> Result<(), Error> {
        self.require_control()?;
        let handle = handle(input)?;
        let chain = chain(input, "chain")?;
        let action = verdict(input, "action")?;
        let protocol = protocol(input, "protocol")?;
        let port = port(input)?;
        let comment = comment(input)?;
        let _serial = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::internal("the firewall mutation lock is poisoned"))?;
        self.ensure_chain(chain)?;
        let table = self.snapshot()?;
        let Some(rule) = table.rules.iter().find(|rule| rule.handle == handle) else {
            return Err(Error::not_found(format!(
                "no firewall rule with handle {handle}"
            )));
        };
        if rule.action.is_none() {
            return Err(Error::invalid("custom rules can be deleted but not edited"));
        }
        if let Some(other) = table.rules.iter().find(|other| {
            other.handle != handle
                && other.action.is_some()
                && other.chain == chain
                && other.protocol == Some(protocol)
                && other.port == Some(port)
        }) {
            return Err(Error::conflict(format!(
                "rule #{} already manages {protocol}/{port} on {chain}",
                other.handle
            )));
        }
        if rule.chain == chain {
            self.run_ok(
                &replace_args(handle, chain, action, protocol, port, comment.as_deref()),
                "updating the firewall rule",
            )?;
        } else {
            self.run_ok(
                &add_args(chain, action, protocol, port, comment.as_deref()),
                "moving the firewall rule",
            )?;
            self.run_ok(
                &delete_args(rule.chain, handle),
                "removing the old firewall rule",
            )?;
        }
        Ok(())
    }

    fn delete_rule(&self, input: &Value) -> Result<(), Error> {
        self.require_control()?;
        let handle = handle(input)?;
        let _serial = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::internal("the firewall mutation lock is poisoned"))?;
        let table = self.snapshot()?;
        let Some(rule) = table.rules.iter().find(|rule| rule.handle == handle) else {
            return Err(Error::not_found(format!(
                "no firewall rule with handle {handle}"
            )));
        };
        self.run_ok(
            &delete_args(rule.chain, handle),
            "deleting the firewall rule",
        )
    }

    fn flush(&self, input: &Value) -> Result<(), Error> {
        self.require_control()?;
        let chain = chain(input, "chain")?;
        let _serial = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::internal("the firewall mutation lock is poisoned"))?;
        let table = self.snapshot()?;
        let initialized = table
            .chains
            .iter()
            .find(|state| state.chain == chain)
            .map(|state| state.initialized)
            .unwrap_or(false);
        if !table.initialized || !initialized {
            return Ok(());
        }
        self.run_ok(
            &[
                "flush".to_string(),
                "chain".to_string(),
                FAMILY.to_string(),
                TABLE.to_string(),
                chain.to_string(),
            ],
            "flushing the firewall chain",
        )
    }

    fn set_policy(&self, input: &Value) -> Result<(), Error> {
        self.require_control()?;
        let chain = chain(input, "chain")?;
        let policy = verdict(input, "policy")?;
        let _serial = self
            .mutation_lock
            .lock()
            .map_err(|_| Error::internal("the firewall mutation lock is poisoned"))?;
        self.ensure_chain(chain)?;
        let table = self.snapshot()?;
        if table
            .chains
            .iter()
            .find(|state| state.chain == chain)
            .map(|state| state.policy == policy)
            .unwrap_or(false)
        {
            return Ok(());
        }
        self.run_ok(
            &[
                "add".to_string(),
                "chain".to_string(),
                FAMILY.to_string(),
                TABLE.to_string(),
                chain.to_string(),
                format!("{{ policy {}; }}", policy.nft()),
            ],
            "setting the firewall policy",
        )
    }

    fn run_ok(&self, args: &[String], what: &str) -> Result<(), Error> {
        match self.runner.run(args) {
            Ok(output) if output.ok => Ok(()),
            Ok(output) => Err(nft_failure(&output.stderr).with_detail(json!({"action": what}))),
            Err(detail) => Err(Error::unavailable(format!(
                "`nft` is not installed or could not be executed: {detail}"
            ))),
        }
    }

    fn snapshot_json(&self) -> Result<Value, Error> {
        let (table, ruleset) = self.read_table()?;
        Ok(json!({
            "initialized": table.initialized,
            "control": self.control,
            "rules": table.rules.iter().map(|rule| json!({
                "handle": rule.handle,
                "chain": rule.chain.to_string(),
                "action": rule.action.map(|action| action.as_str()),
                "protocol": rule.protocol.map(|protocol| protocol.as_str()),
                "port": rule.port,
                "comment": rule.comment,
            })).collect::<Vec<_>>(),
            "chains": table.chains.iter().map(|state| json!({
                "chain": state.chain.to_string(),
                "initialized": state.initialized,
                "policy": state.policy.as_str(),
            })).collect::<Vec<_>>(),
            "ruleset": ruleset,
            "timestamp-ms": now_ms(),
        }))
    }
}

impl Handler for Service {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
        if interface != FIREWALL {
            return Err(Error::not_found(format!(
                "`{interface}` is not exported by the firewall service"
            )));
        }
        match function {
            "snapshot" => self.snapshot_json(),
            "add-rule" => self.add_rule(&input).map(|()| Value::Null),
            "update-rule" => self.update_rule(&input).map(|()| Value::Null),
            "delete-rule" => self.delete_rule(&input).map(|()| Value::Null),
            "flush" => self.flush(&input).map(|()| Value::Null),
            "set-policy" => self.set_policy(&input).map(|()| Value::Null),
            other => Err(Error::not_found(format!(
                "`anthracite:firewall/firewall#{other}` is not implemented"
            ))),
        }
    }
}

#[tokio::main]
async fn main() {
    let socket = match anth_service::socket_argument() {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let control = match control_argument() {
        Ok(control) => control,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let service = Service::new(control, Arc::new(SystemNft));
    if let Err(error) = anth_service::serve(&socket, Arc::new(service)).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn control_argument() -> Result<bool, Error> {
    let mut args = std::env::args().skip(1);
    let mut control = false;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--socket" => {
                args.next();
            }
            "--firewall-control" => control = true,
            other => {
                return Err(Error::invalid(format!(
                    "unknown firewall-service argument `{other}`"
                )));
            }
        }
    }
    Ok(control)
}

fn chain_args(chain: Chain) -> Vec<String> {
    vec![
        "add".to_string(),
        "chain".to_string(),
        FAMILY.to_string(),
        TABLE.to_string(),
        chain.to_string(),
        format!(
            "{{ type filter hook {} priority filter; policy accept; }}",
            chain
        ),
    ]
}

fn add_args(
    chain: Chain,
    action: Verdict,
    protocol: Protocol,
    port: u16,
    comment: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        "add".to_string(),
        "rule".to_string(),
        FAMILY.to_string(),
        TABLE.to_string(),
        chain.to_string(),
        protocol.to_string(),
        "dport".to_string(),
        port.to_string(),
        action.nft().to_string(),
    ];
    args.extend(comment_args(action, protocol, port, comment));
    args
}

fn replace_args(
    handle: u64,
    chain: Chain,
    action: Verdict,
    protocol: Protocol,
    port: u16,
    comment: Option<&str>,
) -> Vec<String> {
    let mut args = vec![
        "replace".to_string(),
        "rule".to_string(),
        FAMILY.to_string(),
        TABLE.to_string(),
        chain.to_string(),
        "handle".to_string(),
        handle.to_string(),
        protocol.to_string(),
        "dport".to_string(),
        port.to_string(),
        action.nft().to_string(),
    ];
    args.extend(comment_args(action, protocol, port, comment));
    args
}

fn delete_args(chain: Chain, handle: u64) -> Vec<String> {
    vec![
        "delete".to_string(),
        "rule".to_string(),
        FAMILY.to_string(),
        TABLE.to_string(),
        chain.to_string(),
        "handle".to_string(),
        handle.to_string(),
    ]
}

fn comment_args(
    action: Verdict,
    protocol: Protocol,
    port: u16,
    comment: Option<&str>,
) -> Vec<String> {
    let mut metadata = format!("anth:{}:{}:{port}", action.as_str(), protocol.as_str());
    if let Some(comment) = comment {
        metadata.push('|');
        metadata.push_str(comment);
    }
    vec!["comment".to_string(), format!("\"{metadata}\"")]
}

fn text(input: &Value, key: &str) -> Result<String, Error> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::invalid(format!("`{key}` is required")))
}

fn optional_text(input: &Value, key: &str) -> Option<String> {
    input.get(key).and_then(Value::as_str).map(str::to_string)
}

fn chain(input: &Value, key: &str) -> Result<Chain, Error> {
    text(input, key)?
        .parse()
        .map_err(|detail: String| Error::invalid(detail))
}

fn verdict(input: &Value, key: &str) -> Result<Verdict, Error> {
    text(input, key)?
        .parse()
        .map_err(|detail: String| Error::invalid(detail))
}

fn protocol(input: &Value, key: &str) -> Result<Protocol, Error> {
    text(input, key)?
        .parse()
        .map_err(|detail: String| Error::invalid(detail))
}

fn port(input: &Value) -> Result<u16, Error> {
    input
        .get("port")
        .and_then(Value::as_u64)
        .and_then(|value| u16::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| Error::invalid("`port` must be between 1 and 65535"))
}

fn handle(input: &Value) -> Result<u64, Error> {
    input
        .get("handle")
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .filter(|value| *value > 0)
        .ok_or_else(|| Error::invalid("`handle` must be greater than zero"))
}

fn comment(input: &Value) -> Result<Option<String>, Error> {
    let Some(value) = optional_text(input, "comment") else {
        return Ok(None);
    };
    let value = value.trim().to_string();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().count() > MAX_COMMENT_CHARS
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
    {
        return Err(Error::invalid(
            "a firewall comment must be 1-32 ASCII letters, digits, `.`, `_` or `-`",
        ));
    }
    Ok(Some(value))
}

fn missing_table(stderr: &str) -> bool {
    let text = stderr.to_ascii_lowercase();
    text.contains("no such file") || text.contains("does not exist") || text.contains("not found")
}

fn nft_failure(stderr: &str) -> Error {
    let text = stderr.to_ascii_lowercase();
    let detail = if stderr.trim().is_empty() {
        "`nft` failed without a message".to_string()
    } else {
        format!("`nft` failed: {stderr}")
    };
    if text.contains("permission denied") || text.contains("operation not permitted") {
        Error::denied(format!("firewall changes need CAP_NET_ADMIN: {detail}"))
    } else {
        Error::unavailable(detail)
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeNft {
        calls: Mutex<Vec<Vec<String>>>,
        snapshot: Mutex<String>,
        missing: Mutex<bool>,
    }

    impl FakeNft {
        fn new(snapshot: &str, missing: bool) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                snapshot: Mutex::new(snapshot.to_string()),
                missing: Mutex::new(missing),
            }
        }
    }

    impl NftRunner for FakeNft {
        fn run(&self, args: &[String]) -> Result<nft::NftOutput, String> {
            self.calls.lock().expect("calls").push(args.to_vec());
            if args.first().map(String::as_str) == Some("-j") {
                if *self.missing.lock().expect("missing") {
                    return Ok(nft::NftOutput {
                        ok: false,
                        stdout: String::new(),
                        stderr: "Error: No such file or directory".to_string(),
                    });
                }
                return Ok(nft::NftOutput {
                    ok: true,
                    stdout: self.snapshot.lock().expect("snapshot").clone(),
                    stderr: String::new(),
                });
            }
            if args.first().map(String::as_str) == Some("add")
                && args.get(1).map(String::as_str) == Some("table")
            {
                *self.missing.lock().expect("missing") = false;
            }
            Ok(nft::NftOutput {
                ok: true,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    fn snapshot() -> String {
        serde_json::json!({
            "nftables": [
                {"table": {"family": "inet", "name": "anthracite"}},
                {"chain": {"family": "inet", "table": "anthracite", "name": "input", "policy": "accept"}},
                {"chain": {"family": "inet", "table": "anthracite", "name": "output", "policy": "accept"}},
                {"rule": {"family": "inet", "table": "anthracite", "chain": "input", "handle": 7, "comment": "anth:deny:tcp:23|telnet"}}
            ]
        })
        .to_string()
    }

    fn service(snapshot: &str) -> (Service, Arc<FakeNft>) {
        let runner = Arc::new(FakeNft::new(snapshot, false));
        (Service::new(true, runner.clone()), runner)
    }

    #[test]
    fn snapshot_projects_the_contract() {
        let (service, _) = service(&snapshot());
        let value = service
            .call(FIREWALL, "snapshot", Value::Null)
            .expect("snapshot");
        assert_eq!(value["rules"][0]["handle"], 7);
        assert_eq!(value["rules"][0]["action"], "deny");
        assert_eq!(value["chains"][0]["policy"], "allow");
        assert_eq!(value["control"], true);
    }

    #[test]
    fn add_rule_replaces_a_managed_target() {
        let (service, runner) = service(&snapshot());
        service
            .call(
                FIREWALL,
                "add-rule",
                json!({"chain": "input", "action": "deny", "protocol": "tcp", "port": 23, "comment": "telnet"}),
            )
            .expect("add");
        let calls = runner.calls.lock().expect("calls");
        let last = calls.last().expect("call");
        assert_eq!(last[0], "replace");
        assert!(last.contains(&"handle".to_string()));
        assert!(last.contains(&"7".to_string()));
        assert!(last.iter().any(|arg| arg == "\"anth:deny:tcp:23|telnet\""));
    }

    #[test]
    fn add_rule_creates_a_missing_table_and_chain() {
        let runner = Arc::new(FakeNft::new(&snapshot(), true));
        let service = Service::new(true, runner.clone());
        service
            .call(
                FIREWALL,
                "add-rule",
                json!({"chain": "input", "action": "allow", "protocol": "tcp", "port": 22, "comment": null}),
            )
            .expect("add");
        let calls = runner.calls.lock().expect("calls");
        assert!(calls
            .iter()
            .any(|args| args[0] == "add" && args[1] == "table"));
        assert!(calls
            .iter()
            .any(|args| args[0] == "add" && args[1] == "chain"));
    }

    #[test]
    fn a_query_handle_is_accepted() {
        let (service, runner) = service(&snapshot());
        service
            .call(FIREWALL, "delete-rule", json!({"handle": "7"}))
            .expect("query handle");
        assert!(runner
            .calls
            .lock()
            .expect("calls")
            .iter()
            .any(|args| args[0] == "delete"));
    }

    #[test]
    fn mutations_require_control() {
        let runner = Arc::new(FakeNft::new(&snapshot(), false));
        let service = Service::new(false, runner);
        assert_eq!(
            service
                .call(FIREWALL, "delete-rule", json!({"handle": 7}),)
                .expect_err("disabled")
                .kind(),
            anth_contract::ErrorKind::Denied
        );
        assert!(service
            .call(FIREWALL, "snapshot", Value::Null)
            .expect("snapshot")
            .get("control")
            .is_some());
    }

    #[test]
    fn custom_rules_cannot_be_updated() {
        let snapshot = serde_json::json!({
            "nftables": [
                {"table": {"family": "inet", "name": "anthracite"}},
                {"chain": {"family": "inet", "table": "anthracite", "name": "input"}},
                {"chain": {"family": "inet", "table": "anthracite", "name": "output"}},
                {"rule": {"family": "inet", "table": "anthracite", "chain": "input", "handle": 4, "comment": "manual"}}
            ]
        })
        .to_string();
        let (service, _) = service(&snapshot);
        assert_eq!(
            service
                .call(
                    FIREWALL,
                    "update-rule",
                    json!({"handle": 4, "chain": "input", "action": "allow", "protocol": "tcp", "port": 22, "comment": null}),
                )
                .expect_err("custom")
                .kind(),
            anth_contract::ErrorKind::Invalid
        );
    }

    #[test]
    fn policy_and_flush_build_expected_argv() {
        let (service, runner) = service(&snapshot());
        service
            .call(
                FIREWALL,
                "set-policy",
                json!({"chain": "input", "policy": "deny"}),
            )
            .expect("policy");
        service
            .call(FIREWALL, "flush", json!({"chain": "output"}))
            .expect("flush");
        let calls = runner.calls.lock().expect("calls");
        assert!(calls.iter().any(|args| {
            args[0] == "add" && args[1] == "chain" && args[5] == "{ policy drop; }"
        }));
        assert!(calls
            .iter()
            .any(|args| args[0] == "flush" && args[4] == "output"));
    }
}
