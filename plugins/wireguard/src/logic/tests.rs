//! Tests for the plugin logic against a recorded executor.
//!
//! The executor is a table, so these assert the exact capability ids and
//! argument values the guest would send. That is the property the grant has to
//! agree with: a test that read the manifest instead would pass even if the
//! guest asked for something the admin never approved.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use super::*;

/// Records calls and answers each capability from a table.
#[derive(Default)]
struct Recorded {
    calls: Vec<(String, Value)>,
    replies: BTreeMap<String, Value>,
    logs: Vec<String>,
}

impl Recorded {
    fn with(capability: &str, stdout: &str) -> Self {
        let mut replies = BTreeMap::new();
        replies.insert(
            capability.to_string(),
            json!({"ok": true, "stdout": stdout, "stderr": ""}),
        );
        Self {
            calls: Vec::new(),
            replies,
            logs: Vec::new(),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.len()
    }

    fn called(&self, capability: &str) -> bool {
        self.calls.iter().any(|(id, _)| id == capability)
    }
}

impl Executor for Recorded {
    fn exec(&mut self, capability: &str, args: &Value) -> Result<Value, String> {
        self.calls.push((capability.to_string(), args.clone()));
        self.replies
            .get(capability)
            .cloned()
            .ok_or_else(|| format!("`{capability}` is not granted"))
    }

    fn log(&mut self, message: &str) {
        self.logs.push(message.to_string());
    }
}

const SHOW: &str = "\
interface: wg0
  public key: yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=
  private key: (hidden)
  listening port: 51820

peer: xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=
  allowed ips: 10.0.0.2/32, 10.0.0.3/32
  latest handshake: 1 minute, 12 seconds ago
  transfer: 1.23 KiB received, 4.56 KiB sent
";

fn http(method: &str, path: &str) -> HttpRequest {
    HttpRequest {
        method: method.to_string(),
        path: path.to_string(),
        query: Map::new(),
        body: None,
    }
}

#[test]
fn a_show_block_parses_into_an_interface_and_its_peers() {
    let parsed = parse_interface("wg0", SHOW);
    assert_eq!(parsed["name"], "wg0");
    assert_eq!(parsed["listen_port"], 51820);
    assert_eq!(
        parsed["public_key"],
        "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk="
    );
    let peers = parsed["peers"].as_array().unwrap();
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0]["allowed_ips"].as_array().unwrap().len(), 2);
    assert_eq!(peers[0]["received"], "1.23 KiB");
    assert_eq!(peers[0]["sent"], "4.56 KiB");
    assert_eq!(peers[0]["latest_handshake"], "1 minute, 12 seconds ago");
}

#[test]
fn an_interface_with_no_peers_parses_to_an_empty_list() {
    let text = "\
interface: wg1
  public key: yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=
  listening port: 51821
";
    let parsed = parse_interface("wg1", text);
    assert_eq!(parsed["listen_port"], 51821);
    assert!(parsed["peers"].as_array().unwrap().is_empty());
}

#[test]
fn several_peers_are_all_kept_in_order() {
    let text = "\
interface: wg0
  listening port: 1

peer: first
  allowed ips: 10.0.0.2/32

peer: second
  allowed ips: 10.0.0.3/32
";
    let parsed = parse_interface("wg0", text);
    let peers = parsed["peers"].as_array().unwrap();
    assert_eq!(peers.len(), 2);
    assert_eq!(peers[0]["public_key"], "first");
    assert_eq!(peers[1]["public_key"], "second");
}

#[test]
fn the_interface_key_survives_a_peer_block() {
    // Both sections use `public key`; only the interface one may fill the field.
    let text = "\
interface: wg0
  public key: interface-key
  listening port: 51820

peer: peer-key
  allowed ips: 10.0.0.2/32
";
    let parsed = parse_interface("wg0", text);
    assert_eq!(parsed["public_key"], "interface-key");
    assert_eq!(parsed["listen_port"], 51820);
}

#[test]
fn a_transfer_line_keeps_both_sides() {
    assert_eq!(
        split_transfer("0 B received, 0 B sent"),
        ("0 B".to_string(), "0 B".to_string())
    );
    assert_eq!(
        split_transfer("1.00 KiB received"),
        ("1.00 KiB".to_string(), String::new())
    );
}

#[test]
fn listing_interfaces_asks_the_host_for_each_one() {
    let mut executor = Recorded::with("wg.interfaces", "wg0 wg1");
    executor
        .replies
        .insert("wg.show".to_string(), json!({"ok": true, "stdout": SHOW}));

    let body = route(&mut executor, &http("GET", "/interfaces"));
    let parsed: Value = serde_json::from_str(&body).unwrap();

    assert_eq!(parsed["status"], 200);
    assert_eq!(parsed["body"]["interfaces"].as_array().unwrap().len(), 2);
    assert!(executor.called("wg.interfaces"));
    let shows = executor
        .calls
        .iter()
        .filter(|(id, _)| id == "wg.show")
        .count();
    assert_eq!(shows, 2, "one `wg show` per interface");
}

#[test]
fn an_unreadable_interface_is_reported_in_place() {
    // `wg.interfaces` succeeds; `wg.show` is not granted. The listing still
    // answers, with the failure attached to the entry it belongs to.
    let mut executor = Recorded::with("wg.interfaces", "wg0");
    let body = route(&mut executor, &http("GET", "/interfaces"));
    let parsed: Value = serde_json::from_str(&body).unwrap();

    assert_eq!(parsed["status"], 200);
    let entry = &parsed["body"]["interfaces"][0];
    assert_eq!(entry["name"], "wg0");
    assert!(entry["error"].as_str().unwrap().contains("not granted"));
    assert_eq!(executor.logs.len(), 1);
}

#[test]
fn a_missing_interface_list_is_a_bad_gateway() {
    let mut executor = Recorded::default();
    let body = route(&mut executor, &http("GET", "/interfaces"));
    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["status"], 502);
}

#[test]
fn status_counts_interfaces_and_peers() {
    let mut executor = Recorded::with("wg.interfaces", "wg0");
    executor
        .replies
        .insert("wg.show".to_string(), json!({"ok": true, "stdout": SHOW}));
    let body = route(&mut executor, &http("GET", "/status"));
    let parsed: Value = serde_json::from_str(&body).unwrap();

    assert_eq!(parsed["body"]["interfaces"], 1);
    assert_eq!(parsed["body"]["peers"], 1);
    assert_eq!(parsed["body"]["names"][0], "wg0");
}

#[test]
fn adding_a_peer_sends_values_and_never_an_argv() {
    let mut executor = Recorded::with("wg.add_peer", "");
    let mut request = http("POST", "/peers");
    request.body = Some(json!({
        "interface": "wg0",
        "public_key": "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=",
        "allowed_ips": ["10.0.0.2/32", "10.0.0.3/32"],
    }));

    let body = route(&mut executor, &request);
    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["status"], 201);
    assert_eq!(parsed["body"]["added"], true);

    let (capability, args) = executor
        .calls
        .iter()
        .find(|(id, _)| id == "wg.add_peer")
        .expect("the capability was called");
    assert_eq!(capability, "wg.add_peer");
    assert_eq!(args["allowed_ips"], "10.0.0.2/32,10.0.0.3/32");
    // The host renders both from the grant; a guest that sent either would be
    // asking for authority it does not have.
    assert!(args.get("argv").is_none());
    assert!(args.get("program").is_none());
}

#[test]
fn an_empty_endpoint_is_omitted_so_the_optional_element_drops() {
    let mut executor = Recorded::with("wg.add_peer", "");
    let mut request = http("POST", "/peers");
    request.body = Some(json!({
        "interface": "wg0",
        "public_key": "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=",
        "allowed_ips": "10.0.0.2/32",
        "endpoint": "",
    }));

    route(&mut executor, &request);
    let (_, args) = executor
        .calls
        .iter()
        .find(|(id, _)| id == "wg.add_peer")
        .unwrap();
    assert!(
        args.get("endpoint").is_none(),
        "an empty endpoint must be omitted so the optional element is dropped"
    );
}

#[test]
fn an_endpoint_selects_the_recipe_that_carries_it() {
    // `wg set ... endpoint <value>` needs both elements, so the endpoint has its
    // own capability. A single recipe with an optional trailing placeholder
    // would leave a bare `endpoint` when the value was absent.
    let mut executor = Recorded::with("wg.add_peer_endpoint", "");
    let mut request = http("POST", "/peers");
    request.body = Some(json!({
        "interface": "wg0",
        "public_key": "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=",
        "allowed_ips": "10.0.0.2/32",
        "endpoint": "vpn.example.net:51820",
    }));

    let parsed: Value = serde_json::from_str(&route(&mut executor, &request)).unwrap();
    assert_eq!(parsed["status"], 201);
    assert_eq!(parsed["body"]["endpoint"], true);
    let (capability, args) = executor
        .calls
        .iter()
        .find(|(id, _)| id == "wg.add_peer_endpoint")
        .expect("the endpoint recipe ran");
    assert_eq!(capability, "wg.add_peer_endpoint");
    assert_eq!(args["endpoint"], "vpn.example.net:51820");
}

#[test]
fn a_peer_body_without_allowed_ips_is_refused_before_any_call() {
    let mut executor = Recorded::with("wg.add_peer", "");
    let mut request = http("POST", "/peers");
    request.body = Some(json!({"interface": "wg0", "public_key": "a2V5"}));

    let parsed: Value = serde_json::from_str(&route(&mut executor, &request)).unwrap();
    assert_eq!(parsed["status"], 400);
    assert_eq!(executor.call_count(), 0);
}

#[test]
fn a_peer_body_that_is_not_an_object_is_a_400() {
    let mut executor = Recorded::default();
    let mut request = http("POST", "/peers");
    request.body = Some(json!(["wg0"]));
    let parsed: Value = serde_json::from_str(&route(&mut executor, &request)).unwrap();
    assert_eq!(parsed["status"], 400);
}

#[test]
fn peers_require_an_interface() {
    let mut executor = Recorded::default();
    let body = route(&mut executor, &http("GET", "/peers"));
    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["status"], 400);
    assert!(
        parsed["body"]["error"]
            .as_str()
            .unwrap()
            .contains("interface")
    );
}

#[test]
fn peers_for_an_interface_return_only_its_list() {
    let mut executor = Recorded::with("wg.show", SHOW);
    let mut request = http("GET", "/peers");
    request
        .query
        .insert("interface".to_string(), Value::from("wg0"));
    let parsed: Value = serde_json::from_str(&route(&mut executor, &request)).unwrap();
    assert_eq!(parsed["status"], 200);
    assert_eq!(parsed["body"]["peers"].as_array().unwrap().len(), 1);
}

#[test]
fn an_unknown_route_is_a_404_that_names_it() {
    let mut executor = Recorded::default();
    let parsed: Value = serde_json::from_str(&route(&mut executor, &http("GET", "/nope"))).unwrap();
    assert_eq!(parsed["status"], 404);
    assert!(parsed["body"]["error"].as_str().unwrap().contains("/nope"));
}

#[test]
fn the_health_route_needs_no_capability() {
    let mut executor = Recorded::default();
    let parsed: Value = serde_json::from_str(&route(&mut executor, &http("GET", "/"))).unwrap();
    assert_eq!(parsed["status"], 200);
    assert_eq!(parsed["body"]["ok"], true);
    assert_eq!(executor.call_count(), 0);
}

#[test]
fn an_unknown_cli_subcommand_names_the_ones_it_has() {
    let mut executor = Recorded::default();
    let body = cli(&mut executor, &["frobnicate".to_string()]);
    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["status"], 400);
    assert!(
        parsed["body"]["error"]
            .as_str()
            .unwrap()
            .contains("list, status or peers")
    );
}

#[test]
fn the_cli_peers_subcommand_takes_the_interface_from_argv() {
    let mut executor = Recorded::with("wg.show", SHOW);
    let body = cli(&mut executor, &["peers".to_string(), "wg0".to_string()]);
    let parsed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["status"], 200);
    let (_, args) = executor
        .calls
        .iter()
        .find(|(id, _)| id == "wg.show")
        .unwrap();
    assert_eq!(args["interface"], "wg0");
}

#[test]
fn a_malformed_envelope_is_a_400_not_a_trap() {
    let mut executor = Recorded::default();
    let parsed: Value = serde_json::from_str(&handle(&mut executor, "{")).unwrap();
    assert_eq!(parsed["status"], 400);
    assert!(
        parsed["body"]["error"]
            .as_str()
            .unwrap()
            .contains("envelope")
    );
}

#[test]
fn an_envelope_round_trips_through_handle() {
    let mut executor = Recorded::with("wg.interfaces", "wg0");
    executor
        .replies
        .insert("wg.show".to_string(), json!({"ok": true, "stdout": SHOW}));
    let request = json!({"kind": "http", "method": "GET", "path": "/status"}).to_string();
    let parsed: Value = serde_json::from_str(&handle(&mut executor, &request)).unwrap();
    assert_eq!(parsed["status"], 200);
    assert_eq!(parsed["body"]["interfaces"], 1);
}

#[test]
fn a_cli_envelope_reaches_the_cli_half() {
    let mut executor = Recorded::with("wg.interfaces", "wg0 wg1");
    let request = json!({"kind": "cli", "argv": ["list"]}).to_string();
    let parsed: Value = serde_json::from_str(&handle(&mut executor, &request)).unwrap();
    assert_eq!(parsed["status"], 200);
    assert_eq!(parsed["body"]["interfaces"].as_array().unwrap().len(), 2);
}
