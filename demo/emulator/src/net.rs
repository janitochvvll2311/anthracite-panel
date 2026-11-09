//! Deterministic interface and route fixture for the demo (§19 §2, `net`).

use anth_contract::Error;
use serde_json::{json, Value};

pub struct NetFixture;

impl NetFixture {
    pub fn new() -> Self {
        Self
    }

    pub fn handle(&self, function: &str, _input: &Value) -> Result<Value, Error> {
        match function {
            "interfaces" => Ok(json!([
                {
                    "name": "lo",
                    "up": true,
                    "mac": "00:00:00:00:00:00",
                    "mtu": 65536,
                    "addresses": [
                        { "family": "ipv4", "address": "127.0.0.1", "prefix": 8 },
                        { "family": "ipv6", "address": "::1", "prefix": 128 },
                    ],
                    "rx-bytes": 1_048_576,
                    "tx-bytes": 1_048_576,
                },
                {
                    "name": "eth0",
                    "up": true,
                    "mac": "02:42:ac:11:00:02",
                    "mtu": 1500,
                    "addresses": [
                        { "family": "ipv4", "address": "10.0.0.2", "prefix": 24 },
                        { "family": "ipv6", "address": "fe80::42:acff:fe11:2", "prefix": 64 },
                    ],
                    "rx-bytes": 8_589_934_592u64,
                    "tx-bytes": 2_147_483_648u64,
                },
                {
                    "name": "wlan0",
                    "up": false,
                    "mac": "02:42:ac:11:00:03",
                    "mtu": 1500,
                    "addresses": [
                        { "family": "ipv4", "address": "192.168.1.40", "prefix": 24 },
                    ],
                    "rx-bytes": 0,
                    "tx-bytes": 0,
                },
            ])),
            "routes" => Ok(json!([
                {
                    "family": "ipv4",
                    "destination": "0.0.0.0/0",
                    "gateway": "10.0.0.1",
                    "device": "eth0",
                    "metric": 100,
                },
                {
                    "family": "ipv4",
                    "destination": "10.0.0.0/24",
                    "gateway": null,
                    "device": "eth0",
                    "metric": 100,
                },
                {
                    "family": "ipv6",
                    "destination": "::/0",
                    "gateway": "fe80::1",
                    "device": "eth0",
                    "metric": 1024,
                },
                {
                    "family": "ipv6",
                    "destination": "fe80::/64",
                    "gateway": null,
                    "device": "eth0",
                    "metric": 100,
                },
            ])),
            other => Err(Error::not_found(format!(
                "`anthracite:net/net#{other}` is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interfaces_and_routes_are_stable() {
        let fixture = NetFixture::new();
        let interfaces = fixture
            .handle("interfaces", &Value::Null)
            .expect("interfaces");
        assert_eq!(interfaces[1]["name"], "eth0");
        assert_eq!(interfaces[1]["addresses"][0]["address"], "10.0.0.2");
        let routes = fixture.handle("routes", &Value::Null).expect("routes");
        assert_eq!(routes[0]["destination"], "0.0.0.0/0");
        assert_eq!(routes[0]["gateway"], "10.0.0.1");
    }
}
