//! The managed nftables firewall.
//!
//! The daemon owns exactly one table with two base chains:
//!
//! ```text
//! inet anthracite
//! ├── input  (type filter hook input priority filter; policy <accept|drop>)
//! └── output (type filter hook output priority filter; policy <accept|drop>)
//! ```
//!
//! Every read and write is scoped to that table. `nft` is invoked directly with
//! an argument vector; nothing goes through a shell. Managed rules carry an
//! `anth:` comment so their intent and handle can be recovered from nft alone.
//!
//! Both chains keep `accept` and `drop` rules, so the panel shows the whole
//! allow/deny list instead of silently removing allowed ports, and each chain
//! has an editable default policy. One managed rule exists per protocol/port
//! pair within a chain: adding a rule for a target that is already managed
//! replaces that rule, which keeps the list unambiguous. An accept verdict ends
//! this chain, but it cannot override a drop in another table.

mod parse;

use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_wire::{
    ErrorKind, FirewallAction, FirewallChain, FirewallProtocol, FirewallRule, FirewallSnapshot,
    WireError,
};

use parse::{COMMENT_PREFIX, ParsedTable, default_chains, parse_table};

const FAMILY: &str = "inet";
const TABLE: &str = "anthracite";
const MAX_COMMENT_CHARS: usize = 32;

/// One `nft` invocation result, decoupled from `std::process::Output`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NftOutput {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Runs `nft` with an argument vector. Tests implement this trait.
pub trait NftRunner: Send + Sync {
    /// `Err` means the program could not be executed at all; an `nft` refusal
    /// is an `Ok(NftOutput { ok: false, .. })`.
    fn run(&self, args: &[String]) -> Result<NftOutput, String>;
}

/// The real `nft` binary, invoked directly with the daemon's privileges.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemNft;

impl NftRunner for SystemNft {
    fn run(&self, args: &[String]) -> Result<NftOutput, String> {
        let output = Command::new("nft")
            .args(args)
            .output()
            .map_err(|error| error.to_string())?;
        Ok(NftOutput {
            ok: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// The firewall service, built once from the policy the daemon read at startup.
pub struct FirewallService {
    control: bool,
    nft: Arc<dyn NftRunner>,
}

impl Clone for FirewallService {
    fn clone(&self) -> Self {
        Self {
            control: self.control,
            nft: Arc::clone(&self.nft),
        }
    }
}

impl FirewallService {
    pub fn new(control: bool) -> Self {
        Self::with_runner(control, Arc::new(SystemNft))
    }

    /// Replaces the `nft` runner. Tests use it to record argv without touching
    /// the host.
    pub fn with_runner(control: bool, nft: Arc<dyn NftRunner>) -> Self {
        Self { control, nft }
    }

    pub fn snapshot(&self) -> Result<FirewallSnapshot, WireError> {
        let Some((table, ruleset)) = self.read_table()? else {
            return Ok(FirewallSnapshot {
                initialized: false,
                control_enabled: self.control,
                rules: Vec::new(),
                chains: default_chains(),
                ruleset: String::new(),
                timestamp_ms: now_ms(),
            });
        };
        Ok(FirewallSnapshot {
            initialized: true,
            control_enabled: self.control,
            rules: table.rules,
            chains: table.chains,
            ruleset,
            timestamp_ms: now_ms(),
        })
    }

    /// Adds an allow or deny rule to one chain. A managed rule for the same
    /// protocol/port in that chain is replaced in place, so a target never has
    /// two conflicting verdicts.
    pub fn add(
        &self,
        chain: FirewallChain,
        action: FirewallAction,
        protocol: FirewallProtocol,
        port: u16,
        comment: Option<&str>,
    ) -> Result<String, WireError> {
        self.require_control()?;
        validate_port(port)?;
        let comment = normalize_comment(comment)?;

        self.ensure_chain(chain)?;
        let Some(table) = self.read_state()? else {
            return Err(WireError::new(
                ErrorKind::Internal,
                "firewall_state",
                "the managed firewall table disappeared after it was created",
            ));
        };

        let mut matches = matching_handles(&table.rules, chain, protocol, port).into_iter();
        if let Some(handle) = matches.next() {
            self.replace_handle(chain, handle, action, protocol, port, comment.as_deref())?;
            for extra in matches {
                self.delete_handle(chain, extra)?;
            }
        } else {
            self.add_rule(chain, action, protocol, port, comment.as_deref())?;
        }
        Ok(format!(
            "{} {protocol}/{port} on {chain}",
            action_past(action)
        ))
    }

    /// Rewrites one managed rule in place, preserving its position in the
    /// chain. Moving it to the other chain deletes the old rule and adds a new
    /// one there.
    pub fn update(
        &self,
        handle: u64,
        chain: FirewallChain,
        action: FirewallAction,
        protocol: FirewallProtocol,
        port: u16,
        comment: Option<&str>,
    ) -> Result<String, WireError> {
        self.require_control()?;
        if handle == 0 {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "firewall_handle",
                "rule handle must be greater than zero",
            ));
        }
        validate_port(port)?;
        let comment = normalize_comment(comment)?;

        let Some(table) = self.read_state()? else {
            return Err(WireError::new(
                ErrorKind::NotFound,
                "firewall_not_found",
                "the managed firewall table does not exist",
            ));
        };
        let Some(rule) = table.rules.iter().find(|rule| rule.handle == handle) else {
            return Err(WireError::new(
                ErrorKind::NotFound,
                "firewall_rule_not_found",
                format!("no firewall rule with handle {handle} in the managed chains"),
            ));
        };
        if rule.action == "custom" {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "firewall_custom_rule",
                "custom rules can be deleted but not edited",
            ));
        }
        if let Some(other) = table.rules.iter().find(|other| {
            other.handle != handle
                && other.chain == chain
                && other.action != "custom"
                && other.protocol == Some(protocol)
                && other.port == Some(port)
        }) {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "firewall_duplicate",
                format!(
                    "rule #{} already manages {protocol}/{port} on {chain}; edit or delete it first",
                    other.handle
                ),
            ));
        }

        if rule.chain == chain {
            self.replace_handle(chain, handle, action, protocol, port, comment.as_deref())?;
        } else {
            self.ensure_chain(chain)?;
            self.add_rule(chain, action, protocol, port, comment.as_deref())?;
            self.delete_handle(rule.chain, handle)?;
        }
        Ok(format!(
            "updated firewall rule {handle} to {} {protocol}/{port} on {chain}",
            action.as_str()
        ))
    }

    /// Deletes one rule from the managed table by its handle.
    pub fn delete(&self, handle: u64) -> Result<String, WireError> {
        self.require_control()?;
        if handle == 0 {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "firewall_handle",
                "rule handle must be greater than zero",
            ));
        }
        let Some(table) = self.read_state()? else {
            return Err(WireError::new(
                ErrorKind::NotFound,
                "firewall_not_found",
                "the managed firewall table does not exist",
            ));
        };
        let Some(rule) = table.rules.iter().find(|rule| rule.handle == handle) else {
            return Err(WireError::new(
                ErrorKind::NotFound,
                "firewall_rule_not_found",
                format!("no firewall rule with handle {handle} in the managed chains"),
            ));
        };
        self.delete_handle(rule.chain, handle)?;
        Ok(format!("deleted firewall rule {handle}"))
    }

    /// Removes every rule from one managed chain.
    pub fn flush(&self, chain: FirewallChain) -> Result<String, WireError> {
        self.require_control()?;
        let Some(table) = self.read_state()? else {
            return Ok("firewall table is not initialized".to_string());
        };
        let Some(state) = table.chains.iter().find(|state| state.chain == chain) else {
            return Ok(format!("{chain} chain is not initialized"));
        };
        if !state.initialized {
            return Ok(format!("{chain} chain is not initialized"));
        }
        let count = table
            .rules
            .iter()
            .filter(|rule| rule.chain == chain)
            .count();
        self.run_ok(
            &["flush", "chain", FAMILY, TABLE, chain.as_str()],
            "could not flush the firewall chain",
        )?;
        Ok(format!(
            "flushed {count} firewall rule{} from {chain}",
            if count == 1 { "" } else { "s" }
        ))
    }

    /// Changes the default verdict of one base chain. Setting `drop` without
    /// enough accept rules can lock the operator out; the CLI and the web UI
    /// ask for confirmation before calling this.
    pub fn set_policy(
        &self,
        chain: FirewallChain,
        policy: FirewallAction,
    ) -> Result<String, WireError> {
        self.require_control()?;
        self.ensure_chain(chain)?;
        let current = self
            .read_state()?
            .and_then(|table| table.chains.into_iter().find(|state| state.chain == chain));
        if current.as_ref().is_some_and(|state| state.policy == policy) {
            return Ok(format!("{chain} policy is already {}", policy.as_str()));
        }
        let spec = format!("{{ policy {}; }}", policy.verdict());
        self.run_ok(
            &["add", "chain", FAMILY, TABLE, chain.as_str(), spec.as_str()],
            "could not change the default firewall policy",
        )?;
        Ok(format!("set {chain} policy to {}", policy.as_str()))
    }

    fn require_control(&self) -> Result<(), WireError> {
        if self.control {
            Ok(())
        } else {
            Err(WireError::new(
                ErrorKind::Forbidden,
                "firewall_control_disabled",
                "firewall control is disabled; set `[firewall] control = true`",
            ))
        }
    }

    /// Reads the managed table and its textual ruleset. A missing table is
    /// `None`; any other refusal is an error.
    fn read_table(&self) -> Result<Option<(ParsedTable, String)>, WireError> {
        let Some(table) = self.read_state()? else {
            return Ok(None);
        };

        let text = self.run(&["list", "table", FAMILY, TABLE])?;
        if !text.ok {
            return Err(nft_error("could not read the firewall table", &text));
        }

        Ok(Some((table, text.stdout.trim_end().to_string())))
    }

    /// Reads the parsed rules and chain policies. A missing table is `None`.
    fn read_state(&self) -> Result<Option<ParsedTable>, WireError> {
        let json = self.run(&["-j", "list", "table", FAMILY, TABLE])?;
        if !json.ok {
            if is_missing(&json.stderr) {
                return Ok(None);
            }
            return Err(nft_error("could not read the firewall table", &json));
        }

        parse_table(&json.stdout)
            .map(Some)
            .map_err(|message| WireError::new(ErrorKind::Internal, "nft_parse", message))
    }

    fn add_rule(
        &self,
        chain: FirewallChain,
        action: FirewallAction,
        protocol: FirewallProtocol,
        port: u16,
        comment: Option<&str>,
    ) -> Result<(), WireError> {
        let encoded = quoted_comment(action, protocol, port, comment);
        let port_text = port.to_string();
        self.run_ok(
            &[
                "add",
                "rule",
                FAMILY,
                TABLE,
                chain.as_str(),
                protocol.as_str(),
                "dport",
                port_text.as_str(),
                action.verdict(),
                "comment",
                encoded.as_str(),
            ],
            "could not add the firewall rule",
        )
    }

    fn replace_handle(
        &self,
        chain: FirewallChain,
        handle: u64,
        action: FirewallAction,
        protocol: FirewallProtocol,
        port: u16,
        comment: Option<&str>,
    ) -> Result<(), WireError> {
        let handle_text = handle.to_string();
        let port_text = port.to_string();
        let encoded = quoted_comment(action, protocol, port, comment);
        self.run_ok(
            &[
                "replace",
                "rule",
                FAMILY,
                TABLE,
                chain.as_str(),
                "handle",
                handle_text.as_str(),
                protocol.as_str(),
                "dport",
                port_text.as_str(),
                action.verdict(),
                "comment",
                encoded.as_str(),
            ],
            "could not update the firewall rule",
        )
    }

    fn delete_handle(&self, chain: FirewallChain, handle: u64) -> Result<(), WireError> {
        let handle_text = handle.to_string();
        self.run_ok(
            &[
                "delete",
                "rule",
                FAMILY,
                TABLE,
                chain.as_str(),
                "handle",
                handle_text.as_str(),
            ],
            "could not delete the firewall rule",
        )
    }

    fn ensure_chain(&self, chain: FirewallChain) -> Result<(), WireError> {
        let table = self.run(&["list", "table", FAMILY, TABLE])?;
        if !table.ok {
            if !is_missing(&table.stderr) {
                return Err(nft_error("could not read the firewall table", &table));
            }
            self.run_ok(
                &["add", "table", FAMILY, TABLE],
                "could not create the firewall table",
            )?;
        }

        let listed = self.run(&["list", "chain", FAMILY, TABLE, chain.as_str()])?;
        if !listed.ok {
            if !is_missing(&listed.stderr) {
                return Err(nft_error("could not read the firewall chain", &listed));
            }
            let definition = format!(
                "{{ type filter hook {} priority filter; policy accept; }}",
                chain.hook()
            );
            self.run_ok(
                &[
                    "add",
                    "chain",
                    FAMILY,
                    TABLE,
                    chain.as_str(),
                    definition.as_str(),
                ],
                "could not create the firewall chain",
            )?;
        }
        Ok(())
    }

    fn run(&self, args: &[&str]) -> Result<NftOutput, WireError> {
        let argv: Vec<String> = args.iter().map(|value| (*value).to_string()).collect();
        self.nft.run(&argv).map_err(|detail| {
            WireError::new(
                ErrorKind::Unsupported,
                "nft_unavailable",
                format!("`nft` is not installed or could not be executed: {detail}"),
            )
        })
    }

    fn run_ok(&self, args: &[&str], context: &str) -> Result<(), WireError> {
        let output = self.run(args)?;
        if output.ok {
            Ok(())
        } else {
            Err(nft_error(context, &output))
        }
    }
}

fn action_past(action: FirewallAction) -> &'static str {
    match action {
        FirewallAction::Allow => "allowed",
        FirewallAction::Deny => "denied",
    }
}

fn validate_port(port: u16) -> Result<(), WireError> {
    if port == 0 {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "firewall_port",
            "port must be between 1 and 65535",
        ));
    }
    Ok(())
}

fn matching_handles(
    rules: &[FirewallRule],
    chain: FirewallChain,
    protocol: FirewallProtocol,
    port: u16,
) -> Vec<u64> {
    rules
        .iter()
        .filter(|rule| {
            rule.chain == chain
                && rule.action != "custom"
                && rule.protocol == Some(protocol)
                && rule.port == Some(port)
        })
        .map(|rule| rule.handle)
        .collect()
}

fn normalize_comment(comment: Option<&str>) -> Result<Option<String>, WireError> {
    let Some(comment) = comment.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(None);
    };
    let valid = comment.chars().count() <= MAX_COMMENT_CHARS
        && comment.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        });
    if !valid {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "firewall_comment",
            format!(
                "comment must be at most {MAX_COMMENT_CHARS} characters and use only letters, digits, `.`, `_` or `-`"
            ),
        ));
    }
    Ok(Some(comment.to_string()))
}

fn encode_comment(
    action: FirewallAction,
    protocol: FirewallProtocol,
    port: u16,
    comment: Option<&str>,
) -> String {
    let mut encoded = format!("{COMMENT_PREFIX}{action}:{protocol}:{port}");
    if let Some(comment) = comment {
        encoded.push('|');
        encoded.push_str(comment);
    }
    encoded
}

fn quoted_comment(
    action: FirewallAction,
    protocol: FirewallProtocol,
    port: u16,
    comment: Option<&str>,
) -> String {
    format!("\"{}\"", encode_comment(action, protocol, port, comment))
}

fn is_missing(stderr: &str) -> bool {
    let lowercase = stderr.to_lowercase();
    lowercase.contains("no such file")
        || lowercase.contains("does not exist")
        || lowercase.contains("not found")
        || lowercase.contains("could not process rule")
}

fn nft_error(context: &str, output: &NftOutput) -> WireError {
    let lowercase = output.stderr.to_lowercase();
    let (kind, code) = if lowercase.contains("operation not permitted")
        || lowercase.contains("permission denied")
        || lowercase.contains("you must be root")
    {
        (ErrorKind::Forbidden, "nft_permission")
    } else if is_missing(&output.stderr) {
        (ErrorKind::NotFound, "firewall_not_found")
    } else if lowercase.contains("syntax error") || lowercase.contains("invalid") {
        (ErrorKind::Invalid, "nft_refused")
    } else {
        (ErrorKind::Internal, "nft_failed")
    };
    let stderr = output
        .stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    let detail = if stderr.is_empty() {
        context.to_string()
    } else if code == "nft_permission" {
        format!("firewall changes need CAP_NET_ADMIN; `nft` said: {stderr}")
    } else {
        format!("{context}: {stderr}")
    };
    WireError::new(kind, code, detail)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct FakeNft {
        calls: Mutex<Vec<Vec<String>>>,
        responses: Mutex<VecDeque<Result<NftOutput, String>>>,
    }

    impl FakeNft {
        fn new(responses: Vec<Result<NftOutput, String>>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                responses: Mutex::new(responses.into()),
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().expect("lock").clone()
        }
    }

    impl NftRunner for FakeNft {
        fn run(&self, args: &[String]) -> Result<NftOutput, String> {
            self.calls.lock().expect("lock").push(args.to_vec());
            self.responses
                .lock()
                .expect("lock")
                .pop_front()
                .unwrap_or_else(|| Err("fake runner ran out of responses".to_string()))
        }
    }

    fn ok(stdout: &str) -> Result<NftOutput, String> {
        Ok(NftOutput {
            ok: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    fn fail(stderr: &str) -> Result<NftOutput, String> {
        Ok(NftOutput {
            ok: false,
            stdout: String::new(),
            stderr: stderr.to_string(),
        })
    }

    fn build_service(
        control: bool,
        responses: Vec<Result<NftOutput, String>>,
    ) -> (FirewallService, Arc<FakeNft>) {
        let nft = Arc::new(FakeNft::new(responses));
        let service = FirewallService::with_runner(control, nft.clone());
        (service, nft)
    }

    fn json_with_rules() -> String {
        serde_json::json!({
            "nftables": [
                {"chain": {"family": "inet", "table": "anthracite", "name": "input", "type": "filter", "hook": "input", "policy": "drop"}},
                {"chain": {"family": "inet", "table": "anthracite", "name": "output", "type": "filter", "hook": "output", "policy": "accept"}},
                {"rule": {"family": "inet", "table": "anthracite", "chain": "input", "handle": 3, "comment": "anth:deny:tcp:80|http"}},
                {"rule": {"family": "inet", "table": "anthracite", "chain": "input", "handle": 4, "comment": "anth:deny:udp:53|dns"}},
                {"rule": {"family": "inet", "table": "anthracite", "chain": "input", "handle": 5, "comment": "manual"}},
                {"rule": {"family": "inet", "table": "anthracite", "chain": "output", "handle": 6, "comment": "anth:allow:tcp:443|https"}}
            ]
        })
        .to_string()
    }

    #[test]
    fn status_parses_rules_chains_and_the_raw_ruleset() {
        let (service, _nft) = build_service(
            true,
            vec![
                ok(&json_with_rules()),
                ok("table inet anthracite { chain input { ... } }"),
            ],
        );
        let snapshot = service.snapshot().expect("snapshot");
        assert!(snapshot.initialized);
        assert!(snapshot.control_enabled);
        assert!(snapshot.timestamp_ms > 0);
        assert_eq!(snapshot.rules.len(), 4);
        assert_eq!(snapshot.rules[0].handle, 3);
        assert_eq!(snapshot.rules[0].chain, FirewallChain::Input);
        assert_eq!(snapshot.rules[2].action, "custom");
        assert_eq!(snapshot.rules[3].handle, 6);
        assert_eq!(snapshot.rules[3].chain, FirewallChain::Output);
        assert_eq!(snapshot.chains.len(), 2);
        assert_eq!(snapshot.chains[0].chain, FirewallChain::Input);
        assert_eq!(snapshot.chains[0].policy, FirewallAction::Deny);
        assert_eq!(snapshot.chains[1].policy, FirewallAction::Allow);
        assert!(snapshot.ruleset.contains("table inet anthracite"));
    }

    #[test]
    fn a_missing_table_is_an_uninitialized_snapshot() {
        let (service, _nft) = build_service(false, vec![fail("No such file or directory")]);
        let snapshot = service.snapshot().expect("snapshot");
        assert!(!snapshot.initialized);
        assert!(!snapshot.control_enabled);
        assert!(snapshot.rules.is_empty());
        assert!(snapshot.ruleset.is_empty());
        assert_eq!(snapshot.chains.len(), 2);
        assert!(snapshot.chains.iter().all(|state| !state.initialized));
        assert!(
            snapshot
                .chains
                .iter()
                .all(|state| state.policy == FirewallAction::Allow)
        );
    }

    #[test]
    fn deny_creates_the_table_and_input_chain_before_the_rule() {
        let (service, nft) = build_service(
            true,
            vec![
                fail("No such file or directory"),
                ok(""),
                fail("No such file or directory"),
                ok(""),
                ok(r#"{"nftables":[]}"#),
                ok(""),
            ],
        );
        let message = service
            .add(
                FirewallChain::Input,
                FirewallAction::Deny,
                FirewallProtocol::Tcp,
                80,
                Some("http"),
            )
            .expect("deny");
        assert_eq!(message, "denied tcp/80 on input");
        assert_eq!(
            nft.calls(),
            vec![
                vec!["list", "table", "inet", "anthracite"],
                vec!["add", "table", "inet", "anthracite"],
                vec!["list", "chain", "inet", "anthracite", "input"],
                vec![
                    "add",
                    "chain",
                    "inet",
                    "anthracite",
                    "input",
                    "{ type filter hook input priority filter; policy accept; }"
                ],
                vec!["-j", "list", "table", "inet", "anthracite"],
                vec![
                    "add",
                    "rule",
                    "inet",
                    "anthracite",
                    "input",
                    "tcp",
                    "dport",
                    "80",
                    "drop",
                    "comment",
                    "\"anth:deny:tcp:80|http\""
                ],
            ]
        );
    }

    #[test]
    fn allow_adds_an_accept_rule() {
        let (service, nft) =
            build_service(true, vec![ok(""), ok(""), ok(r#"{"nftables":[]}"#), ok("")]);
        let message = service
            .add(
                FirewallChain::Input,
                FirewallAction::Allow,
                FirewallProtocol::Tcp,
                80,
                Some("http"),
            )
            .expect("allow");
        assert_eq!(message, "allowed tcp/80 on input");
        assert_eq!(
            nft.calls().last().expect("add"),
            &vec![
                "add",
                "rule",
                "inet",
                "anthracite",
                "input",
                "tcp",
                "dport",
                "80",
                "accept",
                "comment",
                "\"anth:allow:tcp:80|http\""
            ]
        );
    }

    #[test]
    fn add_on_output_creates_the_output_chain_with_its_own_hook() {
        let (service, nft) = build_service(
            true,
            vec![
                fail("No such file or directory"),
                ok(""),
                fail("No such file or directory"),
                ok(""),
                ok(r#"{"nftables":[]}"#),
                ok(""),
            ],
        );
        let message = service
            .add(
                FirewallChain::Output,
                FirewallAction::Deny,
                FirewallProtocol::Udp,
                53,
                Some("dns"),
            )
            .expect("deny");
        assert_eq!(message, "denied udp/53 on output");
        let calls = nft.calls();
        assert_eq!(
            calls[3],
            vec![
                "add",
                "chain",
                "inet",
                "anthracite",
                "output",
                "{ type filter hook output priority filter; policy accept; }"
            ]
        );
        assert_eq!(calls[5][4], "output");
    }

    #[test]
    fn adding_an_existing_target_replaces_the_rule_in_place() {
        let (service, nft) =
            build_service(true, vec![ok(""), ok(""), ok(&json_with_rules()), ok("")]);
        let message = service
            .add(
                FirewallChain::Input,
                FirewallAction::Allow,
                FirewallProtocol::Tcp,
                80,
                Some("web"),
            )
            .expect("allow");
        assert_eq!(message, "allowed tcp/80 on input");
        assert_eq!(
            nft.calls().last().expect("replace"),
            &vec![
                "replace",
                "rule",
                "inet",
                "anthracite",
                "input",
                "handle",
                "3",
                "tcp",
                "dport",
                "80",
                "accept",
                "comment",
                "\"anth:allow:tcp:80|web\""
            ]
        );
    }

    #[test]
    fn the_same_target_in_the_other_chain_is_added_separately() {
        let (service, nft) =
            build_service(true, vec![ok(""), ok(""), ok(&json_with_rules()), ok("")]);
        service
            .add(
                FirewallChain::Output,
                FirewallAction::Allow,
                FirewallProtocol::Tcp,
                80,
                None,
            )
            .expect("allow");
        let calls = nft.calls();
        assert_eq!(
            calls.last().expect("add"),
            &vec![
                "add",
                "rule",
                "inet",
                "anthracite",
                "output",
                "tcp",
                "dport",
                "80",
                "accept",
                "comment",
                "\"anth:allow:tcp:80\""
            ]
        );
    }

    #[test]
    fn extra_duplicate_rules_are_removed_after_the_replace() {
        let rules = serde_json::json!({
            "nftables": [
                {"rule": {"family": "inet", "table": "anthracite", "chain": "input", "handle": 3, "comment": "anth:deny:tcp:80|http"}},
                {"rule": {"family": "inet", "table": "anthracite", "chain": "input", "handle": 7, "comment": "anth:deny:tcp:80|old"}}
            ]
        })
        .to_string();
        let (service, nft) = build_service(true, vec![ok(""), ok(""), ok(&rules), ok(""), ok("")]);
        service
            .add(
                FirewallChain::Input,
                FirewallAction::Allow,
                FirewallProtocol::Tcp,
                80,
                None,
            )
            .expect("allow");
        let calls = nft.calls();
        assert_eq!(calls[3][0], "replace");
        assert_eq!(
            calls[4],
            vec![
                "delete",
                "rule",
                "inet",
                "anthracite",
                "input",
                "handle",
                "7"
            ]
        );
    }

    #[test]
    fn update_rewrites_one_managed_rule() {
        let (service, nft) = build_service(true, vec![ok(&json_with_rules()), ok("")]);
        let message = service
            .update(
                3,
                FirewallChain::Input,
                FirewallAction::Allow,
                FirewallProtocol::Tcp,
                443,
                Some("https"),
            )
            .expect("update");
        assert_eq!(message, "updated firewall rule 3 to allow tcp/443 on input");
        assert_eq!(
            nft.calls(),
            vec![
                vec!["-j", "list", "table", "inet", "anthracite"],
                vec![
                    "replace",
                    "rule",
                    "inet",
                    "anthracite",
                    "input",
                    "handle",
                    "3",
                    "tcp",
                    "dport",
                    "443",
                    "accept",
                    "comment",
                    "\"anth:allow:tcp:443|https\""
                ],
            ]
        );
    }

    #[test]
    fn update_moves_a_rule_to_the_other_chain() {
        let (service, nft) = build_service(
            true,
            vec![ok(&json_with_rules()), ok(""), ok(""), ok(""), ok("")],
        );
        let message = service
            .update(
                3,
                FirewallChain::Output,
                FirewallAction::Allow,
                FirewallProtocol::Tcp,
                8443,
                Some("admin"),
            )
            .expect("update");
        assert_eq!(
            message,
            "updated firewall rule 3 to allow tcp/8443 on output"
        );
        let calls = nft.calls();
        assert_eq!(calls[1], vec!["list", "table", "inet", "anthracite"]);
        assert_eq!(
            calls[2],
            vec!["list", "chain", "inet", "anthracite", "output"]
        );
        assert_eq!(
            calls[3],
            vec![
                "add",
                "rule",
                "inet",
                "anthracite",
                "output",
                "tcp",
                "dport",
                "8443",
                "accept",
                "comment",
                "\"anth:allow:tcp:8443|admin\""
            ]
        );
        assert_eq!(
            calls[4],
            vec![
                "delete",
                "rule",
                "inet",
                "anthracite",
                "input",
                "handle",
                "3"
            ]
        );
    }

    #[test]
    fn update_refuses_unknown_custom_and_duplicate_rules() {
        let (service, nft) = build_service(true, vec![fail("No such file or directory")]);
        assert_eq!(
            service
                .update(
                    3,
                    FirewallChain::Input,
                    FirewallAction::Allow,
                    FirewallProtocol::Tcp,
                    80,
                    None
                )
                .expect_err("missing table")
                .code,
            "firewall_not_found"
        );
        assert_eq!(nft.calls().len(), 1);

        let (service, nft) = build_service(true, vec![ok(&json_with_rules())]);
        assert_eq!(
            service
                .update(
                    5,
                    FirewallChain::Input,
                    FirewallAction::Allow,
                    FirewallProtocol::Tcp,
                    80,
                    None
                )
                .expect_err("custom")
                .code,
            "firewall_custom_rule"
        );
        assert_eq!(nft.calls().len(), 1);

        let (service, nft) = build_service(true, vec![ok(&json_with_rules())]);
        assert_eq!(
            service
                .update(
                    99,
                    FirewallChain::Input,
                    FirewallAction::Allow,
                    FirewallProtocol::Tcp,
                    80,
                    None
                )
                .expect_err("unknown")
                .code,
            "firewall_rule_not_found"
        );
        assert_eq!(nft.calls().len(), 1);

        let (service, nft) = build_service(true, vec![ok(&json_with_rules())]);
        assert_eq!(
            service
                .update(
                    3,
                    FirewallChain::Input,
                    FirewallAction::Deny,
                    FirewallProtocol::Udp,
                    53,
                    None
                )
                .expect_err("duplicate")
                .code,
            "firewall_duplicate"
        );
        assert_eq!(nft.calls().len(), 1);
    }

    #[test]
    fn delete_finds_the_rule_chain_before_deleting() {
        let (service, nft) = build_service(true, vec![ok(&json_with_rules()), ok("")]);
        assert_eq!(
            service.delete(6).expect("delete"),
            "deleted firewall rule 6"
        );
        assert_eq!(
            nft.calls().last().expect("delete"),
            &vec![
                "delete",
                "rule",
                "inet",
                "anthracite",
                "output",
                "handle",
                "6"
            ]
        );

        let (service, _nft) = build_service(true, vec![ok(&json_with_rules())]);
        assert_eq!(
            service.delete(99).expect_err("unknown").code,
            "firewall_rule_not_found"
        );
    }

    #[test]
    fn flush_targets_one_chain() {
        let (service, nft) = build_service(true, vec![ok(&json_with_rules()), ok("")]);
        assert_eq!(
            service.flush(FirewallChain::Input).expect("flush"),
            "flushed 3 firewall rules from input"
        );
        assert_eq!(
            nft.calls().last().expect("flush"),
            &vec!["flush", "chain", "inet", "anthracite", "input"]
        );

        let (service, nft) = build_service(true, vec![ok(&json_with_rules()), ok("")]);
        assert_eq!(
            service.flush(FirewallChain::Output).expect("flush"),
            "flushed 1 firewall rule from output"
        );
        assert_eq!(
            nft.calls().last().expect("flush"),
            &vec!["flush", "chain", "inet", "anthracite", "output"]
        );
    }

    #[test]
    fn set_policy_redefines_the_chain_policy_without_touching_rules() {
        let (service, nft) =
            build_service(true, vec![ok(""), ok(""), ok(&json_with_rules()), ok("")]);
        let message = service
            .set_policy(FirewallChain::Input, FirewallAction::Allow)
            .expect("policy");
        assert_eq!(message, "set input policy to allow");
        assert_eq!(
            nft.calls().last().expect("policy"),
            &vec![
                "add",
                "chain",
                "inet",
                "anthracite",
                "input",
                "{ policy accept; }"
            ]
        );
    }

    #[test]
    fn setting_the_same_policy_is_a_no_op() {
        let (service, nft) = build_service(true, vec![ok(""), ok(""), ok(&json_with_rules())]);
        let message = service
            .set_policy(FirewallChain::Input, FirewallAction::Deny)
            .expect("policy");
        assert_eq!(message, "input policy is already deny");
        assert_eq!(nft.calls().len(), 3);
    }

    #[test]
    fn flush_of_a_missing_chain_is_a_no_op() {
        let (service, nft) = build_service(true, vec![ok(r#"{"nftables":[]}"#)]);
        assert_eq!(
            service.flush(FirewallChain::Output).expect("flush"),
            "output chain is not initialized"
        );
        assert_eq!(nft.calls().len(), 1);
    }

    #[test]
    fn set_policy_creates_a_missing_chain_before_changing_it() {
        let after_create = serde_json::json!({
            "nftables": [
                {"chain": {"family": "inet", "table": "anthracite", "name": "output", "type": "filter", "hook": "output", "policy": "accept"}}
            ]
        })
        .to_string();
        let (service, nft) = build_service(
            true,
            vec![
                ok(""),
                fail("No such file or directory"),
                ok(""),
                ok(&after_create),
                ok(""),
            ],
        );
        assert_eq!(
            service
                .set_policy(FirewallChain::Output, FirewallAction::Deny)
                .expect("policy"),
            "set output policy to deny"
        );
        let calls = nft.calls();
        assert_eq!(calls[2][4], "output");
        assert_eq!(
            calls.last().expect("policy"),
            &vec![
                "add",
                "chain",
                "inet",
                "anthracite",
                "output",
                "{ policy drop; }"
            ]
        );
    }

    #[test]
    fn policy_and_argument_guards_run_before_nft() {
        let (disabled, nft) = build_service(false, vec![]);
        assert_eq!(
            disabled
                .add(
                    FirewallChain::Input,
                    FirewallAction::Deny,
                    FirewallProtocol::Tcp,
                    80,
                    None
                )
                .expect_err("disabled")
                .code,
            "firewall_control_disabled"
        );
        assert_eq!(
            disabled
                .set_policy(FirewallChain::Input, FirewallAction::Deny)
                .expect_err("disabled policy")
                .code,
            "firewall_control_disabled"
        );
        assert!(nft.calls().is_empty());

        let (service, nft) = build_service(true, vec![]);
        assert_eq!(
            service
                .add(
                    FirewallChain::Input,
                    FirewallAction::Deny,
                    FirewallProtocol::Tcp,
                    0,
                    None
                )
                .expect_err("port")
                .code,
            "firewall_port"
        );
        assert_eq!(
            service
                .add(
                    FirewallChain::Input,
                    FirewallAction::Deny,
                    FirewallProtocol::Tcp,
                    80,
                    Some("bad comment;")
                )
                .expect_err("comment")
                .code,
            "firewall_comment"
        );
        assert_eq!(
            service.delete(0).expect_err("handle").code,
            "firewall_handle"
        );
        assert_eq!(
            service
                .update(
                    0,
                    FirewallChain::Input,
                    FirewallAction::Allow,
                    FirewallProtocol::Tcp,
                    80,
                    None
                )
                .expect_err("update handle")
                .code,
            "firewall_handle"
        );
        assert_eq!(
            service
                .update(
                    3,
                    FirewallChain::Input,
                    FirewallAction::Allow,
                    FirewallProtocol::Tcp,
                    0,
                    None
                )
                .expect_err("update port")
                .code,
            "firewall_port"
        );
        assert_eq!(
            service
                .update(
                    3,
                    FirewallChain::Input,
                    FirewallAction::Allow,
                    FirewallProtocol::Tcp,
                    80,
                    Some("bad comment;")
                )
                .expect_err("update comment")
                .code,
            "firewall_comment"
        );
        assert!(nft.calls().is_empty());
    }

    #[test]
    fn nft_failures_are_classified() {
        let (service, _nft) = build_service(
            true,
            vec![fail("Operation not permitted (you must be root)")],
        );
        let error = service.snapshot().expect_err("permission");
        assert_eq!(error.kind, ErrorKind::Forbidden);
        assert_eq!(error.code, "nft_permission");

        let (service, _nft) = build_service(true, vec![Err("not found".to_string())]);
        let error = service.snapshot().expect_err("unavailable");
        assert_eq!(error.kind, ErrorKind::Unsupported);
        assert_eq!(error.code, "nft_unavailable");
    }
}
