//! Network interface inventory and mutation values.
//!
//! The daemon reads the kernel view (sysfs, `/proc/net/dev` and `getifaddrs`)
//! and turns the validated mutations into `ip` argv. This module only carries
//! the transport shape; no policy or command line lives here.

use serde::{Deserialize, Serialize};

/// One address assigned to an interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkAddress {
    /// `inet` or `inet6`.
    pub family: String,
    pub address: String,
    /// Prefix length, not a dotted netmask.
    pub prefix: u8,
    /// `global`, `link` or `host`.
    pub scope: String,
}

/// One network interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkInterface {
    pub name: String,
    pub index: u32,
    /// `loopback`, `ether`, `wifi`, `bridge`, `bond`, `vlan`, `tun`, `veth`,
    /// `dummy` or `other`.
    pub kind: String,
    /// Hardware address, empty when the interface has none.
    pub mac: String,
    pub mtu: u32,
    /// `/sys/class/net/<name>/operstate`.
    pub state: String,
    /// `IFF_UP`, independent of the operational state.
    pub up: bool,
    /// `IFF_RUNNING`; a carrier is present and the driver considers it running.
    #[serde(default)]
    pub running: bool,
    pub loopback: bool,
    /// The interface has a `device` link in sysfs, so it is real hardware.
    /// Deleting it removes the card's link, not a virtual device.
    #[serde(default)]
    pub physical: bool,
    pub addresses: Vec<NetworkAddress>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub rx_errors: u64,
    pub tx_errors: u64,
    pub rx_dropped: u64,
    pub tx_dropped: u64,
    /// Link speed in Mbps when the driver reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speed_mbps: Option<u64>,
}

/// A default route the daemon can see.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkRoute {
    /// `inet` or `inet6`.
    pub family: String,
    pub gateway: String,
    pub interface: String,
}

/// One virtual interface to create.
///
/// Only the kinds the panel can build are accepted: `veth`, `bridge`, `bond`,
/// `vlan`, `dummy` and `tun`. Physical and wireless devices cannot be created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkCreate {
    pub name: String,
    /// `veth`, `bridge`, `bond`, `vlan`, `dummy` or `tun`.
    pub kind: String,
    /// Peer name, required for a veth.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer: Option<String>,
    /// Parent interface, required for a vlan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    /// VLAN identifier, 1-4094, required for a vlan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vlan_id: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtu: Option<u32>,
    /// Bring the interface up once it exists.
    #[serde(default)]
    pub up: bool,
}

/// A change to an existing interface.
///
/// Absent fields are left alone: sending only `mtu` never touches the
/// administrative state, so an edit cannot take a link down by omission.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct NetworkEdit {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtu: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up: Option<bool>,
}

/// One network inventory snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkSnapshot {
    pub interfaces: Vec<NetworkInterface>,
    /// Default routes, IPv4 and IPv6 when present.
    #[serde(default)]
    pub routes: Vec<NetworkRoute>,
    /// When the snapshot was taken, in milliseconds since the Unix epoch.
    pub timestamp_ms: u64,
    /// Whether `[network] control` allows up/down.
    pub control_enabled: bool,
    /// Whether `[network] manage` allows create/edit/rename/delete.
    pub manage_enabled: bool,
}
