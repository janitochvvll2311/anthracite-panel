//! Linux network-interface inspection and state changes.
//!
//! The read path uses `/sys/class/net` and `/proc/net/dev`; IPv4 and IPv6
//! addresses come from `getifaddrs(3)`. State changes use `SIOCSIFFLAGS` on an
//! `AF_INET` socket. `collect_from` and `check_state_change` take a filesystem
//! root so tests can exercise the fixtures without touching the host.
//!
//! No external command such as `ip` is required, and the interface list is
//! read-only unless `[network] control` is enabled.

use std::collections::HashMap;
use std::ffi::CStr;
use std::fs;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::os::raw::c_char;
use std::path::{Component, Path};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_proto::{
    NetworkAddress, NetworkAddressFamily, NetworkError, NetworkErrorKind, NetworkInterface,
    NetworkSnapshot,
};

use crate::text::read_trimmed;

const SYS_CLASS_NET: &str = "sys/class/net";
const PROC_NET_DEV: &str = "proc/net/dev";

/// Maximum interfaces returned in one snapshot. The protocol is newline
/// delimited and one line is capped at 1 MiB; at roughly 400 bytes per
/// interface this leaves ample headroom.
pub const MAX_INTERFACES: usize = 2000;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Counters {
    rx_bytes: u64,
    tx_bytes: u64,
    rx_packets: u64,
    tx_packets: u64,
    rx_errors: u64,
    tx_errors: u64,
    rx_dropped: u64,
    tx_dropped: u64,
}

/// Reads every interface on the host, including its IP addresses.
pub fn collect(control_enabled: bool) -> NetworkSnapshot {
    let mut snapshot = collect_from("/", control_enabled);
    let mut addresses = interface_addresses();

    for interface in &mut snapshot.interfaces {
        interface.addresses = addresses.remove(&interface.name).unwrap_or_default();
    }

    snapshot
}

/// Reads a snapshot from an alternative filesystem root. Addresses are not
/// resolved because they come from the live kernel, not from the fixture tree.
pub fn collect_from(root: impl AsRef<Path>, control_enabled: bool) -> NetworkSnapshot {
    let root = root.as_ref();
    let counters = fs::read_to_string(root.join(PROC_NET_DEV))
        .map(|text| parse_net_dev(&text))
        .unwrap_or_default();

    let mut interfaces = read_interfaces(&root.join(SYS_CLASS_NET), &counters);
    interfaces.sort_by(|left, right| left.name.cmp(&right.name));
    let (total, truncated, warning) = cap_interfaces(&mut interfaces);

    NetworkSnapshot {
        interfaces,
        timestamp_ms: timestamp_ms(),
        control_enabled,
        total,
        truncated,
        warning,
    }
}

fn cap_interfaces(interfaces: &mut Vec<NetworkInterface>) -> (usize, bool, Option<String>) {
    let total = interfaces.len();
    let truncated = total > MAX_INTERFACES;
    if truncated {
        interfaces.truncate(MAX_INTERFACES);
    }
    let warning = truncated.then(|| format!("showing {MAX_INTERFACES} of {total} interfaces"));

    (total, truncated, warning)
}

/// Brings one interface up or down.
pub fn set_state(control_enabled: bool, name: &str, up: bool) -> Result<(), NetworkError> {
    check_state_change(Path::new("/"), control_enabled, name)?;
    apply_link_state(name, up)
}

fn check_state_change(root: &Path, control_enabled: bool, name: &str) -> Result<(), NetworkError> {
    if !control_enabled {
        return Err(NetworkError::new(
            NetworkErrorKind::Disabled,
            "network control is disabled",
        ));
    }

    validate_interface_name(name)?;

    if !root.join(SYS_CLASS_NET).join(name).is_dir() {
        return Err(NetworkError::new(
            NetworkErrorKind::NotFound,
            format!("interface `{name}` does not exist"),
        ));
    }

    Ok(())
}

/// Refuses a destructive change that would clear `IFF_UP` on a loopback.
///
/// This runs on the authoritative flags returned by `SIOCGIFFLAGS`, not on the
/// best-effort sysfs read used by the snapshot path. A missing sysfs `flags`
/// file therefore cannot turn the guard off.
fn check_link_flags(flags: u32, up: bool) -> Result<(), NetworkError> {
    if !up && flags & (libc::IFF_LOOPBACK as u32) != 0 {
        return Err(NetworkError::new(
            NetworkErrorKind::Protected,
            "refusing to bring a loopback interface down",
        ));
    }

    Ok(())
}

fn validate_interface_name(name: &str) -> Result<(), NetworkError> {
    let invalid = name.is_empty()
        || name.len() >= libc::IFNAMSIZ
        || name
            .chars()
            .any(|c| c.is_whitespace() || c == '\0' || c == '/');

    if invalid {
        return Err(NetworkError::new(
            NetworkErrorKind::Invalid,
            format!("invalid interface name `{name}`"),
        ));
    }

    // A single path component prevents names such as `.` or `..` from escaping
    // the sysfs directory even though they contain no slash.
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err(NetworkError::new(
            NetworkErrorKind::Invalid,
            format!("invalid interface name `{name}`"),
        ));
    }

    Ok(())
}

fn read_interfaces(
    directory: &Path,
    counters: &HashMap<String, Counters>,
) -> Vec<NetworkInterface> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };

    let mut interfaces = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.is_empty() {
            continue;
        }

        let base = entry.path();
        let flags = read_trimmed(base.join("flags"))
            .and_then(|text| parse_flags(&text))
            .unwrap_or(0);
        let traffic = counters.get(&name).copied().unwrap_or_default();

        interfaces.push(NetworkInterface {
            name,
            index: read_trimmed(base.join("ifindex"))
                .and_then(|text| text.parse().ok())
                .unwrap_or(0),
            state: read_trimmed(base.join("operstate")).unwrap_or_else(|| "unknown".to_string()),
            up: flags & (libc::IFF_UP as u32) != 0,
            running: flags & (libc::IFF_RUNNING as u32) != 0,
            loopback: flags & (libc::IFF_LOOPBACK as u32) != 0,
            mac: read_trimmed(base.join("address")).unwrap_or_default(),
            mtu: read_trimmed(base.join("mtu"))
                .and_then(|text| text.parse().ok())
                .unwrap_or(0),
            speed_mbps: read_trimmed(base.join("speed"))
                .and_then(|text| text.parse::<i64>().ok())
                .filter(|speed| *speed > 0)
                .map(|speed| speed as u64),
            duplex: read_trimmed(base.join("duplex"))
                .filter(|duplex| duplex != "unknown" && !duplex.is_empty()),
            addresses: Vec::new(),
            rx_bytes: traffic.rx_bytes,
            tx_bytes: traffic.tx_bytes,
            rx_packets: traffic.rx_packets,
            tx_packets: traffic.tx_packets,
            rx_errors: traffic.rx_errors,
            tx_errors: traffic.tx_errors,
            rx_dropped: traffic.rx_dropped,
            tx_dropped: traffic.tx_dropped,
        });
    }

    interfaces
}

fn parse_flags(text: &str) -> Option<u32> {
    let digits = text
        .trim()
        .strip_prefix("0x")
        .or_else(|| text.trim().strip_prefix("0X"))
        .unwrap_or_else(|| text.trim());
    u32::from_str_radix(digits, 16).ok()
}

/// Parses the two-header-line `/proc/net/dev` format. Missing or malformed
/// fields degrade to zero so one odd row cannot hide the rest of the list.
fn parse_net_dev(text: &str) -> HashMap<String, Counters> {
    let mut counters = HashMap::new();

    for line in text.lines() {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }

        let fields: Vec<&str> = rest.split_whitespace().collect();
        if fields.len() < 16 {
            continue;
        }
        let value = |index: usize| fields[index].parse::<u64>().unwrap_or(0);

        counters.insert(
            name.to_string(),
            Counters {
                rx_bytes: value(0),
                rx_packets: value(1),
                rx_errors: value(2),
                rx_dropped: value(3),
                tx_bytes: value(8),
                tx_packets: value(9),
                tx_errors: value(10),
                tx_dropped: value(11),
            },
        );
    }

    counters
}

fn interface_addresses() -> HashMap<String, Vec<NetworkAddress>> {
    let mut result: HashMap<String, Vec<NetworkAddress>> = HashMap::new();
    let mut first: *mut libc::ifaddrs = std::ptr::null_mut();

    if unsafe { libc::getifaddrs(&mut first) } != 0 {
        return result;
    }
    let guard = IfAddrs(first);
    let mut current = guard.0;

    while !current.is_null() {
        let entry = unsafe { &*current };
        current = entry.ifa_next;

        if entry.ifa_name.is_null() || entry.ifa_addr.is_null() {
            continue;
        }

        let name = unsafe { CStr::from_ptr(entry.ifa_name) }
            .to_string_lossy()
            .into_owned();
        let address = match unsafe { (*entry.ifa_addr).sa_family } as i32 {
            libc::AF_INET => ipv4_address(entry.ifa_addr, entry.ifa_netmask),
            libc::AF_INET6 => ipv6_address(entry.ifa_addr, entry.ifa_netmask),
            _ => None,
        };

        if let Some(address) = address {
            result.entry(name).or_default().push(address);
        }
    }

    for addresses in result.values_mut() {
        addresses.sort_by(|left, right| {
            family_rank(left.family)
                .cmp(&family_rank(right.family))
                .then_with(|| left.address.cmp(&right.address))
        });
        addresses.dedup();
    }

    result
}

struct IfAddrs(*mut libc::ifaddrs);

impl Drop for IfAddrs {
    fn drop(&mut self) {
        unsafe { libc::freeifaddrs(self.0) };
    }
}

fn family_rank(family: NetworkAddressFamily) -> u8 {
    match family {
        NetworkAddressFamily::Ipv4 => 0,
        NetworkAddressFamily::Ipv6 => 1,
    }
}

fn ipv4_address(
    address: *const libc::sockaddr,
    netmask: *const libc::sockaddr,
) -> Option<NetworkAddress> {
    let address = unsafe { &*(address as *const libc::sockaddr_in) };
    let address_text = Ipv4Addr::from(u32::from_be(address.sin_addr.s_addr)).to_string();
    let prefix_len = if netmask.is_null() {
        0
    } else {
        let mask = unsafe { &*(netmask as *const libc::sockaddr_in) };
        u32::from_be(mask.sin_addr.s_addr).count_ones() as u8
    };

    Some(NetworkAddress {
        family: NetworkAddressFamily::Ipv4,
        address: address_text,
        prefix_len,
    })
}

fn ipv6_address(
    address: *const libc::sockaddr,
    netmask: *const libc::sockaddr,
) -> Option<NetworkAddress> {
    let address = unsafe { &*(address as *const libc::sockaddr_in6) };
    let address_text = Ipv6Addr::from(address.sin6_addr.s6_addr).to_string();
    let prefix_len = if netmask.is_null() {
        0
    } else {
        let mask = unsafe { &*(netmask as *const libc::sockaddr_in6) };
        mask.sin6_addr
            .s6_addr
            .iter()
            .map(|byte| byte.count_ones())
            .sum::<u32>() as u8
    };

    Some(NetworkAddress {
        family: NetworkAddressFamily::Ipv6,
        address: address_text,
        prefix_len,
    })
}

fn apply_link_state(name: &str, up: bool) -> Result<(), NetworkError> {
    let fd = unsafe { libc::socket(libc::AF_INET, libc::SOCK_DGRAM, 0) };
    if fd < 0 {
        return Err(io_network_error("could not open a socket"));
    }
    let guard = Socket(fd);

    let mut request: libc::ifreq = unsafe { std::mem::zeroed() };
    copy_interface_name(&mut request.ifr_name, name)?;

    if unsafe { libc::ioctl(guard.0, libc::SIOCGIFFLAGS as _, &mut request) } < 0 {
        return Err(io_network_error("could not read the interface flags"));
    }

    let mut flags = unsafe { request.ifr_ifru.ifru_flags } as u32;
    check_link_flags(flags, up)?;
    if up {
        flags |= libc::IFF_UP as u32;
    } else {
        flags &= !(libc::IFF_UP as u32);
    }
    request.ifr_ifru.ifru_flags = flags as libc::c_short;

    if unsafe { libc::ioctl(guard.0, libc::SIOCSIFFLAGS as _, &mut request) } < 0 {
        return Err(io_network_error("could not change the interface flags"));
    }

    Ok(())
}

fn copy_interface_name(
    destination: &mut [c_char; libc::IFNAMSIZ],
    name: &str,
) -> Result<(), NetworkError> {
    if name.len() >= destination.len() {
        return Err(NetworkError::new(
            NetworkErrorKind::Invalid,
            format!("interface name `{name}` is too long"),
        ));
    }

    // The zero-initialised ifreq keeps the terminating NUL byte.
    unsafe {
        std::ptr::copy_nonoverlapping(
            name.as_ptr() as *const c_char,
            destination.as_mut_ptr(),
            name.len(),
        );
    }
    Ok(())
}

struct Socket(libc::c_int);

impl Drop for Socket {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

fn io_network_error(context: &str) -> NetworkError {
    let error = io::Error::last_os_error();
    let kind = match error.raw_os_error() {
        Some(libc::EPERM | libc::EACCES) => NetworkErrorKind::PermissionDenied,
        Some(libc::ENODEV | libc::ENXIO | libc::ENOENT) => NetworkErrorKind::NotFound,
        Some(libc::EINVAL) => NetworkErrorKind::Invalid,
        _ => NetworkErrorKind::Failed,
    };
    NetworkError::new(kind, format!("{context}: {error}"))
}

fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_dir(name: &str) -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("anth-panel-{name}-{}-{nanos}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn fixture() -> std::path::PathBuf {
        let root = unique_dir("network-fixture");
        let eth0 = root.join("sys/class/net/eth0");
        let lo = root.join("sys/class/net/lo");
        fs::create_dir_all(&eth0).unwrap();
        fs::create_dir_all(&lo).unwrap();
        fs::create_dir_all(root.join("proc/net")).unwrap();

        fs::write(eth0.join("ifindex"), "2\n").unwrap();
        fs::write(eth0.join("operstate"), "up\n").unwrap();
        fs::write(eth0.join("flags"), "0x1043\n").unwrap();
        fs::write(eth0.join("address"), "aa:bb:cc:dd:ee:ff\n").unwrap();
        fs::write(eth0.join("mtu"), "1500\n").unwrap();
        fs::write(eth0.join("speed"), "1000\n").unwrap();
        fs::write(eth0.join("duplex"), "full\n").unwrap();
        fs::write(lo.join("ifindex"), "1\n").unwrap();
        fs::write(lo.join("operstate"), "unknown\n").unwrap();
        fs::write(lo.join("flags"), "0x9\n").unwrap();
        fs::write(lo.join("address"), "00:00:00:00:00:00\n").unwrap();
        fs::write(lo.join("mtu"), "65536\n").unwrap();

        fs::write(
            root.join("proc/net/dev"),
            concat!(
                "Inter-|   Receive                                                |  Transmit\n",
                " face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed\n",
                "    lo: 100 2 0 0 0 0 0 0 200 3 0 0 0 0 0 0\n",
                "  eth0: 12345 10 1 2 0 0 0 0 67890 20 3 4 0 0 0 0\n",
            ),
        )
        .unwrap();

        root
    }

    #[test]
    fn parse_net_dev_reads_both_directions_and_ignores_junk() {
        let text = concat!(
            "Inter-|   Receive | Transmit\n",
            " face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n",
            "  eth0: 10 1 2 3 0 0 0 0 20 4 5 6 0 0 0 0\n",
            "broken: 1 2\n",
        );
        let counters = parse_net_dev(text);

        assert_eq!(
            counters["eth0"],
            Counters {
                rx_bytes: 10,
                rx_packets: 1,
                rx_errors: 2,
                rx_dropped: 3,
                tx_bytes: 20,
                tx_packets: 4,
                tx_errors: 5,
                tx_dropped: 6,
            }
        );
        assert!(!counters.contains_key("broken"));
    }

    #[test]
    fn collect_from_reads_sysfs_and_proc_fixtures() {
        let root = fixture();
        let snapshot = collect_from(&root, true);

        assert!(snapshot.timestamp_ms > 0);
        assert!(snapshot.control_enabled);
        assert_eq!(snapshot.interfaces.len(), 2);
        assert_eq!(snapshot.interfaces[0].name, "eth0");
        assert_eq!(snapshot.interfaces[0].index, 2);
        assert_eq!(snapshot.interfaces[0].state, "up");
        assert!(snapshot.interfaces[0].up);
        assert!(snapshot.interfaces[0].running);
        assert!(!snapshot.interfaces[0].loopback);
        assert_eq!(snapshot.interfaces[0].mac, "aa:bb:cc:dd:ee:ff");
        assert_eq!(snapshot.interfaces[0].mtu, 1500);
        assert_eq!(snapshot.interfaces[0].speed_mbps, Some(1000));
        assert_eq!(snapshot.interfaces[0].duplex.as_deref(), Some("full"));
        assert_eq!(snapshot.interfaces[0].rx_bytes, 12345);
        assert_eq!(snapshot.interfaces[0].tx_packets, 20);
        assert_eq!(snapshot.interfaces[0].rx_errors, 1);
        assert_eq!(snapshot.interfaces[1].name, "lo");
        assert!(snapshot.interfaces[1].up);
        assert!(!snapshot.interfaces[1].running);
        assert!(snapshot.interfaces[1].loopback);
        assert_eq!(snapshot.interfaces[1].speed_mbps, None);
    }

    #[test]
    fn interface_lists_are_capped() {
        let mut interfaces = (0..MAX_INTERFACES + 5)
            .map(|index| NetworkInterface {
                name: format!("eth{index}"),
                ..NetworkInterface::default()
            })
            .collect::<Vec<_>>();

        let (total, truncated, warning) = cap_interfaces(&mut interfaces);

        assert_eq!(interfaces.len(), MAX_INTERFACES);
        assert_eq!(total, MAX_INTERFACES + 5);
        assert!(truncated);
        let expected = format!(
            "showing {MAX_INTERFACES} of {} interfaces",
            MAX_INTERFACES + 5
        );
        assert_eq!(warning.as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn collect_reads_the_live_interface_list() {
        let snapshot = collect(false);
        assert!(snapshot.timestamp_ms > 0);
        assert!(!snapshot.control_enabled);
        assert!(
            snapshot
                .interfaces
                .iter()
                .all(|iface| !iface.name.is_empty())
        );
    }

    #[test]
    fn state_change_policy_runs_before_touching_the_kernel() {
        let root = fixture();

        assert_eq!(
            check_state_change(&root, false, "eth0").unwrap_err().kind,
            NetworkErrorKind::Disabled
        );
        assert_eq!(
            check_state_change(&root, true, "missing").unwrap_err().kind,
            NetworkErrorKind::NotFound
        );
        assert!(check_state_change(&root, true, "eth0").is_ok());
        assert!(check_state_change(&root, true, "lo").is_ok());
    }

    #[test]
    fn loopback_flags_protect_only_the_down_direction() {
        assert_eq!(
            check_link_flags(libc::IFF_LOOPBACK as u32 | libc::IFF_UP as u32, false)
                .unwrap_err()
                .kind,
            NetworkErrorKind::Protected
        );
        assert!(check_link_flags(libc::IFF_LOOPBACK as u32 | libc::IFF_UP as u32, true).is_ok());
        assert!(check_link_flags(libc::IFF_UP as u32, false).is_ok());
    }

    #[test]
    fn interface_names_must_be_one_safe_component() {
        for name in ["eth0", "br-abc", "veth123", "enp0s3"] {
            assert!(validate_interface_name(name).is_ok(), "{name}");
        }

        for name in ["", ".", "..", "../eth0", "eth 0", "eth\0", "eth0/1"] {
            assert!(validate_interface_name(name).is_err(), "{name:?}");
        }
    }

    #[test]
    fn flags_accept_the_sysfs_hex_and_plain_forms() {
        assert_eq!(parse_flags("0x1003\n"), Some(0x1003));
        assert_eq!(parse_flags("1003"), Some(0x1003));
        assert_eq!(parse_flags("nope"), None);
    }
}
