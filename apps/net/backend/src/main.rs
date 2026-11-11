//! Host network service for the Network app.
//!
//! The read half reports interfaces, addresses, counters and the kernel routing
//! tables; `watch` streams the same snapshot as SSE. Applying configuration
//! (bring an interface up/down, add or remove a route) runs through `ip` after
//! strict validation.
//!
//! Everything lives in this native service: the engine and the sandboxed panel
//! view never read `/sys` or `/proc/net`.

use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::process::Command;
use std::time::Duration;

use anth_contract::Error;
use anth_service::{ChunkStream, Handler};
use futures_util::stream::unfold;
use serde_json::{json, Value};

const NET: &str = "anthracite:net/net@1.2.0";

struct Service;

impl Handler for Service {
    fn call(&self, interface: &str, function: &str, input: Value) -> Result<Value, Error> {
        if interface != NET {
            return Err(Error::not_found(format!(
                "`{interface}` is not exported by the network service"
            )));
        }
        match function {
            "interfaces" => Ok(Value::Array(interfaces())),
            "routes" => Ok(Value::Array(routes())),
            "set-up" => set_up(&input),
            "add-route" => add_route(&input),
            "remove-route" => remove_route(&input),
            other => Err(Error::not_found(format!(
                "`anthracite:net/net#{other}` is not implemented"
            ))),
        }
    }

    fn stream(&self, interface: &str, function: &str, input: Value) -> Result<ChunkStream, Error> {
        if interface != NET {
            return Err(Error::not_found(format!(
                "`{interface}` is not exported by the network service"
            )));
        }
        match function {
            "watch" => watch(&input),
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

/// One snapshot every `interval-ms` (clamped to 1–60 s), encoded as the JSON
/// chunks the engine frames as SSE.
fn watch(input: &Value) -> Result<ChunkStream, Error> {
    let milliseconds = match input.get("interval-ms") {
        Some(Value::Number(value)) => value.as_u64(),
        Some(Value::String(value)) => value.parse::<u64>().ok(),
        _ => None,
    }
    .unwrap_or(2_000)
    .clamp(1_000, 60_000);
    let interval = Duration::from_millis(milliseconds);
    Ok(Box::pin(unfold(
        (true, interval),
        |(first, interval)| async move {
            if !first {
                tokio::time::sleep(interval).await;
            }
            let snapshot = json!({
                "interfaces": interfaces(),
                "routes": routes(),
            });
            let result = serde_json::to_vec(&snapshot).map_err(|error| {
                Error::internal(format!("encoding the network snapshot: {error}"))
            });
            Some((result, (false, interval)))
        },
    )))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    V4,
    V6,
}
impl Family {
    fn matches(self, address: IpAddr) -> bool {
        matches!(
            (self, address),
            (Self::V4, IpAddr::V4(_)) | (Self::V6, IpAddr::V6(_))
        )
    }

    fn max_prefix(self) -> u8 {
        match self {
            Self::V4 => 32,
            Self::V6 => 128,
        }
    }
}

fn set_up(input: &Value) -> Result<Value, Error> {
    let name = text(input, "name")?;
    let up = boolean(input, "up")?;
    ensure_interface(&name)?;
    run_ip(&link_args(&name, up))?;
    Ok(Value::Null)
}

fn add_route(input: &Value) -> Result<Value, Error> {
    let family = family(text(input, "family")?.as_str())?;
    let (destination, _) = parse_destination(family, text(input, "destination")?.trim())?;
    let device = text(input, "device")?;
    ensure_interface(&device)?;
    let gateway = match optional_text(input, "gateway") {
        Some(value) if !value.trim().is_empty() => Some(parse_gateway(family, value.trim())?),
        _ => None,
    };
    let metric = number(input, "metric")
        .unwrap_or(0)
        .min(u64::from(u32::MAX)) as u32;
    run_ip(&route_args(
        family,
        true,
        &destination,
        gateway.as_deref(),
        &device,
        metric,
    ))?;
    Ok(Value::Null)
}

fn remove_route(input: &Value) -> Result<Value, Error> {
    let destination = text(input, "destination")?;
    let (family, destination) = parse_any_destination(destination.trim())?;
    let device = text(input, "device")?;
    ensure_interface(&device)?;
    run_ip(&route_args(family, false, &destination, None, &device, 0))?;
    Ok(Value::Null)
}

fn family(name: &str) -> Result<Family, Error> {
    match name {
        "ipv4" => Ok(Family::V4),
        "ipv6" => Ok(Family::V6),
        _ => Err(Error::invalid("`family` is `ipv4` or `ipv6`")),
    }
}

/// Only an interface the kernel reports can be touched. This also rejects path
/// tricks such as `..`, because the name is compared with the `/sys/class/net`
/// entries instead of being joined to a path unchecked.
fn ensure_interface(name: &str) -> Result<(), Error> {
    if name.is_empty() || name.contains('/') || name.contains('\0') {
        return Err(Error::invalid("`name` is not an interface name"));
    }
    let known = std::fs::read_dir("/sys/class/net")
        .map(|entries| {
            entries
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy() == name)
        })
        .unwrap_or(false);
    if known {
        Ok(())
    } else {
        Err(Error::not_found(format!(
            "interface `{name}` does not exist"
        )))
    }
}

fn parse_destination(family: Family, destination: &str) -> Result<(String, IpAddr), Error> {
    if destination.is_empty() {
        return Err(Error::invalid("`destination` cannot be empty"));
    }
    let (address, prefix) = match destination.split_once('/') {
        Some((address, prefix)) => {
            let prefix = prefix
                .parse::<u8>()
                .map_err(|_| Error::invalid("`destination` has an invalid prefix"))?;
            (address, prefix)
        }
        None => (destination, family.max_prefix()),
    };
    let address: IpAddr = address
        .parse()
        .map_err(|_| Error::invalid("`destination` is not an IP address"))?;
    if !family.matches(address) {
        return Err(Error::invalid("`destination` does not match `family`"));
    }
    if prefix > family.max_prefix() {
        return Err(Error::invalid("`destination` prefix is out of range"));
    }
    Ok((format!("{address}/{prefix}"), address))
}

fn parse_any_destination(destination: &str) -> Result<(Family, String), Error> {
    let address = destination
        .split_once('/')
        .map(|(address, _)| address)
        .unwrap_or(destination);
    let address: IpAddr = address
        .parse()
        .map_err(|_| Error::invalid("`destination` is not an IP address"))?;
    let family = match address {
        IpAddr::V4(_) => Family::V4,
        IpAddr::V6(_) => Family::V6,
    };
    let (destination, _) = parse_destination(family, destination)?;
    Ok((family, destination))
}

fn parse_gateway(family: Family, gateway: &str) -> Result<String, Error> {
    let address: IpAddr = gateway
        .parse()
        .map_err(|_| Error::invalid("`gateway` is not an IP address"))?;
    if !family.matches(address) {
        return Err(Error::invalid("`gateway` does not match `family`"));
    }
    Ok(address.to_string())
}

fn route_args(
    family: Family,
    add: bool,
    destination: &str,
    gateway: Option<&str>,
    device: &str,
    metric: u32,
) -> Vec<String> {
    let mut args = Vec::new();
    if family == Family::V6 {
        args.push("-6".to_string());
    }
    args.push("route".to_string());
    args.push(if add { "add" } else { "del" }.to_string());
    args.push(destination.to_string());
    if add {
        if let Some(gateway) = gateway {
            args.push("via".to_string());
            args.push(gateway.to_string());
        }
        args.push("dev".to_string());
        args.push(device.to_string());
        args.push("metric".to_string());
        args.push(metric.to_string());
    } else {
        args.push("dev".to_string());
        args.push(device.to_string());
    }
    args
}

fn link_args(name: &str, up: bool) -> Vec<String> {
    vec![
        "link".to_string(),
        "set".to_string(),
        "dev".to_string(),
        name.to_string(),
        if up { "up" } else { "down" }.to_string(),
    ]
}

fn run_ip(args: &[String]) -> Result<(), Error> {
    let output = Command::new("ip")
        .args(args)
        .output()
        .map_err(|error| Error::unavailable(format!("running `ip`: {error}")))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let message = if stderr.is_empty() {
        format!("`ip` exited with {}", output.status)
    } else {
        stderr
    };
    if message.contains("Operation not permitted") || message.contains("Permission denied") {
        return Err(Error::denied(format!(
            "the network apply flow needs CAP_NET_ADMIN: {message}"
        )));
    }
    Err(Error::unavailable(format!(
        "`ip {}` failed: {message}",
        args.join(" ")
    )))
}

fn text(input: &Value, key: &str) -> Result<String, Error> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::invalid(format!("`{key}` is required")))
}

fn boolean(input: &Value, key: &str) -> Result<bool, Error> {
    input
        .get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| Error::invalid(format!("`{key}` must be a boolean")))
}

fn optional_text(input: &Value, key: &str) -> Option<String> {
    input.get(key).and_then(Value::as_str).map(str::to_string)
}

fn number(input: &Value, key: &str) -> Option<u64> {
    input.get(key).and_then(Value::as_u64)
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
    fn destinations_are_validated_against_the_family() {
        assert_eq!(
            parse_destination(Family::V4, "10.1.0.0/24")
                .expect("ipv4")
                .0,
            "10.1.0.0/24"
        );
        assert_eq!(
            parse_destination(Family::V4, "10.1.0.1")
                .expect("host prefix")
                .0,
            "10.1.0.1/32"
        );
        assert!(parse_destination(Family::V4, "::/0").is_err());
        assert!(parse_destination(Family::V4, "10.0.0.0/33").is_err());
        assert!(parse_destination(Family::V6, "fe80::/64").is_ok());
        assert!(parse_gateway(Family::V6, "192.168.1.1").is_err());
        assert_eq!(parse_gateway(Family::V4, "10.0.0.1").unwrap(), "10.0.0.1");
    }

    #[test]
    fn route_and_link_arguments_match_iproute2() {
        assert_eq!(
            link_args("eth0", true),
            vec!["link", "set", "dev", "eth0", "up"]
        );
        assert_eq!(
            route_args(Family::V4, true, "0.0.0.0/0", Some("10.0.0.1"), "eth0", 100,),
            vec![
                "route",
                "add",
                "0.0.0.0/0",
                "via",
                "10.0.0.1",
                "dev",
                "eth0",
                "metric",
                "100",
            ]
        );
        assert_eq!(
            route_args(Family::V6, false, "::/0", None, "eth0", 0),
            vec!["-6", "route", "del", "::/0", "dev", "eth0"]
        );
    }

    #[test]
    fn only_kernel_interfaces_are_accepted() {
        if std::path::Path::new("/sys/class/net/lo").exists() {
            ensure_interface("lo").expect("loopback");
        }
        assert!(ensure_interface("..").is_err());
        assert!(ensure_interface("../lo").is_err());
        assert!(ensure_interface("").is_err());
    }

    #[tokio::test]
    async fn the_watch_stream_emits_a_snapshot() {
        use futures_util::StreamExt;

        let service = Service;
        let mut stream = service
            .stream(NET, "watch", json!({"interval-ms": 1000}))
            .expect("watch");
        let chunk = stream.next().await.expect("chunk").expect("ok");
        let snapshot: Value = serde_json::from_slice(&chunk).expect("json");
        assert!(snapshot["interfaces"].is_array());
        assert!(snapshot["routes"].is_array());
    }

    #[test]
    fn the_handler_validates_before_touching_the_host() {
        let service = Service;
        let error = service
            .call(
                NET,
                "add-route",
                json!({"family": "ipv4", "destination": "::/0", "device": "eth0"}),
            )
            .expect_err("family mismatch");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
        let error = service
            .call(
                NET,
                "set-up",
                json!({"name": "this-is-not-an-interface", "up": true}),
            )
            .expect_err("missing interface");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
    }
}
