//! In-memory network backend for the demo build.
//!
//! The snapshot starts from a deterministic fixture and every mutation edits
//! that copy. No sysfs, `ip` or netlink call is made.

use std::sync::Mutex;

use anth_wire::{
    ErrorKind, NetworkAddress, NetworkCreate, NetworkEdit, NetworkInterface, NetworkRoute,
    NetworkSnapshot, WireError,
};

use crate::backend::NetworkService;
use crate::network::{validate_mtu, validate_name};
use crate::now_ms;

pub struct DemoNetwork {
    control: bool,
    manage: bool,
    state: Mutex<DemoState>,
}

struct DemoState {
    interfaces: Vec<NetworkInterface>,
    routes: Vec<NetworkRoute>,
    next_index: u32,
}

impl DemoNetwork {
    pub fn new(control: bool, manage: bool) -> Self {
        Self {
            control,
            manage,
            state: Mutex::new(seed()),
        }
    }

    fn snapshot(&self) -> NetworkSnapshot {
        let state = self.state.lock().expect("demo network lock");
        NetworkSnapshot {
            interfaces: state.interfaces.clone(),
            routes: state.routes.clone(),
            timestamp_ms: now_ms(),
            control_enabled: self.control,
            manage_enabled: self.manage,
        }
    }

    fn get<'a>(
        state: &'a mut DemoState,
        name: &str,
    ) -> Result<&'a mut NetworkInterface, WireError> {
        state
            .interfaces
            .iter_mut()
            .find(|interface| interface.name == name)
            .ok_or_else(|| not_found(name))
    }
}

impl NetworkService for DemoNetwork {
    fn snapshot(&self) -> Result<NetworkSnapshot, WireError> {
        Ok(DemoNetwork::snapshot(self))
    }

    fn set_state(&self, name: &str, up: bool) -> Result<String, WireError> {
        if !self.control {
            return Err(disabled_control());
        }
        validate_name(name)?;
        let mut state = self.state.lock().expect("demo network lock");
        let target = DemoNetwork::get(&mut state, name)?;
        if !up && target.loopback {
            return Err(protected("refusing to bring a loopback interface down"));
        }
        target.up = up;
        target.running = up;
        target.state = if up { "up" } else { "down" }.to_string();
        Ok(format!("`{name}` is {}", if up { "up" } else { "down" }))
    }

    fn create(&self, args: &NetworkCreate) -> Result<String, WireError> {
        if !self.manage {
            return Err(disabled_manage());
        }
        validate_name(&args.name)?;
        if let Some(mtu) = args.mtu {
            validate_mtu(mtu)?;
        }
        let mut state = self.state.lock().expect("demo network lock");
        if state
            .interfaces
            .iter()
            .any(|interface| interface.name == args.name)
        {
            return Err(invalid(format!("interface `{}` already exists", args.name)));
        }
        let kind = match args.kind.as_str() {
            "veth" | "bridge" | "bond" | "vlan" | "dummy" | "tun" => args.kind.clone(),
            other => {
                return Err(invalid(format!(
                    "`{other}` is not a kind the panel can create"
                )));
            }
        };
        let index = state.next_index;
        state.next_index += 1;
        state.interfaces.push(NetworkInterface {
            name: args.name.clone(),
            index,
            kind: kind.clone(),
            mac: format!("02:00:00:00:{:02x}:{:02x}", index / 256, index % 256),
            mtu: args.mtu.unwrap_or(1500),
            state: if args.up { "up" } else { "down" }.to_string(),
            up: args.up,
            running: args.up,
            loopback: false,
            physical: false,
            addresses: Vec::new(),
            rx_bytes: 0,
            tx_bytes: 0,
            rx_packets: 0,
            tx_packets: 0,
            rx_errors: 0,
            tx_errors: 0,
            rx_dropped: 0,
            tx_dropped: 0,
            speed_mbps: None,
        });
        Ok(format!(
            "created {} interface `{}`",
            if kind == "tun" { "tun/tap" } else { &kind },
            args.name
        ))
    }

    fn edit(&self, args: &NetworkEdit) -> Result<String, WireError> {
        if !self.manage {
            return Err(disabled_manage());
        }
        validate_name(&args.name)?;
        if let Some(mtu) = args.mtu {
            validate_mtu(mtu)?;
        }
        if args.up.is_some() && !self.control {
            return Err(disabled_control());
        }
        let mut state = self.state.lock().expect("demo network lock");
        let target = DemoNetwork::get(&mut state, &args.name)?;
        let mut changes = Vec::new();
        if let Some(mtu) = args.mtu {
            target.mtu = mtu;
            changes.push(format!("mtu {mtu}"));
        }
        if let Some(up) = args.up {
            if !up && target.loopback {
                return Err(protected("refusing to bring a loopback interface down"));
            }
            target.up = up;
            target.running = up;
            target.state = if up { "up" } else { "down" }.to_string();
            changes.push(if up {
                "up".to_string()
            } else {
                "down".to_string()
            });
        }
        if changes.is_empty() {
            return Err(invalid("no change was requested"));
        }
        Ok(format!("updated `{}`: {}", args.name, changes.join(", ")))
    }

    fn rename(&self, name: &str, new_name: &str) -> Result<String, WireError> {
        if !self.manage {
            return Err(disabled_manage());
        }
        validate_name(name)?;
        validate_name(new_name)?;
        let mut state = self.state.lock().expect("demo network lock");
        let target = state
            .interfaces
            .iter()
            .find(|interface| interface.name == name)
            .ok_or_else(|| not_found(name))?;
        if target.up {
            return Err(invalid(format!(
                "refusing to rename `{name}` while it is up; bring it down first"
            )));
        }
        if state
            .interfaces
            .iter()
            .any(|interface| interface.name == new_name)
        {
            return Err(invalid(format!("interface `{new_name}` already exists")));
        }
        let target = DemoNetwork::get(&mut state, name)?;
        target.name = new_name.to_string();
        for route in &mut state.routes {
            if route.interface == name {
                route.interface = new_name.to_string();
            }
        }
        Ok(format!("renamed `{name}` to `{new_name}`"))
    }

    fn delete(&self, name: &str, peer: Option<&str>) -> Result<String, WireError> {
        if !self.manage {
            return Err(disabled_manage());
        }
        validate_name(name)?;
        let mut state = self.state.lock().expect("demo network lock");
        let (loopback, physical, default, routes) = {
            let target = state
                .interfaces
                .iter()
                .find(|interface| interface.name == name)
                .ok_or_else(|| not_found(name))?;
            (
                target.loopback,
                target.physical,
                state.routes.iter().any(|route| route.interface == name),
                state.routes.len(),
            )
        };
        if loopback {
            return Err(protected("refusing to delete a loopback interface"));
        }
        if physical {
            return Err(protected(format!(
                "refusing to delete physical interface `{name}`"
            )));
        }
        if default && routes <= 1 {
            return Err(protected(format!(
                "refusing to delete `{name}`: it is the last interface carrying the default route"
            )));
        }
        if peer == Some(name) {
            return Err(protected(format!(
                "refusing to delete `{name}`: this request arrived through it, so the panel would lose its own connection"
            )));
        }
        state.interfaces.retain(|interface| interface.name != name);
        state.routes.retain(|route| route.interface != name);
        Ok(format!("deleted interface `{name}`"))
    }
}

fn seed() -> DemoState {
    let interface = |name: &str,
                     index: u32,
                     kind: &str,
                     mac: &str,
                     up: bool,
                     loopback: bool,
                     addresses: Vec<NetworkAddress>| NetworkInterface {
        name: name.to_string(),
        index,
        kind: kind.to_string(),
        mac: mac.to_string(),
        mtu: if loopback { 65536 } else { 1500 },
        state: if up { "up" } else { "down" }.to_string(),
        up,
        running: up,
        loopback,
        physical: kind == "ether" || kind == "wifi",
        addresses,
        rx_bytes: 12_345_678,
        tx_bytes: 7_654_321,
        rx_packets: 9_001,
        tx_packets: 8_002,
        rx_errors: 0,
        tx_errors: 0,
        rx_dropped: 2,
        tx_dropped: 0,
        speed_mbps: Some(1000),
    };
    DemoState {
        interfaces: vec![
            interface(
                "lo",
                1,
                "loopback",
                "",
                true,
                true,
                vec![
                    NetworkAddress {
                        family: "inet".to_string(),
                        address: "127.0.0.1".to_string(),
                        prefix: 8,
                        scope: "host".to_string(),
                    },
                    NetworkAddress {
                        family: "inet6".to_string(),
                        address: "::1".to_string(),
                        prefix: 128,
                        scope: "host".to_string(),
                    },
                ],
            ),
            interface(
                "eth0",
                2,
                "ether",
                "02:00:00:00:00:02",
                true,
                false,
                vec![NetworkAddress {
                    family: "inet".to_string(),
                    address: "192.0.2.10".to_string(),
                    prefix: 24,
                    scope: "global".to_string(),
                }],
            ),
            interface(
                "wlan0",
                3,
                "wifi",
                "02:00:00:00:00:03",
                false,
                false,
                vec![NetworkAddress {
                    family: "inet".to_string(),
                    address: "198.51.100.20".to_string(),
                    prefix: 24,
                    scope: "global".to_string(),
                }],
            ),
        ],
        routes: vec![NetworkRoute {
            family: "inet".to_string(),
            gateway: "192.0.2.1".to_string(),
            interface: "eth0".to_string(),
        }],
        next_index: 4,
    }
}

fn disabled_control() -> WireError {
    WireError::new(
        ErrorKind::Forbidden,
        "network_control_disabled",
        "network control is disabled; set `[network] control = true`",
    )
}

fn disabled_manage() -> WireError {
    WireError::new(
        ErrorKind::Forbidden,
        "network_manage_disabled",
        "network management is disabled; set `[network] manage = true`",
    )
}

fn invalid(message: impl Into<String>) -> WireError {
    WireError::new(ErrorKind::Invalid, "invalid_interface", message)
}

fn not_found(name: &str) -> WireError {
    WireError::new(
        ErrorKind::NotFound,
        "interface_not_found",
        format!("interface `{name}` does not exist"),
    )
}

fn protected(message: impl Into<String>) -> WireError {
    WireError::new(ErrorKind::Forbidden, "protected_interface", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutations_edit_the_in_memory_snapshot() {
        let demo = DemoNetwork::new(true, true);
        assert_eq!(demo.snapshot().interfaces.len(), 3);

        demo.set_state("eth0", false).expect("down");
        demo.create(&NetworkCreate {
            name: "demo0".to_string(),
            kind: "dummy".to_string(),
            peer: None,
            link: None,
            vlan_id: None,
            mtu: Some(9000),
            up: true,
        })
        .expect("create");
        let snapshot = demo.snapshot();
        assert!(
            !snapshot
                .interfaces
                .iter()
                .find(|i| i.name == "eth0")
                .unwrap()
                .up
        );
        let created = snapshot
            .interfaces
            .iter()
            .find(|i| i.name == "demo0")
            .unwrap();
        assert_eq!(created.mtu, 9000);
        assert!(created.up);

        demo.rename("wlan0", "wifi0").expect("rename");
        demo.delete("demo0", None).expect("delete");
        assert!(!demo.snapshot().interfaces.iter().any(|i| i.name == "demo0"));
    }

    #[test]
    fn guards_refuse_loopback_physical_and_the_last_default_route() {
        let demo = DemoNetwork::new(true, true);
        assert_eq!(
            demo.delete("lo", None).expect_err("loopback").code,
            "protected_interface"
        );
        assert_eq!(
            demo.delete("eth0", None).expect_err("default route").code,
            "protected_interface"
        );
        assert_eq!(
            demo.set_state("lo", false).expect_err("loopback down").code,
            "protected_interface"
        );
    }

    #[test]
    fn policy_switches_refuse_changes() {
        let demo = DemoNetwork::new(false, false);
        assert_eq!(
            demo.set_state("eth0", false).expect_err("control").code,
            "network_control_disabled"
        );
        assert_eq!(
            demo.delete("wlan0", None).expect_err("manage").code,
            "network_manage_disabled"
        );
    }
}
