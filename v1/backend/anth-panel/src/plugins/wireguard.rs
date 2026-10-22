//! Persistent WireGuard interfaces for the plugin job table.
//!
//! The guest never sees a command line. It supplies an interface name, an
//! optional private key, a port, addresses and an MTU; this module owns the
//! config file under `/etc/wireguard`, the `wg-quick`/`wg`/`ip` argv and the
//! secret handling. The config is written before the interface is brought up,
//! and a create that fails removes the file it just wrote.
//!
//! The private key is never returned. The only key in a job reply is the public
//! key `wg show <iface> public-key` prints.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::command::CommandRunner;
use crate::plugins::manifest::ArgType;

/// Where production keeps WireGuard configuration.
pub const DEFAULT_DIR: &str = "/etc/wireguard";

/// Runs one granted host job. `runner` owns sudo escalation.
pub fn run(
    job: &str,
    args: &BTreeMap<String, String>,
    runner: &dyn CommandRunner,
    root: &Path,
) -> Result<String, String> {
    match job {
        "wireguard.list" => list(root, runner),
        "wireguard.create" => create(args, runner, root),
        "wireguard.update" => update(args, runner, root),
        "wireguard.delete" => delete(args, runner, root),
        "wireguard.add_peer" => add_peer(args, runner, root),
        "wireguard.remove_peer" => remove_peer(args, runner, root),
        other => Err(format!("host job `{other}` is not implemented")),
    }
}

fn required<'a>(args: &'a BTreeMap<String, String>, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .map(String::as_str)
        .ok_or_else(|| format!("`{key}` is required"))
}

fn optional<'a>(args: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    args.get(key)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
}

fn valid_ifname(name: &str) -> Result<(), String> {
    ArgType::Ifname
        .validate(name)
        .map_err(|error| format!("invalid interface name: {error}"))
}

fn path_for(root: &Path, interface: &str) -> Result<PathBuf, String> {
    valid_ifname(interface)?;
    Ok(root.join(format!("{interface}.conf")))
}

fn create_private_dir(root: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root)
        .map_err(|error| format!("could not create {}: {error}", root.display()))
}

fn read_doc(path: &Path) -> Result<Document, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect {}: {error}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("{} is a symlink", path.display()));
    }
    let text = fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    Ok(Document::parse(&text))
}

fn write_doc(root: &Path, path: &Path, doc: &Document) -> Result<(), String> {
    create_private_dir(root)?;
    crate::config::write_private_atomic(path, &doc.render())
        .map_err(|error| format!("could not write {}: {error}", path.display()))
}

fn run_ok(runner: &dyn CommandRunner, program: &str, args: &[&str]) -> Result<String, String> {
    let output = runner
        .run(program, args)
        .map_err(|error| format!("{program}: {error:?}"))?;
    if !output.ok {
        return Err(format!(
            "{program} {} failed: {}",
            args.join(" "),
            output.stderr.trim()
        ));
    }
    Ok(output.stdout)
}

fn run_stdin_ok(
    runner: &dyn CommandRunner,
    program: &str,
    args: &[&str],
    input: &str,
) -> Result<String, String> {
    let output = runner
        .run_with_stdin(program, args, input)
        .map_err(|error| format!("{program}: {error:?}"))?;
    if !output.ok {
        return Err(format!(
            "{program} {} failed: {}",
            args.join(" "),
            output.stderr.trim()
        ));
    }
    Ok(output.stdout)
}

fn interface_names(runner: &dyn CommandRunner) -> Vec<String> {
    run_ok(runner, "wg", &["show", "interfaces"])
        .map(|stdout| stdout.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default()
}

fn is_up(runner: &dyn CommandRunner, interface: &str) -> bool {
    interface_names(runner).iter().any(|name| name == interface)
}

fn generate_key(runner: &dyn CommandRunner) -> Result<String, String> {
    let key = run_ok(runner, "wg", &["genkey"])?;
    let key = key.trim().to_string();
    ArgType::Base64Key
        .validate(&key)
        .map_err(|error| format!("`wg genkey` produced an invalid key: {error}"))?;
    Ok(key)
}

fn apply(runner: &dyn CommandRunner, interface: &str) -> Result<(), String> {
    if is_up(runner, interface) {
        let stripped = run_ok(runner, "wg-quick", &["strip", interface])?;
        run_stdin_ok(
            runner,
            "wg",
            &["syncconf", interface, "/dev/stdin"],
            &stripped,
        )?;
    } else {
        run_ok(runner, "wg-quick", &["up", interface])?;
    }
    Ok(())
}

fn list(root: &Path, runner: &dyn CommandRunner) -> Result<String, String> {
    let mut entries = Vec::new();
    let up = interface_names(runner);
    let dir = match fs::read_dir(root) {
        Ok(dir) => dir,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok("[]".to_string());
        }
        Err(error) => return Err(format!("could not read {}: {error}", root.display())),
    };
    for entry in dir {
        let entry = entry.map_err(|error| format!("could not read {}: {error}", root.display()))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("conf") {
            continue;
        }
        let Some(interface) = path.file_stem().and_then(|value| value.to_str()) else {
            continue;
        };
        if valid_ifname(interface).is_err() {
            continue;
        }
        match read_doc(&path) {
            Ok(doc) => entries.push(summary(
                &doc,
                interface,
                up.iter().any(|name| name == interface),
                runner,
            )),
            Err(error) => entries.push(json!({"interface": interface, "error": error})),
        }
    }
    entries.sort_by(|left, right| {
        left.get("interface")
            .and_then(Value::as_str)
            .cmp(&right.get("interface").and_then(Value::as_str))
    });
    serde_json::to_string(&entries).map_err(|error| error.to_string())
}

fn create(
    args: &BTreeMap<String, String>,
    runner: &dyn CommandRunner,
    root: &Path,
) -> Result<String, String> {
    let interface = required(args, "interface")?;
    let path = path_for(root, interface)?;
    if path.exists() {
        return Err(format!("`{interface}` already has a configuration"));
    }
    if is_up(runner, interface) {
        return Err(format!("interface `{interface}` already exists"));
    }

    let key = match optional(args, "private_key") {
        Some(key) => key.to_string(),
        None => generate_key(runner)?,
    };

    let mut doc = Document::default();
    doc.ensure_interface_preamble();
    doc.set_interface("PrivateKey", &key);
    if let Some(port) = optional(args, "listen_port") {
        doc.set_interface("ListenPort", port);
    }
    if let Some(addresses) = optional(args, "addresses") {
        doc.set_interface_list("Address", split_list(addresses));
    }
    if let Some(mtu) = optional(args, "mtu") {
        doc.set_interface("MTU", mtu);
    }

    write_doc(root, &path, &doc)?;
    if let Err(error) = run_ok(runner, "wg-quick", &["up", interface]) {
        let _ = fs::remove_file(&path);
        return Err(error);
    }
    Ok(summary_json(&doc, interface, true, runner))
}

fn update(
    args: &BTreeMap<String, String>,
    runner: &dyn CommandRunner,
    root: &Path,
) -> Result<String, String> {
    let interface = required(args, "interface")?;
    let path = path_for(root, interface)?;
    let mut doc = read_doc(&path)?;
    let old_addresses = doc.interface_list("Address");
    let was_up = is_up(runner, interface);

    if let Some(key) = optional(args, "private_key") {
        doc.set_interface("PrivateKey", key);
    }
    if let Some(port) = optional(args, "listen_port") {
        doc.set_interface("ListenPort", port);
    }
    let new_addresses = optional(args, "addresses").map(split_list);
    if let Some(addresses) = &new_addresses {
        doc.set_interface_list("Address", addresses.clone());
    }
    if let Some(mtu) = optional(args, "mtu") {
        doc.set_interface("MTU", mtu);
    }

    write_doc(root, &path, &doc)?;

    if was_up {
        let stripped = run_ok(runner, "wg-quick", &["strip", interface])?;
        run_stdin_ok(
            runner,
            "wg",
            &["syncconf", interface, "/dev/stdin"],
            &stripped,
        )?;
        if let Some(new_addresses) = &new_addresses {
            for address in &old_addresses {
                if !new_addresses.contains(address) {
                    let _ = runner.run("ip", &["address", "del", address, "dev", interface]);
                }
            }
            for address in new_addresses {
                run_ok(
                    runner,
                    "ip",
                    &["address", "replace", address, "dev", interface],
                )?;
            }
        }
        if let Some(mtu) = optional(args, "mtu") {
            run_ok(runner, "ip", &["link", "set", "dev", interface, "mtu", mtu])?;
        }
    } else {
        run_ok(runner, "wg-quick", &["up", interface])?;
    }

    Ok(summary_json(&doc, interface, true, runner))
}

fn delete(
    args: &BTreeMap<String, String>,
    runner: &dyn CommandRunner,
    root: &Path,
) -> Result<String, String> {
    let interface = required(args, "interface")?;
    let path = path_for(root, interface)?;
    if !path.exists() {
        return Err(format!("`{interface}` has no configuration"));
    }
    // Reject a symlink before `wg-quick down` reads through it.
    let _ = read_doc(&path)?;
    if is_up(runner, interface) {
        run_ok(runner, "wg-quick", &["down", interface])?;
    }
    fs::remove_file(&path)
        .map_err(|error| format!("could not remove {}: {error}", path.display()))?;
    if is_up(runner, interface) {
        let _ = runner.run("ip", &["link", "delete", "dev", interface]);
    }
    Ok(json!({"interface": interface, "removed": true}).to_string())
}

fn add_peer(
    args: &BTreeMap<String, String>,
    runner: &dyn CommandRunner,
    root: &Path,
) -> Result<String, String> {
    let interface = required(args, "interface")?;
    let path = path_for(root, interface)?;
    let mut doc = read_doc(&path)?;
    let public_key = required(args, "public_key")?;
    let allowed = required(args, "allowed_ips")?;

    doc.remove_peer(public_key);
    let mut peer = BTreeMap::new();
    peer.insert("PublicKey".to_string(), public_key.to_string());
    peer.insert("AllowedIPs".to_string(), allowed.to_string());
    if let Some(endpoint) = optional(args, "endpoint") {
        peer.insert("Endpoint".to_string(), endpoint.to_string());
    }
    doc.add_peer(&peer);
    write_doc(root, &path, &doc)?;
    if is_up(runner, interface) {
        apply(runner, interface)?;
    }
    Ok(summary_json(
        &doc,
        interface,
        is_up(runner, interface),
        runner,
    ))
}

fn remove_peer(
    args: &BTreeMap<String, String>,
    runner: &dyn CommandRunner,
    root: &Path,
) -> Result<String, String> {
    let interface = required(args, "interface")?;
    let path = path_for(root, interface)?;
    let mut doc = read_doc(&path)?;
    let public_key = required(args, "public_key")?;
    doc.remove_peer(public_key);
    write_doc(root, &path, &doc)?;
    if is_up(runner, interface) {
        apply(runner, interface)?;
    }
    Ok(summary_json(
        &doc,
        interface,
        is_up(runner, interface),
        runner,
    ))
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect()
}

fn summary_json(doc: &Document, interface: &str, up: bool, runner: &dyn CommandRunner) -> String {
    serde_json::to_string(&summary(doc, interface, up, runner)).unwrap_or_else(|_| "{}".to_string())
}

fn summary(doc: &Document, interface: &str, up: bool, runner: &dyn CommandRunner) -> Value {
    let listen_port = doc
        .interface_value("ListenPort")
        .and_then(|value| value.parse::<u64>().ok());
    let addresses = doc.interface_list("Address");
    let mtu = doc
        .interface_value("MTU")
        .and_then(|value| value.parse::<u64>().ok());
    let public_key = if up {
        run_ok(runner, "wg", &["show", interface, "public-key"])
            .ok()
            .map(|value| value.trim().to_string())
    } else {
        None
    };
    let peers: Vec<Value> = doc
        .peers()
        .into_iter()
        .map(|peer| {
            let allowed_ips = peer
                .get("AllowedIPs")
                .map(|value| split_list(value))
                .unwrap_or_default();
            json!({
                "public_key": peer.get("PublicKey").cloned().unwrap_or_default(),
                "allowed_ips": allowed_ips,
                "endpoint": peer.get("Endpoint").cloned(),
                "persistent_keepalive": peer.get("PersistentKeepalive").cloned(),
            })
        })
        .collect();
    json!({
        "interface": interface,
        "up": up,
        "public_key": public_key,
        "listen_port": listen_port,
        "addresses": addresses,
        "mtu": mtu,
        "peers": peers,
    })
}

/// A config file as lines, so comments and unknown keys survive an update.
#[derive(Debug, Clone, Default)]
struct Document {
    lines: Vec<String>,
}

impl Document {
    fn parse(text: &str) -> Self {
        let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
        if lines.last().is_some_and(|line| line.trim().is_empty()) {
            lines.pop();
        }
        Self { lines }
    }

    fn render(&self) -> String {
        let mut body = self.lines.join("\n");
        body.push('\n');
        body
    }

    fn ensure_interface_preamble(&mut self) {
        if self.interface_bounds().is_none() {
            self.lines.insert(0, "[Interface]".to_string());
        }
    }

    fn interface_bounds(&self) -> Option<(usize, usize)> {
        let start = self
            .lines
            .iter()
            .position(|line| line.trim().eq_ignore_ascii_case("[Interface]"))?;
        let end = self
            .lines
            .iter()
            .enumerate()
            .skip(start + 1)
            .find(|(_, line)| is_section(line))
            .map(|(index, _)| index)
            .unwrap_or(self.lines.len());
        Some((start, end))
    }

    fn interface_value(&self, key: &str) -> Option<String> {
        let (start, end) = self.interface_bounds()?;
        self.lines[start + 1..end].iter().find_map(|line| {
            let (name, value) = split_key_value(line)?;
            name.eq_ignore_ascii_case(key).then(|| value.to_string())
        })
    }

    fn interface_list(&self, key: &str) -> Vec<String> {
        let Some((start, end)) = self.interface_bounds() else {
            return Vec::new();
        };
        let mut values = Vec::new();
        for line in &self.lines[start + 1..end] {
            if let Some((name, value)) = split_key_value(line)
                && name.eq_ignore_ascii_case(key)
            {
                values.extend(split_list(value));
            }
        }
        values
    }

    fn set_interface(&mut self, key: &str, value: &str) {
        self.ensure_interface_preamble();
        let Some((start, end)) = self.interface_bounds() else {
            return;
        };
        for index in start + 1..end {
            if let Some((name, _)) = split_key_value(&self.lines[index])
                && name.eq_ignore_ascii_case(key)
            {
                self.lines[index] = format!("{key} = {value}");
                return;
            }
        }
        self.lines.insert(end, format!("{key} = {value}"));
    }

    fn set_interface_list(&mut self, key: &str, values: Vec<String>) {
        self.ensure_interface_preamble();
        let Some((start, mut end)) = self.interface_bounds() else {
            return;
        };
        let mut index = start + 1;
        while index < end {
            let matches = split_key_value(&self.lines[index])
                .is_some_and(|(name, _)| name.eq_ignore_ascii_case(key));
            if matches {
                self.lines.remove(index);
                end -= 1;
            } else {
                index += 1;
            }
        }
        for (offset, value) in values.into_iter().enumerate() {
            self.lines
                .insert(start + 1 + offset, format!("{key} = {value}"));
        }
    }

    fn peers(&self) -> Vec<BTreeMap<String, String>> {
        self.peer_ranges()
            .into_iter()
            .map(|(_, _, values)| values)
            .collect()
    }

    fn peer_ranges(&self) -> Vec<(usize, usize, BTreeMap<String, String>)> {
        let mut peers = Vec::new();
        let mut index = 0;
        while index < self.lines.len() {
            if !self.lines[index].trim().eq_ignore_ascii_case("[Peer]") {
                index += 1;
                continue;
            }
            let start = index;
            index += 1;
            let mut values = BTreeMap::new();
            while index < self.lines.len() && !is_section(&self.lines[index]) {
                if let Some((key, value)) = split_key_value(&self.lines[index]) {
                    values.insert(key.to_string(), value.to_string());
                }
                index += 1;
            }
            peers.push((start, index, values));
        }
        peers
    }

    fn add_peer(&mut self, peer: &BTreeMap<String, String>) {
        if !self.lines.is_empty() && !self.lines.last().is_some_and(|line| line.trim().is_empty()) {
            self.lines.push(String::new());
        }
        self.lines.push("[Peer]".to_string());
        for (key, value) in peer {
            self.lines.push(format!("{key} = {value}"));
        }
    }

    fn remove_peer(&mut self, public_key: &str) {
        let ranges = self.peer_ranges();
        for (start, end, values) in ranges.into_iter().rev() {
            if values
                .get("PublicKey")
                .is_some_and(|value| value == public_key)
            {
                self.lines.drain(start..end);
                if start > 0 && self.lines[start - 1].trim().is_empty() {
                    self.lines.remove(start - 1);
                }
            }
        }
    }
}

fn is_section(line: &str) -> bool {
    let line = line.trim();
    line.starts_with('[') && line.ends_with(']')
}

fn split_key_value(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once('=')?;
    Some((key.trim(), value.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandError, CommandOutput};
    use std::cell::RefCell;
    use std::os::unix::fs::PermissionsExt;

    const KEY: &str = "yAnz5TF+lXXJte14tji3zlMNq+hd2rYUIgJBgB3fBmk=";
    const PUBLIC: &str = "xTIBA5rboUvnH4htodjb6e697QjLERt1NAB4mZqp8Dg=";

    struct FakeRunner {
        up: Vec<String>,
        fail_up: bool,
        calls: RefCell<Vec<(String, Vec<String>)>>,
        stdins: RefCell<Vec<String>>,
    }

    impl FakeRunner {
        fn new(up: &[&str]) -> Self {
            Self {
                up: up.iter().map(|name| (*name).to_string()).collect(),
                fail_up: false,
                calls: RefCell::new(Vec::new()),
                stdins: RefCell::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<(String, Vec<String>)> {
            self.calls.borrow().clone()
        }
    }

    impl CommandRunner for FakeRunner {
        fn run(&self, program: &str, args: &[&str]) -> Result<CommandOutput, CommandError> {
            self.calls.borrow_mut().push((
                program.to_string(),
                args.iter().map(|value| (*value).to_string()).collect(),
            ));
            if program == "wg-quick" && args.first() == Some(&"up") && self.fail_up {
                return Ok(CommandOutput {
                    ok: false,
                    stdout: String::new(),
                    stderr: "up failed".to_string(),
                });
            }
            let stdout = if program == "wg" && args == ["show", "interfaces"] {
                self.up.join(" ")
            } else if program == "wg" && args == ["genkey"] {
                KEY.to_string()
            } else if program == "wg"
                && args.first() == Some(&"show")
                && args.get(2) == Some(&"public-key")
            {
                PUBLIC.to_string()
            } else if program == "wg-quick" && args.first() == Some(&"strip") {
                "[Interface]\nPrivateKey = old\n".to_string()
            } else {
                String::new()
            };
            Ok(CommandOutput {
                ok: true,
                stdout,
                stderr: String::new(),
            })
        }

        fn run_with_stdin(
            &self,
            program: &str,
            args: &[&str],
            input: &str,
        ) -> Result<CommandOutput, CommandError> {
            self.calls.borrow_mut().push((
                program.to_string(),
                args.iter().map(|value| (*value).to_string()).collect(),
            ));
            self.stdins.borrow_mut().push(input.to_string());
            Ok(CommandOutput {
                ok: true,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    fn root(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "anth-wireguard-{name}-{}-{nanos}",
            std::process::id()
        ))
    }

    fn args(values: &[(&str, &str)]) -> BTreeMap<String, String> {
        values
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect()
    }

    #[test]
    fn create_generates_a_key_writes_a_private_config_and_returns_only_the_public_key() {
        let root = root("create");
        let runner = FakeRunner::new(&[]);
        let body = run(
            "wireguard.create",
            &args(&[
                ("interface", "wg0"),
                ("listen_port", "51820"),
                ("addresses", "10.0.0.1/24"),
                ("mtu", "1420"),
            ]),
            &runner,
            &root,
        )
        .unwrap();

        let path = root.join("wg0.conf");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(&format!("PrivateKey = {KEY}")), "{text}");
        assert!(text.contains("ListenPort = 51820"), "{text}");
        assert!(text.contains("Address = 10.0.0.1/24"), "{text}");
        assert!(text.contains("MTU = 1420"), "{text}");
        assert!(
            !body.contains(KEY),
            "the private key must not leave the host"
        );
        assert!(body.contains(PUBLIC), "{body}");
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(
            runner
                .calls()
                .iter()
                .any(|(program, argv)| program == "wg-quick" && argv == &["up", "wg0"])
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_failed_create_removes_the_config_it_wrote() {
        let root = root("rollback");
        let mut runner = FakeRunner::new(&[]);
        runner.fail_up = true;

        let error = run(
            "wireguard.create",
            &args(&[("interface", "wg0"), ("private_key", KEY)]),
            &runner,
            &root,
        )
        .unwrap_err();

        assert!(error.contains("up failed"), "{error}");
        assert!(!root.join("wg0.conf").exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn update_preserves_the_private_key_and_peers_when_they_are_omitted() {
        let root = root("update");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("wg0.conf");
        fs::write(
            &path,
            format!(
                "[Interface]\nPrivateKey = {KEY}\nAddress = 10.0.0.1/24\nListenPort = 51820\n\n[Peer]\nPublicKey = {PUBLIC}\nAllowedIPs = 10.0.0.2/32\n"
            ),
        )
        .unwrap();
        let runner = FakeRunner::new(&["wg0"]);

        let body = run(
            "wireguard.update",
            &args(&[
                ("interface", "wg0"),
                ("listen_port", "51821"),
                ("addresses", "10.0.0.9/24"),
                ("mtu", "1400"),
            ]),
            &runner,
            &root,
        )
        .unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains(&format!("PrivateKey = {KEY}")), "{text}");
        assert!(text.contains("ListenPort = 51821"), "{text}");
        assert!(text.contains("Address = 10.0.0.9/24"), "{text}");
        assert!(text.contains("MTU = 1400"), "{text}");
        assert!(text.contains(&format!("PublicKey = {PUBLIC}")), "{text}");
        assert!(body.contains("\"up\":true"), "{body}");
        assert_eq!(runner.stdins.borrow().len(), 1);
        assert!(
            runner.calls().iter().any(
                |(program, argv)| program == "wg" && argv == &["syncconf", "wg0", "/dev/stdin"]
            )
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn peers_are_written_to_the_config_and_removed_again() {
        let root = root("peers");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("wg0.conf");
        fs::write(&path, format!("[Interface]\nPrivateKey = {KEY}\n")).unwrap();
        let runner = FakeRunner::new(&["wg0"]);

        run(
            "wireguard.add_peer",
            &args(&[
                ("interface", "wg0"),
                ("public_key", PUBLIC),
                ("allowed_ips", "10.0.0.2/32"),
                ("endpoint", "vpn.example:51820"),
            ]),
            &runner,
            &root,
        )
        .unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("[Peer]"), "{text}");
        assert!(text.contains(&format!("PublicKey = {PUBLIC}")), "{text}");
        assert!(text.contains("AllowedIPs = 10.0.0.2/32"), "{text}");
        assert!(text.contains("Endpoint = vpn.example:51820"), "{text}");

        run(
            "wireguard.remove_peer",
            &args(&[("interface", "wg0"), ("public_key", PUBLIC)]),
            &runner,
            &root,
        )
        .unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.contains("[Peer]"), "{text}");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn delete_brings_the_interface_down_and_removes_its_config() {
        let root = root("delete");
        fs::create_dir_all(&root).unwrap();
        let path = root.join("wg0.conf");
        fs::write(&path, "[Interface]\n").unwrap();
        let runner = FakeRunner::new(&["wg0"]);

        let body = run(
            "wireguard.delete",
            &args(&[("interface", "wg0")]),
            &runner,
            &root,
        )
        .unwrap();

        assert!(!path.exists());
        assert!(body.contains("\"removed\":true"), "{body}");
        assert!(
            runner
                .calls()
                .iter()
                .any(|(program, argv)| program == "wg-quick" && argv == &["down", "wg0"])
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn list_includes_configured_but_down_interfaces_without_the_private_key() {
        let root = root("list");
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("wg0.conf"),
            format!("[Interface]\nPrivateKey = {KEY}\nAddress = 10.0.0.1/24\nListenPort = 51820\n"),
        )
        .unwrap();
        let runner = FakeRunner::new(&[]);

        let body = run("wireguard.list", &args(&[]), &runner, &root).unwrap();

        assert!(body.contains("\"interface\":\"wg0\""), "{body}");
        assert!(body.contains("\"up\":false"), "{body}");
        assert!(body.contains("10.0.0.1/24"), "{body}");
        assert!(!body.contains(KEY), "the private key must stay in the file");
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn an_interface_name_cannot_escape_the_config_directory() {
        let root = root("escape");
        let error = run(
            "wireguard.create",
            &args(&[("interface", "../shadow")]),
            &FakeRunner::new(&[]),
            &root,
        )
        .unwrap_err();
        assert!(error.contains("invalid interface name"), "{error}");
    }
}
