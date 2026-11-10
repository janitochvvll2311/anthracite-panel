//! Host information for the Home app.
//!
//! This is a native service on purpose: the engine and the sandboxed panel views
//! never receive host facts. Reading `/proc`, `df` and `ip` lives here, behind
//! the approved `host` service class.

use std::collections::BTreeMap;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

use anth_contract::Error;
use anth_service::{ChunkStream, Handler};
use serde_json::{json, Value};

const HOST: &str = "anthracite:home/host@1.0.0";

struct Home;

impl Handler for Home {
    fn call(&self, interface: &str, function: &str, _input: Value) -> Result<Value, Error> {
        match (interface, function) {
            (HOST, "info") => Ok(info()),
            _ => Err(Error::not_found(format!(
                "`{interface}#{function}` is not implemented"
            ))),
        }
    }

    fn stream(&self, interface: &str, function: &str, input: Value) -> Result<ChunkStream, Error> {
        match (interface, function) {
            (HOST, "watch") => home_service::watch(&input, info),
            _ => Err(Error::not_found(format!(
                "`{interface}#{function}` is not implemented"
            ))),
        }
    }
}

fn main() {
    home_service::serve(Arc::new(Home));
}
fn info() -> Value {
    json!({
        "hostname": hostname(),
        "os": os_name(),
        "kernel": kernel(),
        "arch": std::env::consts::ARCH,
        "uptime-seconds": uptime_seconds(),
        "cpu": cpu_info(),
        "memory": memory_info(),
        "disks": disks(),
        "networks": networks(),
    })
}

fn read(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn first_line(path: &str, fallback: &str) -> String {
    let value = read(path);
    let value = value.lines().next().unwrap_or("").trim();
    if value.is_empty() {
        fallback.to_string()
    } else {
        value.to_string()
    }
}

fn hostname() -> String {
    let name = first_line("/proc/sys/kernel/hostname", "");
    if !name.is_empty() {
        return name;
    }
    std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string())
}

fn os_name() -> String {
    for line in read("/etc/os-release").lines() {
        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
            return value.trim().trim_matches('"').to_string();
        }
    }
    std::env::consts::OS.to_string()
}

fn kernel() -> String {
    let value = first_line("/proc/sys/kernel/osrelease", "");
    if !value.is_empty() {
        return value;
    }
    Command::new("uname")
        .arg("-r")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

fn uptime_seconds() -> u64 {
    let value = read("/proc/uptime");
    value
        .split_whitespace()
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0) as u64
}

fn cpu_info() -> Value {
    let cpuinfo = read("/proc/cpuinfo");
    let model = cpuinfo
        .lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == "model name").then(|| value.trim().to_string())
        })
        .or_else(|| {
            cpuinfo.lines().find_map(|line| {
                let (key, value) = line.split_once(':')?;
                (key.trim() == "Hardware").then(|| value.trim().to_string())
            })
        })
        .unwrap_or_else(|| "Unknown".to_string());
    let detected = cpuinfo
        .lines()
        .filter(|line| line.starts_with("processor"))
        .count();
    let cores = if detected > 0 {
        detected as u32
    } else {
        std::thread::available_parallelism()
            .map(|count| count.get() as u32)
            .unwrap_or(1)
    };
    let (load_1, load_5, load_15) = load_average();
    json!({
        "model": model,
        "cores": cores,
        "usage-percent": cpu_usage(),
        "load-1": load_1,
        "load-5": load_5,
        "load-15": load_15,
    })
}

fn cpu_times() -> Option<(u64, u64)> {
    let stat = read("/proc/stat");
    let line = stat.lines().find(|line| line.starts_with("cpu "))?;
    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|field| field.parse().ok())
        .collect();
    if fields.len() < 4 {
        return None;
    }
    let total: u64 = fields.iter().take(8).sum();
    let idle = fields[3] + fields.get(4).copied().unwrap_or(0);
    Some((total, idle))
}

fn cpu_usage() -> f64 {
    let Some((total_before, idle_before)) = cpu_times() else {
        return 0.0;
    };
    std::thread::sleep(Duration::from_millis(120));
    let Some((total_after, idle_after)) = cpu_times() else {
        return 0.0;
    };
    let total = total_after.saturating_sub(total_before) as f64;
    let idle = idle_after.saturating_sub(idle_before) as f64;
    if total <= 0.0 {
        0.0
    } else {
        ((1.0 - idle / total) * 100.0).clamp(0.0, 100.0)
    }
}

fn load_average() -> (f64, f64, f64) {
    let value = read("/proc/loadavg");
    let mut fields = value.split_whitespace();
    let one = fields
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0);
    let five = fields
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0);
    let fifteen = fields
        .next()
        .and_then(|value| value.parse::<f64>().ok())
        .unwrap_or(0.0);
    (one, five, fifteen)
}

fn memory_info() -> Value {
    let meminfo = read("/proc/meminfo");
    let mut values = BTreeMap::new();
    for line in meminfo.lines() {
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let kib = rest
            .split_whitespace()
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);
        values.insert(key.trim().to_string(), kib.saturating_mul(1024));
    }
    let total = values.get("MemTotal").copied().unwrap_or(0);
    let available = values
        .get("MemAvailable")
        .or_else(|| values.get("MemFree"))
        .copied()
        .unwrap_or(0);
    let swap_total = values.get("SwapTotal").copied().unwrap_or(0);
    let swap_free = values.get("SwapFree").copied().unwrap_or(0);
    json!({
        "total": total,
        "used": total.saturating_sub(available),
        "available": available,
        "swap-total": swap_total,
        "swap-used": swap_total.saturating_sub(swap_free),
    })
}

fn disks() -> Vec<Value> {
    let Ok(output) = Command::new("df").args(["-B1", "-P", "-T"]).output() else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut disks = Vec::new();
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 7 {
            continue;
        }
        let file_system = fields[1];
        let mount = fields[6..].join(" ");
        if pseudo(file_system) || pseudo_mount(&mount) || is_file(&mount) {
            continue;
        }
        let Ok(total) = fields[2].parse::<u64>() else {
            continue;
        };
        let Ok(used) = fields[3].parse::<u64>() else {
            continue;
        };
        let Ok(available) = fields[4].parse::<u64>() else {
            continue;
        };
        disks.push(json!({
            "name": fields[0],
            "mount": mount,
            "file-system": file_system,
            "total": total,
            "used": used,
            "available": available,
        }));
    }
    disks
}

fn pseudo(file_system: &str) -> bool {
    matches!(
        file_system,
        "tmpfs"
            | "devtmpfs"
            | "proc"
            | "sysfs"
            | "cgroup"
            | "cgroup2"
            | "squashfs"
            | "ramfs"
            | "devpts"
            | "mqueue"
            | "hugetlbfs"
            | "securityfs"
            | "debugfs"
            | "tracefs"
            | "pstore"
            | "bpf"
            | "autofs"
            | "configfs"
            | "fusectl"
            | "nsfs"
            | "efivarfs"
            | "binfmt_misc"
    )
}

/// Bind mounts of single files (`/etc/hostname`, `/etc/resolv.conf`) are not
/// storage: `df` lists the whole backing device for each one.
fn is_file(mount: &str) -> bool {
    std::fs::metadata(mount)
        .map(|meta| meta.is_file())
        .unwrap_or(false)
}

fn pseudo_mount(mount: &str) -> bool {
    ["/proc", "/sys", "/dev", "/run"]
        .iter()
        .any(|prefix| mount == *prefix || mount.starts_with(&format!("{prefix}/")))
}

#[derive(Default)]
struct Network {
    name: String,
    mac: String,
    addresses: Vec<String>,
    state: String,
    received: u64,
    transmitted: u64,
    packets_received: u64,
    packets_transmitted: u64,
    errors: u64,
}

fn networks() -> Vec<Value> {
    let mut found: BTreeMap<String, Network> = BTreeMap::new();
    if let Ok(output) = Command::new("ip").args(["-j", "addr", "show"]).output() {
        if output.status.success() {
            if let Ok(Value::Array(items)) = serde_json::from_slice::<Value>(&output.stdout) {
                for item in items {
                    let Some(name) = item.get("ifname").and_then(Value::as_str) else {
                        continue;
                    };
                    let entry = found.entry(name.to_string()).or_insert_with(|| Network {
                        name: name.to_string(),
                        ..Network::default()
                    });
                    if let Some(mac) = item.get("address").and_then(Value::as_str) {
                        entry.mac = mac.to_string();
                    }
                    if let Some(state) = item.get("operstate").and_then(Value::as_str) {
                        entry.state = state.to_lowercase();
                    }
                    if let Some(addresses) = item.get("addr_info").and_then(Value::as_array) {
                        for address in addresses {
                            let Some(local) = address.get("local").and_then(Value::as_str) else {
                                continue;
                            };
                            let prefix = address.get("prefixlen").and_then(Value::as_u64);
                            entry.addresses.push(match prefix {
                                Some(prefix) => format!("{local}/{prefix}"),
                                None => local.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }
    for line in read("/proc/net/dev").lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        let fields: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|value| value.parse().ok())
            .collect();
        if fields.len() < 16 {
            continue;
        }
        let entry = found.entry(name.to_string()).or_insert_with(|| Network {
            name: name.to_string(),
            mac: read(&format!("/sys/class/net/{name}/address"))
                .trim()
                .to_string(),
            state: read(&format!("/sys/class/net/{name}/operstate"))
                .trim()
                .to_lowercase(),
            ..Network::default()
        });
        entry.received = fields[0];
        entry.packets_received = fields[1];
        entry.errors = entry.errors.saturating_add(fields[2]);
        entry.transmitted = fields[8];
        entry.packets_transmitted = fields[9];
        entry.errors = entry.errors.saturating_add(fields[10]);
        if entry.mac.is_empty() {
            entry.mac = read(&format!("/sys/class/net/{name}/address"))
                .trim()
                .to_string();
        }
        if entry.state.is_empty() {
            entry.state = read(&format!("/sys/class/net/{name}/operstate"))
                .trim()
                .to_lowercase();
        }
    }
    found
        .into_values()
        .map(|network| {
            json!({
                "name": network.name,
                "mac": if network.mac.is_empty() {
                    Value::Null
                } else {
                    Value::String(network.mac)
                },
                "addresses": network.addresses,
                "state": if network.state.is_empty() {
                    "unknown"
                } else {
                    network.state.as_str()
                },
                "received": network.received,
                "transmitted": network.transmitted,
                "packets-received": network.packets_received,
                "packets-transmitted": network.packets_transmitted,
                "errors": network.errors,
            })
        })
        .collect()
}
