//! Deterministic in-memory firewall fixture for the demo.
//!
//! It models the managed `inet anthracite` table and never executes `nft`.

use std::collections::BTreeMap;

use anth_contract::Error;
use serde_json::{json, Value};

use crate::text;

#[derive(Clone)]
struct Rule {
    handle: u64,
    chain: String,
    action: Option<String>,
    protocol: Option<String>,
    port: Option<u16>,
    comment: Option<String>,
}

pub struct FirewallFixture {
    rules: Vec<Rule>,
    policies: BTreeMap<String, String>,
    next_handle: u64,
}

impl FirewallFixture {
    pub fn new() -> Self {
        Self {
            rules: vec![
                Rule {
                    handle: 1,
                    chain: "input".to_string(),
                    action: Some("deny".to_string()),
                    protocol: Some("tcp".to_string()),
                    port: Some(23),
                    comment: Some("telnet".to_string()),
                },
                Rule {
                    handle: 2,
                    chain: "input".to_string(),
                    action: Some("allow".to_string()),
                    protocol: Some("tcp".to_string()),
                    port: Some(22),
                    comment: Some("ssh".to_string()),
                },
                Rule {
                    handle: 3,
                    chain: "output".to_string(),
                    action: Some("allow".to_string()),
                    protocol: Some("tcp".to_string()),
                    port: Some(443),
                    comment: Some("https".to_string()),
                },
                Rule {
                    handle: 4,
                    chain: "input".to_string(),
                    action: None,
                    protocol: None,
                    port: None,
                    comment: Some("manual rule".to_string()),
                },
            ],
            policies: BTreeMap::from([
                ("input".to_string(), "allow".to_string()),
                ("output".to_string(), "allow".to_string()),
            ]),
            next_handle: 5,
        }
    }

    pub fn handle(&mut self, function: &str, input: &Value) -> Result<Value, Error> {
        match function {
            "snapshot" => {
                let chains = ["input", "output"];
                Ok(json!({
                    "initialized": true,
                    "control": true,
                    "rules": self.rules.iter().map(|rule| json!({
                        "handle": rule.handle,
                        "chain": rule.chain,
                        "action": rule.action,
                        "protocol": rule.protocol,
                        "port": rule.port,
                        "comment": rule.comment,
                    })).collect::<Vec<_>>(),
                    "chains": chains.iter().map(|chain| json!({
                        "chain": chain,
                        "initialized": true,
                        "policy": self.policies.get(*chain).cloned().unwrap_or_else(|| "allow".to_string()),
                    })).collect::<Vec<_>>(),
                    "ruleset": "table inet anthracite { chain input { } chain output { } }",
                    "timestamp-ms": 1_760_000_000_000u64,
                }))
            }
            "add-rule" => {
                let chain = chain(input)?;
                let action = verdict(input, "action")?;
                let protocol = protocol(input)?;
                let port = port(input)?;
                let comment = comment(input)?;
                let existing: Vec<usize> = self
                    .rules
                    .iter()
                    .enumerate()
                    .filter(|(_, rule)| {
                        rule.chain == chain
                            && rule.action.is_some()
                            && rule.protocol.as_deref() == Some(protocol.as_str())
                            && rule.port == Some(port)
                    })
                    .map(|(index, _)| index)
                    .collect();
                if let Some(index) = existing.first().copied() {
                    let rule = &mut self.rules[index];
                    rule.action = Some(action);
                    rule.comment = comment;
                    for index in existing.into_iter().skip(1).rev() {
                        self.rules.remove(index);
                    }
                } else {
                    let handle = self.next_handle;
                    self.next_handle += 1;
                    self.rules.push(Rule {
                        handle,
                        chain,
                        action: Some(action),
                        protocol: Some(protocol),
                        port: Some(port),
                        comment,
                    });
                }
                Ok(Value::Null)
            }
            "update-rule" => {
                let handle = handle(input)?;
                let chain = chain(input)?;
                let action = verdict(input, "action")?;
                let protocol = protocol(input)?;
                let port = port(input)?;
                let comment = comment(input)?;
                let index = self
                    .rules
                    .iter()
                    .position(|rule| rule.handle == handle)
                    .ok_or_else(|| {
                        Error::not_found(format!("no firewall rule with handle {handle}"))
                    })?;
                if self.rules[index].action.is_none() {
                    return Err(Error::invalid("custom rules can be deleted but not edited"));
                }
                if self.rules.iter().any(|rule| {
                    rule.handle != handle
                        && rule.action.is_some()
                        && rule.chain == chain
                        && rule.protocol.as_deref() == Some(protocol.as_str())
                        && rule.port == Some(port)
                }) {
                    return Err(Error::conflict(format!(
                        "{protocol}/{port} is already managed on {chain}"
                    )));
                }
                let rule = &mut self.rules[index];
                rule.chain = chain;
                rule.action = Some(action);
                rule.protocol = Some(protocol);
                rule.port = Some(port);
                rule.comment = comment;
                Ok(Value::Null)
            }
            "delete-rule" => {
                let handle = handle(input)?;
                let index = self
                    .rules
                    .iter()
                    .position(|rule| rule.handle == handle)
                    .ok_or_else(|| {
                        Error::not_found(format!("no firewall rule with handle {handle}"))
                    })?;
                self.rules.remove(index);
                Ok(Value::Null)
            }
            "flush" => {
                let chain = chain(input)?;
                self.rules.retain(|rule| rule.chain != chain);
                Ok(Value::Null)
            }
            "set-policy" => {
                let chain = chain(input)?;
                let policy = verdict(input, "policy")?;
                self.policies.insert(chain, policy);
                Ok(Value::Null)
            }
            other => Err(Error::not_found(format!(
                "`anthracite:firewall/firewall#{other}` is not implemented"
            ))),
        }
    }
}

fn chain(input: &Value) -> Result<String, Error> {
    match text(input, "chain")?.as_str() {
        "input" | "output" => Ok(text(input, "chain")?),
        _ => Err(Error::invalid("`chain` is `input` or `output`")),
    }
}

fn verdict(input: &Value, key: &str) -> Result<String, Error> {
    match text(input, key)?.as_str() {
        "allow" | "deny" => Ok(text(input, key)?),
        _ => Err(Error::invalid(format!("`{key}` is `allow` or `deny`"))),
    }
}

fn protocol(input: &Value) -> Result<String, Error> {
    match text(input, "protocol")?.as_str() {
        "tcp" | "udp" => Ok(text(input, "protocol")?),
        _ => Err(Error::invalid("`protocol` is `tcp` or `udp`")),
    }
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
        .and_then(Value::as_u64)
        .filter(|value| *value > 0)
        .ok_or_else(|| Error::invalid("`handle` must be greater than zero"))
}

fn comment(input: &Value) -> Result<Option<String>, Error> {
    let Some(value) = input.get("comment").and_then(Value::as_str) else {
        return Ok(None);
    };
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    if value.chars().count() > 32
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
    {
        return Err(Error::invalid(
            "a firewall comment must be 1-32 ASCII letters, digits, `.`, `_` or `-`",
        ));
    }
    Ok(Some(value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(fixture: &mut FirewallFixture, function: &str, input: Value) -> Value {
        fixture
            .handle(function, &input)
            .expect("the fixture answers")
    }

    #[test]
    fn snapshot_has_chains_rules_and_custom_rules() {
        let mut fixture = FirewallFixture::new();
        let snapshot = call(&mut fixture, "snapshot", Value::Null);
        assert_eq!(snapshot["initialized"], true);
        assert_eq!(snapshot["control"], true);
        assert!(snapshot["rules"]
            .as_array()
            .expect("rules")
            .iter()
            .any(|rule| { rule["action"].is_null() && rule["comment"] == "manual rule" }));
    }

    #[test]
    fn rules_replace_update_delete_and_flush() {
        let mut fixture = FirewallFixture::new();
        call(
            &mut fixture,
            "add-rule",
            json!({"chain": "input", "action": "allow", "protocol": "tcp", "port": 23, "comment": "telnet"}),
        );
        let snapshot = call(&mut fixture, "snapshot", Value::Null);
        let rule = snapshot["rules"]
            .as_array()
            .expect("rules")
            .iter()
            .find(|rule| rule["protocol"] == "tcp" && rule["port"] == 23)
            .expect("rule");
        assert_eq!(rule["action"], "allow");
        assert_eq!(
            snapshot["rules"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|rule| rule["port"] == 23)
                .count(),
            1
        );

        call(
            &mut fixture,
            "update-rule",
            json!({"handle": rule["handle"], "chain": "output", "action": "deny", "protocol": "udp", "port": 53, "comment": null}),
        );
        let snapshot = call(&mut fixture, "snapshot", Value::Null);
        let rule = snapshot["rules"]
            .as_array()
            .expect("rules")
            .iter()
            .find(|rule| rule["chain"] == "output" && rule["port"] == 53)
            .expect("moved rule");
        assert_eq!(rule["action"], "deny");

        call(
            &mut fixture,
            "delete-rule",
            json!({"handle": rule["handle"]}),
        );
        call(&mut fixture, "flush", json!({"chain": "input"}));
        let snapshot = call(&mut fixture, "snapshot", Value::Null);
        assert!(!snapshot["rules"]
            .as_array()
            .expect("rules")
            .iter()
            .any(|rule| rule["chain"] == "input"));
    }

    #[test]
    fn policies_change_and_custom_rules_are_protected() {
        let mut fixture = FirewallFixture::new();
        call(
            &mut fixture,
            "set-policy",
            json!({"chain": "input", "policy": "deny"}),
        );
        let snapshot = call(&mut fixture, "snapshot", Value::Null);
        assert!(snapshot["chains"]
            .as_array()
            .expect("chains")
            .iter()
            .any(|chain| chain["chain"] == "input" && chain["policy"] == "deny"));
        let error = fixture
            .handle(
                "update-rule",
                &json!({"handle": 4, "chain": "input", "action": "allow", "protocol": "tcp", "port": 22, "comment": null}),
            )
            .expect_err("custom");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
    }
}
