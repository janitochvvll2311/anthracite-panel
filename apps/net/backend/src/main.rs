//! Host network status for the Network app.
//!
//! This is the read-only half of `anthracite:net/net@1.1.0`: interfaces,
//! addresses, counters and the kernel routing tables. Applying configuration
//! (bring an interface up/down, add or remove a route) is the separate approved
//! flow, so those functions answer with a typed refusal instead of silently
//! changing the host.
//!
//! Everything lives in this native service: the engine and the sandboxed panel
//! view never read `/sys` or `/proc/net`.

use std::collections::BTreeMap;
use std::net::{Ipv4Addr, Ipv6Addr};

use anth_contract::Error;
use anth_service::Handler;
use serde_json::{json, Value};

const NET: &str = "anthracite:net/net@1.1.0";

struct Service;

impl Handler for Service {
    fn call(&self, interface: &str, function: &str, _input: Value) -> Result<Value, Error> {
        if interface != NET {
            return Err(Error::not_found(format!(
                "`{interface}` is not exported by the network service"
            )));
        }
        match function {
            "interfaces" => Ok(Value::Array(interfaces())),
            "routes" => Ok(Value::Array(routes())),
            "set-up" | "add-route" | "remove-route" => Err(Error::denied(format!(
                "`{function}` needs the approved network apply flow; this service is read-only"
            ))),
            other => Err(Error::not_found(format!(
                "`anthracite:net/net#{other}` is not implemented"
            ))),
        }
    }
}

#[tokio::main]
async fn main() {
    let socket = match anth_service::socket_argument() {
        Ok(socket) => socket,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    if let Err(error) = anth_service::serve(&socket, std::sync::Arc::new(Service)).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[derive(Debug, Clone)]
struct Address {
    family: &'static str,
    address: String,
    prefix: u8,
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

/// `/sys/class/net/<name>/flags` reports the kernel `IFF_UP` bit as `0x...`.
fn is_up(flags: &str) -> bool {
    u64::from_str_radix(flags.trim().trim_start_matches("0x"), 16)
        .map(|value| value & 0x1 != 0)
        .unwrap_or(false)
}

fn interfaces() -> Vec<Value> {
    let addresses = interface_addresses();
    let mut interfaces = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let base = format!("/sys/class/net/{name}");
        let flags = read(&format!("{base}/flags"));
        let mut list = addresses.get(&name).cloned().unwrap_or_default();
        list.sort_by(|left, right| {
            left.family
                .cmp(right.family)
                .then_with(|| left.address.cmp(&right.address))
        });
        interfaces.insert(
            name.clone(),
            json!({
                "name": name,
                "up": is_up(&flags),
                "mac": read(&format!("{base}/address")).trim(),
                "mtu": read(&format!("{base}/mtu")).trim().parse::<u32>().unwrap_or(0),
                "addresses": list.iter().map(|address| json!({
                    "family": address.family,
                    "address": address.address,
                    "prefix": address.prefix,
                })).collect::<Vec<_>>(),
                "rx-bytes": read(&format!("{base}/statistics/rx_bytes")).trim().parse::<u64>().unwrap_or(0),
                "tx-bytes": read(&format!("{base}/statistics/tx_bytes")).trim().parse::<u64>().unwrap_or(0),
            }),
        );
    }
    interfaces.into_values().collect()
}

fn interface_addresses() -> BTreeMap<String, Vec<Address>> {
    let mut addresses: BTreeMap<String, Vec<Address>> = BTreeMap::new();
    let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: `getifaddrs` writes a valid linked list into `head`; every pointer
    // is checked before it is dereferenced and the list is freed at the end.
    if unsafe { libc::getifaddrs(&mut head) } != 0 {
        return addresses;
    }
    let mut current = head;
    while !current.is_null() {
        // SAFETY: `current` came from `getifaddrs` and is non-null here.
        let entry = unsafe { &*current };
        if !entry.ifa_addr.is_null() && !entry.ifa_name.is_null() {
            // SAFETY: both pointers were checked; `ifa_name` is a NUL-terminated
            // C string and `ifa_addr` matches the family it reports.
            let family = unsafe { (*entry.ifa_addr).sa_family as i32 };
            // SAFETY: `ifa_name` is a NUL-terminated C string.
            let name = unsafe { std::ffi::CStr::from_ptr(entry.ifa_name) }
                .to_string_lossy()
                .into_owned();
            if family == libc::AF_INET {
                // SAFETY: the family is `AF_INET`, so the address is a
                // `sockaddr_in`.
                let address = unsafe { &*(entry.ifa_addr as *const libc::sockaddr_in) };
                let ip = Ipv4Addr::from(u32::from_be(address.sin_addr.s_addr));
                let prefix = unsafe { prefix_v4(entry.ifa_netmask as *const libc::sockaddr_in) };
                addresses.entry(name).or_default().push(Address {
                    family: "ipv4",
                    address: ip.to_string(),
                    prefix,
                });
            } else if family == libc::AF_INET6 {
                // SAFETY: the family is `AF_INET6`, so the address is a
                // `sockaddr_in6`.
                let address = unsafe { &*(entry.ifa_addr as *const libc::sockaddr_in6) };
                let ip = Ipv6Addr::from(address.sin6_addr.s6_addr);
                let prefix = unsafe { prefix_v6(entry.ifa_netmask as *const libc::sockaddr_in6) };
                addresses.entry(name).or_default().push(Address {
                    family: "ipv6",
                    address: ip.to_string(),
                    prefix,
                });
            }
        }
        current = entry.ifa_next;
    }
    // SAFETY: `head` is the list allocated by `getifaddrs` and is freed once.
    unsafe { libc::freeifaddrs(head) };
    addresses
}

/// Counts the bits in an IPv4 netmask. A null or absent mask is a `/0`.
///
/// # Safety
/// `mask` must be null or point to an initialized `sockaddr_in`.
unsafe fn prefix_v4(mask: *const libc::sockaddr_in) -> u8 {
    if mask.is_null() {
        return 0;
    }
    let mask = unsafe { &*mask };
    mask.sin_addr
        .s_addr
        .to_be()
        .count_ones()
        .try_into()
        .unwrap_or(0)
}

/// Counts the bits in an IPv6 netmask. A null or absent mask is a `/0`.
///
/// # Safety
/// `mask` must be null or point to an initialized `sockaddr_in6`.
unsafe fn prefix_v6(mask: *const libc::sockaddr_in6) -> u8 {
    if mask.is_null() {
        return 0;
    }
    let mask = unsafe { &*mask };
    mask.sin6_addr
        .s6_addr
        .iter()
        .map(|byte| byte.count_ones() as u8)
        .sum()
}

fn routes() -> Vec<Value> {
    let mut routes = parse_ipv4_routes(&read("/proc/net/route"));
    routes.extend(parse_ipv6_routes(&read("/proc/net/ipv6_route")));
    routes
}

fn parse_ipv4_routes(text: &str) -> Vec<Value> {
    let mut routes = Vec::new();
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 11 {
            continue;
        }
        let Some(destination) = ipv4_hex(fields[1]) else {
            continue;
        };
        let Some(mask) = ipv4_hex(fields[7]) else {
            continue;
        };
        let prefix = u32::from_be_bytes(mask.octets()).count_ones() as u8;
        let gateway = ipv4_hex(fields[2]).filter(|address| !address.is_unspecified());
        let metric = fields[6].parse::<u32>().unwrap_or(0);
        routes.push(json!({
            "family": "ipv4",
            "destination": format!("{destination}/{prefix}"),
            "gateway": gateway.map(|address| address.to_string()),
            "device": fields[0],
            "metric": metric,
        }));
    }
    routes
}

fn parse_ipv6_routes(text: &str) -> Vec<Value> {
    let mut routes = Vec::new();
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }
        let Some(destination) = ipv6_hex(fields[0]) else {
            continue;
        };
        let Ok(prefix) = u8::from_str_radix(fields[1], 16) else {
            continue;
        };
        if prefix > 128 {
            continue;
        }
        let gateway = ipv6_hex(fields[4]).filter(|address| !address.is_unspecified());
        let metric = u32::from_str_radix(fields[5], 16).unwrap_or(0);
        // The kernel lists its unreachable defaults with the maximum metric;
        // they are not routes an operator can use.
        if metric == u32::MAX {
            continue;
        }
        routes.push(json!({
            "family": "ipv6",
            "destination": format!("{destination}/{prefix}"),
            "gateway": gateway.map(|address| address.to_string()),
            "device": fields[9],
            "metric": metric,
        }));
    }
    routes
}

/// `/proc/net/route` stores IPv4 values as the little-endian hex of the
/// network-order address, so `to_le_bytes` reconstructs the readable form.
fn ipv4_hex(text: &str) -> Option<Ipv4Addr> {
    let value = u32::from_str_radix(text, 16).ok()?;
    Some(Ipv4Addr::from(value.to_le_bytes()))
}

fn ipv6_hex(text: &str) -> Option<Ipv6Addr> {
    if text.len() != 32 {
        return None;
    }
    Some(Ipv6Addr::from(u128::from_str_radix(text, 16).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_routes_are_decoded_from_proc() {
        let text = "\
Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
eth0\t0001A8C0\t00000000\t0001\t0\t0\t100\t00FFFFFF\t0\t0\t0
eth0\t00000000\t0101A8C0\t0003\t0\t0\t1024\t00000000\t0\t0\t0
";
        let routes = parse_ipv4_routes(text);
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0]["destination"], "192.168.1.0/24");
        assert_eq!(routes[0]["gateway"], Value::Null);
        assert_eq!(routes[0]["metric"], 100);
        assert_eq!(routes[1]["destination"], "0.0.0.0/0");
        assert_eq!(routes[1]["gateway"], "192.168.1.1");
        assert_eq!(routes[1]["metric"], 1024);
    }

    #[test]
    fn ipv6_routes_are_decoded_from_proc() {
        let text = "\
00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 00000400 00000000 00000000 00000000 eth0
fe800000000000000000000000000000 40 00000000000000000000000000000000 00 00000000000000000000000000000000 00000100 00000000 00000000 00000000 eth0
00000000000000000000000000000000 00 00000000000000000000000000000000 00 00000000000000000000000000000000 ffffffff 00000001 00000000 00200200 lo
";
        let routes = parse_ipv6_routes(text);
        assert_eq!(routes.len(), 2, "the unreachable default is skipped");
        assert_eq!(routes[0]["destination"], "::/0");
        assert_eq!(routes[0]["gateway"], Value::Null);
        assert_eq!(routes[0]["metric"], 1024);
        assert_eq!(routes[1]["destination"], "fe80::/64");
        assert_eq!(routes[1]["device"], "eth0");
    }

    #[test]
    fn the_flags_file_marks_up_interfaces() {
        assert!(is_up("0x1003"));
        assert!(is_up("0x9"));
        assert!(!is_up("0x1002"));
        assert!(!is_up(""));
    }

    #[test]
    fn the_host_interfaces_have_the_contract_shape() {
        if !std::path::Path::new("/sys/class/net/lo").exists() {
            return;
        }
        let list = interfaces();
        let loopback = list
            .iter()
            .find(|interface| interface["name"] == "lo")
            .expect("the host has a loopback interface");
        assert!(loopback["up"].is_boolean());
        assert!(loopback["mtu"].as_u64().unwrap_or(0) > 0);
        assert!(loopback["addresses"].is_array());
        assert!(loopback["rx-bytes"].is_number());
        assert!(loopback["tx-bytes"].is_number());
    }

    #[test]
    fn mutations_are_refused_until_the_apply_flow_exists() {
        let service = Service;
        for function in ["set-up", "add-route", "remove-route"] {
            let error = service
                .call(NET, function, json!({}))
                .expect_err("read-only");
            assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        }
    }
}
