//! Model, `nft` runner and pure parsing for the managed table.

use std::fmt;
use std::process::Command;
use std::str::FromStr;

use serde_json::Value;

pub const FAMILY: &str = "inet";
pub const TABLE: &str = "anthracite";
pub const COMMENT_PREFIX: &str = "anth:";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chain {
    Input,
    Output,
}

impl Chain {
    pub const ALL: [Self; 2] = [Self::Input, Self::Output];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Output => "output",
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Self::Input => 0,
            Self::Output => 1,
        }
    }
}

impl FromStr for Chain {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "input" => Ok(Self::Input),
            "output" => Ok(Self::Output),
            _ => Err(format!(
                "unknown firewall chain `{value}`; expected input or output"
            )),
        }
    }
}

impl fmt::Display for Chain {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Deny,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }

    pub fn nft(self) -> &'static str {
        match self {
            Self::Allow => "accept",
            Self::Deny => "drop",
        }
    }

    pub fn parse_nft(value: &str) -> Option<Self> {
        match value {
            "accept" => Some(Self::Allow),
            "drop" => Some(Self::Deny),
            _ => None,
        }
    }
}

impl FromStr for Verdict {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "allow" | "accept" => Ok(Self::Allow),
            "deny" | "drop" => Ok(Self::Deny),
            _ => Err(format!(
                "unknown firewall action `{value}`; expected allow or deny"
            )),
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Tcp,
    Udp,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

impl FromStr for Protocol {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "tcp" => Ok(Self::Tcp),
            "udp" => Ok(Self::Udp),
            _ => Err(format!(
                "unknown firewall protocol `{value}`; expected tcp or udp"
            )),
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    pub handle: u64,
    pub chain: Chain,
    /// `None` for a rule the panel cannot parse and therefore cannot edit.
    pub action: Option<Verdict>,
    pub protocol: Option<Protocol>,
    pub port: Option<u16>,
    pub comment: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainState {
    pub chain: Chain,
    pub initialized: bool,
    pub policy: Verdict,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedTable {
    pub initialized: bool,
    pub rules: Vec<Rule>,
    pub chains: Vec<ChainState>,
}

pub fn default_chains() -> Vec<ChainState> {
    Chain::ALL
        .iter()
        .map(|chain| ChainState {
            chain: *chain,
            initialized: false,
            policy: Verdict::Allow,
        })
        .collect()
}

pub fn parse_table(json: &str) -> Result<ParsedTable, String> {
    let value: Value =
        serde_json::from_str(json).map_err(|error| format!("could not parse nft JSON: {error}"))?;
    let mut initialized = false;
    let mut chains = default_chains();
    let mut rules = Vec::new();
    let Some(items) = value.get("nftables").and_then(Value::as_array) else {
        return Ok(ParsedTable {
            initialized,
            rules,
            chains,
        });
    };

    for item in items {
        if let Some(table) = item.get("table") {
            if table.get("family").and_then(Value::as_str) == Some(FAMILY)
                && table.get("name").and_then(Value::as_str) == Some(TABLE)
            {
                initialized = true;
            }
            continue;
        }
        if let Some(chain) = item.get("chain") {
            let Some(name) = chain.get("name").and_then(Value::as_str) else {
                continue;
            };
            let Ok(id) = name.parse::<Chain>() else {
                continue;
            };
            let policy = chain
                .get("policy")
                .and_then(Value::as_str)
                .and_then(Verdict::parse_nft)
                .unwrap_or(Verdict::Allow);
            if let Some(state) = chains.iter_mut().find(|state| state.chain == id) {
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
            .and_then(|name| name.parse::<Chain>().ok())
        else {
            continue;
        };
        let (action, protocol, port, comment) =
            parse_comment(rule.get("comment").and_then(Value::as_str));
        rules.push(Rule {
            handle,
            chain,
            action,
            protocol,
            port,
            comment,
        });
    }

    rules.sort_by_key(|rule| (rule.chain.rank(), rule.handle));
    Ok(ParsedTable {
        initialized,
        rules,
        chains,
    })
}

pub fn parse_comment(
    raw: Option<&str>,
) -> (
    Option<Verdict>,
    Option<Protocol>,
    Option<u16>,
    Option<String>,
) {
    let Some(raw) = raw else {
        return (None, None, None, None);
    };
    let Some(body) = raw.strip_prefix(COMMENT_PREFIX) else {
        return (None, None, None, Some(raw.to_string()));
    };
    let (metadata, comment) = match body.split_once('|') {
        Some((metadata, comment)) => (metadata, Some(comment.to_string())),
        None => (body, None),
    };
    let parts: Vec<&str> = metadata.split(':').collect();
    if parts.len() != 3 {
        return (None, None, None, Some(raw.to_string()));
    }
    let action = match parts[0] {
        "allow" => Verdict::Allow,
        "deny" => Verdict::Deny,
        _ => return (None, None, None, Some(raw.to_string())),
    };
    let Ok(protocol) = parts[1].parse::<Protocol>() else {
        return (None, None, None, Some(raw.to_string()));
    };
    let Ok(port) = parts[2].parse::<u16>() else {
        return (None, None, None, Some(raw.to_string()));
    };
    if port == 0 {
        return (None, None, None, Some(raw.to_string()));
    }
    (Some(action), Some(protocol), Some(port), comment)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NftOutput {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

pub trait NftRunner: Send + Sync {
    fn run(&self, args: &[String]) -> Result<NftOutput, String>;
}

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
        assert!(parsed.initialized);
        assert_eq!(parsed.rules.len(), 3);
        assert_eq!(parsed.rules[0].handle, 4);
        assert_eq!(parsed.rules[0].action, Some(Verdict::Deny));
        assert_eq!(parsed.rules[0].protocol, Some(Protocol::Udp));
        assert_eq!(parsed.rules[0].port, Some(53));
        assert_eq!(parsed.rules[0].comment.as_deref(), Some("dns"));
        assert_eq!(parsed.rules[1].handle, 9);
        assert_eq!(parsed.rules[1].action, None);
        assert_eq!(parsed.rules[2].handle, 3);
        assert_eq!(parsed.rules[2].chain, Chain::Output);
    }

    #[test]
    fn policies_and_missing_chains_are_reported() {
        let parsed = parse_table(&sample()).expect("parse");
        assert_eq!(parsed.chains[0].chain, Chain::Input);
        assert!(parsed.chains[0].initialized);
        assert_eq!(parsed.chains[0].policy, Verdict::Deny);
        assert_eq!(parsed.chains[1].policy, Verdict::Allow);
        let empty = parse_table("{}").expect("empty");
        assert!(!empty.initialized);
        assert!(!empty.chains[0].initialized);
    }

    #[test]
    fn malformed_comments_stay_custom() {
        for raw in [
            Some("anth:deny:tcp"),
            Some("anth:drop:tcp:80"),
            Some("anth:deny:icmp:80"),
            Some("anth:deny:tcp:0"),
            None,
        ] {
            let (action, protocol, port, _) = parse_comment(raw);
            assert!(action.is_none(), "{raw:?}");
            assert!(protocol.is_none());
            assert!(port.is_none());
        }
    }
}
