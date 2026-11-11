//! Deterministic interface and route fixture for the demo (§19 §2, `net`).

use std::net::IpAddr;

use anth_contract::Error;
use serde_json::{json, Value};

use crate::{boolean, number, optional_text, text};

#[derive(serde::Serialize, serde::Deserialize)]
struct Address {
    family: String,
    address: String,
    prefix: u8,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Interface {
    name: String,
    up: bool,
    mac: String,
    mtu: u32,
    addresses: Vec<Address>,
    rx_bytes: u64,
    tx_bytes: u64,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Route {
    family: String,
    destination: String,
    gateway: Option<String>,
    device: String,
    metric: u32,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct NetFixture {
    interfaces: Vec<Interface>,
    routes: Vec<Route>,
    /// Advanced once per snapshot so the demo counters move when the view is
    /// refreshed, without ever reading the host.
    #[serde(default)]
    tick: u64,
}

impl NetFixture {
    pub fn new() -> Self {
        Self {
            interfaces: vec![
                Interface {
                    name: "lo".to_string(),
                    up: true,
                    mac: "00:00:00:00:00:00".to_string(),
                    mtu: 65536,
                    addresses: vec![
                        Address {
                            family: "ipv4".to_string(),
                            address: "127.0.0.1".to_string(),
                            prefix: 8,
                        },
                        Address {
                            family: "ipv6".to_string(),
                            address: "::1".to_string(),
                            prefix: 128,
                        },
                    ],
                    rx_bytes: 1_048_576,
                    tx_bytes: 1_048_576,
                },
                Interface {
                    name: "eth0".to_string(),
                    up: true,
                    mac: "02:42:ac:11:00:02".to_string(),
                    mtu: 1500,
                    addresses: vec![
                        Address {
                            family: "ipv4".to_string(),
                            address: "10.0.0.2".to_string(),
                            prefix: 24,
                        },
                        Address {
                            family: "ipv6".to_string(),
                            address: "fe80::42:acff:fe11:2".to_string(),
                            prefix: 64,
                        },
                    ],
                    rx_bytes: 12_582_912,
                    tx_bytes: 3_145_728,
                },
                Interface {
                    name: "wlan0".to_string(),
                    up: false,
                    mac: "02:42:ac:11:00:03".to_string(),
                    mtu: 1500,
                    addresses: vec![Address {
                        family: "ipv4".to_string(),
                        address: "192.168.1.40".to_string(),
                        prefix: 24,
                    }],
                    rx_bytes: 0,
                    tx_bytes: 0,
                },
            ],
            routes: vec![
                Route {
                    family: "ipv4".to_string(),
                    destination: "0.0.0.0/0".to_string(),
                    gateway: Some("10.0.0.1".to_string()),
                    device: "eth0".to_string(),
                    metric: 100,
                },
                Route {
                    family: "ipv4".to_string(),
                    destination: "10.0.0.0/24".to_string(),
                    gateway: None,
                    device: "eth0".to_string(),
                    metric: 100,
                },
                Route {
                    family: "ipv6".to_string(),
                    destination: "::/0".to_string(),
                    gateway: Some("fe80::1".to_string()),
                    device: "eth0".to_string(),
                    metric: 1024,
                },
                Route {
                    family: "ipv6".to_string(),
                    destination: "fe80::/64".to_string(),
                    gateway: None,
                    device: "eth0".to_string(),
                    metric: 100,
                },
            ],
            tick: 0,
        }
    }

    pub fn handle(&mut self, function: &str, input: &Value) -> Result<Value, Error> {
        match function {
            "interfaces" => {
                self.tick = self.tick.saturating_add(1);
                let tick = self.tick;
                Ok(Value::Array(
                    self.interfaces
                        .iter()
                        .map(|interface| {
                            let (rx_rate, tx_rate) = rates(&interface.name, interface.up);
                            json!({
                                "name": interface.name,
                                "up": interface.up,
                                "mac": interface.mac,
                                "mtu": interface.mtu,
                                "addresses": interface.addresses.iter().map(|address| json!({
                                    "family": address.family,
                                    "address": address.address,
                                    "prefix": address.prefix,
                                })).collect::<Vec<_>>(),
                                "rx-bytes": interface.rx_bytes.saturating_add(tick.saturating_mul(rx_rate)),
                                "tx-bytes": interface.tx_bytes.saturating_add(tick.saturating_mul(tx_rate)),
                            })
                        })
                        .collect(),
                ))
            }
            "routes" => Ok(Value::Array(
                self.routes
                    .iter()
                    .map(|route| {
                        json!({
                            "family": route.family,
                            "destination": route.destination,
                            "gateway": route.gateway,
                            "device": route.device,
                            "metric": route.metric,
                        })
                    })
                    .collect(),
            )),
            "set-up" => {
                let name = text(input, "name")?;
                let up = boolean(input, "up");
                let interface = self
                    .interfaces
                    .iter_mut()
                    .find(|interface| interface.name == name)
                    .ok_or_else(|| {
                        Error::not_found(format!("interface `{name}` does not exist"))
                    })?;
                interface.up = up;
                Ok(Value::Null)
            }
            "add-route" => {
                let family = text(input, "family")?;
                let destination =
                    normalize_destination(&family, text(input, "destination")?.trim())?;
                let device = text(input, "device")?;
                if !self
                    .interfaces
                    .iter()
                    .any(|interface| interface.name == device)
                {
                    return Err(Error::not_found(format!(
                        "interface `{device}` does not exist"
                    )));
                }
                let gateway = match optional_text(input, "gateway") {
                    Some(value) if !value.trim().is_empty() => {
                        Some(normalize_gateway(&family, value.trim())?)
                    }
                    _ => None,
                };
                if self
                    .routes
                    .iter()
                    .any(|route| route.destination == destination && route.device == device)
                {
                    return Err(Error::conflict(format!(
                        "`{destination}` is already routed through `{device}`"
                    )));
                }
                self.routes.push(Route {
                    family,
                    destination,
                    gateway,
                    device,
                    metric: number(input, "metric")
                        .unwrap_or(100)
                        .min(u64::from(u32::MAX)) as u32,
                });
                Ok(Value::Null)
            }
            "remove-route" => {
                let destination = text(input, "destination")?;
                let (_, destination) = normalize_any_destination(destination.trim())?;
                let device = text(input, "device")?;
                let index = self
                    .routes
                    .iter()
                    .position(|route| route.destination == destination && route.device == device)
                    .ok_or_else(|| {
                        Error::not_found(format!(
                            "`{destination}` is not routed through `{device}`"
                        ))
                    })?;
                self.routes.remove(index);
                Ok(Value::Null)
            }
            other => Err(Error::not_found(format!(
                "`anthracite:net/net#{other}` is not implemented"
            ))),
        }
    }
}

/// Bytes added to the counters on every snapshot, per interface. The demo is
/// deterministic; it only needs the numbers to move when the view refreshes.
fn rates(name: &str, up: bool) -> (u64, u64) {
    match name {
        "eth0" => (1_572_864, 524_288),
        "lo" => (131_072, 131_072),
        "wlan0" if up => (393_216, 131_072),
        _ => (0, 0),
    }
}

/// Normalizes a route destination to `address/prefix`, checking the family and
/// the prefix range. A bare address is a host route.
fn normalize_destination(family: &str, destination: &str) -> Result<String, Error> {
    let max = match family {
        "ipv4" => 32,
        "ipv6" => 128,
        _ => return Err(Error::invalid("`family` is `ipv4` or `ipv6`")),
    };
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
        None => (destination, max),
    };
    let address: IpAddr = address
        .parse()
        .map_err(|_| Error::invalid("`destination` is not an IP address"))?;
    let matches =
        (family == "ipv4" && address.is_ipv4()) || (family == "ipv6" && address.is_ipv6());
    if !matches {
        return Err(Error::invalid("`destination` does not match `family`"));
    }
    if prefix > max {
        return Err(Error::invalid("`destination` prefix is out of range"));
    }
    Ok(format!("{address}/{prefix}"))
}

fn normalize_any_destination(destination: &str) -> Result<(String, String), Error> {
    let address = destination
        .split_once('/')
        .map(|(address, _)| address)
        .unwrap_or(destination);
    let address: IpAddr = address
        .parse()
        .map_err(|_| Error::invalid("`destination` is not an IP address"))?;
    let family = if address.is_ipv4() { "ipv4" } else { "ipv6" };
    Ok((
        family.to_string(),
        normalize_destination(family, destination)?,
    ))
}

fn normalize_gateway(family: &str, gateway: &str) -> Result<String, Error> {
    let address: IpAddr = gateway
        .parse()
        .map_err(|_| Error::invalid("`gateway` is not an IP address"))?;
    if (family == "ipv4") != address.is_ipv4() {
        return Err(Error::invalid("`gateway` does not match `family`"));
    }
    Ok(address.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(fixture: &mut NetFixture, function: &str, input: Value) -> Value {
        fixture
            .handle(function, &input)
            .expect("the fixture answers")
    }

    #[test]
    fn interfaces_and_routes_have_the_seeded_shape() {
        let mut fixture = NetFixture::new();
        let interfaces = call(&mut fixture, "interfaces", Value::Null);
        assert_eq!(interfaces[1]["name"], "eth0");
        assert_eq!(interfaces[1]["addresses"][0]["address"], "10.0.0.2");
        let routes = call(&mut fixture, "routes", Value::Null);
        assert_eq!(routes[0]["destination"], "0.0.0.0/0");
        assert_eq!(routes[0]["gateway"], "10.0.0.1");
    }

    #[test]
    fn an_interface_can_be_brought_up_and_down() {
        let mut fixture = NetFixture::new();
        call(&mut fixture, "set-up", json!({"name": "wlan0", "up": true}));
        let interfaces = call(&mut fixture, "interfaces", Value::Null);
        assert_eq!(interfaces[2]["up"], true);
        let error = fixture
            .handle("set-up", &json!({"name": "nope", "up": true}))
            .expect_err("missing");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
    }

    #[test]
    fn the_interface_counters_move_on_each_snapshot() {
        let mut fixture = NetFixture::new();
        let first = call(&mut fixture, "interfaces", Value::Null);
        let second = call(&mut fixture, "interfaces", Value::Null);
        assert!(
            second[1]["rx-bytes"].as_u64().expect("rx")
                > first[1]["rx-bytes"].as_u64().expect("rx")
        );
        assert_eq!(
            first[2]["rx-bytes"], second[2]["rx-bytes"],
            "the down interface stays idle"
        );
        call(&mut fixture, "set-up", json!({"name": "wlan0", "up": true}));
        let third = call(&mut fixture, "interfaces", Value::Null);
        let fourth = call(&mut fixture, "interfaces", Value::Null);
        assert!(
            fourth[2]["rx-bytes"].as_u64().expect("rx")
                > third[2]["rx-bytes"].as_u64().expect("rx")
        );
    }

    #[test]
    fn routes_can_be_added_and_removed() {
        let mut fixture = NetFixture::new();
        call(
            &mut fixture,
            "add-route",
            json!({"family": "ipv4", "destination": "10.1.0.0/24", "device": "eth0", "metric": 50}),
        );
        let routes = call(&mut fixture, "routes", Value::Null);
        assert!(routes
            .as_array()
            .expect("routes")
            .iter()
            .any(|route| route["destination"] == "10.1.0.0/24"));
        call(
            &mut fixture,
            "remove-route",
            json!({"destination": "10.1.0.0/24", "device": "eth0"}),
        );
        let routes = call(&mut fixture, "routes", Value::Null);
        assert!(!routes
            .as_array()
            .expect("routes")
            .iter()
            .any(|route| route["destination"] == "10.1.0.0/24"));

        let error = fixture
            .handle(
                "add-route",
                &json!({"family": "ipv4", "destination": "10.1.0.0/24", "device": "nope"}),
            )
            .expect_err("missing device");
        assert_eq!(error.kind(), anth_contract::ErrorKind::NotFound);
    }

    #[test]
    fn routes_validate_family_prefix_and_gateway() {
        let mut fixture = NetFixture::new();
        for input in [
            json!({"family": "ipv4", "destination": "::/0", "device": "eth0"}),
            json!({"family": "ipv4", "destination": "10.0.0.0/33", "device": "eth0"}),
            json!({"family": "ipv4", "destination": "10.0.0.0/24", "gateway": "fe80::1", "device": "eth0"}),
        ] {
            let error = fixture
                .handle("add-route", &input)
                .expect_err("invalid route");
            assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
        }
        call(
            &mut fixture,
            "add-route",
            json!({"family": "ipv4", "destination": "10.2.0.1", "device": "eth0"}),
        );
        let routes = call(&mut fixture, "routes", Value::Null);
        assert!(routes
            .as_array()
            .expect("routes")
            .iter()
            .any(|route| route["destination"] == "10.2.0.1/32"));
    }
}
