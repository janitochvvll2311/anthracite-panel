//! Network interface monitoring and state changes.
//!
//! These types are the transport contract only. The daemon owns the sysfs and
//! `/proc` reads, the address resolution and the write policy.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum NetworkAddressFamily {
    Ipv4,
    Ipv6,
}

/// One address assigned to an interface, in CIDR form.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct NetworkAddress {
    pub family: NetworkAddressFamily,
    pub address: String,
    pub prefix_len: u8,
}

/// One network interface as seen in a snapshot.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct NetworkInterface {
    pub name: String,
    pub index: u32,
    /// Kernel operational state from `/sys/class/net/<name>/operstate`.
    pub state: String,
    /// Administratively up (`IFF_UP`), independent of the carrier state.
    pub up: bool,
    /// The interface has a carrier and its driver considers it running
    /// (`IFF_RUNNING`).
    pub running: bool,
    /// The interface is a loopback (`IFF_LOOPBACK`). The daemon refuses to
    /// bring one down.
    pub loopback: bool,
    pub mac: String,
    pub mtu: u32,
    pub speed_mbps: Option<u64>,
    pub duplex: Option<String>,
    pub addresses: Vec<NetworkAddress>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
}

/// One point-in-time view of every network interface on the host.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct NetworkSnapshot {
    pub interfaces: Vec<NetworkInterface>,
    /// Wall-clock time of the snapshot in milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
    /// Whether the daemon accepts up/down requests.
    pub control_enabled: bool,
    /// Number of interfaces found before the row cap.
    pub total: usize,
    /// Whether `interfaces` was cut at the daemon's row cap.
    pub truncated: bool,
    /// Human-readable notice when the list was shortened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkErrorKind {
    Disabled,
    Protected,
    NotFound,
    PermissionDenied,
    Invalid,
    Failed,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct NetworkError {
    pub kind: NetworkErrorKind,
    pub detail: String,
}

impl NetworkError {
    pub fn new(kind: NetworkErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for NetworkError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.detail)
    }
}

impl std::error::Error for NetworkError {}
