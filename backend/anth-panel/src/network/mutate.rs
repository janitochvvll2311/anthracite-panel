//! Guarded interface mutations: state, create, edit, rename and delete.
//!
//! Every mutation re-reads the live inventory before it checks a guard, so a
//! change cannot be applied to an interface that disappeared or came up since
//! the operator looked. Commands are always an argv vector for `ip`; nothing
//! goes through a shell and nothing is interpolated into a script.
//!
//! The delete guards are the important ones: a panel that removes the link it
//! is served over locks its own operator out. The daemon refuses a loopback, the
//! only interface carrying a default route, and the interface the request's
//! peer address is routed through.

use std::net::IpAddr;
use std::process::Command;

use anth_wire::{ErrorKind, NetworkCreate, NetworkEdit, NetworkSnapshot, WireError};

pub const MIN_MTU: u32 = 68;
pub const MAX_MTU: u32 = 65535;
const MAX_NAME_LEN: usize = 15;

/// One `ip` invocation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IpError {
    /// The program is missing or could not be executed.
    Unavailable(String),
    /// `ip` ran and refused, usually with a message on stderr.
    Failed(String),
}

/// Runs `ip` with an argument vector. Tests implement this trait to record the
/// exact argv without touching the host.
pub trait IpRunner: Send + Sync {
    fn run(&self, args: &[String]) -> Result<(), IpError>;
}

/// The real `ip` binary, invoked directly with the daemon's privileges.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemIp;

impl IpRunner for SystemIp {
    fn run(&self, args: &[String]) -> Result<(), IpError> {
        let output = Command::new("ip")
            .args(args)
            .output()
            .map_err(|error| IpError::Unavailable(error.to_string()))?;
        if output.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("ip exited with {}", output.status)
        };
        Err(IpError::Failed(detail))
    }
}

/// One interface as the guards need to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceState {
    pub name: String,
    pub up: bool,
    pub kind: String,
    pub loopback: bool,
    pub physical: bool,
    pub addresses: Vec<(IpAddr, u8)>,
    pub default_route: bool,
}

/// The snapshot reduced to the facts a guard reads.
#[derive(Debug, Default, Clone)]
pub struct NetworkState {
    pub interfaces: Vec<InterfaceState>,
}

impl NetworkState {
    pub fn from_snapshot(snapshot: &NetworkSnapshot) -> Self {
        let default_routes: Vec<&str> = snapshot
            .routes
            .iter()
            .map(|route| route.interface.as_str())
            .collect();
        Self {
            interfaces: snapshot
                .interfaces
                .iter()
                .map(|interface| InterfaceState {
                    name: interface.name.clone(),
                    up: interface.up,
                    kind: interface.kind.clone(),
                    loopback: interface.loopback,
                    physical: interface.physical,
                    default_route: default_routes.contains(&interface.name.as_str()),
                    addresses: interface
                        .addresses
                        .iter()
                        .filter_map(|address| {
                            Some((address.address.parse::<IpAddr>().ok()?, address.prefix))
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&InterfaceState> {
        self.interfaces
            .iter()
            .find(|interface| interface.name == name)
    }

    fn with_default_route(&self) -> Vec<&InterfaceState> {
        self.interfaces
            .iter()
            .filter(|interface| interface.default_route)
            .collect()
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

/// Interface names reach `ip` as one argv element, so the only risk is a name
/// the kernel rejects. These mirror the kernel's limits: at most 15 bytes, no
/// slash, whitespace or colon.
pub fn validate_name(name: &str) -> Result<(), WireError> {
    if name.is_empty() {
        return Err(invalid("an interface name cannot be empty"));
    }
    if name.len() > MAX_NAME_LEN {
        return Err(invalid(format!(
            "`{name}` is longer than {MAX_NAME_LEN} bytes"
        )));
    }
    if name == "." || name == ".." {
        return Err(invalid(format!("`{name}` is not a usable interface name")));
    }
    if name
        .chars()
        .any(|character| character.is_whitespace() || matches!(character, '/' | ':' | '\0'))
    {
        return Err(invalid(format!(
            "`{name}` contains a character the kernel does not allow in a name"
        )));
    }
    Ok(())
}

pub fn validate_mtu(mtu: u32) -> Result<(), WireError> {
    if !(MIN_MTU..=MAX_MTU).contains(&mtu) {
        return Err(invalid(format!(
            "MTU {mtu} is outside the supported range {MIN_MTU}-{MAX_MTU}"
        )));
    }
    Ok(())
}

/// Resolves which interface a peer address is routed through.
///
/// Returns `None` when the address says nothing about the path: a loopback peer
/// means the client is on this host, and an address matching no interface is a
/// proxy or a NAT hop. Both skip the guard rather than guess.
pub fn interface_for_peer(state: &NetworkState, peer: &str) -> Option<String> {
    let address = peer.parse::<IpAddr>().ok()?;
    if address.is_loopback() || address.is_unspecified() {
        return None;
    }

    let mut best: Option<(u8, String)> = None;
    for interface in &state.interfaces {
        for (network, prefix) in &interface.addresses {
            if std::mem::discriminant(network) != std::mem::discriminant(&address) {
                continue;
            }
            let bits = common_prefix_bits(*network, address);
            if bits >= *prefix && best.as_ref().is_none_or(|(longest, _)| bits > *longest) {
                best = Some((bits, interface.name.clone()));
            }
        }
    }
    best.map(|(_, name)| name)
}

fn common_prefix_bits(left: IpAddr, right: IpAddr) -> u8 {
    match (left, right) {
        (IpAddr::V4(left), IpAddr::V4(right)) => {
            (u32::from(left) ^ u32::from(right)).leading_zeros() as u8
        }
        (IpAddr::V6(left), IpAddr::V6(right)) => {
            (u128::from(left) ^ u128::from(right)).leading_zeros() as u8
        }
        _ => 0,
    }
}

fn check_delete_guards(
    state: &NetworkState,
    name: &str,
    peer: Option<&str>,
) -> Result<(), WireError> {
    let target = state.get(name).ok_or_else(|| not_found(name))?;

    if target.loopback {
        return Err(protected("refusing to delete a loopback interface"));
    }

    if let Some(peer) = peer
        && let Some(carrier) = interface_for_peer(state, peer)
        && carrier == name
    {
        return Err(protected(format!(
            "refusing to delete `{name}`: this request arrived through it, so the panel would lose its own connection"
        )));
    }

    if target.default_route && state.with_default_route().len() <= 1 {
        return Err(protected(format!(
            "refusing to delete `{name}`: it is the last interface carrying the default route"
        )));
    }

    Ok(())
}

pub fn set_state(
    control: bool,
    runner: &dyn IpRunner,
    state: &NetworkState,
    name: &str,
    up: bool,
) -> Result<String, WireError> {
    if !control {
        return Err(disabled_control());
    }
    validate_name(name)?;
    let target = state.get(name).ok_or_else(|| not_found(name))?;
    if !up && target.loopback {
        return Err(protected("refusing to bring a loopback interface down"));
    }
    run(
        runner,
        &["link", "set", "dev", name, if up { "up" } else { "down" }],
    )?;
    Ok(format!("`{name}` is {}", if up { "up" } else { "down" }))
}

pub fn create(
    manage: bool,
    runner: &dyn IpRunner,
    state: &NetworkState,
    args: &NetworkCreate,
) -> Result<String, WireError> {
    if !manage {
        return Err(disabled_manage());
    }
    validate_name(&args.name)?;
    if state.get(&args.name).is_some() {
        return Err(invalid(format!("interface `{}` already exists", args.name)));
    }
    if let Some(mtu) = args.mtu {
        validate_mtu(mtu)?;
    }

    let argv = create_argv(state, args)?;
    let argv_refs: Vec<&str> = argv.iter().map(String::as_str).collect();
    run(runner, &argv_refs)?;

    if let Some(mtu) = args.mtu {
        let mtu = mtu.to_string();
        run(runner, &["link", "set", "dev", &args.name, "mtu", &mtu])
            .map_err(|error| created_but_failed(&args.name, error))?;
    }
    if args.up {
        run(runner, &["link", "set", "dev", &args.name, "up"])
            .map_err(|error| created_but_failed(&args.name, error))?;
    }

    Ok(format!(
        "created {} interface `{}`",
        kind_label(&args.kind),
        args.name
    ))
}

fn created_but_failed(name: &str, error: WireError) -> WireError {
    WireError::new(
        error.kind,
        error.code.clone(),
        format!(
            "interface `{name}` may have been created, but {}",
            error.message
        ),
    )
}

/// Builds the create argv. Every caller-controlled value was validated above
/// and stays its own argument.
pub fn create_argv(state: &NetworkState, args: &NetworkCreate) -> Result<Vec<String>, WireError> {
    let name = args.name.clone();
    match args.kind.as_str() {
        "veth" => {
            let peer = args
                .peer
                .as_deref()
                .ok_or_else(|| invalid("a veth needs a peer interface name"))?;
            validate_name(peer)?;
            if peer == name {
                return Err(invalid("a veth peer cannot have the same name"));
            }
            if state.get(peer).is_some() {
                return Err(invalid(format!("interface `{peer}` already exists")));
            }
            Ok(vec![
                "link".into(),
                "add".into(),
                name,
                "type".into(),
                "veth".into(),
                "peer".into(),
                "name".into(),
                peer.into(),
            ])
        }
        "bridge" => Ok(vec![
            "link".into(),
            "add".into(),
            name,
            "type".into(),
            "bridge".into(),
        ]),
        "bond" => Ok(vec![
            "link".into(),
            "add".into(),
            name,
            "type".into(),
            "bond".into(),
            "mode".into(),
            "active-backup".into(),
        ]),
        "dummy" => Ok(vec![
            "link".into(),
            "add".into(),
            name,
            "type".into(),
            "dummy".into(),
        ]),
        "vlan" => {
            let parent = args
                .link
                .as_deref()
                .ok_or_else(|| invalid("a vlan needs a parent interface"))?;
            validate_name(parent)?;
            if state.get(parent).is_none() {
                return Err(not_found(parent));
            }
            let vlan_id = args
                .vlan_id
                .ok_or_else(|| invalid("a vlan needs a VLAN identifier"))?;
            if !(1..=4094).contains(&vlan_id) {
                return Err(invalid(format!(
                    "VLAN identifier {vlan_id} is outside 1-4094"
                )));
            }
            Ok(vec![
                "link".into(),
                "add".into(),
                "link".into(),
                parent.into(),
                "name".into(),
                name,
                "type".into(),
                "vlan".into(),
                "id".into(),
                vlan_id.to_string(),
            ])
        }
        "tun" => Ok(vec![
            "tuntap".into(),
            "add".into(),
            "dev".into(),
            name,
            "mode".into(),
            "tap".into(),
        ]),
        _ => Err(invalid(format!(
            "`{}` is not a kind the panel can create",
            args.kind
        ))),
    }
}

fn kind_label(kind: &str) -> &str {
    if kind == "tun" { "tun/tap" } else { kind }
}

pub fn edit(
    control: bool,
    manage: bool,
    runner: &dyn IpRunner,
    state: &NetworkState,
    args: &NetworkEdit,
) -> Result<String, WireError> {
    if !manage {
        return Err(disabled_manage());
    }
    if args.up.is_some() && !control {
        return Err(disabled_control());
    }
    validate_name(&args.name)?;
    let target = state.get(&args.name).ok_or_else(|| not_found(&args.name))?;
    if args.mtu.is_none() && args.up.is_none() {
        return Err(invalid("nothing to change"));
    }

    let mut changes = Vec::new();
    if let Some(mtu) = args.mtu {
        validate_mtu(mtu)?;
        let mtu = mtu.to_string();
        run(runner, &["link", "set", "dev", &args.name, "mtu", &mtu])?;
        changes.push(format!("mtu {mtu}"));
    }
    if let Some(up) = args.up {
        if !up && target.loopback {
            return Err(protected("refusing to bring a loopback interface down"));
        }
        run(
            runner,
            &[
                "link",
                "set",
                "dev",
                &args.name,
                if up { "up" } else { "down" },
            ],
        )?;
        changes.push(if up { "up" } else { "down" }.to_string());
    }

    Ok(format!("updated `{}`: {}", args.name, changes.join(", ")))
}

pub fn rename(
    manage: bool,
    runner: &dyn IpRunner,
    state: &NetworkState,
    name: &str,
    new_name: &str,
) -> Result<String, WireError> {
    if !manage {
        return Err(disabled_manage());
    }
    validate_name(name)?;
    validate_name(new_name)?;
    let target = state.get(name).ok_or_else(|| not_found(name))?;
    if target.up {
        return Err(WireError::new(
            ErrorKind::Conflict,
            "interface_up",
            format!("refusing to rename `{name}` while it is up; bring it down first"),
        ));
    }
    if state.get(new_name).is_some() {
        return Err(invalid(format!("interface `{new_name}` already exists")));
    }

    run(runner, &["link", "set", "dev", name, "name", new_name])?;
    Ok(format!("renamed `{name}` to `{new_name}`"))
}

pub fn delete(
    manage: bool,
    runner: &dyn IpRunner,
    state: &NetworkState,
    name: &str,
    peer: Option<&str>,
) -> Result<String, WireError> {
    if !manage {
        return Err(disabled_manage());
    }
    validate_name(name)?;
    check_delete_guards(state, name, peer)?;
    run(runner, &["link", "delete", "dev", name])?;
    Ok(format!("deleted interface `{name}`"))
}

fn run(runner: &dyn IpRunner, args: &[&str]) -> Result<(), WireError> {
    let argv: Vec<String> = args.iter().map(|value| (*value).to_string()).collect();
    runner.run(&argv).map_err(classify)
}

/// `ip` reports every refusal on stderr, so the message is the only signal.
fn classify(error: IpError) -> WireError {
    match error {
        IpError::Unavailable(detail) => WireError::new(
            ErrorKind::Internal,
            "ip_unavailable",
            format!("`ip` is not installed or could not be executed: {detail}"),
        ),
        IpError::Failed(stderr) => {
            let lowercase = stderr.to_lowercase();
            let (kind, code) = if lowercase.contains("operation not permitted")
                || lowercase.contains("permission denied")
                || lowercase.contains("must be root")
            {
                (ErrorKind::Forbidden, "ip_permission")
            } else if lowercase.contains("usage:") || lowercase.contains("multi-call binary") {
                (ErrorKind::Unsupported, "ip_unsupported")
            } else if lowercase.contains("open: no such file") {
                (ErrorKind::Unsupported, "tun_unavailable")
            } else if lowercase.contains("cannot find device")
                || lowercase.contains("does not exist")
            {
                (ErrorKind::NotFound, "interface_not_found")
            } else if lowercase.contains("file exists") || lowercase.contains("already exists") {
                (ErrorKind::Conflict, "interface_exists")
            } else if lowercase.contains("invalid")
                || lowercase.contains("argument")
                || lowercase.contains("not supported")
            {
                (ErrorKind::Invalid, "ip_refused")
            } else {
                (ErrorKind::Internal, "ip_failed")
            };
            let detail = if stderr.is_empty() {
                "`ip` refused the change without a message".to_string()
            } else if code == "ip_unsupported" {
                "this `ip` does not support the command; install iproute2".to_string()
            } else if code == "tun_unavailable" {
                "`ip tuntap` could not open /dev/net/tun; expose the device to the container"
                    .to_string()
            } else if code == "ip_permission" {
                format!("network changes need CAP_NET_ADMIN; `ip` said: {stderr}")
            } else {
                format!("`ip` refused the change: {stderr}")
            };
            WireError::new(kind, code, detail)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use anth_wire::{NetworkAddress, NetworkInterface, NetworkRoute};

    #[derive(Default)]
    struct Recorder {
        calls: Mutex<Vec<Vec<String>>>,
        failure: Option<IpError>,
    }

    impl Recorder {
        fn failing(stderr: &str) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                failure: Some(IpError::Failed(stderr.to_string())),
            }
        }

        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().expect("lock").clone()
        }

        fn argv(calls: &[Vec<String>]) -> Vec<Vec<&str>> {
            calls
                .iter()
                .map(|call| call.iter().map(String::as_str).collect())
                .collect()
        }
    }

    impl IpRunner for Recorder {
        fn run(&self, args: &[String]) -> Result<(), IpError> {
            self.calls.lock().expect("lock").push(args.to_vec());
            match &self.failure {
                Some(error) => Err(error.clone()),
                None => Ok(()),
            }
        }
    }

    fn iface(name: &str, up: bool) -> NetworkInterface {
        NetworkInterface {
            name: name.to_string(),
            index: 1,
            kind: "ether".to_string(),
            mac: String::new(),
            mtu: 1500,
            state: if up { "up" } else { "down" }.to_string(),
            up,
            running: up,
            loopback: name == "lo",
            physical: true,
            addresses: vec![NetworkAddress {
                family: "inet".to_string(),
                address: "192.168.1.10".to_string(),
                prefix: 24,
                scope: "global".to_string(),
            }],
            rx_bytes: 0,
            tx_bytes: 0,
            rx_packets: 0,
            tx_packets: 0,
            rx_errors: 0,
            tx_errors: 0,
            rx_dropped: 0,
            tx_dropped: 0,
            speed_mbps: None,
        }
    }

    fn create_args(name: &str, kind: &str) -> NetworkCreate {
        NetworkCreate {
            name: name.to_string(),
            kind: kind.to_string(),
            peer: None,
            link: None,
            vlan_id: None,
            mtu: None,
            up: false,
        }
    }

    fn state(interfaces: Vec<NetworkInterface>, routes: Vec<&str>) -> NetworkState {
        NetworkState::from_snapshot(&NetworkSnapshot {
            interfaces,
            routes: routes
                .into_iter()
                .map(|name| NetworkRoute {
                    family: "inet".to_string(),
                    gateway: "192.168.1.1".to_string(),
                    interface: name.to_string(),
                })
                .collect(),
            timestamp_ms: 1,
            control_enabled: true,
            manage_enabled: true,
        })
    }

    #[test]
    fn state_changes_are_argv_vectors_and_loopback_cannot_go_down() {
        let recorder = Recorder::default();
        let snapshot = state(vec![iface("eth0", true), iface("lo", true)], vec!["eth0"]);

        assert_eq!(
            set_state(true, &recorder, &snapshot, "eth0", false).expect("down"),
            "`eth0` is down"
        );
        assert_eq!(
            Recorder::argv(&recorder.calls()),
            vec![vec!["link", "set", "dev", "eth0", "down"]]
        );

        let refused = set_state(true, &recorder, &snapshot, "lo", false).expect_err("loopback");
        assert_eq!(refused.kind, ErrorKind::Forbidden);
        assert_eq!(refused.code, "protected_interface");
        assert_eq!(recorder.calls().len(), 1);

        let refused = set_state(false, &recorder, &snapshot, "eth0", true).expect_err("disabled");
        assert_eq!(refused.code, "network_control_disabled");
    }

    #[test]
    fn names_and_mtus_are_validated_before_ip_runs() {
        let recorder = Recorder::default();
        let snapshot = state(vec![iface("eth0", true)], vec![]);

        for name in [
            "",
            "waytoolonginterfacename",
            "bad name",
            "bad/name",
            "bad:name",
        ] {
            assert!(validate_name(name).is_err(), "`{name}` must be refused");
        }
        assert!(validate_name("br0.100").is_ok());
        assert_eq!(validate_mtu(67).expect_err("low").code, "invalid_interface");
        assert_eq!(
            validate_mtu(65536).expect_err("high").code,
            "invalid_interface"
        );
        assert!(validate_mtu(1500).is_ok());
        assert!(recorder.calls().is_empty());
        assert!(snapshot.get("eth0").is_some());
    }

    #[test]
    fn create_builds_the_exact_argv_for_each_kind() {
        let snapshot = state(vec![iface("eth0", true)], vec![]);

        let bridge = create_argv(&snapshot, &create_args("br0", "bridge")).expect("bridge");
        assert_eq!(bridge, vec!["link", "add", "br0", "type", "bridge"]);

        let veth = create_argv(
            &snapshot,
            &NetworkCreate {
                peer: Some("v1".to_string()),
                ..create_args("v0", "veth")
            },
        )
        .expect("veth");
        assert_eq!(
            veth,
            vec!["link", "add", "v0", "type", "veth", "peer", "name", "v1"]
        );

        let vlan = create_argv(
            &snapshot,
            &NetworkCreate {
                link: Some("eth0".to_string()),
                vlan_id: Some(20),
                ..create_args("eth0.20", "vlan")
            },
        )
        .expect("vlan");
        assert_eq!(
            vlan,
            vec![
                "link", "add", "link", "eth0", "name", "eth0.20", "type", "vlan", "id", "20"
            ]
        );

        assert!(create_argv(&snapshot, &create_args("x", "veth")).is_err());
        assert!(
            create_argv(
                &snapshot,
                &NetworkCreate {
                    link: Some("missing".to_string()),
                    vlan_id: Some(1),
                    ..create_args("x", "vlan")
                },
            )
            .is_err()
        );
    }

    #[test]
    fn create_runs_optional_mtu_and_up_after_the_link() {
        let recorder = Recorder::default();
        let snapshot = state(vec![], vec![]);
        let args = NetworkCreate {
            mtu: Some(1400),
            up: true,
            ..create_args("br0", "bridge")
        };
        assert_eq!(
            create(true, &recorder, &snapshot, &args).expect("create"),
            "created bridge interface `br0`"
        );
        assert_eq!(
            Recorder::argv(&recorder.calls()),
            vec![
                vec!["link", "add", "br0", "type", "bridge"],
                vec!["link", "set", "dev", "br0", "mtu", "1400"],
                vec!["link", "set", "dev", "br0", "up"],
            ]
        );
    }

    #[test]
    fn edit_changes_only_what_the_caller_named() {
        let recorder = Recorder::default();
        let snapshot = state(vec![iface("eth0", true), iface("lo", true)], vec![]);

        edit(
            true,
            true,
            &recorder,
            &snapshot,
            &NetworkEdit {
                name: "eth0".to_string(),
                mtu: Some(1400),
                up: None,
            },
        )
        .expect("mtu");
        edit(
            true,
            true,
            &recorder,
            &snapshot,
            &NetworkEdit {
                name: "eth0".to_string(),
                mtu: None,
                up: Some(false),
            },
        )
        .expect("down");
        assert_eq!(
            Recorder::argv(&recorder.calls()),
            vec![
                vec!["link", "set", "dev", "eth0", "mtu", "1400"],
                vec!["link", "set", "dev", "eth0", "down"],
            ]
        );

        let nothing = edit(
            true,
            true,
            &recorder,
            &snapshot,
            &NetworkEdit {
                name: "eth0".to_string(),
                mtu: None,
                up: None,
            },
        )
        .expect_err("nothing");
        assert_eq!(nothing.code, "invalid_interface");

        let loopback = edit(
            true,
            true,
            &recorder,
            &snapshot,
            &NetworkEdit {
                name: "lo".to_string(),
                mtu: None,
                up: Some(false),
            },
        )
        .expect_err("loopback");
        assert_eq!(loopback.code, "protected_interface");

        let no_control = edit(
            false,
            true,
            &recorder,
            &snapshot,
            &NetworkEdit {
                name: "eth0".to_string(),
                mtu: None,
                up: Some(false),
            },
        )
        .expect_err("control");
        assert_eq!(no_control.code, "network_control_disabled");

        let no_manage = edit(
            true,
            false,
            &recorder,
            &snapshot,
            &NetworkEdit {
                name: "eth0".to_string(),
                mtu: Some(1400),
                up: None,
            },
        )
        .expect_err("manage");
        assert_eq!(no_manage.code, "network_manage_disabled");
    }

    #[test]
    fn rename_needs_a_down_interface_and_a_free_target() {
        let recorder = Recorder::default();
        let snapshot = state(vec![iface("eth0", true), iface("eth1", false)], vec![]);

        let up = rename(true, &recorder, &snapshot, "eth0", "lan0").expect_err("up");
        assert_eq!(up.kind, ErrorKind::Conflict);
        assert_eq!(up.code, "interface_up");

        rename(true, &recorder, &snapshot, "eth1", "lan1").expect("rename");
        assert_eq!(
            Recorder::argv(&recorder.calls()),
            vec![vec!["link", "set", "dev", "eth1", "name", "lan1"]]
        );

        let exists = rename(true, &recorder, &snapshot, "eth1", "eth0").expect_err("exists");
        assert_eq!(exists.code, "invalid_interface");
    }

    #[test]
    fn delete_guards_protect_loopback_peer_and_last_default_route() {
        let recorder = Recorder::default();
        let snapshot = state(
            vec![iface("eth0", true), iface("eth1", true), iface("lo", true)],
            vec!["eth0"],
        );

        assert_eq!(
            delete(true, &recorder, &snapshot, "eth0", None)
                .expect_err("last default route")
                .code,
            "protected_interface"
        );
        assert_eq!(
            delete(true, &recorder, &snapshot, "lo", None)
                .expect_err("loopback")
                .code,
            "protected_interface"
        );
        assert_eq!(
            delete(true, &recorder, &snapshot, "eth0", Some("192.168.1.50"))
                .expect_err("peer")
                .code,
            "protected_interface"
        );
        assert_eq!(
            delete(true, &recorder, &snapshot, "eth1", Some("192.168.1.50")).expect("unrelated"),
            "deleted interface `eth1`"
        );
        assert_eq!(
            Recorder::argv(&recorder.calls()),
            vec![vec!["link", "delete", "dev", "eth1"]]
        );
    }

    #[test]
    fn peer_resolution_skips_loopback_and_unknown_paths() {
        let snapshot = state(vec![iface("eth0", true), iface("lo", true)], vec![]);
        assert_eq!(
            interface_for_peer(&snapshot, "192.168.1.50"),
            Some("eth0".to_string())
        );
        assert_eq!(interface_for_peer(&snapshot, "127.0.0.1"), None);
        assert_eq!(interface_for_peer(&snapshot, "10.9.9.9"), None);
        assert_eq!(interface_for_peer(&snapshot, "not-an-ip"), None);
    }

    #[test]
    fn ip_failures_are_classified() {
        let snapshot = state(vec![iface("eth0", true)], vec![]);
        let permission = Recorder::failing("RTNETLINK answers: Operation not permitted");
        let error = set_state(true, &permission, &snapshot, "eth0", true).expect_err("permission");
        assert_eq!(error.kind, ErrorKind::Forbidden);
        assert_eq!(error.code, "ip_permission");

        let missing = Recorder::failing("Cannot find device \"eth9\"");
        let error = set_state(true, &missing, &snapshot, "eth9", true).expect_err("missing");
        assert_eq!(error.kind, ErrorKind::NotFound);

        let unavailable = Recorder {
            calls: Mutex::new(Vec::new()),
            failure: Some(IpError::Unavailable("not found".to_string())),
        };
        let error =
            set_state(true, &unavailable, &snapshot, "eth0", true).expect_err("unavailable");
        assert_eq!(error.kind, ErrorKind::Internal);
        assert_eq!(error.code, "ip_unavailable");

        let busybox = Recorder::failing(
            "BusyBox v1.37.0 multi-call binary.\nUsage: ip [OPTIONS] address|route|link",
        );
        let error = set_state(true, &busybox, &snapshot, "eth0", true).expect_err("unsupported");
        assert_eq!(error.kind, ErrorKind::Unsupported);
        assert_eq!(error.code, "ip_unsupported");
        assert!(error.message.contains("install iproute2"));

        let no_tun = Recorder::failing("open: No such file or directory");
        let error = set_state(true, &no_tun, &snapshot, "eth0", true).expect_err("tun");
        assert_eq!(error.kind, ErrorKind::Unsupported);
        assert_eq!(error.code, "tun_unavailable");
        assert!(error.message.contains("/dev/net/tun"));
    }
}
