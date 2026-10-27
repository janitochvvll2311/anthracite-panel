//! Network interface inventory values.
//!
//! The daemon reads the kernel view (sysfs, `/proc/net/dev` and `getifaddrs`);
//! this module carries it without any policy. Mutations are a later slice.

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
    /// `loopback`, `ether`, `wifi`, `bridge`, `tun`, `veth` or `other`.
    pub kind: String,
    /// Hardware address, empty when the interface has none.
    pub mac: String,
    pub mtu: u32,
    /// `/sys/class/net/<name>/operstate`.
    pub state: String,
    /// `IFF_UP`, independent of the operational state.
    pub up: bool,
    pub loopback: bool,
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
