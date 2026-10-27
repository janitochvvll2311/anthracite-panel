//! Pure parsing of `nft -j list table inet anthracite`.
//!
//! The panel only manages the `input` chain of its own table. A rule in another
//! chain is ignored, and a comment the panel did not write becomes `custom`
//! without inventing a protocol, port or verdict.

use anth_wire::{FirewallProtocol, FirewallRule};
use serde_json::Value;

pub const COMMENT_PREFIX: &str = "anth:";

/// Parses every rule of the managed `input` chain, sorted by handle.
pub fn parse_rules(json: &str) -> Result<Vec<FirewallRule>, String> {
    let value: Value =
        serde_json::from_str(json).map_err(|error| format!("could not parse nft JSON: {error}"))?;

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
        if chain != "input" {
            continue;
        }
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

    rules.sort_by_key(|rule| rule.handle);
    Ok(rules)
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
                {"chain": {"family": "inet", "table": "anthracite", "name": "input"}},
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
                        "chain": "input",
                        "handle": 3,
                        "comment": "anth:allow:tcp:8080"
                    }
                },
                {
                    "rule": {
                        "family": "inet",
                        "table": "anthracite",
                        "chain": "output",
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
        let rules = parse_rules(&sample()).expect("parse");
        assert_eq!(rules.len(), 3);
        assert_eq!(rules[0].handle, 3);
        assert_eq!(rules[0].action, "allow");
        assert_eq!(rules[0].protocol, Some(FirewallProtocol::Tcp));
        assert_eq!(rules[0].port, Some(8080));
        assert_eq!(rules[0].comment, None);

        assert_eq!(rules[1].handle, 4);
        assert_eq!(rules[1].action, "deny");
        assert_eq!(rules[1].protocol, Some(FirewallProtocol::Udp));
        assert_eq!(rules[1].port, Some(53));
        assert_eq!(rules[1].comment.as_deref(), Some("dns"));

        assert_eq!(rules[2].handle, 9);
        assert_eq!(rules[2].action, "custom");
        assert_eq!(rules[2].protocol, None);
        assert_eq!(rules[2].port, None);
        assert_eq!(rules[2].comment.as_deref(), Some("manual rule"));
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
        assert!(parse_rules("{}").expect("empty object").is_empty());
        assert!(
            parse_rules(r#"{"nftables":[]}"#)
                .expect("empty list")
                .is_empty()
        );
        assert!(parse_rules("not json").is_err());
    }
}
