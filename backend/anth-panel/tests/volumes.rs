//! Storage inventory over the service, HTTP and the socket.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_panel::api::{self, AppState};
use anth_panel::auth::AuthService;
use anth_panel::client::Client;
use anth_panel::config::{AuthConfig, Config, VolumesConfig};
use anth_panel::dispatch::Daemon;
use anth_panel::log::{Logger, Settings};
use anth_panel::socket;
use anth_panel::volumes::{Probe, VolumesService};
use anth_wire::{Request, Response, VolumeUsage};
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

const LSBLK: &str = r#"{
  "blockdevices": [
    {
      "name": "sda", "path": "/dev/sda", "type": "disk", "size": 1000,
      "mountpoints": [null], "ro": false, "rm": false,
      "children": [
        {
          "name": "sda1", "path": "/dev/sda1", "type": "part", "size": 500,
          "fstype": "ext4", "label": "root", "uuid": "abc",
          "mountpoints": ["/"], "ro": false, "rm": false
        }
      ]
    }
  ]
}"#;

const MOUNTINFO: &str = "\
36 25 0:30 / / rw,relatime shared:1 - overlay overlay rw
37 36 8:1 / /mnt/data rw,relatime - ext4 /dev/sda1 rw
";

struct FakeProbe {
    usage: Mutex<HashMap<String, VolumeUsage>>,
}

impl FakeProbe {
    fn new() -> Self {
        Self {
            usage: Mutex::new(HashMap::from([(
                "/mnt/data".to_string(),
                VolumeUsage {
                    total_bytes: 100,
                    used_bytes: 40,
                    available_bytes: 60,
                },
            )])),
        }
    }
}

impl Probe for FakeProbe {
    fn lsblk(&self) -> Result<String, String> {
        Ok(LSBLK.to_string())
    }

    fn mountinfo(&self) -> Result<String, String> {
        Ok(MOUNTINFO.to_string())
    }

    fn swaps(&self) -> Option<Vec<String>> {
        Some(vec!["/dev/sda2".to_string()])
    }

    fn usage(&self, mountpoint: &str) -> Option<VolumeUsage> {
        self.usage.lock().expect("lock").get(mountpoint).copied()
    }
}

fn volumes_config() -> VolumesConfig {
    VolumesConfig {
        control: true,
        partition: true,
        format: true,
        mount_root: "/mnt".to_string(),
        disk_allow: vec!["/dev/sda".to_string()],
    }
}

fn scratch(name: &str) -> (PathBuf, PathBuf) {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = PathBuf::from("/tmp/anthracite-panel/tests")
        .join(format!("{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&root).expect("create scratch");
    (root.clone(), root.join("config.toml"))
}

struct Built {
    daemon: Arc<Daemon>,
    paths: anth_panel::config::Paths,
}

fn daemon(name: &str) -> Built {
    let (_root, config_path) = scratch(name);
    let mut config = Config::dev_default();
    config.volumes = volumes_config();
    let paths = config.paths(&config_path).expect("paths");
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let volumes = VolumesService::with_probe(config.volumes.clone(), Arc::new(FakeProbe::new()));
    let daemon = Daemon::new(config, paths.clone(), logger).with_volumes(volumes);
    Built {
        daemon: Arc::new(daemon),
        paths,
    }
}

fn state(daemon: Arc<Daemon>) -> AppState {
    let (_root, config_path) = scratch("volumes-state");
    let config = Config::dev_default();
    let paths = config.paths(&config_path).expect("paths");
    let auth = AuthConfig {
        enabled: false,
        ..AuthConfig::default()
    };
    let (auth, _) = AuthService::load(&auth, &paths).expect("auth");
    AppState::new(
        daemon,
        auth,
        Arc::new(Logger::buffer(Settings::default()).0),
    )
}

async fn body_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    serde_json::from_slice(&bytes).expect("json")
}

#[tokio::test]
async fn the_http_route_returns_the_inventory_and_policy() {
    let built = daemon("volumes-http");
    let router = api::router(state(built.daemon));
    let response = router
        .oneshot(
            HttpRequest::builder()
                .uri("/api/volumes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("volumes");
    assert_eq!(response.status(), StatusCode::OK);
    let snapshot = body_json(response).await;
    assert_eq!(snapshot["devices"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["devices"][1]["parent"], "sda");
    assert_eq!(snapshot["mounts"].as_array().unwrap().len(), 2);
    assert_eq!(snapshot["swap_devices"][0], "/dev/sda2");
    assert_eq!(snapshot["control_enabled"], true);
    assert_eq!(snapshot["partition_enabled"], true);
    assert_eq!(snapshot["format_enabled"], true);
    assert_eq!(snapshot["mount_root"], "/mnt");
    assert_eq!(snapshot["disk_allow"][0], "/dev/sda");
    assert_eq!(snapshot["mounts"][1]["usage"]["used_bytes"], 40);
    assert!(snapshot.get("warning").is_none());
}

#[tokio::test]
async fn the_socket_returns_the_same_inventory() {
    let built = daemon("volumes-socket");
    let listener = socket::bind(&built.paths.socket).expect("bind");
    let server = tokio::spawn(socket::serve(listener, Arc::clone(&built.daemon)));

    let mut client = Client::connect(&built.paths.socket).await.expect("connect");
    client.hello().await.expect("hello");
    match client.call(&Request::Volumes).await.expect("volumes") {
        Response::Volumes(snapshot) => {
            assert_eq!(snapshot.devices.len(), 2);
            assert_eq!(snapshot.mounts.len(), 2);
            assert_eq!(snapshot.devices[1].parent.as_deref(), Some("sda"));
            assert_eq!(snapshot.mount_root, "/mnt");
        }
        other => panic!("expected a storage snapshot, got {other:?}"),
    }
    server.abort();
}

#[test]
fn the_live_probe_reports_the_root_mount() {
    let service = VolumesService::new(VolumesConfig::default(), Vec::new());
    let snapshot = service.snapshot();
    assert!(snapshot.timestamp_ms > 0);
    assert!(
        snapshot.mounts.iter().any(|mount| mount.mountpoint == "/"),
        "the live mountinfo must contain /"
    );
    // `lsblk` may be absent in a minimal CI image; the warning is the contract
    // for that degradation, not a test failure.
    if snapshot.devices.is_empty() {
        assert!(snapshot.warning.is_some());
    }
}
