//! Pure parsing of `nft -j list table inet anthracite`.
//!
//! The panel only manages the `input` and `output` base chains of its own
//! table. A rule in another chain is ignored, and a comment the panel did not
//! write becomes `custom` without inventing a protocol, port or verdict.

use anth_wire::{
    FirewallAction, FirewallChain, FirewallChainState, FirewallProtocol, FirewallRule,
};
use serde_json::Value;

pub const COMMENT_PREFIX: &str = "anth:";

/// The parsed managed table: rules plus the presence and policy of each chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTable {
    pub rules: Vec<FirewallRule>,
    pub chains: Vec<FirewallChainState>,
}

/// The state shown for a table that does not exist yet.
pub fn default_chains() -> Vec<FirewallChainState> {
    FirewallChain::ALL
        .iter()
        .map(|chain| FirewallChainState {
            chain: *chain,
            initialized: false,
            policy: FirewallAction::Allow,
        })
        .collect()
}

/// Parses every rule and chain of the managed table.
///
/// Rules are sorted by chain then handle so the UI shows input before output
/// and keeps a stable order within each chain.
pub fn parse_table(json: &str) -> Result<ParsedTable, String> {
    let value: Value =
        serde_json::from_str(json).map_err(|error| format!("could not parse nft JSON: {error}"))?;

    let mut chains = default_chains();
    let mut rules = Vec::new();
    let Some(items) = value.get("nftables").and_then(Value::as_array) else {
        return Ok(ParsedTable { rules, chains });
    };

    for item in items {
        if let Some(chain) = item.get("chain") {
            let Some(name) = chain.get("name").and_then(Value::as_str) else {
                continue;
            };
            let Ok(chain_id) = name.parse::<FirewallChain>() else {
                continue;
            };
            let policy = chain
                .get("policy")
                .and_then(Value::as_str)
                .and_then(|policy| policy.parse::<FirewallAction>().ok())
                .unwrap_or(FirewallAction::Allow);
            if let Some(state) = chains.iter_mut().find(|state| state.chain == chain_id) {
                state.initialized = true;
                state.policy = policy;
            }
            continue;
        }

        let Some(rule) = item.get("rule") else {
            continue;
        };
        let Some(handle) = rule.get("handle").and_then(Value::as_u64) else {
            continue;
        };
        let Some(chain) = rule
            .get("chain")
            .and_then(Value::as_str)
            .and_then(|name| name.parse::<FirewallChain>().ok())
        else {
            continue;
        };
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

    rules.sort_by_key(|rule| (chain_rank(rule.chain), rule.handle));
    Ok(ParsedTable { rules, chains })
}

fn chain_rank(chain: FirewallChain) -> u8 {
    match chain {
        FirewallChain::Input => 0,
        FirewallChain::Output => 1,
    }
}

/// Splits `anth:<action>:<protocol>:<port>[|<comment>]`.
///
/// Anything that does not match exactly is a custom rule: the original comment
/// is preserved for the operator and no field is guessed.
pub fn parse_comment(
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
        _ => return ("custom".to_string(), None, None, Some(raw.to_string())),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> String {
        serde_json::json!({
            "nftables": [
                {"metainfo": {}},
                {"table": {"family": "inet", "name": "anthracite"}},
                {
                    "chain": {
                        "family": "inet",
                        "table": "anthracite",
                        "name": "input",
                        "type": "filter",
                        "hook": "input",
                        "policy": "drop"
                    }
                },
                {"chain": {"family": "inet", "table": "anthracite", "name": "output"}},
                {
                    "rule": {
                        "family": "inet",
                        "table": "anthracite",
                        "chain": "input",
                        "handle": 9,
                        "comment": "manual rule"
                    }
                },
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
                        "chain": "output",
                        "handle": 3,
                        "comment": "anth:allow:tcp:8080"
                    }
                },
                {
                    "rule": {
                        "family": "inet",
                        "table": "anthracite",
                        "chain": "forward",
                        "handle": 11,
                        "comment": "anth:deny:tcp:25|smtp"
                    }
                }
            ]
        })
        .to_string()
    }

    #[test]
    fn managed_custom_and_other_chains_are_separated() {
        let parsed = parse_table(&sample()).expect("parse");
        assert_eq!(parsed.rules.len(), 3);

        assert_eq!(parsed.rules[0].handle, 4);
        assert_eq!(parsed.rules[0].chain, FirewallChain::Input);
        assert_eq!(parsed.rules[0].action, "deny");
        assert_eq!(parsed.rules[0].protocol, Some(FirewallProtocol::Udp));
        assert_eq!(parsed.rules[0].port, Some(53));
        assert_eq!(parsed.rules[0].comment.as_deref(), Some("dns"));

        assert_eq!(parsed.rules[1].handle, 9);
        assert_eq!(parsed.rules[1].action, "custom");
        assert_eq!(parsed.rules[1].protocol, None);
        assert_eq!(parsed.rules[1].port, None);
        assert_eq!(parsed.rules[1].comment.as_deref(), Some("manual rule"));

        assert_eq!(parsed.rules[2].handle, 3);
        assert_eq!(parsed.rules[2].chain, FirewallChain::Output);
        assert_eq!(parsed.rules[2].action, "allow");
        assert_eq!(parsed.rules[2].protocol, Some(FirewallProtocol::Tcp));
        assert_eq!(parsed.rules[2].port, Some(8080));
    }

    #[test]
    fn chain_policies_are_parsed_and_missing_chains_default_to_accept() {
        let parsed = parse_table(&sample()).expect("parse");
        assert_eq!(parsed.chains.len(), 2);
        assert_eq!(parsed.chains[0].chain, FirewallChain::Input);
        assert!(parsed.chains[0].initialized);
        assert_eq!(parsed.chains[0].policy, FirewallAction::Deny);
        assert_eq!(parsed.chains[1].chain, FirewallChain::Output);
        assert!(parsed.chains[1].initialized);
        assert_eq!(parsed.chains[1].policy, FirewallAction::Allow);

        let empty = parse_table("{}").expect("empty");
        assert!(!empty.chains[0].initialized);
        assert_eq!(empty.chains[0].policy, FirewallAction::Allow);
    }

    #[test]
    fn malformed_managed_comments_stay_custom() {
        for raw in [
            Some("anth:deny:tcp"),
            Some("anth:drop:tcp:80"),
            Some("anth:deny:icmp:80"),
            Some("anth:deny:tcp:0"),
            Some("anth:deny:tcp:70000"),
            None,
        ] {
            let (action, protocol, port, comment) = parse_comment(raw);
            assert_eq!(action, "custom", "raw={raw:?}");
            assert_eq!(protocol, None);
            assert_eq!(port, None);
            assert_eq!(comment, raw.map(str::to_string));
        }
    }

    #[test]
    fn a_missing_or_empty_nftables_list_is_empty() {
        assert!(parse_table("{}").expect("empty object").rules.is_empty());
        assert!(
            parse_table(r#"{"nftables":[]}"#)
                .expect("empty list")
                .rules
                .is_empty()
        );
        assert!(parse_table("not json").is_err());
    }
}
