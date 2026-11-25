//! Mapping between the Docker JSON documents and the WIT records, plus the
//! create-request validation: no bind mounts, no privileged mode, no host
//! namespaces.

use anth_contract::Error;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Label {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EnvVar {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VolumeMount {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CreateRequest {
    pub image: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub command: Vec<String>,
    #[serde(default)]
    pub env: Vec<EnvVar>,
    #[serde(default)]
    pub ports: Vec<String>,
    #[serde(default)]
    pub volumes: Vec<VolumeMount>,
    #[serde(default)]
    pub network: Option<String>,
    #[serde(default = "default_restart")]
    pub restart: String,
    #[serde(default)]
    pub labels: Vec<Label>,
}

fn default_restart() -> String {
    "no".to_string()
}

struct Port {
    container: u16,
    protocol: String,
    host: Option<(String, u16)>,
}

impl CreateRequest {
    /// Validates and returns the Docker create body plus the optional name.
    pub fn body(&self) -> Result<(Option<String>, Value), Error> {
        if self.image.trim().is_empty() {
            return Err(Error::invalid("`image` is required"));
        }
        if self
            .image
            .chars()
            .any(|character| character.is_whitespace() || character == '\0')
        {
            return Err(Error::invalid("`image` is not a reference"));
        }
        if let Some(name) = &self.name {
            if !valid_name(name) {
                return Err(Error::invalid(format!("`{name}` is not a container name")));
            }
        }
        let mut env = Vec::with_capacity(self.env.len());
        for entry in &self.env {
            if entry.name.is_empty()
                || entry.name.contains('=')
                || entry.name.contains('\0')
                || entry.value.contains('\0')
            {
                return Err(Error::invalid(format!(
                    "`{}` is not a usable environment variable name",
                    entry.name
                )));
            }
            env.push(format!("{}={}", entry.name, entry.value));
        }
        let mut labels = Map::new();
        for label in &self.labels {
            if label.name.is_empty() || label.name.contains('\0') || label.value.contains('\0') {
                return Err(Error::invalid(format!(
                    "`{}` is not a usable label",
                    label.name
                )));
            }
            labels.insert(label.name.clone(), Value::String(label.value.clone()));
        }
        if let Some(network) = &self.network {
            if network == "host" || network.starts_with("container:") {
                return Err(Error::invalid(
                    "host and shared network namespaces are refused",
                ));
            }
            if !valid_name(network) {
                return Err(Error::invalid(format!("`{network}` is not a network name")));
            }
        }
        let (restart, retries) = restart_policy(&self.restart)?;
        let mut ports = Vec::with_capacity(self.ports.len());
        for port in &self.ports {
            ports.push(parse_port(port)?);
        }
        let mut mounts = Vec::with_capacity(self.volumes.len());
        for volume in &self.volumes {
            if !valid_name(&volume.name) {
                return Err(Error::invalid(format!(
                    "`{}` is not a named volume; bind mounts are refused",
                    volume.name
                )));
            }
            if !volume.path.starts_with('/')
                || volume.path.split('/').any(|segment| segment == "..")
                || volume.path.contains('\0')
            {
                return Err(Error::invalid(format!(
                    "`{}` is not an absolute container path",
                    volume.path
                )));
            }
            mounts.push(json!({
                "Type": "volume",
                "Source": volume.name,
                "Target": volume.path,
            }));
        }
        if self.command.iter().any(|part| part.contains('\0')) {
            return Err(Error::invalid("a command argument contains a NUL byte"));
        }

        let mut exposed = Map::new();
        let mut bindings = Map::new();
        for port in &ports {
            let key = format!("{}/{}", port.container, port.protocol);
            exposed.insert(key.clone(), json!({}));
            if let Some((ip, host)) = &port.host {
                bindings.insert(key, json!([{"HostIp": ip, "HostPort": host.to_string()}]));
            }
        }
        let mut host = Map::new();
        host.insert("Privileged".to_string(), json!(false));
        host.insert("PublishAllPorts".to_string(), json!(false));
        host.insert("Mounts".to_string(), Value::Array(mounts));
        host.insert(
            "RestartPolicy".to_string(),
            json!({"Name": restart, "MaximumRetryCount": retries}),
        );
        if !bindings.is_empty() {
            host.insert("PortBindings".to_string(), Value::Object(bindings));
        }
        if let Some(network) = &self.network {
            host.insert("NetworkMode".to_string(), Value::String(network.clone()));
        }
        let mut body = Map::new();
        body.insert("Image".to_string(), Value::String(self.image.clone()));
        if !self.command.is_empty() {
            body.insert("Cmd".to_string(), json!(self.command));
        }
        if !env.is_empty() {
            body.insert("Env".to_string(), json!(env));
        }
        if !labels.is_empty() {
            body.insert("Labels".to_string(), Value::Object(labels));
        }
        if !exposed.is_empty() {
            body.insert("ExposedPorts".to_string(), Value::Object(exposed));
        }
        body.insert("HostConfig".to_string(), Value::Object(host));
        if let Some(network) = &self.network {
            body.insert(
                "NetworkingConfig".to_string(),
                json!({"EndpointsConfig": {network.clone(): {}}}),
            );
        }
        Ok((self.name.clone(), Value::Object(body)))
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && !name.starts_with('.')
        && !name.starts_with('-')
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "_.-".contains(character))
}

fn restart_policy(value: &str) -> Result<(String, u64), Error> {
    let value = value.trim();
    if value.is_empty() || value == "no" {
        return Ok(("no".to_string(), 0));
    }
    for name in ["always", "unless-stopped"] {
        if value == name {
            return Ok((name.to_string(), 0));
        }
    }
    if let Some(retries) = value.strip_prefix("on-failure") {
        let retries = retries
            .strip_prefix(':')
            .unwrap_or("0")
            .parse::<u64>()
            .map_err(|_| Error::invalid("`restart` has an invalid retry count"))?;
        return Ok(("on-failure".to_string(), retries));
    }
    Err(Error::invalid(format!(
        "`{value}` is not a restart policy; use no, always, unless-stopped or on-failure[:N]"
    )))
}

fn parse_port(value: &str) -> Result<Port, Error> {
    let (value, protocol) = match value.rsplit_once('/') {
        Some((value, protocol)) => {
            let protocol = protocol.to_ascii_lowercase();
            if !matches!(protocol.as_str(), "tcp" | "udp" | "sctp") {
                return Err(Error::invalid(format!(
                    "`{protocol}` is not a port protocol"
                )));
            }
            (value, protocol)
        }
        None => (value, "tcp".to_string()),
    };
    let parts: Vec<&str> = value.split(':').collect();
    let number = |value: &str| -> Result<u16, Error> {
        let port = value
            .parse::<u16>()
            .map_err(|_| Error::invalid(format!("`{value}` is not a port")))?;
        if port == 0 {
            return Err(Error::invalid("port 0 is not usable"));
        }
        Ok(port)
    };
    match parts.as_slice() {
        [container] => Ok(Port {
            container: number(container)?,
            protocol,
            host: None,
        }),
        [host, container] => Ok(Port {
            container: number(container)?,
            protocol,
            host: Some(("0.0.0.0".to_string(), number(host)?)),
        }),
        [ip, host, container] => {
            if ip.parse::<std::net::IpAddr>().is_err() {
                return Err(Error::invalid(format!("`{ip}` is not a bind address")));
            }
            Ok(Port {
                container: number(container)?,
                protocol,
                host: Some((ip.to_string(), number(host)?)),
            })
        }
        _ => Err(Error::invalid(format!("`{value}` is not a port mapping"))),
    }
}

pub fn containers(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .map(|entries| entries.iter().map(container_from_list).collect())
        .unwrap_or_default()
}

fn container_from_list(entry: &Value) -> Value {
    let name = entry
        .get("Names")
        .and_then(Value::as_array)
        .and_then(|names| names.first())
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim_start_matches('/')
        .to_string();
    json!({
        "id": entry.get("Id").and_then(Value::as_str).unwrap_or(""),
        "name": name,
        "image": entry.get("Image").and_then(Value::as_str).unwrap_or(""),
        "state": entry.get("State").and_then(Value::as_str).unwrap_or("unknown"),
        "status": entry.get("Status").and_then(Value::as_str).unwrap_or(""),
        "health": null,
        "created-at": entry.get("Created").and_then(Value::as_u64).unwrap_or(0),
        "ports": ports_from_list(entry.get("Ports")),
        "labels": labels(entry.get("Labels")),
    })
}

pub fn container_from_inspect(entry: &Value) -> Value {
    let name = entry
        .get("Name")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim_start_matches('/')
        .to_string();
    let config = entry.get("Config").cloned().unwrap_or(Value::Null);
    let state = entry.get("State").cloned().unwrap_or(Value::Null);
    let health = state
        .pointer("/Health/Status")
        .and_then(Value::as_str)
        .map(|value| Value::String(value.to_string()))
        .unwrap_or(Value::Null);
    json!({
        "id": entry.get("Id").and_then(Value::as_str).unwrap_or(""),
        "name": name,
        "image": config.get("Image").and_then(Value::as_str).unwrap_or(""),
        "state": state.get("Status").and_then(Value::as_str).unwrap_or("unknown"),
        "status": state.get("Status").and_then(Value::as_str).unwrap_or(""),
        "health": health,
        "created-at": entry.get("Created").and_then(Value::as_str).and_then(timestamp).unwrap_or(0),
        "ports": ports_from_map(entry.pointer("/NetworkSettings/Ports")),
        "labels": labels(config.get("Labels")),
    })
}

fn ports_from_list(value: Option<&Value>) -> Vec<Value> {
    value
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    let private = entry
                        .get("PrivatePort")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let public = entry.get("PublicPort").and_then(Value::as_u64);
                    let kind = entry.get("Type").and_then(Value::as_str).unwrap_or("tcp");
                    let ip = entry.get("IP").and_then(Value::as_str).unwrap_or("0.0.0.0");
                    match public {
                        Some(public) => Value::String(format!("{ip}:{public}->{private}/{kind}")),
                        None => Value::String(format!("{private}/{kind}")),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

fn ports_from_map(value: Option<&Value>) -> Vec<Value> {
    let Some(map) = value.and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut ports = Vec::new();
    for (key, entries) in map {
        match entries.as_array().and_then(|entries| entries.first()) {
            Some(entry) => {
                let ip = entry
                    .get("HostIp")
                    .and_then(Value::as_str)
                    .unwrap_or("0.0.0.0");
                let host = entry.get("HostPort").and_then(Value::as_str).unwrap_or("");
                ports.push(Value::String(format!("{ip}:{host}->{key}")));
            }
            None => ports.push(Value::String(key.clone())),
        }
    }
    ports
}

pub fn images(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    let tags = entry
                        .get("RepoTags")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    let digest = entry
                        .get("RepoDigests")
                        .and_then(Value::as_array)
                        .and_then(|entries| entries.first())
                        .and_then(Value::as_str)
                        .and_then(|value| {
                            value.split_once('@').map(|(_, digest)| digest.to_string())
                        })
                        .map(Value::String)
                        .unwrap_or(Value::Null);
                    json!({
                        "id": entry.get("Id").and_then(Value::as_str).unwrap_or(""),
                        "tags": tags,
                        "size": entry.get("Size").and_then(Value::as_u64).unwrap_or(0),
                        "created-at": entry.get("Created").and_then(Value::as_u64).unwrap_or(0),
                        "digest": digest,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn volumes(value: &Value) -> Vec<Value> {
    value
        .get("Volumes")
        .and_then(Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    json!({
                        "name": entry.get("Name").and_then(Value::as_str).unwrap_or(""),
                        "driver": entry.get("Driver").and_then(Value::as_str).unwrap_or(""),
                        "labels": labels(entry.get("Labels")),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn networks(value: &Value) -> Vec<Value> {
    value
        .as_array()
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    let subnet = entry
                        .pointer("/IPAM/Config")
                        .and_then(Value::as_array)
                        .and_then(|entries| entries.first())
                        .and_then(|config| config.get("Subnet"))
                        .and_then(Value::as_str)
                        .map(|subnet| Value::String(subnet.to_string()))
                        .unwrap_or(Value::Null);
                    let containers = entry
                        .get("Containers")
                        .and_then(Value::as_object)
                        .map(|entries| entries.len() as u64)
                        .unwrap_or(0);
                    json!({
                        "name": entry.get("Name").and_then(Value::as_str).unwrap_or(""),
                        "driver": entry.get("Driver").and_then(Value::as_str).unwrap_or(""),
                        "subnet": subnet,
                        "containers": containers,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn labels(value: Option<&Value>) -> Value {
    let entries = value
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .map(|(name, value)| json!({"name": name, "value": value.as_str().unwrap_or("")}))
                .collect()
        })
        .unwrap_or_default();
    Value::Array(entries)
}

/// RFC 3339 (with or without fractional seconds) to Unix seconds.
fn timestamp(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    let number = |start: usize, end: usize| value.get(start..end)?.parse::<i64>().ok();
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    let mut offset = 0i64;
    if let Some(rest) = value.get(19..) {
        let rest = rest
            .trim_start_matches('.')
            .trim_start_matches(|c: char| c.is_ascii_digit());
        if let Some(sign) = rest.chars().next().filter(|c| *c == '+' || *c == '-') {
            let hours = rest.get(1..3)?.parse::<i64>().ok()?;
            let minutes = rest.get(4..6)?.parse::<i64>().ok()?;
            offset = (hours * 3600 + minutes * 60) * if sign == '-' { -1 } else { 1 };
        }
    }
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted = (month + 9) % 12;
    let day_of_year = (153 * shifted + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some((days * 86_400 + hour * 3600 + minute * 60 + second - offset).max(0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> CreateRequest {
        serde_json::from_value(json!({
            "image": "nginx:1.27",
            "name": "web",
            "command": ["nginx", "-g", "daemon off;"],
            "env": [{"name": "TZ", "value": "UTC"}],
            "ports": ["8080:80"],
            "volumes": [{"name": "site", "path": "/usr/share/nginx/html"}],
            "network": "app",
            "restart": "unless-stopped",
            "labels": [{"name": "com.example", "value": "web"}]
        }))
        .expect("request")
    }

    #[test]
    fn a_valid_request_becomes_a_named_volume_container() {
        let (name, body) = request().body().expect("body");
        assert_eq!(name.as_deref(), Some("web"));
        assert_eq!(body["HostConfig"]["Privileged"], false);
        assert_eq!(body["HostConfig"]["Mounts"][0]["Type"], "volume");
        assert_eq!(body["HostConfig"]["NetworkMode"], "app");
        assert_eq!(
            body["HostConfig"]["PortBindings"]["80/tcp"][0]["HostPort"],
            "8080"
        );
        assert!(body["HostConfig"].get("Binds").is_none());
    }

    #[test]
    fn a_bind_mount_is_refused() {
        let mut value = serde_json::to_value(request()).expect("json");
        value["volumes"] = json!([{"name": "/etc", "path": "/etc"}]);
        let request: CreateRequest = serde_json::from_value(value).expect("request");
        assert!(request.body().is_err());
    }

    #[test]
    fn an_unknown_privileged_field_is_refused() {
        let value = json!({"image": "nginx", "privileged": true});
        assert!(serde_json::from_value::<CreateRequest>(value).is_err());
    }

    #[test]
    fn a_host_network_is_refused() {
        let mut value = serde_json::to_value(request()).expect("json");
        value["network"] = json!("host");
        let request: CreateRequest = serde_json::from_value(value).expect("request");
        let error = request.body().expect_err("host namespace");
        assert!(error.to_string().contains("host"));
    }

    #[test]
    fn timestamps_parse() {
        assert_eq!(timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            timestamp("2026-11-25T12:00:00.123456789Z"),
            Some(1_795_608_000)
        );
    }

    #[test]
    fn a_container_list_maps_ports_and_labels() {
        let value = json!([{
            "Id": "abc",
            "Names": ["/web"],
            "Image": "nginx",
            "State": "running",
            "Status": "Up 2 minutes",
            "Created": 10,
            "Ports": [{"IP": "0.0.0.0", "PrivatePort": 80, "PublicPort": 8080, "Type": "tcp"}],
            "Labels": {"com.example": "web"}
        }]);
        let list = containers(&value);
        assert_eq!(list[0]["name"], "web");
        assert_eq!(list[0]["ports"][0], "0.0.0.0:8080->80/tcp");
        assert_eq!(list[0]["labels"][0]["name"], "com.example");
    }
}
