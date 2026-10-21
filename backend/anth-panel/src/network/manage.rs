//! Interface creation, editing, renaming and deletion.
//!
//! Every mutation validates against the same guards before it runs a command:
//! the interface exists (or does not, for create), the name is legal, and the
//! change cannot cut the connection the operator is using. The last one is why
//! this module is more than a thin `ip` wrapper: a panel that can delete the
//! link it is served over locks its own operator out.
//!
//! Commands run through [`CommandRunner`] with an argument vector, never a
//! shell. Tests script the runner rather than touching the host.

use std::net::IpAddr;

use anth_proto::{
    NetworkCreateArgs, NetworkEditArgs, NetworkError, NetworkErrorKind, NetworkInterfaceKind,
    NetworkSnapshot,
};

use crate::command::{CommandError, CommandOutput, CommandRunner};

const MIN_MTU: u32 = 68;
const MAX_MTU: u32 = 65535;
const MAX_NAME_LEN: usize = 15;

/// One interface as the guards need to see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceState {
    pub name: String,
    pub up: bool,
    pub kind: NetworkInterfaceKind,
    pub physical: bool,
    /// The interface carries the default route.
    pub default_route: bool,
    pub addresses: Vec<(IpAddr, u8)>,
}

/// The decisions the guards need, in a form a test can build without sysfs.
#[derive(Debug, Default, Clone)]
pub struct NetworkState {
    pub interfaces: Vec<InterfaceState>,
}

impl NetworkState {
    pub fn from_snapshot(snapshot: &NetworkSnapshot, default_routes: &[String]) -> Self {
        Self {
            interfaces: snapshot
                .interfaces
                .iter()
                .map(|interface| InterfaceState {
                    name: interface.name.clone(),
                    up: interface.up,
                    kind: interface.kind,
                    physical: interface.physical,
                    default_route: default_routes.iter().any(|name| name == &interface.name),
                    addresses: interface
                        .addresses
                        .iter()
                        .filter_map(|address| {
                            let parsed = address.address.parse::<IpAddr>().ok()?;
                            Some((parsed, address.prefix_len))
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    fn get(&self, name: &str) -> Option<&InterfaceState> {
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

fn disabled() -> NetworkError {
    NetworkError::new(
        NetworkErrorKind::Disabled,
        "network management is disabled; set `[network] manage = true`",
    )
}

fn invalid(detail: impl Into<String>) -> NetworkError {
    NetworkError::new(NetworkErrorKind::Invalid, detail)
}

fn protected(detail: impl Into<String>) -> NetworkError {
    NetworkError::new(NetworkErrorKind::Protected, detail)
}

fn not_found(name: &str) -> NetworkError {
    NetworkError::new(
        NetworkErrorKind::NotFound,
        format!("interface `{name}` does not exist"),
    )
}

/// Interface names reach `ip` as one argv element, so the only risk is a name
/// the kernel rejects. The rules mirror the kernel's: at most 15 bytes, no
/// slash, no whitespace, no `:`.
fn validate_name(name: &str) -> Result<(), NetworkError> {
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
        .any(|c| c.is_whitespace() || c == '/' || c == '\0' || c == ':')
    {
        return Err(invalid(format!(
            "`{name}` contains a character the kernel does not allow in a name"
        )));
    }
    Ok(())
}

fn validate_mtu(mtu: u32) -> Result<(), NetworkError> {
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
/// proxy or a NAT hop. Both skip the guard rather than guess, because a wrong
/// guess either blocks a legitimate delete or claims a protection it does not
/// provide.
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
        (IpAddr::V4(a), IpAddr::V4(b)) => (u32::from(a) ^ u32::from(b)).leading_zeros() as u8,
        (IpAddr::V6(a), IpAddr::V6(b)) => (u128::from(a) ^ u128::from(b)).leading_zeros() as u8,
        _ => 0,
    }
}

/// Refuses a delete that would take the panel's own connection away.
fn check_delete_guards(
    state: &NetworkState,
    name: &str,
    peer: Option<&str>,
) -> Result<(), NetworkError> {
    let target = state.get(name).ok_or_else(|| not_found(name))?;

    if target.kind == NetworkInterfaceKind::Loopback {
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

/// Creates one virtual interface.
pub fn create(
    manage_enabled: bool,
    state: &NetworkState,
    runner: &dyn CommandRunner,
    args: &NetworkCreateArgs,
) -> Result<String, NetworkError> {
    if !manage_enabled {
        return Err(disabled());
    }
    validate_name(&args.name)?;
    if state.get(&args.name).is_some() {
        return Err(invalid(format!("interface `{}` already exists", args.name)));
    }
    if let Some(mtu) = args.mtu {
        validate_mtu(mtu)?;
    }

    let argv = create_command(state, args)?;
    let argv = argv.iter().map(String::as_str).collect::<Vec<_>>();
    run(runner, "ip", &argv, "create")?;

    if let Some(mtu) = args.mtu {
        let mtu_text = mtu.to_string();
        run(
            runner,
            "ip",
            &["link", "set", "dev", &args.name, "mtu", &mtu_text],
            "create",
        )?;
    }
    if args.up {
        run(
            runner,
            "ip",
            &["link", "set", "dev", &args.name, "up"],
            "create",
        )?;
    }

    Ok(format!(
        "created {} interface `{}`",
        kind_label(args.kind),
        args.name
    ))
}

/// Builds the argv for the create. Everything the caller controls is validated
/// before it gets here, and each value stays a separate element.
///
/// The vector is owned because the VLAN identifier is rendered from a number:
/// borrowing it would either truncate the argv or require the caller to keep a
/// string alive, which is exactly how `id` once lost its value.
fn create_command(
    state: &NetworkState,
    args: &NetworkCreateArgs,
) -> Result<Vec<String>, NetworkError> {
    match args.kind {
        NetworkInterfaceKind::Veth => {
            let peer = args
                .peer
                .as_deref()
                .ok_or_else(|| invalid("a veth needs a peer interface name"))?;
            validate_name(peer)?;
            if state.get(peer).is_some() {
                return Err(invalid(format!("interface `{peer}` already exists")));
            }
            Ok(vec![
                "link".into(),
                "add".into(),
                args.name.clone(),
                "type".into(),
                "veth".into(),
                "peer".into(),
                "name".into(),
                peer.into(),
            ])
        }
        NetworkInterfaceKind::Bridge => Ok(vec![
            "link".into(),
            "add".into(),
            args.name.clone(),
            "type".into(),
            "bridge".into(),
        ]),
        NetworkInterfaceKind::Bond => Ok(vec![
            "link".into(),
            "add".into(),
            args.name.clone(),
            "type".into(),
            "bond".into(),
            "mode".into(),
            "active-backup".into(),
        ]),
        NetworkInterfaceKind::Dummy => Ok(vec![
            "link".into(),
            "add".into(),
            args.name.clone(),
            "type".into(),
            "dummy".into(),
        ]),
        NetworkInterfaceKind::Vlan => {
            let link = args
                .link
                .as_deref()
                .ok_or_else(|| invalid("a vlan needs a parent interface"))?;
            validate_name(link)?;
            if state.get(link).is_none() {
                return Err(not_found(link));
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
                link.into(),
                "name".into(),
                args.name.clone(),
                "type".into(),
                "vlan".into(),
                "id".into(),
                vlan_id.to_string(),
            ])
        }
        NetworkInterfaceKind::TunTap => Ok(vec![
            "tuntap".into(),
            "add".into(),
            "dev".into(),
            args.name.clone(),
            "mode".into(),
            "tap".into(),
        ]),
        NetworkInterfaceKind::Physical
        | NetworkInterfaceKind::Wireless
        | NetworkInterfaceKind::Loopback
        | NetworkInterfaceKind::Other => Err(invalid(
            "only veth, bridge, bond, vlan, dummy and tun/tap interfaces can be created from the panel",
        )),
    }
}

fn kind_label(kind: NetworkInterfaceKind) -> &'static str {
    match kind {
        NetworkInterfaceKind::Physical => "physical",
        NetworkInterfaceKind::Loopback => "loopback",
        NetworkInterfaceKind::Bridge => "bridge",
        NetworkInterfaceKind::Bond => "bond",
        NetworkInterfaceKind::Vlan => "vlan",
        NetworkInterfaceKind::Veth => "veth",
        NetworkInterfaceKind::Dummy => "dummy",
        NetworkInterfaceKind::TunTap => "tun/tap",
        NetworkInterfaceKind::Wireless => "wireless",
        NetworkInterfaceKind::Other => "unknown",
    }
}

/// Changes the MTU and/or the administrative state.
pub fn edit(
    manage_enabled: bool,
    state: &NetworkState,
    runner: &dyn CommandRunner,
    args: &NetworkEditArgs,
) -> Result<String, NetworkError> {
    if !manage_enabled {
        return Err(disabled());
    }
    validate_name(&args.name)?;
    let target = state.get(&args.name).ok_or_else(|| not_found(&args.name))?;
    if args.mtu.is_none() && args.up.is_none() {
        return Err(invalid("nothing to change"));
    }

    let mut changes = Vec::new();
    if let Some(mtu) = args.mtu {
        validate_mtu(mtu)?;
        let mtu_text = mtu.to_string();
        run(
            runner,
            "ip",
            &["link", "set", "dev", &args.name, "mtu", &mtu_text],
            "edit",
        )?;
        changes.push(format!("mtu {mtu}"));
    }
    if let Some(up) = args.up {
        if !up && target.kind == NetworkInterfaceKind::Loopback {
            return Err(protected("refusing to bring a loopback interface down"));
        }
        run(
            runner,
            "ip",
            &[
                "link",
                "set",
                "dev",
                &args.name,
                if up { "up" } else { "down" },
            ],
            "edit",
        )?;
        changes.push(if up { "up" } else { "down" }.to_string());
    }

    Ok(format!("updated `{}`: {}", args.name, changes.join(", ")))
}

/// Renames an interface.
///
/// The link has to be down first: the kernel refuses to rename an interface
/// that is up, and forcing it would drop traffic mid-flight for a reason the
/// operator did not ask for.
pub fn rename(
    manage_enabled: bool,
    state: &NetworkState,
    runner: &dyn CommandRunner,
    name: &str,
    new_name: &str,
) -> Result<String, NetworkError> {
    if !manage_enabled {
        return Err(disabled());
    }
    validate_name(name)?;
    validate_name(new_name)?;
    let target = state.get(name).ok_or_else(|| not_found(name))?;

    if target.up {
        return Err(protected(format!(
            "refusing to rename `{name}` while it is up; bring it down first"
        )));
    }
    if state.get(new_name).is_some() {
        return Err(invalid(format!("interface `{new_name}` already exists")));
    }

    run(
        runner,
        "ip",
        &["link", "set", "dev", name, "name", new_name],
        "rename",
    )?;
    Ok(format!("renamed `{name}` to `{new_name}`"))
}

/// Deletes an interface.
///
/// A physical device is allowed, because an operator may be removing a card on
/// purpose; the UI warns about it instead of hiding the action. The guards stop
/// only the cases where the panel is guaranteed to break its own access.
pub fn delete(
    manage_enabled: bool,
    state: &NetworkState,
    runner: &dyn CommandRunner,
    name: &str,
    peer: Option<&str>,
) -> Result<String, NetworkError> {
    if !manage_enabled {
        return Err(disabled());
    }
    validate_name(name)?;
    check_delete_guards(state, name, peer)?;

    run(runner, "ip", &["link", "delete", "dev", name], "delete")?;
    Ok(format!("deleted interface `{name}`"))
}

fn run(
    runner: &dyn CommandRunner,
    program: &str,
    args: &[&str],
    action: &str,
) -> Result<(), NetworkError> {
    match runner.run(program, args) {
        Ok(CommandOutput { ok: true, .. }) => Ok(()),
        Ok(output) => Err(classify(action, &output.stderr)),
        Err(CommandError::Unavailable { .. }) => Err(NetworkError::new(
            NetworkErrorKind::Failed,
            format!("{program} is not installed or is not on PATH"),
        )),
        Err(CommandError::Failed { detail, .. }) => Err(NetworkError::new(
            NetworkErrorKind::Failed,
            format!("could not run {program}: {detail}"),
        )),
    }
}

/// `ip` reports every refusal as plain text, so the message is the only signal.
fn classify(action: &str, stderr: &str) -> NetworkError {
    let lowercase = stderr.to_lowercase();
    let kind = if lowercase.contains("operation not permitted")
        || lowercase.contains("permission denied")
        || lowercase.contains("must be root")
    {
        NetworkErrorKind::PermissionDenied
    } else if lowercase.contains("does not exist")
        || lowercase.contains("cannot find device")
        || lowercase.contains("no such")
    {
        NetworkErrorKind::NotFound
    } else if lowercase.contains("file exists")
        || lowercase.contains("already exists")
        || lowercase.contains("invalid")
        || lowercase.contains("not supported")
        || lowercase.contains("argument")
    {
        NetworkErrorKind::Invalid
    } else {
        NetworkErrorKind::Failed
    };

    let detail = if stderr.is_empty() {
        format!("could not {action} the interface")
    } else {
        format!("could not {action} the interface: {stderr}")
    };
    NetworkError::new(kind, detail)
}

/// The interfaces carrying a default route, from `ip route`.
///
/// `/proc/net/route` would cover IPv4 only and miss policy routing, so the
/// guard asks the same tool that will perform the delete. A failure returns an
/// empty list: the caller treats that as "unknown", never as "none".
pub fn default_route_interfaces(runner: &dyn CommandRunner) -> Vec<String> {
    let mut names = Vec::new();
    for args in [
        ["-4", "route", "show", "default"],
        ["-6", "route", "show", "default"],
    ] {
        if let Ok(CommandOutput {
            ok: true, stdout, ..
        }) = runner.run("ip", &args)
        {
            for line in stdout.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.first() != Some(&"default") {
                    continue;
                }
                if let Some(position) = parts.iter().position(|part| *part == "dev")
                    && let Some(name) = parts.get(position + 1)
                    && !names.iter().any(|existing| existing == name)
                {
                    names.push((*name).to_string());
                }
            }
        }
    }
    names
}
