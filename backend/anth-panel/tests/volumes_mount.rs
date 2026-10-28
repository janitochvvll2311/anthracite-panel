//! Mount and unmount over HTTP and the socket with scripted probes.

use std::collections::VecDeque;
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
use anth_panel::volumes::{MountOutput, MountRunner, Probe, SystemVolumes};
use anth_wire::{
    Request, Response, VolumeMountRequest, VolumeSnapshot, VolumeUnmountRequest, VolumeUsage,
};
use axum::body::Body;
use axum::http::{Request as HttpRequest, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

const LSBLK: &str = r#"{
  "blockdevices": [
    {
      "name": "sda", "path": "/dev/sda", "type": "disk", "size": 1000,
      "mountpoints": [null], "ro": false, "rm": false,
      "children": [
        {
          "name": "sda1", "path": "/dev/sda1", "type": "part", "size": 500,
          "fstype": "ext4", "mountpoints": ["/"], "ro": false, "rm": false
        },
        {
          "name": "sda2", "path": "/dev/sda2", "type": "part", "size": 200,
          "fstype": "ext4", "mountpoints": [], "ro": false, "rm": false
        }
      ]
    }
  ]
}"#;

struct FakeProbe {
    mountinfo: Mutex<String>,
}

impl FakeProbe {
    fn new() -> Self {
        Self {
            mountinfo: Mutex::new(
                "36 25 0:30 / / rw,relatime shared:1 - overlay overlay rw\n".to_string(),
            ),
        }
    }
}

impl Probe for FakeProbe {
    fn lsblk(&self) -> Result<String, String> {
        Ok(LSBLK.to_string())
    }

    fn mountinfo(&self) -> Result<String, String> {
        Ok(self.mountinfo.lock().expect("lock").clone())
    }

    fn swaps(&self) -> Option<Vec<String>> {
        Some(Vec::new())
    }

    fn usage(&self, _mountpoint: &str) -> Option<VolumeUsage> {
        None
    }
}

struct FakeMountRunner {
    calls: Mutex<Vec<(String, Vec<String>)>>,
    stdin_calls: Mutex<Vec<String>>,
    output: Mutex<VecDeque<MountOutput>>,
}

impl FakeMountRunner {
    fn new(outputs: Vec<MountOutput>) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            stdin_calls: Mutex::new(Vec::new()),
            output: Mutex::new(outputs.into()),
        }
    }

    fn calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.lock().expect("lock").clone()
    }
}

impl MountRunner for FakeMountRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<MountOutput, String> {
        self.calls
            .lock()
            .expect("lock")
            .push((program.to_string(), args.to_vec()));
        Ok(self
            .output
            .lock()
            .expect("lock")
            .pop_front()
            .unwrap_or(MountOutput {
                ok: true,
                stderr: String::new(),
            }))
    }

    fn run_with_stdin(
        &self,
        program: &str,
        args: &[String],
        stdin: &str,
    ) -> Result<MountOutput, String> {
        self.stdin_calls
            .lock()
            .expect("lock")
            .push(stdin.to_string());
        self.run(program, args)
    }
}

fn volumes_config(control: bool) -> VolumesConfig {
    VolumesConfig {
        control,
        partition: true,
        format: true,
        mount_root: "/tmp".to_string(),
        disk_allow: vec!["/dev/sda".to_string()],
    }
}

fn target(name: &str) -> PathBuf {
    let path = PathBuf::from("/tmp/anthracite-panel/tests")
        .join(format!("mount-api-{name}-{}", std::process::id()));
    fs::create_dir_all(&path).expect("target");
    path
}

fn scratch(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = PathBuf::from("/tmp/anthracite-panel/tests")
        .join(format!("{name}-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&root).expect("scratch");
    root.join("config.toml")
}

fn daemon(
    name: &str,
    control: bool,
    probe: Arc<FakeProbe>,
    runner: Arc<FakeMountRunner>,
) -> (Arc<Daemon>, anth_panel::config::Paths) {
    let config_path = scratch(name);
    let mut config = Config::dev_default();
    config.volumes = volumes_config(control);
    let paths = config.paths(&config_path).expect("paths");
    let logger = Arc::new(Logger::buffer(Settings::default()).0);
    let volumes = SystemVolumes::with_parts(config.volumes.settings(), probe, runner, Vec::new());
    let daemon = Daemon::new(config, paths.clone(), logger).with_volumes(volumes);
    (Arc::new(daemon), paths)
}

fn state(daemon: Arc<Daemon>) -> AppState {
    let config_path = scratch("mount-api-state");
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

fn post(uri: &str, body: serde_json::Value) -> HttpRequest<Body> {
    HttpRequest::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .expect("request")
}

#[tokio::test]
async fn the_http_mount_route_runs_the_scripted_runner() {
    let target = target("success");
    let target_text = target.to_string_lossy().into_owned();
    let runner = Arc::new(FakeMountRunner::new(vec![MountOutput {
        ok: true,
        stderr: String::new(),
    }]));
    let (daemon, _paths) = daemon(
        "mount-api",
        true,
        Arc::new(FakeProbe::new()),
        Arc::clone(&runner),
    );
    let router = api::router(state(daemon));
    let response = router
        .oneshot(post(
            "/api/volumes/mount",
            json!({ "device": "/dev/sda2", "target": target_text, "read_only": true }),
        ))
        .await
        .expect("mount");
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(
        body["message"],
        format!("mounted /dev/sda2 on {target_text} as ext4")
    );
    let calls = runner.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "mount");
    assert!(calls[0].1.contains(&"/dev/sda2".to_string()));
    let _ = fs::remove_dir_all(target);
}

#[tokio::test]
async fn the_http_mount_route_is_refused_when_control_is_off() {
    let target = target("off");
    let runner = Arc::new(FakeMountRunner::new(vec![]));
    let (daemon, _paths) = daemon(
        "mount-api-off",
        false,
        Arc::new(FakeProbe::new()),
        Arc::clone(&runner),
    );
    let router = api::router(state(daemon));
    let response = router
        .oneshot(post(
            "/api/volumes/mount",
            json!({ "device": "/dev/sda2", "target": target.to_string_lossy() }),
        ))
        .await
        .expect("mount");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        body_json(response).await["error"]["code"],
        "volumes_control_disabled"
    );
    assert!(runner.calls().is_empty());
    let _ = fs::remove_dir_all(target);
}

#[tokio::test]
async fn the_socket_unmounts_a_scripted_mount() {
    let target = target("socket");
    let target_text = target.to_string_lossy().into_owned();
    let probe = Arc::new(FakeProbe::new());
    probe.mountinfo.lock().expect("lock").push_str(&format!(
        "37 36 8:1 / {target_text} rw,relatime - ext4 /dev/sda2 rw\n"
    ));
    let runner = Arc::new(FakeMountRunner::new(vec![MountOutput {
        ok: true,
        stderr: String::new(),
    }]));
    let (daemon, paths) = daemon("mount-api-socket", true, probe, Arc::clone(&runner));
    let listener = socket::bind(&paths.socket).expect("bind");
    let server = tokio::spawn(socket::serve(listener, Arc::clone(&daemon)));

    let mut client = Client::connect(&paths.socket).await.expect("connect");
    client.hello().await.expect("hello");
    match client
        .call(&Request::VolumesUnmount(VolumeUnmountRequest {
            target: target_text.clone(),
        }))
        .await
        .expect("unmount")
    {
        Response::VolumeChanged { message } => {
            assert_eq!(message, format!("unmounted {target_text}"));
        }
        other => panic!("expected a storage change, got {other:?}"),
    }
    assert_eq!(runner.calls()[0].0, "umount");
    server.abort();
    let _ = fs::remove_dir_all(target);
}

#[tokio::test]
async fn the_socket_mount_uses_the_same_dispatch() {
    let target = target("socket-mount");
    let target_text = target.to_string_lossy().into_owned();
    let runner = Arc::new(FakeMountRunner::new(vec![MountOutput {
        ok: true,
        stderr: String::new(),
    }]));
    let (daemon, paths) = daemon(
        "mount-api-socket-mount",
        true,
        Arc::new(FakeProbe::new()),
        Arc::clone(&runner),
    );
    let listener = socket::bind(&paths.socket).expect("bind");
    let server = tokio::spawn(socket::serve(listener, Arc::clone(&daemon)));
    let mut client = Client::connect(&paths.socket).await.expect("connect");
    client.hello().await.expect("hello");

    match client
        .call(&Request::VolumesMount(VolumeMountRequest {
            device: "/dev/sda2".to_string(),
            target: target_text,
            fs_type: None,
            read_only: false,
            allow_removable: false,
        }))
        .await
        .expect("mount")
    {
        Response::VolumeChanged { message } => {
            assert!(message.starts_with("mounted /dev/sda2"));
        }
        other => panic!("expected a storage change, got {other:?}"),
    }
    server.abort();
    let _ = fs::remove_dir_all(target);
}

#[tokio::test]
async fn the_http_partition_create_and_format_routes_use_the_scripted_runner() {
    let runner = Arc::new(FakeMountRunner::new(vec![]));
    let (daemon, _paths) = daemon(
        "partition-api",
        true,
        Arc::new(FakeProbe::new()),
        Arc::clone(&runner),
    );
    let router = api::router(state(daemon));

    let response = router
        .clone()
        .oneshot(post(
            "/api/volumes/partition/create",
            json!({ "disk": "/dev/sda", "kind": "linux", "size_bytes": 512 }),
        ))
        .await
        .expect("create partition");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["message"],
        "created a linux partition of 512 bytes on /dev/sda"
    );
    let calls = runner.calls();
    assert_eq!(calls[0].0, "sfdisk");
    assert_eq!(calls[0].1, vec!["--no-reread", "--append", "/dev/sda"]);
    assert_eq!(
        runner.stdin_calls.lock().expect("lock").as_slice(),
        &["size=1,type=linux".to_string()]
    );

    let response = router
        .oneshot(post(
            "/api/volumes/format",
            json!({ "partition": "/dev/sda2", "fs_type": "ext4" }),
        ))
        .await
        .expect("format");
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await["message"],
        "formatted /dev/sda2 as ext4"
    );
    let calls = runner.calls();
    assert_eq!(calls[1].0, "mkfs.ext4");
    assert_eq!(calls[1].1, vec!["/dev/sda2"]);
}

#[tokio::test]
async fn the_partition_route_refuses_a_disk_outside_the_allowlist() {
    let runner = Arc::new(FakeMountRunner::new(vec![]));
    let (daemon, _paths) = daemon(
        "partition-api-denied",
        true,
        Arc::new(FakeProbe::new()),
        Arc::clone(&runner),
    );
    let router = api::router(state(daemon));
    let response = router
        .oneshot(post(
            "/api/volumes/partition/delete",
            json!({ "disk": "/dev/sdb", "number": 1 }),
        ))
        .await
        .expect("delete");
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(runner.calls().is_empty());
}

#[test]
fn the_snapshot_type_is_still_serializable() {
    // A small guard that the volume types used by the transports stay stable.
    let snapshot = VolumeSnapshot {
        devices: Vec::new(),
        mounts: Vec::new(),
        swap_devices: Vec::new(),
        control_enabled: false,
        partition_enabled: false,
        format_enabled: false,
        mount_root: "/mnt".to_string(),
        disk_allow: Vec::new(),
        warning: None,
        timestamp_ms: 1,
    };
    let json = serde_json::to_string(&snapshot).expect("json");
    assert!(json.contains("\"mount_root\":\"/mnt\""));
}
