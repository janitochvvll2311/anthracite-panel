//! nftables firewall management.
//!
//! The daemon owns a dedicated table and input chain:
//!
//! ```text
//! inet anthracite
//! └── input (type filter hook input priority filter; policy accept)
//! ```
//!
//! Only that table is read or changed. `nft` is invoked directly with an
//! argument vector, never through a shell, and every rule created by the panel
//! carries an `anth:` comment so its handle and intent can be recovered.

use std::io;
use std::process::Command;

use anth_proto::{
    FirewallAction, FirewallError, FirewallErrorKind, FirewallProtocol, FirewallRule,
    FirewallSnapshot,
};
use serde_json::Value;

const FAMILY: &str = "inet";
const TABLE: &str = "anthracite";
const CHAIN: &str = "input";
const COMMENT_PREFIX: &str = "anth:";
const MAX_COMMENT_CHARS: usize = 32;

/// Result of one `nft` invocation, decoupled from `std::process::Output` so
/// tests can script responses without spawning a process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NftOutput {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

pub trait NftRunner {
    fn run(&self, args: &[&str]) -> Result<NftOutput, FirewallError>;
}

pub struct SystemNft {
    use_sudo: bool,
}

impl SystemNft {
    pub fn new(use_sudo: bool) -> Self {
        Self { use_sudo }
    }
}

impl NftRunner for SystemNft {
    fn run(&self, args: &[&str]) -> Result<NftOutput, FirewallError> {
        if !self.use_sudo {
            return execute(false, args);
        }

        let output = execute(true, args);
        if should_fallback_from_sudo(&output) {
            execute(false, args)
        } else {
            output
        }
    }
}

fn execute(use_sudo: bool, args: &[&str]) -> Result<NftOutput, FirewallError> {
    let output = nft_command(use_sudo, args).env("LC_ALL", "C").output();

    match output {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join("; ");
            Ok(NftOutput {
                ok: output.status.success(),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr,
            })
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let program = if use_sudo { "sudo" } else { "nft" };
            Err(FirewallError::new(
                FirewallErrorKind::Unavailable,
                format!("{program} is not installed or is not on PATH"),
            ))
        }
        Err(error) => Err(FirewallError::new(
            FirewallErrorKind::Failed,
            format!("could not run nft: {error}"),
        )),
    }
}

/// If sudo cannot elevate, retry nft directly so a file capability or a root
/// daemon still works. Genuine nft errors do not trigger a second attempt.
fn should_fallback_from_sudo(result: &Result<NftOutput, FirewallError>) -> bool {
    match result {
        Err(error) => error.kind == FirewallErrorKind::Unavailable,
        Ok(output) if !output.ok => {
            let lowercase = output.stderr.to_lowercase();
            lowercase.contains("a password is required")
                || lowercase.contains("a terminal is required")
                || lowercase.contains("no tty present")
                || lowercase.contains("not in the sudoers")
                || lowercase.contains("not allowed to execute")
                || lowercase.contains("command not found")
                || lowercase.contains("askpass")
        }
        _ => false,
    }
}

fn nft_command(use_sudo: bool, args: &[&str]) -> Command {
    let mut command = if use_sudo {
        let mut command = Command::new("sudo");
        command.arg("-n").arg("nft");
        command
    } else {
        Command::new("nft")
    };
    command.args(args);
    command
}

/// Reads the managed table. A missing table is reported as uninitialized.
pub fn status(
    control_enabled: bool,
    sudo_enabled: bool,
) -> Result<FirewallSnapshot, FirewallError> {
    status_with(&SystemNft::new(sudo_enabled), control_enabled)
}

/// Adds an allow/deny rule to the managed chain.
pub fn add_rule(
    control_enabled: bool,
    sudo_enabled: bool,
    action: FirewallAction,
    port: u16,
    protocol: FirewallProtocol,
    comment: Option<&str>,
) -> Result<String, FirewallError> {
    add_rule_with(
        &SystemNft::new(sudo_enabled),
        control_enabled,
        action,
        port,
        protocol,
        comment,
    )
}

/// Deletes one rule from the managed chain by its nftables handle.
pub fn delete_rule(
    control_enabled: bool,
    sudo_enabled: bool,
    handle: u64,
) -> Result<String, FirewallError> {
    delete_rule_with(&SystemNft::new(sudo_enabled), control_enabled, handle)
}

/// Removes every rule from the managed chain.
pub fn flush(control_enabled: bool, sudo_enabled: bool) -> Result<String, FirewallError> {
    flush_with(&SystemNft::new(sudo_enabled), control_enabled)
}

fn status_with<R: NftRunner>(
    runner: &R,
    control_enabled: bool,
) -> Result<FirewallSnapshot, FirewallError> {
    let json = runner.run(&["-j", "list", "table", FAMILY, TABLE])?;
    if !json.ok {
        if is_missing(&json.stderr) {
            return Ok(FirewallSnapshot {
                initialized: false,
                control_enabled,
                rules: Vec::new(),
                ruleset: String::new(),
            });
        }
        return Err(nft_error(&json, "could not read the firewall table"));
    }

    let text = runner.run(&["list", "table", FAMILY, TABLE])?;
    if !text.ok {
        return Err(nft_error(&text, "could not read the firewall table"));
    }

    let mut rules = parse_rules(&json.stdout)?;
    rules.sort_by_key(|rule| rule.handle);

    Ok(FirewallSnapshot {
        initialized: true,
        control_enabled,
        rules,
        ruleset: text.stdout.trim_end().to_string(),
    })
}

fn add_rule_with<R: NftRunner>(
    runner: &R,
    control_enabled: bool,
    action: FirewallAction,
    port: u16,
    protocol: FirewallProtocol,
    comment: Option<&str>,
) -> Result<String, FirewallError> {
    require_control(control_enabled)?;
    if port == 0 {
        return Err(FirewallError::new(
            FirewallErrorKind::Invalid,
            "port must be between 1 and 65535",
        ));
    }
    let user_comment = normalize_comment(comment)?;

    // The managed chain is a blocklist with an accept policy. `allow` removes a
    // matching managed drop instead of adding an accept verdict, which would only
    // end this chain and cannot override another firewall table.
    if action == FirewallAction::Allow {
        let snapshot = status_with(runner, control_enabled)?;
        let handles: Vec<u64> = snapshot
            .rules
            .iter()
            .filter(|rule| {
                rule.action == "deny" && rule.protocol == Some(protocol) && rule.port == Some(port)
            })
            .map(|rule| rule.handle)
            .collect();

        if handles.is_empty() {
            return Ok(format!(
                "no managed deny rule for {protocol}/{port}; traffic is already allowed"
            ));
        }
        for handle in &handles {
            delete_handle(runner, *handle)?;
        }
        return Ok(format!("allowed {protocol}/{port}"));
    }

    ensure_chain(runner)?;

    let port_text = port.to_string();
    let encoded = encode_comment(action, protocol, port, user_comment.as_deref());
    let quoted_comment = format!("\"{encoded}\"");
    let args = vec![
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
        quoted_comment.as_str(),
    ];
    run_ok(runner, &args, "could not add the firewall rule")?;

    Ok(format!("denied {protocol}/{port}"))
}

fn delete_handle<R: NftRunner>(runner: &R, handle: u64) -> Result<(), FirewallError> {
    let handle_text = handle.to_string();
    let args = vec![
        "delete",
        "rule",
        FAMILY,
        TABLE,
        CHAIN,
        "handle",
        handle_text.as_str(),
    ];
    run_ok(runner, &args, "could not delete the firewall rule")
}

fn delete_rule_with<R: NftRunner>(
    runner: &R,
    control_enabled: bool,
    handle: u64,
) -> Result<String, FirewallError> {
    require_control(control_enabled)?;
    if handle == 0 {
        return Err(FirewallError::new(
            FirewallErrorKind::Invalid,
            "rule handle must be greater than zero",
        ));
    }

    delete_handle(runner, handle)?;
    Ok(format!("deleted firewall rule {handle}"))
}

fn flush_with<R: NftRunner>(runner: &R, control_enabled: bool) -> Result<String, FirewallError> {
    require_control(control_enabled)?;

    let snapshot = status_with(runner, control_enabled)?;
    if !snapshot.initialized {
        return Ok("firewall table is not initialized".to_string());
    }

    let args = vec!["flush", "chain", FAMILY, TABLE, CHAIN];
    run_ok(runner, &args, "could not flush the firewall chain")?;
    Ok(format!(
        "flushed {} firewall rule{}",
        snapshot.rules.len(),
        if snapshot.rules.len() == 1 { "" } else { "s" }
    ))
}

fn ensure_chain<R: NftRunner>(runner: &R) -> Result<(), FirewallError> {
    let table = runner.run(&["list", "table", FAMILY, TABLE])?;
    if !table.ok {
        if !is_missing(&table.stderr) {
            return Err(nft_error(&table, "could not read the firewall table"));
        }
        run_ok(
            runner,
            &["add", "table", FAMILY, TABLE],
            "could not create the firewall table",
        )?;
    }

    let chain = runner.run(&["list", "chain", FAMILY, TABLE, CHAIN])?;
    if !chain.ok {
        if !is_missing(&chain.stderr) {
            return Err(nft_error(&chain, "could not read the firewall chain"));
        }
        run_ok(
            runner,
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

fn require_control(control_enabled: bool) -> Result<(), FirewallError> {
    if control_enabled {
        Ok(())
    } else {
        Err(FirewallError::new(
            FirewallErrorKind::Disabled,
            "firewall control is disabled",
        ))
    }
}

fn normalize_comment(comment: Option<&str>) -> Result<Option<String>, FirewallError> {
    let Some(comment) = comment.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(None);
    };

    let valid = comment.chars().count() <= MAX_COMMENT_CHARS
        && comment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if !valid {
        return Err(FirewallError::new(
            FirewallErrorKind::Invalid,
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

fn parse_rules(json: &str) -> Result<Vec<FirewallRule>, FirewallError> {
    let value: Value = serde_json::from_str(json).map_err(|error| {
        FirewallError::new(
            FirewallErrorKind::Failed,
            format!("could not parse nft JSON: {error}"),
        )
    })?;

    let mut rules = Vec::new();
    let Some(items) = value.get("nftables").and_then(Value::as_array) else {
        return Ok(rules);
    };

    for item in items {
        let Some(rule) = item.get("rule") else {
            continue;
        };
        let Some(handle) = rule.get("handle").and_then(Value::as_u64) else {
            continue;
        };
        let chain = rule
            .get("chain")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let raw_comment = rule.get("comment").and_then(Value::as_str);
        let (action, protocol, port, comment) = parse_comment(raw_comment);

        rules.push(FirewallRule {
            handle,
            chain,
            action,
            protocol,
            port,
            comment,
        });
    }

    Ok(rules)
}

fn parse_comment(
    raw: Option<&str>,
) -> (
    String,
    Option<FirewallProtocol>,
    Option<u16>,
    Option<String>,
) {
    let Some(raw) = raw else {
        return ("custom".to_string(), None, None, None);
    };
    let Some(body) = raw.strip_prefix(COMMENT_PREFIX) else {
        return ("custom".to_string(), None, None, Some(raw.to_string()));
    };

    let (metadata, user_comment) = match body.split_once('|') {
        Some((metadata, comment)) => (metadata, Some(comment.to_string())),
        None => (body, None),
    };
    let parts: Vec<&str> = metadata.split(':').collect();
    if parts.len() != 3 {
        return ("custom".to_string(), None, None, Some(raw.to_string()));
    }

    let action = match parts[0] {
        "allow" | "deny" => parts[0].to_string(),
        _ => {
            return ("custom".to_string(), None, None, Some(raw.to_string()));
        }
    };
    let Ok(protocol) = parts[1].parse::<FirewallProtocol>() else {
        return ("custom".to_string(), None, None, Some(raw.to_string()));
    };
    let Ok(port) = parts[2].parse::<u16>() else {
        return ("custom".to_string(), None, None, Some(raw.to_string()));
    };
    if port == 0 {
        return ("custom".to_string(), None, None, Some(raw.to_string()));
    }

    (action, Some(protocol), Some(port), user_comment)
}

fn is_missing(stderr: &str) -> bool {
    let lowercase = stderr.to_lowercase();
    lowercase.contains("no such file")
        || lowercase.contains("does not exist")
        || lowercase.contains("not found")
        || lowercase.contains("could not process rule")
}

fn nft_error(output: &NftOutput, context: &str) -> FirewallError {
    let lowercase = output.stderr.to_lowercase();
    let kind = if lowercase.contains("operation not permitted")
        || lowercase.contains("permission denied")
        || lowercase.contains("you must be root")
    {
        FirewallErrorKind::PermissionDenied
    } else if lowercase.contains("command not found") {
        FirewallErrorKind::Unavailable
    } else if is_missing(&output.stderr) {
        FirewallErrorKind::NotFound
    } else if lowercase.contains("syntax error") || lowercase.contains("invalid") {
        FirewallErrorKind::Invalid
    } else {
        FirewallErrorKind::Failed
    };

    let detail = if output.stderr.is_empty() {
        context.to_string()
    } else {
        format!("{context}: {}", output.stderr)
    };
    FirewallError::new(kind, detail)
}

fn run_ok<R: NftRunner>(runner: &R, args: &[&str], context: &str) -> Result<(), FirewallError> {
    let output = runner.run(args)?;
    if output.ok {
        Ok(())
    } else {
        Err(nft_error(&output, context))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct FakeRunner {
        calls: RefCell<Vec<Vec<String>>>,
        responses: RefCell<VecDeque<Result<NftOutput, FirewallError>>>,
    }

    impl FakeRunner {
        fn new(responses: Vec<Result<NftOutput, FirewallError>>) -> Self {
            Self {
                calls: RefCell::new(Vec::new()),
                responses: RefCell::new(responses.into()),
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.borrow().clone()
        }
    }

    impl NftRunner for FakeRunner {
        fn run(&self, args: &[&str]) -> Result<NftOutput, FirewallError> {
            self.calls
                .borrow_mut()
                .push(args.iter().map(|arg| (*arg).to_string()).collect());
            self.responses.borrow_mut().pop_front().unwrap_or_else(|| {
                Err(FirewallError::new(
                    FirewallErrorKind::Failed,
                    "fake runner ran out of responses",
                ))
            })
        }
    }

    fn ok(stdout: &str) -> Result<NftOutput, FirewallError> {
        Ok(NftOutput {
            ok: true,
            stdout: stdout.to_string(),
            stderr: String::new(),
        })
    }

    fn fail(stderr: &str) -> Result<NftOutput, FirewallError> {
        Ok(NftOutput {
            ok: false,
            stdout: String::new(),
            stderr: stderr.to_string(),
        })
    }

    fn snapshot_json() -> String {
        serde_json::json!({
            "nftables": [
                {"metainfo": {}},
                {"table": {"family": "inet", "name": "anthracite"}},
                {"chain": {"family": "inet", "table": "anthracite", "name": "input"}},
                {
                    "rule": {
                        "family": "inet",
                        "table": "anthracite",
                        "chain": "input",
                        "handle": 4,
                        "comment": "anth:deny:udp:53|dns"
                    }
                },
                {
                    "rule": {
                        "family": "inet",
                        "table": "anthracite",
                        "chain": "input",
                        "handle": 3,
                        "comment": "anth:allow:tcp:8080"
                    }
                },
                {
                    "rule": {
                        "family": "inet",
                        "table": "anthracite",
                        "chain": "input",
                        "handle": 9,
                        "comment": "manual rule"
                    }
                }
            ]
        })
        .to_string()
    }

    #[test]
    fn status_parses_managed_and_custom_rules() {
        let runner = FakeRunner::new(vec![
            ok(&snapshot_json()),
            ok("table inet anthracite { chain input { ... } }"),
        ]);

        let snapshot = status_with(&runner, true).unwrap();

        assert!(snapshot.initialized);
        assert!(snapshot.control_enabled);
        assert_eq!(snapshot.rules.len(), 3);
        assert_eq!(snapshot.rules[0].handle, 3);
        assert_eq!(snapshot.rules[0].action, "allow");
        assert_eq!(snapshot.rules[0].protocol, Some(FirewallProtocol::Tcp));
        assert_eq!(snapshot.rules[0].port, Some(8080));
        assert_eq!(snapshot.rules[0].comment, None);
        assert_eq!(snapshot.rules[1].action, "deny");
        assert_eq!(snapshot.rules[1].protocol, Some(FirewallProtocol::Udp));
        assert_eq!(snapshot.rules[1].comment.as_deref(), Some("dns"));
        assert_eq!(snapshot.rules[2].action, "custom");
        assert_eq!(snapshot.rules[2].comment.as_deref(), Some("manual rule"));
        assert!(snapshot.ruleset.contains("table inet anthracite"));
    }

    #[test]
    fn a_missing_table_is_an_uninitialized_snapshot() {
        let runner = FakeRunner::new(vec![fail(
            "Error: No such file or directory; did you mean table 'filter'?",
        )]);

        let snapshot = status_with(&runner, false).unwrap();

        assert!(!snapshot.initialized);
        assert!(snapshot.rules.is_empty());
        assert_eq!(runner.calls().len(), 1);
    }

    #[test]
    fn denying_a_rule_initializes_the_table_then_creates_the_drop() {
        let runner = FakeRunner::new(vec![
            fail("No such file or directory"),
            ok(""),
            fail("No such file or directory"),
            ok(""),
            ok(""),
        ]);

        let message = add_rule_with(
            &runner,
            true,
            FirewallAction::Deny,
            8080,
            FirewallProtocol::Tcp,
            Some("web"),
        )
        .unwrap();

        assert_eq!(message, "denied tcp/8080");
        let calls = runner.calls();
        assert_eq!(calls[0], ["list", "table", "inet", "anthracite"]);
        assert_eq!(calls[1], ["add", "table", "inet", "anthracite"]);
        assert_eq!(calls[2], ["list", "chain", "inet", "anthracite", "input"]);
        assert_eq!(calls[4][0], "add");
        assert!(calls[4].contains(&"8080".to_string()));
        assert!(calls[4].contains(&"drop".to_string()));
        assert_eq!(
            calls[4].last().map(String::as_str),
            Some("\"anth:deny:tcp:8080|web\"")
        );
    }

    #[test]
    fn allowing_a_port_removes_the_matching_managed_drop() {
        let runner = FakeRunner::new(vec![
            ok(&snapshot_json()),
            ok("table inet anthracite { ... }"),
            ok(""),
        ]);

        let message = add_rule_with(
            &runner,
            true,
            FirewallAction::Allow,
            53,
            FirewallProtocol::Udp,
            Some("dns"),
        )
        .unwrap();

        assert_eq!(message, "allowed udp/53");
        assert_eq!(
            runner.calls().last().unwrap(),
            &[
                "delete",
                "rule",
                "inet",
                "anthracite",
                "input",
                "handle",
                "4"
            ]
            .iter()
            .map(|arg| (*arg).to_string())
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn allowing_an_unblocked_port_is_a_no_op() {
        let runner = FakeRunner::new(vec![
            ok("{\"nftables\":[]}"),
            ok("table inet anthracite { ... }"),
        ]);

        let message = add_rule_with(
            &runner,
            true,
            FirewallAction::Allow,
            8080,
            FirewallProtocol::Tcp,
            None,
        )
        .unwrap();

        assert!(message.contains("traffic is already allowed"));
        assert_eq!(runner.calls().len(), 2);
    }

    #[test]
    fn control_is_checked_before_validation_or_commands() {
        let runner = FakeRunner::default();
        let error = add_rule_with(
            &runner,
            false,
            FirewallAction::Deny,
            0,
            FirewallProtocol::Udp,
            None,
        )
        .unwrap_err();

        assert_eq!(error.kind, FirewallErrorKind::Disabled);
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn invalid_ports_and_comments_are_rejected() {
        let runner = FakeRunner::default();
        let port = add_rule_with(
            &runner,
            true,
            FirewallAction::Allow,
            0,
            FirewallProtocol::Tcp,
            None,
        )
        .unwrap_err();
        assert_eq!(port.kind, FirewallErrorKind::Invalid);

        let comment = add_rule_with(
            &runner,
            true,
            FirewallAction::Allow,
            22,
            FirewallProtocol::Tcp,
            Some("nightly build"),
        )
        .unwrap_err();
        assert_eq!(comment.kind, FirewallErrorKind::Invalid);
    }

    #[test]
    fn deleting_an_unknown_handle_is_not_found() {
        let runner = FakeRunner::new(vec![fail(
            "Error: Could not process rule: No such file or directory",
        )]);
        let error = delete_rule_with(&runner, true, 99).unwrap_err();
        assert_eq!(error.kind, FirewallErrorKind::NotFound);
    }

    #[test]
    fn flush_counts_the_rules_it_removes() {
        let runner = FakeRunner::new(vec![
            ok(&snapshot_json()),
            ok("table inet anthracite { ... }"),
            ok(""),
        ]);

        let message = flush_with(&runner, true).unwrap();

        assert_eq!(message, "flushed 3 firewall rules");
        assert_eq!(
            runner.calls().last().unwrap(),
            &["flush", "chain", "inet", "anthracite", "input"]
                .iter()
                .map(|arg| (*arg).to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn permission_errors_keep_their_kind() {
        let runner = FakeRunner::new(vec![fail("Operation not permitted (you must be root)")]);
        let error = status_with(&runner, true).unwrap_err();
        assert_eq!(error.kind, FirewallErrorKind::PermissionDenied);
    }

    #[test]
    fn missing_nft_is_unavailable() {
        struct MissingNft;
        impl NftRunner for MissingNft {
            fn run(&self, _args: &[&str]) -> Result<NftOutput, FirewallError> {
                Err(FirewallError::new(
                    FirewallErrorKind::Unavailable,
                    "nft is not installed or is not on PATH",
                ))
            }
        }

        let error = status_with(&MissingNft, true).unwrap_err();
        assert_eq!(error.kind, FirewallErrorKind::Unavailable);
    }

    #[test]
    fn sudo_prefix_is_added_before_nft() {
        let direct = nft_command(false, &["list", "table"]);
        assert_eq!(direct.get_program(), "nft");
        assert_eq!(direct.get_args().collect::<Vec<_>>(), vec!["list", "table"]);

        let elevated = nft_command(true, &["list", "table"]);
        assert_eq!(elevated.get_program(), "sudo");
        assert_eq!(
            elevated.get_args().collect::<Vec<_>>(),
            vec!["-n", "nft", "list", "table"]
        );
    }

    #[test]
    fn only_sudo_failures_fall_back_to_direct_nft() {
        assert!(should_fallback_from_sudo(&fail(
            "sudo: a password is required"
        )));
        assert!(should_fallback_from_sudo(&fail(
            "sudo: no tty present and no askpass program specified"
        )));
        assert!(should_fallback_from_sudo(&fail(
            "user is not in the sudoers file"
        )));
        assert!(should_fallback_from_sudo(&Err(FirewallError::new(
            FirewallErrorKind::Unavailable,
            "sudo is not installed or is not on PATH",
        ))));
        assert!(!should_fallback_from_sudo(&fail("syntax error")));
        assert!(!should_fallback_from_sudo(&ok("")));
    }
}
