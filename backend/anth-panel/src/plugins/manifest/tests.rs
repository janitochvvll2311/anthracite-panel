use super::*;

fn manifest(text: &str) -> Result<Manifest, PluginError> {
    Manifest::parse(text)
}

const MINIMAL: &str = r#"
[plugin]
id = "org.anth.wireguard"
version = "0.1.0"
name = "WireGuard"
abi = 1

[wasm]
path = "plugin.wasm"
sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
"#;

const CAPABILITY: &str = r#"
[[capability]]
id = "wg.status"
program = "wg"
argv = ["show", "{interface}", "dump"]

[capability.args.interface]
type = "ifname"
"#;

#[test]
fn a_minimal_manifest_parses() {
    let parsed = manifest(MINIMAL).unwrap();
    assert_eq!(parsed.plugin.id, "org.anth.wireguard");
    assert!(parsed.capabilities.is_empty());
    assert!(parsed.api.is_empty());
}

#[test]
fn the_id_must_be_reverse_dns_and_lowercase() {
    assert!(validate_id("org.anth.wireguard").is_ok());
    assert!(validate_id("wireguard").is_err());
    assert!(validate_id("Org.Anth.Wg").is_err());
    assert!(validate_id("org..wg").is_err());
}

#[test]
fn a_manifest_for_another_abi_is_refused() {
    let text = MINIMAL.replace("abi = 1", "abi = 7");
    let error = manifest(&text).unwrap_err();
    assert_eq!(error.kind, PluginErrorKind::Denied);
    assert!(error.detail.contains("ABI"));
}

#[test]
fn the_wasm_sha_must_be_a_full_digest() {
    let text = MINIMAL.replace(&"a".repeat(64), "abc");
    assert!(manifest(&text).is_err());
}

#[test]
fn a_wasm_path_cannot_escape_the_bundle() {
    let text = MINIMAL.replace("path = \"plugin.wasm\"", "path = \"../evil.wasm\"");
    let error = manifest(&text).unwrap_err();
    assert_eq!(error.kind, PluginErrorKind::Denied);
}

#[test]
fn a_capability_with_a_typed_placeholder_parses() {
    let parsed = manifest(&format!("{MINIMAL}{CAPABILITY}")).unwrap();
    assert_eq!(parsed.capabilities[0].id, "wg.status");
    assert_eq!(
        parsed.capabilities[0].args["interface"].kind,
        ArgType::Ifname
    );
}

#[test]
fn a_program_outside_the_allowlist_is_denied() {
    let text = format!("{MINIMAL}{CAPABILITY}").replace("program = \"wg\"", "program = \"bash\"");
    let error = manifest(&text).unwrap_err();
    assert_eq!(error.kind, PluginErrorKind::Denied);
    assert!(error.detail.contains("bash"));
}

#[test]
fn a_plugin_cannot_name_a_program_by_path() {
    let text =
        format!("{MINIMAL}{CAPABILITY}").replace("program = \"wg\"", "program = \"/usr/bin/wg\"");
    assert_eq!(manifest(&text).unwrap_err().kind, PluginErrorKind::Denied);
}

#[test]
fn a_job_owns_its_argv() {
    let text = format!("{MINIMAL}{CAPABILITY}").replace(
        "program = \"wg\"\nargv = [\"show\", \"{interface}\", \"dump\"]",
        "job = \"network.list\"\nargv = [\"show\", \"{interface}\"]",
    );
    let error = manifest(&text).unwrap_err();
    assert_eq!(error.kind, PluginErrorKind::Denied);
    assert!(error.detail.contains("owns its argv"));
}

#[test]
fn an_unknown_job_is_denied() {
    let text = format!("{MINIMAL}{CAPABILITY}").replace(
        "program = \"wg\"\nargv = [\"show\", \"{interface}\", \"dump\"]",
        "job = \"shell.anything\"",
    );
    assert_eq!(manifest(&text).unwrap_err().kind, PluginErrorKind::Denied);
}

#[test]
fn a_placeholder_without_a_validator_is_refused() {
    let text = format!("{MINIMAL}{CAPABILITY}")
        .replace("\n[capability.args.interface]\ntype = \"ifname\"\n", "");
    let error = manifest(&text).unwrap_err();
    assert!(error.detail.contains("declares no validator"));
}

#[test]
fn a_declared_argument_the_argv_never_uses_is_refused() {
    let text = format!("{MINIMAL}{CAPABILITY}").replace(
        "argv = [\"show\", \"{interface}\", \"dump\"]",
        "argv = [\"show\", \"all\", \"dump\"]",
    );
    let error = manifest(&text).unwrap_err();
    assert!(error.detail.contains("never uses it"));
}

#[test]
fn an_unknown_validator_type_is_refused() {
    let text = format!("{MINIMAL}{CAPABILITY}").replace("type = \"ifname\"", "type = \"regex\"");
    assert!(manifest(&text).is_err());
}

#[test]
fn free_text_needs_the_capability_to_opt_in() {
    let text = format!("{MINIMAL}{CAPABILITY}").replace("type = \"ifname\"", "type = \"text\"");
    let error = manifest(&text).unwrap_err();
    assert_eq!(error.kind, PluginErrorKind::Denied);
    assert!(error.detail.contains("free text"));
}

#[test]
fn free_text_is_allowed_when_declared() {
    let text = format!("{MINIMAL}{CAPABILITY}")
        .replace("type = \"ifname\"", "type = \"text\"")
        .replace(
            "id = \"wg.status\"",
            "id = \"wg.status\"\naccepts_text = true",
        );
    assert!(manifest(&text).is_ok());
}

#[test]
fn rendering_substitutes_only_placeholders() {
    let parsed = manifest(&format!("{MINIMAL}{CAPABILITY}")).unwrap();
    let mut values = BTreeMap::new();
    values.insert("interface".to_string(), "wg0".to_string());
    assert_eq!(
        render_argv(&parsed.capabilities[0], &values).unwrap(),
        vec!["show", "wg0", "dump"]
    );
}

#[test]
fn rendering_keeps_a_literal_part_untouched() {
    let parsed = manifest(&format!("{MINIMAL}{CAPABILITY}")).unwrap();
    let mut values = BTreeMap::new();
    values.insert("interface".to_string(), "wg0".to_string());
    let argv = render_argv(&parsed.capabilities[0], &values).unwrap();
    assert_eq!(argv[0], "show");
    assert_eq!(argv[2], "dump");
}

#[test]
fn rendering_refuses_an_invalid_value() {
    let parsed = manifest(&format!("{MINIMAL}{CAPABILITY}")).unwrap();
    let mut values = BTreeMap::new();
    values.insert("interface".to_string(), "wg0; rm -rf /".to_string());
    assert_eq!(
        render_argv(&parsed.capabilities[0], &values)
            .unwrap_err()
            .kind,
        PluginErrorKind::Denied
    );
}

#[test]
fn rendering_refuses_an_undeclared_argument() {
    let parsed = manifest(&format!("{MINIMAL}{CAPABILITY}")).unwrap();
    let mut values = BTreeMap::new();
    values.insert("interface".to_string(), "wg0".to_string());
    values.insert("extra".to_string(), "--force".to_string());
    let error = render_argv(&parsed.capabilities[0], &values).unwrap_err();
    assert_eq!(error.kind, PluginErrorKind::Denied);
    assert!(error.detail.contains("no argument `extra`"));
}

#[test]
fn rendering_refuses_a_missing_value() {
    let parsed = manifest(&format!("{MINIMAL}{CAPABILITY}")).unwrap();
    let error = render_argv(&parsed.capabilities[0], &BTreeMap::new()).unwrap_err();
    assert_eq!(error.kind, PluginErrorKind::Denied);
    assert!(error.detail.contains("requires"));
}

#[test]
fn an_unbalanced_placeholder_is_refused() {
    let text = format!("{MINIMAL}{CAPABILITY}").replace(
        "\"show\", \"{interface}\", \"dump\"",
        "\"show\", \"{interface\", \"dump\"",
    );
    assert!(manifest(&text).is_err());
}

#[test]
fn a_route_may_not_escape_its_namespace() {
    let text = format!("{MINIMAL}\n[[api]]\nmethod = \"GET\"\npath = \"/../../auth/login\"\n");
    assert_eq!(manifest(&text).unwrap_err().kind, PluginErrorKind::Denied);
}

#[test]
fn a_route_needs_a_known_method() {
    let text = format!("{MINIMAL}\n[[api]]\nmethod = \"TRACE\"\npath = \"/x\"\n");
    assert!(manifest(&text).is_err());
}

#[test]
fn duplicate_routes_are_refused() {
    let text = format!(
        "{MINIMAL}\n[[api]]\nmethod = \"GET\"\npath = \"/x\"\n\n[[api]]\nmethod = \"GET\"\npath = \"/x\"\n"
    );
    assert!(manifest(&text).is_err());
}

#[test]
fn a_ui_slot_must_be_one_the_shell_provides() {
    let text = format!(
        "{MINIMAL}\n[[ui]]\nslot = \"nowhere.gadget\"\ntag = \"anth-x\"\nmodule = \"web/x.js\"\n"
    );
    assert_eq!(manifest(&text).unwrap_err().kind, PluginErrorKind::Denied);
}

#[test]
fn a_ui_module_must_live_under_web() {
    let text = format!(
        "{MINIMAL}\n[[ui]]\nslot = \"page\"\ntag = \"anth-x\"\nmodule = \"../../etc/passwd\"\n"
    );
    assert_eq!(manifest(&text).unwrap_err().kind, PluginErrorKind::Denied);
}

#[test]
fn a_ui_tag_must_be_a_custom_element() {
    let text =
        format!("{MINIMAL}\n[[ui]]\nslot = \"page\"\ntag = \"div\"\nmodule = \"web/x.js\"\n");
    assert!(manifest(&text).is_err());
}

#[test]
fn a_duplicate_capability_id_is_refused() {
    let text = format!("{MINIMAL}{CAPABILITY}{CAPABILITY}");
    assert!(manifest(&text).unwrap_err().detail.contains("twice"));
}

#[test]
fn a_grant_freezes_the_capability_list_and_hash() {
    let parsed = manifest(&format!("{MINIMAL}{CAPABILITY}")).unwrap();
    let grant = PluginGrant::from_manifest(&parsed);
    assert_eq!(grant.allowed_ids(), vec!["wg.status".to_string()]);
    assert!(grant.allows("wg.status"));
    assert!(!grant.allows("wg.peers"));
    assert!(grant.enabled);
}

#[test]
fn a_disabled_grant_allows_nothing() {
    let parsed = manifest(&format!("{MINIMAL}{CAPABILITY}")).unwrap();
    let mut grant = PluginGrant::from_manifest(&parsed);
    grant.enabled = false;
    assert!(!grant.allows("wg.status"));
}

#[test]
fn every_validator_rejects_control_characters() {
    for kind in [
        ArgType::Ifname,
        ArgType::Hostname,
        ArgType::Path,
        ArgType::Text,
    ] {
        assert!(
            kind.validate("a\nb").is_err(),
            "{kind:?} accepted a newline"
        );
    }
}

#[test]
fn ifname_rejects_separators_and_traversal() {
    assert!(ArgType::Ifname.validate("wg0").is_ok());
    assert!(ArgType::Ifname.validate("wg0.1").is_ok());
    assert!(ArgType::Ifname.validate("a/b").is_err());
    assert!(ArgType::Ifname.validate("..").is_err());
    assert!(ArgType::Ifname.validate("abcdefghijklmnop").is_err());
    assert!(ArgType::Ifname.validate("wg 0").is_err());
}

#[test]
fn path_must_be_absolute_and_normalized() {
    assert!(ArgType::Path.validate("/etc/wireguard/wg0.conf").is_ok());
    assert!(ArgType::Path.validate("etc/wg0.conf").is_err());
    assert!(ArgType::Path.validate("/etc/../etc/shadow").is_err());
}

#[test]
fn cidr_checks_the_prefix_against_the_family() {
    assert!(ArgType::Cidr.validate("10.0.0.0/24").is_ok());
    assert!(ArgType::Cidr.validate("10.0.0.0/33").is_err());
    assert!(ArgType::Cidr.validate("fd00::/64").is_ok());
    assert!(ArgType::Cidr.validate("fd00::/129").is_err());
}

#[test]
fn hex_types_check_the_length_and_alphabet() {
    assert!(ArgType::Hex32.validate(&"a".repeat(32)).is_ok());
    assert!(ArgType::Hex32.validate(&"a".repeat(31)).is_err());
    assert!(ArgType::Hex32.validate(&"z".repeat(32)).is_err());
    assert!(ArgType::Hex64.validate(&"0".repeat(64)).is_ok());
}

#[test]
fn no_validator_accepts_a_leading_dash() {
    // A value that looks like an option is how a plugin would add a flag
    // the admin never approved: `wg show --force`.
    for kind in [
        ArgType::Ifname,
        ArgType::Hostname,
        ArgType::Port,
        ArgType::Ipv4,
        ArgType::Ipv6,
        ArgType::Cidr,
        ArgType::Hex32,
        ArgType::Hex64,
        ArgType::Path,
        ArgType::Text,
    ] {
        for value in ["-f", "--force", "-", "--"] {
            assert!(kind.validate(value).is_err(), "{kind:?} accepted `{value}`");
        }
    }
}

#[test]
fn a_leading_dash_never_reaches_the_argv() {
    let text = format!("{MINIMAL}{CAPABILITY}")
        .replace("type = \"ifname\"", "type = \"text\"")
        .replace(
            "id = \"wg.status\"",
            "id = \"wg.status\"\naccepts_text = true",
        );
    let parsed = manifest(&text).unwrap();
    let mut values = BTreeMap::new();
    values.insert("interface".to_string(), "--force".to_string());
    assert_eq!(
        render_argv(&parsed.capabilities[0], &values)
            .unwrap_err()
            .kind,
        PluginErrorKind::Denied
    );
}

#[test]
fn port_rejects_zero_and_wraparound() {
    assert!(ArgType::Port.validate("51820").is_ok());
    assert!(ArgType::Port.validate("0").is_err());
    assert!(ArgType::Port.validate("65536").is_err());
}
