//! Pure parsers for the kernel network files.
//!
//! `/proc/net/dev`, `/proc/net/route` and `/proc/net/ipv6_route` have stable
//! column layouts; parsing them here keeps the scanner free of offsets and lets
//! the fixtures cover big counters, multiple defaults and empty files.

use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};

use anth_wire::NetworkRoute;

/// Per-interface counters from `/proc/net/dev`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InterfaceStats {
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub rx_errors: u64,
    pub rx_dropped: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
    pub tx_errors: u64,
    pub tx_dropped: u64,
}

/// Parses `/proc/net/dev`, keyed by interface name.
pub fn parse_dev(text: &str) -> HashMap<String, InterfaceStats> {
    let mut stats = HashMap::new();
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
        stats.insert(
            name.to_string(),
            InterfaceStats {
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
    stats
}

/// Parses the lowest-metric IPv4 default route.
pub fn default_route_v4(text: &str) -> Option<NetworkRoute> {
    let mut best: Option<(u32, NetworkRoute)> = None;
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 8 || fields[1] != "00000000" {
            continue;
        }
        let Some(flags) = u32::from_str_radix(fields[3], 16).ok() else {
            continue;
        };
        if flags & 1 == 0 {
            continue;
        }
        let Some(gateway) = parse_gateway_v4(fields[2]) else {
            continue;
        };
        if gateway.is_unspecified() {
            continue;
        }
        let metric = fields[6].parse::<u32>().unwrap_or(0);
        let route = NetworkRoute {
            family: "inet".to_string(),
            gateway: gateway.to_string(),
            interface: fields[0].to_string(),
        };
        if best.as_ref().is_none_or(|(current, _)| metric < *current) {
            best = Some((metric, route));
        }
    }
    best.map(|(_, route)| route)
}

/// Parses the lowest-metric IPv6 default route.
pub fn default_route_v6(text: &str) -> Option<NetworkRoute> {
    let mut best: Option<(u32, NetworkRoute)> = None;
    for line in text.lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 10 {
            continue;
        }
        if fields[0].chars().any(|character| character != '0') || fields[1] != "00" {
            continue;
        }
        let Some(gateway) = parse_gateway_v6(fields[4]) else {
            continue;
        };
        if gateway.is_unspecified() {
            continue;
        }
        let metric = fields[5].parse::<u32>().unwrap_or(0);
        let route = NetworkRoute {
            family: "inet6".to_string(),
            gateway: gateway.to_string(),
            interface: fields[9].to_string(),
        };
        if best.as_ref().is_none_or(|(current, _)| metric < *current) {
            best = Some((metric, route));
        }
    }
    best.map(|(_, route)| route)
}

/// Parses the sysfs flags field, which is hexadecimal with a `0x` prefix.
pub fn parse_flags(text: &str) -> Option<u32> {
    let text = text.trim();
    let text = text.strip_prefix("0x").unwrap_or(text);
    u32::from_str_radix(text, 16).ok()
}

fn parse_gateway_v4(text: &str) -> Option<Ipv4Addr> {
    let value = u32::from_str_radix(text, 16).ok()?;
    let bytes = value.to_le_bytes();
    Some(Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]))
}

fn parse_gateway_v6(text: &str) -> Option<Ipv6Addr> {
    if text.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
    }
    Some(Ipv6Addr::from(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEV: &str = "Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 1000      10    0    0    0     0          0         0   1000      10    0    0    0     0       0          0
  eth0: 2000      20    1    2    0     0          0         0   3000      30    1    2    0     0       0          0
";

    #[test]
    fn dev_parses_both_directions() {
        let stats = parse_dev(DEV);
        assert_eq!(stats.len(), 2);
        let eth0 = stats.get("eth0").expect("eth0");
        assert_eq!(eth0.rx_bytes, 2000);
        assert_eq!(eth0.rx_packets, 20);
        assert_eq!(eth0.rx_errors, 1);
        assert_eq!(eth0.rx_dropped, 2);
        assert_eq!(eth0.tx_bytes, 3000);
        assert_eq!(eth0.tx_packets, 30);
        assert_eq!(eth0.tx_errors, 1);
        assert_eq!(eth0.tx_dropped, 2);
    }

    #[test]
    fn dev_ignores_headers_and_short_lines() {
        let stats = parse_dev("nonsense\neth0: 1 2 3\n");
        assert!(stats.is_empty());
    }

    #[test]
    fn the_lowest_metric_default_route_wins() {
        let routes =
            "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT
eth0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0
wlan0\t00000000\t0101A8C0\t0003\t0\t0\t50\t00000000\t0\t0\t0
";
        let route = default_route_v4(routes).expect("default route");
        assert_eq!(route.interface, "wlan0");
        assert_eq!(route.gateway, "192.168.1.1");
        assert_eq!(route.family, "inet");
    }

    #[test]
    fn a_route_without_up_or_gateway_is_skipped() {
        let down =
            "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT
eth0\t00000000\t0101A8C0\t0000\t0\t0\t100\t00000000\t0\t0\t0
";
        assert!(default_route_v4(down).is_none());

        let no_gateway =
            "Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT
eth0\t00000000\t00000000\t0003\t0\t0\t100\t00000000\t0\t0\t0
";
        assert!(default_route_v4(no_gateway).is_none());
    }

    #[test]
    fn ipv6_default_route_is_parsed() {
        let routes = "00000000000000000000000000000000 00 00000000000000000000000000000000 00 \
             fe800000000000000000000000000001 00000064 00000000 00000000 00000000 eth0\n";
        let route = default_route_v6(routes).expect("v6 route");
        assert_eq!(route.interface, "eth0");
        assert_eq!(route.family, "inet6");
        assert_eq!(route.gateway, "fe80::1");
    }

    #[test]
    fn flags_are_hexadecimal() {
        assert_eq!(parse_flags("0x1003"), Some(0x1003));
        assert_eq!(parse_flags("1003"), Some(0x1003));
        assert_eq!(parse_flags("not flags"), None);
    }
}
