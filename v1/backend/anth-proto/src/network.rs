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
    /// Device class, derived from sysfs.
    #[serde(default = "default_kind")]
    pub kind: NetworkInterfaceKind,
    /// The interface has a `device` link in sysfs, so it is real hardware.
    #[serde(default)]
    pub physical: bool,
}

fn default_kind() -> NetworkInterfaceKind {
    NetworkInterfaceKind::Other
}

/// What kind of device an interface is.
///
/// The daemon derives this from `/sys/class/net/<name>`: a `device` link means
/// real hardware, `bridge/` means a bridge, and so on. The panel needs it to
/// tell an operator that deleting a physical interface will remove the link
/// they may be connected through.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum NetworkInterfaceKind {
    Physical,
    Loopback,
    Bridge,
    Bond,
    Vlan,
    Veth,
    Dummy,
    TunTap,
    Wireless,
    /// Something the daemon does not recognise, including virtual devices
    /// other tools created. Delete is still allowed but reported as unknown.
    #[default]
    Other,
}

/// Request to create one virtual interface.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct NetworkCreateArgs {
    pub name: String,
    pub kind: NetworkInterfaceKind,
    /// Required for a veth, where the peer is another interface name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer: Option<String>,
    /// Parent device for a vlan or a tun/tap, when the tool needs one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// VLAN identifier, 1-4094, for a vlan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vlan_id: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtu: Option<u32>,
    /// Bring the interface up once it exists.
    #[serde(default)]
    pub up: bool,
}

/// Request to change an existing interface.
///
/// Absent fields are left alone: sending only `mtu` never touches the
/// administrative state, so an edit cannot take a link down by omission.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct NetworkEditArgs {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtu: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up: Option<bool>,
}

/// One point-in-time view of every network interface on the host.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct NetworkSnapshot {
    pub interfaces: Vec<NetworkInterface>,
    /// Wall-clock time of the snapshot in milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
    /// Whether the daemon accepts up/down requests.
    pub control_enabled: bool,
    /// Whether the daemon accepts create, edit, rename and delete.
    #[serde(default)]
    pub manage_enabled: bool,
    /// Number of interfaces found before any truncation.
    pub total: usize,
    /// Whether `interfaces` was shortened to satisfy the snapshot budget.
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
