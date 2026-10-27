//! Firewall status and rule values.
//!
//! The daemon owns the dedicated `inet anthracite` table and its `input`
//! chain. These types carry the snapshot and the requests; no nft command line
//! lives here.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// What the panel does with matching traffic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FirewallAction {
    Allow,
    Deny,
}

impl FirewallAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }

    /// The nft verdict for a managed rule.
    pub fn verdict(self) -> &'static str {
        match self {
            Self::Allow => "accept",
            Self::Deny => "drop",
        }
    }
}

impl FromStr for FirewallAction {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "allow" | "accept" => Ok(Self::Allow),
            "deny" | "drop" => Ok(Self::Deny),
            _ => Err(format!(
                "unknown firewall action `{value}`; expected allow or deny"
            )),
        }
    }
}

impl fmt::Display for FirewallAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// The transport protocol a managed rule matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FirewallProtocol {
    Tcp,
    Udp,
}

impl FirewallProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tcp => "tcp",
            Self::Udp => "udp",
        }
    }
}

impl FromStr for FirewallProtocol {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "tcp" => Ok(Self::Tcp),
            "udp" => Ok(Self::Udp),
            _ => Err(format!(
                "unknown firewall protocol `{value}`; expected tcp or udp"
            )),
        }
    }
}

impl fmt::Display for FirewallProtocol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One rule in the managed `input` chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirewallRule {
    pub handle: u64,
    pub chain: String,
    /// `allow`, `deny`, or `custom` for a rule the panel did not create.
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<FirewallProtocol>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// User comment for a managed rule, or the raw nft comment for a custom one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// One point-in-time view of the managed table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FirewallSnapshot {
    /// Whether `inet anthracite` and its `input` chain exist.
    pub initialized: bool,
    /// Whether `[firewall] control` accepts rule changes.
    pub control_enabled: bool,
    pub rules: Vec<FirewallRule>,
    /// Textual `nft list table inet anthracite` output for diagnostics.
    #[serde(default)]
    pub ruleset: String,
    /// When the snapshot was taken, in milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
}
