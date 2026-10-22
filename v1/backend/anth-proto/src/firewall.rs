//! Firewall status and rule requests.
//!
//! These types are the transport contract only. The daemon owns the nftables
//! table, the command runner and the write policy.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
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

/// One rule in the managed `inet anthracite input` chain.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct FirewallRule {
    pub handle: u64,
    pub chain: String,
    /// `allow`, `deny`, or `custom` for rules not created by this panel.
    pub action: String,
    pub protocol: Option<FirewallProtocol>,
    pub port: Option<u16>,
    /// User comment encoded in the rule, when present.
    pub comment: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct FirewallSnapshot {
    /// Whether the managed table already exists.
    pub initialized: bool,
    /// Whether the daemon accepts rule changes.
    pub control_enabled: bool,
    pub rules: Vec<FirewallRule>,
    /// Textual `nft list table inet anthracite` output for diagnostics. It is
    /// dropped first when the snapshot must be shortened.
    pub ruleset: String,
    /// Number of managed rules found before any truncation.
    pub total: usize,
    /// Whether `rules` or `ruleset` was shortened to fit the protocol line cap.
    pub truncated: bool,
    /// Human-readable notice when the snapshot was shortened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FirewallErrorKind {
    Disabled,
    Unavailable,
    PermissionDenied,
    Invalid,
    NotFound,
    Failed,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FirewallError {
    pub kind: FirewallErrorKind,
    pub detail: String,
}

impl FirewallError {
    pub fn new(kind: FirewallErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for FirewallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for FirewallError {}
