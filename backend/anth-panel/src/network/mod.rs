//! The network inventory: interfaces, addresses, counters and default routes.
//!
//! The kernel exposes the identity in sysfs, the counters in `/proc/net/dev`,
//! the routes in `/proc/net/route` and `/proc/net/ipv6_route`, and the addresses
//! through `getifaddrs`. The scan runs on the blocking pool and is read-only;
//! mutations are a separate slice gated by `[network] control` and
//! `[network] manage`.

mod parse;

use std::collections::HashMap;
use std::ffi::CStr;
use std::fs;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_wire::{NetworkAddress, NetworkInterface, NetworkSnapshot, WireError};

use parse::{default_route_v4, default_route_v6, parse_dev, parse_flags};

/// The network service, built once from the policy the daemon read at startup.
#[derive(Debug, Clone)]
pub struct NetworkService {
    control: bool,
    manage: bool,
}

impl NetworkService {
    pub fn new(control: bool, manage: bool) -> Self {
        Self { control, manage }
    }

    /// One snapshot of every interface and the default routes.
    pub fn snapshot(&self) -> Result<NetworkSnapshot, WireError> {
        let stats = read_text("/proc/net/dev")
            .map(|text| parse_dev(&text))
            .unwrap_or_default();
        let mut routes = Vec::new();
        if let Some(route) = read_text("/proc/net/route").and_then(|text| default_route_v4(&text)) {
            routes.push(route);
        }
        if let Some(route) =
            read_text("/proc/net/ipv6_route").and_then(|text| default_route_v6(&text))
        {
            routes.push(route);
        }

        let mut addresses = read_addresses()?;
        let directory = fs::read_dir("/sys/class/net")
            .map_err(|error| WireError::internal(format!("reading /sys/class/net: {error}")))?;
        let mut interfaces = Vec::new();
        for entry in directory.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let flags = read_text(path.join("flags"))
                .and_then(|text| parse_flags(&text))
                .unwrap_or(0);
            let up = flags & libc::IFF_UP as u32 != 0;
            let loopback = flags & libc::IFF_LOOPBACK as u32 != 0;
            let state = read_text(path.join("operstate"))
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| if up { "up" } else { "down" }.to_string());
            let mac = read_text(path.join("address"))
                .map(|text| text.trim().to_string())
                .filter(|text| {
                    !loopback
                        && !text.is_empty()
                        && text.chars().any(|digit| digit != '0' && digit != ':')
                })
                .unwrap_or_default();
            let counters = stats.get(&name).copied().unwrap_or_default();
            interfaces.push(NetworkInterface {
                index: read_number(path.join("ifindex")).unwrap_or(0) as u32,
                mtu: read_number(path.join("mtu")).unwrap_or(0) as u32,
                speed_mbps: read_number(path.join("speed")).filter(|speed| *speed > 0),
                kind: kind(&name, &path, flags),
                name: name.clone(),
                mac,
                state,
                up,
                loopback,
                addresses: addresses.remove(&name).unwrap_or_default(),
                rx_bytes: counters.rx_bytes,
                tx_bytes: counters.tx_bytes,
                rx_packets: counters.rx_packets,
                tx_packets: counters.tx_packets,
                rx_errors: counters.rx_errors,
                tx_errors: counters.tx_errors,
                rx_dropped: counters.rx_dropped,
                tx_dropped: counters.tx_dropped,
            });
        }
        interfaces.sort_by(|left, right| {
            left.index
                .cmp(&right.index)
                .then_with(|| left.name.cmp(&right.name))
        });

        Ok(NetworkSnapshot {
            interfaces,
            routes,
            timestamp_ms: now_ms(),
            control_enabled: self.control,
            manage_enabled: self.manage,
        })
    }
}

/// Classifies an interface from its sysfs entries rather than its name alone.
fn kind(name: &str, path: &Path, flags: u32) -> String {
    if flags & libc::IFF_LOOPBACK as u32 != 0 {
        return "loopback".to_string();
    }
    if path.join("wireless").exists() || path.join("phy80211").exists() {
        return "wifi".to_string();
    }
    if path.join("bridge").exists() {
        return "bridge".to_string();
    }
    if name.starts_with("veth") {
        return "veth".to_string();
    }
    if name.starts_with("tun") || name.starts_with("tap") {
        return "tun".to_string();
    }
    match read_number(path.join("type")).unwrap_or(0) {
        1 => "ether".to_string(),
        _ => "other".to_string(),
    }
}

fn read_text(path: impl AsRef<Path>) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn read_number(path: impl AsRef<Path>) -> Option<u64> {
    read_text(path)?.trim().parse::<u64>().ok()
}

/// Collects `getifaddrs` once and groups the addresses by interface.
fn read_addresses() -> Result<HashMap<String, Vec<NetworkAddress>>, WireError> {
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: `getifaddrs` writes the head of a list the C library owns; the
    // matching `freeifaddrs` releases it after the loop below.
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return Err(WireError::internal(format!(
            "getifaddrs: {}",
            std::io::Error::last_os_error()
        )));
    }

    let mut grouped: HashMap<String, Vec<NetworkAddress>> = HashMap::new();
    let mut cursor = head;
    while !cursor.is_null() {
        // SAFETY: `cursor` points at a live `ifaddrs` node until `freeifaddrs`.
        let entry = unsafe { &*cursor };
        if !entry.ifa_addr.is_null() {
            // SAFETY: `ifa_name` is a NUL-terminated string owned by the node.
            let name = unsafe { CStr::from_ptr(entry.ifa_name) }
                .to_string_lossy()
                .into_owned();
            // SAFETY: `ifa_addr` is non-null and its family selects the cast.
            let address = match unsafe { (*entry.ifa_addr).sa_family } as i32 {
                libc::AF_INET => ipv4(entry.ifa_addr, entry.ifa_netmask),
                libc::AF_INET6 => ipv6(entry.ifa_addr, entry.ifa_netmask),
                _ => None,
            };
            if let Some(address) = address {
                grouped.entry(name).or_default().push(address);
            }
        }
        cursor = entry.ifa_next;
    }
    // SAFETY: the list came from `getifaddrs` and is not used afterwards.
    unsafe { libc::freeifaddrs(head) };

    for addresses in grouped.values_mut() {
        addresses.sort_by(|left, right| {
            left.family
                .cmp(&right.family)
                .then_with(|| left.address.cmp(&right.address))
        });
        addresses.dedup();
    }
    Ok(grouped)
}

fn ipv4(address: *const libc::sockaddr, mask: *const libc::sockaddr) -> Option<NetworkAddress> {
    // SAFETY: the caller checked the family, so the cast is the matching type.
    let address = unsafe { &*(address as *const libc::sockaddr_in) }
        .sin_addr
        .s_addr;
    let address = Ipv4Addr::from(u32::from_be(address));
    let prefix = if mask.is_null() {
        32
    } else {
        // SAFETY: the caller checked the address family; the netmask shares it.
        let mask = unsafe { &*(mask as *const libc::sockaddr_in) }
            .sin_addr
            .s_addr;
        u32::from_be(mask).count_ones() as u8
    };
    Some(NetworkAddress {
        family: "inet".to_string(),
        address: address.to_string(),
        prefix,
        scope: scope_v4(&address),
    })
}

fn ipv6(address: *const libc::sockaddr, mask: *const libc::sockaddr) -> Option<NetworkAddress> {
    // SAFETY: the caller checked the family, so the cast is the matching type.
    let address = unsafe { &*(address as *const libc::sockaddr_in6) }
        .sin6_addr
        .s6_addr;
    let address = Ipv6Addr::from(address);
    let prefix = if mask.is_null() {
        128
    } else {
        // SAFETY: the caller checked the address family; the netmask shares it.
        let mask = unsafe { &*(mask as *const libc::sockaddr_in6) }
            .sin6_addr
            .s6_addr;
        mask.iter().map(|byte| byte.count_ones()).sum::<u32>() as u8
    };
    Some(NetworkAddress {
        family: "inet6".to_string(),
        address: address.to_string(),
        prefix,
        scope: scope_v6(&address),
    })
}

fn scope_v4(address: &Ipv4Addr) -> String {
    if address.is_loopback() {
        "host".to_string()
    } else if address.is_link_local() {
        "link".to_string()
    } else {
        "global".to_string()
    }
}

fn scope_v6(address: &Ipv6Addr) -> String {
    if address.is_loopback() {
        "host".to_string()
    } else if (address.segments()[0] & 0xffc0) == 0xfe80 {
        "link".to_string()
    } else {
        "global".to_string()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scopes_follow_the_address_ranges() {
        assert_eq!(scope_v4(&Ipv4Addr::LOCALHOST), "host");
        assert_eq!(scope_v4(&Ipv4Addr::new(169, 254, 1, 1)), "link");
        assert_eq!(scope_v4(&Ipv4Addr::new(192, 168, 1, 1)), "global");
        assert_eq!(scope_v6(&Ipv6Addr::LOCALHOST), "host");
        assert_eq!(scope_v6(&"fe80::1".parse().unwrap()), "link");
        assert_eq!(scope_v6(&"2001:db8::1".parse().unwrap()), "global");
    }
}
