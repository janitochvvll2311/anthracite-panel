//! The managed nftables firewall.
//!
//! The daemon owns exactly one table and one chain:
//!
//! ```text
//! inet anthracite
//! └── input (type filter hook input priority filter; policy accept)
//! ```
//!
//! Every read and write is scoped to that table. `nft` is invoked directly with
//! an argument vector; nothing goes through a shell. Managed rules carry an
//! `anth:` comment so their intent and handle can be recovered from nft alone.
//!
//! The chain is a blocklist with an accept policy. `allow` therefore removes a
//! matching managed `drop` rule instead of adding an `accept` rule: an accept
//! in this table cannot override a drop in another table, so pretending
//! otherwise would be misleading.

mod parse;

use std::process::Command;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_wire::{
    ErrorKind, FirewallAction, FirewallProtocol, FirewallRule, FirewallSnapshot, WireError,
};

use parse::{COMMENT_PREFIX, parse_rules};

const FAMILY: &str = "inet";
const TABLE: &str = "anthracite";
const CHAIN: &str = "input";
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
        let Some((rules, ruleset)) = self.read_table()? else {
            return Ok(FirewallSnapshot {
                initialized: false,
                control_enabled: self.control,
                rules: Vec::new(),
                ruleset: String::new(),
                timestamp_ms: now_ms(),
            });
        };
        Ok(FirewallSnapshot {
            initialized: true,
            control_enabled: self.control,
            rules,
            ruleset,
            timestamp_ms: now_ms(),
        })
    }

    /// Adds a deny rule, or removes the matching managed deny rules for allow.
    pub fn add(
        &self,
        action: FirewallAction,
        protocol: FirewallProtocol,
        port: u16,
        comment: Option<&str>,
    ) -> Result<String, WireError> {
        self.require_control()?;
        if port == 0 {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "firewall_port",
                "port must be between 1 and 65535",
            ));
        }
        let comment = normalize_comment(comment)?;

        if action == FirewallAction::Allow {
            let Some((rules, _)) = self.read_table()? else {
                return Ok(already_allowed(protocol, port));
            };
            let handles: Vec<u64> = rules
                .iter()
                .filter(|rule| {
                    rule.action == "deny"
                        && rule.protocol == Some(protocol)
                        && rule.port == Some(port)
                })
                .map(|rule| rule.handle)
                .collect();
            if handles.is_empty() {
                return Ok(already_allowed(protocol, port));
            }
            for handle in handles {
                self.delete_handle(handle)?;
            }
            return Ok(format!("allowed {protocol}/{port}"));
        }

        self.ensure_chain()?;
        let encoded = encode_comment(action, protocol, port, comment.as_deref());
        let quoted = format!("\"{encoded}\"");
        let port_text = port.to_string();
        let args = [
            "add",
            "rule",
            FAMILY,
            TABLE,
            CHAIN,
            protocol.as_str(),
            "dport",
            port_text.as_str(),
            action.verdict(),
            "comment",
            quoted.as_str(),
        ];
        self.run_ok(&args, "could not add the firewall rule")?;
        Ok(format!("denied {protocol}/{port}"))
    }

    /// Deletes one rule in the managed chain by its handle.
    pub fn delete(&self, handle: u64) -> Result<String, WireError> {
        self.require_control()?;
        if handle == 0 {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "firewall_handle",
                "rule handle must be greater than zero",
            ));
        }
        self.delete_handle(handle)?;
        Ok(format!("deleted firewall rule {handle}"))
    }

    /// Removes every rule from the managed chain.
    pub fn flush(&self) -> Result<String, WireError> {
        self.require_control()?;
        let Some((rules, _)) = self.read_table()? else {
            return Ok("firewall table is not initialized".to_string());
        };
        self.run_ok(
            &["flush", "chain", FAMILY, TABLE, CHAIN],
            "could not flush the firewall chain",
        )?;
        Ok(format!(
            "flushed {} firewall rule{}",
            rules.len(),
            if rules.len() == 1 { "" } else { "s" }
        ))
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

    /// Reads the managed chain and its textual ruleset. A missing table is
    /// `None`; any other refusal is an error.
    fn read_table(&self) -> Result<Option<(Vec<FirewallRule>, String)>, WireError> {
        let json = self.run(&["-j", "list", "table", FAMILY, TABLE])?;
        if !json.ok {
            if is_missing(&json.stderr) {
                return Ok(None);
            }
            return Err(nft_error("could not read the firewall table", &json));
        }

        let text = self.run(&["list", "table", FAMILY, TABLE])?;
        if !text.ok {
            return Err(nft_error("could not read the firewall table", &text));
        }

        let rules = parse_rules(&json.stdout)
            .map_err(|message| WireError::new(ErrorKind::Internal, "nft_parse", message))?;
        Ok(Some((rules, text.stdout.trim_end().to_string())))
    }

    fn delete_handle(&self, handle: u64) -> Result<(), WireError> {
        let handle_text = handle.to_string();
        self.run_ok(
            &[
                "delete",
                "rule",
                FAMILY,
                TABLE,
                CHAIN,
                "handle",
                handle_text.as_str(),
            ],
            "could not delete the firewall rule",
        )
    }

    fn ensure_chain(&self) -> Result<(), WireError> {
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

        let chain = self.run(&["list", "chain", FAMILY, TABLE, CHAIN])?;
        if !chain.ok {
            if !is_missing(&chain.stderr) {
                return Err(nft_error("could not read the firewall chain", &chain));
            }
            self.run_ok(
                &[
                    "add",
                    "chain",
                    FAMILY,
                    TABLE,
                    CHAIN,
                    "{ type filter hook input priority filter; policy accept; }",
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

fn already_allowed(protocol: FirewallProtocol, port: u16) -> String {
    format!("no managed deny rule for {protocol}/{port}; traffic is already allowed")
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
                {"rule": {"family": "inet", "table": "anthracite", "chain": "input", "handle": 3, "comment": "anth:deny:tcp:80|http"}},
                {"rule": {"family": "inet", "table": "anthracite", "chain": "input", "handle": 4, "comment": "anth:deny:udp:53|dns"}},
                {"rule": {"family": "inet", "table": "anthracite", "chain": "input", "handle": 5, "comment": "manual"}}
            ]
        })
        .to_string()
    }

    #[test]
    fn status_parses_rules_and_reads_the_raw_ruleset() {
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
        assert_eq!(snapshot.rules.len(), 3);
        assert_eq!(snapshot.rules[0].handle, 3);
        assert_eq!(snapshot.rules[2].action, "custom");
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
    }

    #[test]
    fn deny_creates_the_table_and_chain_before_the_rule() {
        let (service, nft) = build_service(
            true,
            vec![
                fail("No such file or directory"),
                ok(""),
                fail("No such file or directory"),
                ok(""),
                ok(""),
            ],
        );
        let message = service
            .add(
                FirewallAction::Deny,
                FirewallProtocol::Tcp,
                80,
                Some("http"),
            )
            .expect("deny");
        assert_eq!(message, "denied tcp/80");
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
    fn allow_removes_matching_denies_and_never_adds_an_accept_rule() {
        let (service, nft) = build_service(true, vec![ok(&json_with_rules()), ok("text"), ok("")]);
        let message = service
            .add(FirewallAction::Allow, FirewallProtocol::Tcp, 80, None)
            .expect("allow");
        assert_eq!(message, "allowed tcp/80");
        assert_eq!(
            nft.calls(),
            vec![
                vec!["-j", "list", "table", "inet", "anthracite"],
                vec!["list", "table", "inet", "anthracite"],
                vec![
                    "delete",
                    "rule",
                    "inet",
                    "anthracite",
                    "input",
                    "handle",
                    "3"
                ],
            ]
        );
    }

    #[test]
    fn allow_without_a_matching_deny_is_a_no_op() {
        let (service, nft) = build_service(true, vec![ok(r#"{"nftables":[]}"#), ok("text")]);
        let message = service
            .add(FirewallAction::Allow, FirewallProtocol::Udp, 123, None)
            .expect("allow");
        assert!(message.contains("already allowed"));
        assert_eq!(nft.calls().len(), 2);
    }

    #[test]
    fn delete_and_flush_use_the_managed_chain_only() {
        let (service, nft) = build_service(
            true,
            vec![ok(""), ok(&json_with_rules()), ok("text"), ok("")],
        );
        assert_eq!(
            service.delete(7).expect("delete"),
            "deleted firewall rule 7"
        );
        assert_eq!(service.flush().expect("flush"), "flushed 3 firewall rules");
        let calls = nft.calls();
        assert_eq!(
            calls[0],
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
        assert_eq!(
            calls.last().expect("flush"),
            &vec!["flush", "chain", "inet", "anthracite", "input"]
        );
    }

    #[test]
    fn policy_and_argument_guards_run_before_nft() {
        let (disabled, nft) = build_service(false, vec![]);
        assert_eq!(
            disabled
                .add(FirewallAction::Deny, FirewallProtocol::Tcp, 80, None)
                .expect_err("disabled")
                .code,
            "firewall_control_disabled"
        );
        assert!(nft.calls().is_empty());

        let (service, nft) = build_service(true, vec![]);
        assert_eq!(
            service
                .add(FirewallAction::Deny, FirewallProtocol::Tcp, 0, None)
                .expect_err("port")
                .code,
            "firewall_port"
        );
        assert_eq!(
            service
                .add(
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
