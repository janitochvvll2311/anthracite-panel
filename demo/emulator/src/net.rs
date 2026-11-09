//! Deterministic interface and route fixture for the demo (§19 §2, `net`).

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
                    rx_bytes: 8_589_934_592,
                    tx_bytes: 2_147_483_648,
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
        }
    }

    pub fn handle(&mut self, function: &str, input: &Value) -> Result<Value, Error> {
        match function {
            "interfaces" => Ok(Value::Array(
                self.interfaces
                    .iter()
                    .map(|interface| {
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
                            "rx-bytes": interface.rx_bytes,
                            "tx-bytes": interface.tx_bytes,
                        })
                    })
                    .collect(),
            )),
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
                if !matches!(family.as_str(), "ipv4" | "ipv6") {
                    return Err(Error::invalid("`family` is `ipv4` or `ipv6`"));
                }
                let destination = text(input, "destination")?;
                if destination.trim().is_empty() {
                    return Err(Error::invalid("`destination` cannot be empty"));
                }
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
                    gateway: optional_text(input, "gateway"),
                    device,
                    metric: number(input, "metric")
                        .unwrap_or(100)
                        .min(u64::from(u32::MAX)) as u32,
                });
                Ok(Value::Null)
            }
            "remove-route" => {
                let destination = text(input, "destination")?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn call(fixture: &mut NetFixture, function: &str, input: Value) -> Value {
        fixture
            .handle(function, &input)
            .expect("the fixture answers")
    }

    #[test]
    fn interfaces_and_routes_are_stable() {
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
}
