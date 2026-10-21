//! Interface management guards.
//!
//! Every test scripts the command runner, so the suite asserts what the daemon
//! would run — and, more importantly, what it refuses to run — without touching
//! the host's network.

use std::cell::RefCell;
use std::net::IpAddr;

use anth_panel::command::{CommandError, CommandOutput, CommandRunner};
use anth_panel::network::manage::{
    self, InterfaceState, NetworkState, default_route_interfaces, interface_for_peer,
};
use anth_proto::{NetworkCreateArgs, NetworkEditArgs, NetworkErrorKind, NetworkInterfaceKind};

struct RecordingRunner {
    calls: RefCell<Vec<(String, Vec<String>)>>,
    result: Result<CommandOutput, CommandError>,
}

impl RecordingRunner {
    fn ok() -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            result: Ok(CommandOutput {
                ok: true,
                stdout: String::new(),
                stderr: String::new(),
            }),
        }
    }

    fn failing(stderr: &str) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            result: Ok(CommandOutput {
                ok: false,
                stdout: String::new(),
                stderr: stderr.to_string(),
            }),
        }
    }

    fn with_stdout(stdout: &str) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            result: Ok(CommandOutput {
                ok: true,
                stdout: stdout.to_string(),
                stderr: String::new(),
            }),
        }
    }

    fn calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.borrow().clone()
    }
}

impl CommandRunner for RecordingRunner {
    fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError> {
        self.calls.borrow_mut().push((
            program.to_string(),
            args.iter().map(|arg| (*arg).to_string()).collect(),
        ));
        self.result.clone()
    }

    fn run_with_stdin(
        &self,
        program: &str,
        args: &[&str],
        _input: &str,
    ) -> Result<CommandOutput, CommandError> {
        self.run(program, args)
    }
}

fn interface(name: &str, kind: NetworkInterfaceKind) -> InterfaceState {
    InterfaceState {
        name: name.to_string(),
        up: true,
        kind,
        physical: kind == NetworkInterfaceKind::Physical,
        default_route: false,
        addresses: Vec::new(),
    }
}

fn address(network: &str, prefix: u8) -> (IpAddr, u8) {
    (network.parse().unwrap(), prefix)
}

fn state(interfaces: Vec<InterfaceState>) -> NetworkState {
    NetworkState { interfaces }
}

#[test]
fn create_runs_the_expected_argument_vector() {
    let runner = RecordingRunner::ok();
    let message = manage::create(
        true,
        &state(vec![]),
        &runner,
        &NetworkCreateArgs {
            name: "br0".into(),
            kind: NetworkInterfaceKind::Bridge,
            peer: None,
            link: None,
            vlan_id: None,
            mtu: None,
            up: false,
        },
    )
    .unwrap();

    assert_eq!(message, "created bridge interface `br0`");
    assert_eq!(
        runner.calls(),
        vec![(
            "ip".to_string(),
            vec!["link", "add", "br0", "type", "bridge"]
                .into_iter()
                .map(str::to_string)
                .collect()
        )]
    );
}

#[test]
fn create_refuses_a_duplicate_name_without_running_anything() {
    let runner = RecordingRunner::ok();
    let error = manage::create(
        true,
        &state(vec![interface("br0", NetworkInterfaceKind::Bridge)]),
        &runner,
        &NetworkCreateArgs {
            name: "br0".into(),
            kind: NetworkInterfaceKind::Bridge,
            peer: None,
            link: None,
            vlan_id: None,
            mtu: None,
            up: false,
        },
    )
    .unwrap_err();

    assert_eq!(error.kind, NetworkErrorKind::Invalid);
    assert!(runner.calls().is_empty(), "nothing may run");
}

#[test]
fn create_refuses_a_hostile_name() {
    // The name reaches `ip` as one argv element, so this is about rejecting a
    // name the kernel would refuse rather than about shell escaping. A value
    // like `eth0; reboot` must not be attempted at all.
    for name in ["eth0; reboot", "with space", "a/b", "sixteen-bytes-ok"] {
        let runner = RecordingRunner::ok();
        let error = manage::create(
            true,
            &state(vec![]),
            &runner,
            &NetworkCreateArgs {
                name: name.into(),
                kind: NetworkInterfaceKind::Dummy,
                peer: None,
                link: None,
                vlan_id: None,
                mtu: None,
                up: false,
            },
        )
        .unwrap_err();

        assert_eq!(error.kind, NetworkErrorKind::Invalid, "{name}");
        assert!(runner.calls().is_empty(), "{name} must not reach ip");
    }
}

#[test]
fn create_cannot_build_a_physical_interface() {
    let runner = RecordingRunner::ok();
    let error = manage::create(
        true,
        &state(vec![]),
        &runner,
        &NetworkCreateArgs {
            name: "eth9".into(),
            kind: NetworkInterfaceKind::Physical,
            peer: None,
            link: None,
            vlan_id: None,
            mtu: None,
            up: false,
        },
    )
    .unwrap_err();

    assert_eq!(error.kind, NetworkErrorKind::Invalid);
    assert!(error.detail.contains("only veth"), "{}", error.detail);
    assert!(runner.calls().is_empty());
}

#[test]
fn create_veth_requires_a_peer_and_checks_it() {
    let runner = RecordingRunner::ok();
    let args = NetworkCreateArgs {
        name: "veth0".into(),
        kind: NetworkInterfaceKind::Veth,
        peer: None,
        link: None,
        vlan_id: None,
        mtu: None,
        up: false,
    };
    let error = manage::create(true, &state(vec![]), &runner, &args).unwrap_err();
    assert_eq!(error.kind, NetworkErrorKind::Invalid);
    assert!(error.detail.contains("peer"), "{}", error.detail);

    let taken = NetworkCreateArgs {
        peer: Some("eth0".into()),
        ..args
    };
    let error = manage::create(
        true,
        &state(vec![interface("eth0", NetworkInterfaceKind::Physical)]),
        &runner,
        &taken,
    )
    .unwrap_err();
    assert_eq!(error.kind, NetworkErrorKind::Invalid);
    assert!(error.detail.contains("already exists"), "{}", error.detail);
    assert!(runner.calls().is_empty());
}

#[test]
fn create_vlan_validates_the_identifier_and_the_parent() {
    let runner = RecordingRunner::ok();
    let base = NetworkCreateArgs {
        name: "eth0.10".into(),
        kind: NetworkInterfaceKind::Vlan,
        peer: None,
        link: Some("eth0".into()),
        vlan_id: None,
        mtu: None,
        up: false,
    };

    let missing_parent = manage::create(true, &state(vec![]), &runner, &base).unwrap_err();
    assert_eq!(missing_parent.kind, NetworkErrorKind::NotFound);

    let bad_id = manage::create(
        true,
        &state(vec![interface("eth0", NetworkInterfaceKind::Physical)]),
        &runner,
        &NetworkCreateArgs {
            vlan_id: Some(4095),
            ..base.clone()
        },
    )
    .unwrap_err();
    assert_eq!(bad_id.kind, NetworkErrorKind::Invalid);
    assert!(bad_id.detail.contains("1-4094"), "{}", bad_id.detail);

    let runner = RecordingRunner::ok();
    manage::create(
        true,
        &state(vec![interface("eth0", NetworkInterfaceKind::Physical)]),
        &runner,
        &NetworkCreateArgs {
            vlan_id: Some(10),
            ..base
        },
    )
    .unwrap();
    // The identifier must reach the argv: `ip ... type vlan id` with a bare
    // `id` fails on every host.
    assert_eq!(
        runner.calls()[0].1,
        vec![
            "link", "add", "link", "eth0", "name", "eth0.10", "type", "vlan", "id", "10",
        ]
    );
}

#[test]
fn create_rejects_an_out_of_range_mtu() {
    let runner = RecordingRunner::ok();
    let error = manage::create(
        true,
        &state(vec![]),
        &runner,
        &NetworkCreateArgs {
            name: "d0".into(),
            kind: NetworkInterfaceKind::Dummy,
            peer: None,
            link: None,
            vlan_id: None,
            mtu: Some(10),
            up: false,
        },
    )
    .unwrap_err();

    assert_eq!(error.kind, NetworkErrorKind::Invalid);
    assert!(runner.calls().is_empty());
}

#[test]
fn manage_switches_off_every_mutation() {
    // The switch is separate from `control`: an operator who enabled up/down
    // must not get create/delete by implication.
    let runner = RecordingRunner::ok();
    let existing = state(vec![interface("d0", NetworkInterfaceKind::Dummy)]);

    let create = manage::create(
        false,
        &state(vec![]),
        &runner,
        &NetworkCreateArgs {
            name: "d0".into(),
            kind: NetworkInterfaceKind::Dummy,
            peer: None,
            link: None,
            vlan_id: None,
            mtu: None,
            up: false,
        },
    )
    .unwrap_err();
    let edit = manage::edit(
        false,
        &existing,
        &runner,
        &NetworkEditArgs {
            name: "d0".into(),
            mtu: Some(1500),
            up: None,
        },
    )
    .unwrap_err();
    let rename = manage::rename(false, &existing, &runner, "d0", "d1").unwrap_err();
    let delete = manage::delete(false, &existing, &runner, "d0", None).unwrap_err();

    for error in [create, edit, rename, delete] {
        assert_eq!(error.kind, NetworkErrorKind::Disabled);
        assert!(error.detail.contains("manage"), "{}", error.detail);
    }
    assert!(runner.calls().is_empty());
}

#[test]
fn delete_refuses_the_interface_the_request_arrived_through() {
    // This is the guard that stops the panel locking its own operator out.
    let mut eth0 = interface("eth0", NetworkInterfaceKind::Physical);
    eth0.addresses = vec![address("10.0.0.5", 24)];
    let mut eth1 = interface("eth1", NetworkInterfaceKind::Physical);
    eth1.addresses = vec![address("192.168.1.5", 24)];
    let state = state(vec![eth0, eth1]);
    let runner = RecordingRunner::ok();

    let error = manage::delete(true, &state, &runner, "eth0", Some("10.0.0.77")).unwrap_err();

    assert_eq!(error.kind, NetworkErrorKind::Protected);
    assert!(
        error.detail.contains("arrived through it"),
        "{}",
        error.detail
    );
    assert!(runner.calls().is_empty(), "ip must not run");
}

#[test]
fn delete_allows_an_interface_the_peer_is_not_using() {
    let mut eth0 = interface("eth0", NetworkInterfaceKind::Physical);
    eth0.addresses = vec![address("10.0.0.5", 24)];
    let mut eth1 = interface("eth1", NetworkInterfaceKind::Physical);
    eth1.addresses = vec![address("192.168.1.5", 24)];
    let state = state(vec![eth0, eth1]);
    let runner = RecordingRunner::ok();

    let message = manage::delete(true, &state, &runner, "eth1", Some("10.0.0.77")).unwrap();

    assert_eq!(message, "deleted interface `eth1`");
    assert_eq!(runner.calls()[0].1, vec!["link", "delete", "dev", "eth1"]);
}

#[test]
fn a_loopback_peer_skips_the_peer_guard() {
    // A client on this host says nothing about which link carries the panel to
    // a remote operator, so the guard must not claim a protection it lacks.
    let mut eth0 = interface("eth0", NetworkInterfaceKind::Physical);
    eth0.addresses = vec![address("10.0.0.5", 24)];
    let state = state(vec![eth0]);
    let runner = RecordingRunner::ok();

    manage::delete(true, &state, &runner, "eth0", Some("127.0.0.1")).unwrap();
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn an_unresolvable_peer_skips_the_peer_guard_but_not_the_others() {
    let mut eth0 = interface("eth0", NetworkInterfaceKind::Physical);
    eth0.addresses = vec![address("10.0.0.5", 24)];
    let state = state(vec![eth0]);
    let runner = RecordingRunner::ok();

    // A proxy address matches no interface: the guard is skipped rather than
    // guessed at, and the delete proceeds.
    manage::delete(true, &state, &runner, "eth0", Some("203.0.113.9")).unwrap();
    assert_eq!(runner.calls().len(), 1);

    // A malformed peer is also "unknown", not a reason to refuse.
    let runner = RecordingRunner::ok();
    manage::delete(true, &state, &runner, "eth0", Some("not-an-ip")).unwrap();
}

#[test]
fn the_peer_guard_picks_the_most_specific_address() {
    // Two interfaces can hold overlapping prefixes; the longest match is the
    // one the kernel would route through.
    let mut eth0 = interface("eth0", NetworkInterfaceKind::Physical);
    eth0.addresses = vec![address("10.0.0.5", 8)];
    let mut eth1 = interface("eth1", NetworkInterfaceKind::Physical);
    eth1.addresses = vec![address("10.1.2.5", 24)];
    let state = state(vec![eth0, eth1]);

    assert_eq!(
        interface_for_peer(&state, "10.1.2.99").as_deref(),
        Some("eth1")
    );
    assert_eq!(
        interface_for_peer(&state, "10.9.9.9").as_deref(),
        Some("eth0")
    );
}

#[test]
fn delete_refuses_the_last_default_route_carrier() {
    let mut eth0 = interface("eth0", NetworkInterfaceKind::Physical);
    eth0.default_route = true;
    let state = state(vec![eth0]);
    let runner = RecordingRunner::ok();

    let error = manage::delete(true, &state, &runner, "eth0", None).unwrap_err();

    assert_eq!(error.kind, NetworkErrorKind::Protected);
    assert!(
        error
            .detail
            .contains("last interface carrying the default route"),
        "{}",
        error.detail
    );
    assert!(runner.calls().is_empty());
}

#[test]
fn delete_allows_a_default_route_carrier_while_another_remains() {
    let mut eth0 = interface("eth0", NetworkInterfaceKind::Physical);
    eth0.default_route = true;
    let mut eth1 = interface("eth1", NetworkInterfaceKind::Physical);
    eth1.default_route = true;
    let state = state(vec![eth0, eth1]);
    let runner = RecordingRunner::ok();

    manage::delete(true, &state, &runner, "eth0", None).unwrap();
    assert_eq!(runner.calls().len(), 1);
}

#[test]
fn a_physical_interface_can_be_deleted() {
    // The maintainer's decision: warn, do not forbid. The warning lives in the
    // UI and the CLI prompt; the daemon only refuses the self-lockout cases.
    let eth0 = interface("eth0", NetworkInterfaceKind::Physical);
    let state = state(vec![eth0]);
    let runner = RecordingRunner::ok();

    manage::delete(true, &state, &runner, "eth0", None).unwrap();
    assert_eq!(runner.calls()[0].1, vec!["link", "delete", "dev", "eth0"]);
}

#[test]
fn delete_refuses_a_loopback() {
    let lo = interface("lo", NetworkInterfaceKind::Loopback);
    let state = state(vec![lo]);
    let runner = RecordingRunner::ok();

    let error = manage::delete(true, &state, &runner, "lo", None).unwrap_err();
    assert_eq!(error.kind, NetworkErrorKind::Protected);
    assert!(runner.calls().is_empty());
}

#[test]
fn delete_refuses_a_missing_interface() {
    let runner = RecordingRunner::ok();
    let error = manage::delete(true, &state(vec![]), &runner, "eth9", None).unwrap_err();

    assert_eq!(error.kind, NetworkErrorKind::NotFound);
    assert!(runner.calls().is_empty());
}

#[test]
fn rename_requires_the_link_to_be_down() {
    let mut dummy = interface("d0", NetworkInterfaceKind::Dummy);
    dummy.up = true;
    let state = state(vec![dummy]);
    let runner = RecordingRunner::ok();

    let error = manage::rename(true, &state, &runner, "d0", "d1").unwrap_err();

    assert_eq!(error.kind, NetworkErrorKind::Protected);
    assert!(error.detail.contains("while it is up"), "{}", error.detail);
    assert!(runner.calls().is_empty());
}

#[test]
fn rename_runs_once_the_link_is_down() {
    let mut dummy = interface("d0", NetworkInterfaceKind::Dummy);
    dummy.up = false;
    let state = state(vec![dummy]);
    let runner = RecordingRunner::ok();

    let message = manage::rename(true, &state, &runner, "d0", "d1").unwrap();

    assert_eq!(message, "renamed `d0` to `d1`");
    assert_eq!(
        runner.calls()[0].1,
        vec!["link", "set", "dev", "d0", "name", "d1"]
    );
}

#[test]
fn edit_leaves_the_administrative_state_alone_when_only_the_mtu_changes() {
    // An edit that omits `up` must not take the link down: the CLI models that
    // as `None`, and the daemon has to honour it.
    let eth0 = interface("eth0", NetworkInterfaceKind::Physical);
    let state = state(vec![eth0]);
    let runner = RecordingRunner::ok();

    let message = manage::edit(
        true,
        &state,
        &runner,
        &NetworkEditArgs {
            name: "eth0".into(),
            mtu: Some(9000),
            up: None,
        },
    )
    .unwrap();

    assert_eq!(message, "updated `eth0`: mtu 9000");
    assert_eq!(runner.calls().len(), 1);
    assert_eq!(
        runner.calls()[0].1,
        vec!["link", "set", "dev", "eth0", "mtu", "9000"]
    );
}

#[test]
fn edit_refuses_an_empty_change() {
    let eth0 = interface("eth0", NetworkInterfaceKind::Physical);
    let runner = RecordingRunner::ok();
    let error = manage::edit(
        true,
        &state(vec![eth0]),
        &runner,
        &NetworkEditArgs {
            name: "eth0".into(),
            mtu: None,
            up: None,
        },
    )
    .unwrap_err();

    assert_eq!(error.kind, NetworkErrorKind::Invalid);
    assert!(error.detail.contains("nothing to change"));
}

#[test]
fn edit_refuses_to_bring_a_loopback_down() {
    let lo = interface("lo", NetworkInterfaceKind::Loopback);
    let runner = RecordingRunner::ok();
    let error = manage::edit(
        true,
        &state(vec![lo]),
        &runner,
        &NetworkEditArgs {
            name: "lo".into(),
            mtu: None,
            up: Some(false),
        },
    )
    .unwrap_err();

    assert_eq!(error.kind, NetworkErrorKind::Protected);
    assert!(runner.calls().is_empty());
}

#[test]
fn a_permission_failure_is_classified_rather_than_reported_as_generic() {
    let eth0 = interface("eth0", NetworkInterfaceKind::Physical);
    let runner = RecordingRunner::failing("RTNETLINK answers: Operation not permitted");

    let error = manage::delete(true, &state(vec![eth0]), &runner, "eth0", None).unwrap_err();

    assert_eq!(error.kind, NetworkErrorKind::PermissionDenied);
    assert!(
        error.detail.contains("could not delete"),
        "{}",
        error.detail
    );
}

#[test]
fn default_routes_are_read_from_ip_output() {
    let runner = RecordingRunner::with_stdout(
        "default via 10.0.0.1 dev eth0 proto dhcp metric 100\ndefault via 192.168.1.1 dev wlan0 metric 600\n",
    );

    let names = default_route_interfaces(&runner);

    assert_eq!(names, vec!["eth0".to_string(), "wlan0".to_string()]);
    assert_eq!(runner.calls()[0].1, vec!["-4", "route", "show", "default"]);
}

#[test]
fn a_failed_route_read_returns_no_names() {
    // Empty means "unknown", and the caller only refuses when a delete would
    // remove the last known carrier. A read failure must not invent a carrier.
    let runner = RecordingRunner::failing("ip is not installed");

    assert!(default_route_interfaces(&runner).is_empty());
}

#[test]
fn a_physical_device_is_reported_as_physical() {
    // Guards rely on `InterfaceState::physical`, which the snapshot derives
    // from the `device` link in sysfs.
    let eth0 = interface("eth0", NetworkInterfaceKind::Physical);
    assert!(eth0.physical);

    let dummy = interface("d0", NetworkInterfaceKind::Dummy);
    assert!(!dummy.physical);
}
