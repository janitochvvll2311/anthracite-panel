//! Storage volume inventory and mount management.
//!
//! Two sources answer two different questions. `lsblk` knows the block-device
//! tree, filesystems, labels and UUIDs but cannot see an overlay or a tmpfs.
//! `/proc/self/mountinfo` knows every mount but nothing about a device that is
//! present and unmounted. The snapshot carries both lists and links them by
//! source path.
//!
//! `mount` and `umount` are the only writes, and both go through `validate_*`
//! before anything is executed. The daemon can only unmount what its own fresh
//! mountinfo reports, never an arbitrary path.

use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_proto::{
    MAX_LINE_LEN, VolumeDevice, VolumeError, VolumeErrorKind, VolumeKind, VolumeMount,
    VolumeSnapshot, VolumeUsage,
};
use serde::Deserialize;
use serde_json::Value;

use crate::command::{CommandError, CommandOutput, CommandRunner, SystemRunner};

mod partition;

pub use partition::{
    FORMAT_TYPES, PartitionKind, PlannedCall, TableEdit, allowed_disk, create_with, edit_with,
    execute, partition_number, partitions_on, perform, perform_create, plan_delete, plan_edit,
    plan_format, plan_resize, refuse_busy_disk, refuse_protected_disk,
    refuse_whole_disk_filesystem, snapshot_for, validate_device_path,
};

/// Filesystems with no block device behind them. They are reported but never
/// unmountable through the panel.
const VIRTUAL_FILESYSTEMS: &[&str] = &[
    "autofs",
    "bpf",
    "cgroup",
    "cgroup2",
    "configfs",
    "debugfs",
    "devpts",
    "devtmpfs",
    "efivarfs",
    "fusectl",
    "hugetlbfs",
    "mqueue",
    "nsfs",
    "overlay",
    "proc",
    "pstore",
    "ramfs",
    "rpc_pipefs",
    "securityfs",
    "squashfs",
    "sysfs",
    "tmpfs",
    "tracefs",
];

/// `MOUNTPOINTS` replaced `MOUNTPOINT` in util-linux 2.37. The first list is
/// tried first; an older binary rejects the unknown column and the second list
/// is used instead.
const LSBLK_COLUMNS: &str = "NAME,PATH,TYPE,SIZE,FSTYPE,LABEL,UUID,MOUNTPOINTS,RO,RM,MODEL,TRAN";
const LSBLK_COLUMNS_LEGACY: &str =
    "NAME,PATH,TYPE,SIZE,FSTYPE,LABEL,UUID,MOUNTPOINT,RO,RM,MODEL,TRAN";

/// Everything the read path needs from the host, behind a trait so tests can
/// script it without spawning `lsblk` or touching the real mounts.
pub trait Probe {
    fn lsblk(&self) -> Result<String, VolumeError>;
    fn mountinfo(&self) -> Result<String, VolumeError>;
    fn usage(&self, mountpoint: &str) -> Option<VolumeUsage>;

    /// Device paths of the active swap areas, from `/proc/swaps`.
    ///
    /// A swap area is never in `/proc/self/mountinfo`, so a guard that only
    /// reads mounts cannot see it. `None` means the probe could not tell, and
    /// callers must treat that as "unknown", never as "nothing is swapped".
    fn swaps(&self) -> Option<Vec<String>> {
        None
    }
}

pub struct SystemProbe {
    runner: SystemRunner,
}

impl Default for SystemProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemProbe {
    pub fn new() -> Self {
        Self {
            runner: SystemRunner::new(false),
        }
    }
}

impl Probe for SystemProbe {
    fn lsblk(&self) -> Result<String, VolumeError> {
        let modern = self.runner.run("lsblk", &["-J", "-b", "-o", LSBLK_COLUMNS]);
        match modern {
            Ok(output) if output.ok => Ok(output.stdout),
            _ => {
                let legacy = self
                    .runner
                    .run("lsblk", &["-J", "-b", "-o", LSBLK_COLUMNS_LEGACY]);
                match legacy {
                    Ok(output) if output.ok => Ok(output.stdout),
                    Ok(output) => Err(unavailable("lsblk", &output.stderr)),
                    Err(error) => Err(command_error("lsblk", error)),
                }
            }
        }
    }

    fn mountinfo(&self) -> Result<String, VolumeError> {
        std::fs::read_to_string("/proc/self/mountinfo")
            .map_err(|e| failed(format!("cannot read /proc/self/mountinfo: {e}")))
    }

    fn swaps(&self) -> Option<Vec<String>> {
        swaps_from(&std::fs::read_to_string("/proc/swaps").ok()?)
    }

    fn usage(&self, mountpoint: &str) -> Option<VolumeUsage> {
        usage_for(mountpoint)
    }
}

fn unavailable(program: &str, stderr: &str) -> VolumeError {
    let detail = if stderr.is_empty() {
        format!("{program} failed")
    } else {
        format!("{program} failed: {stderr}")
    };
    failed(detail)
}

fn failed(detail: impl Into<String>) -> VolumeError {
    VolumeError::new(VolumeErrorKind::Failed, detail)
}

fn command_error(program: &str, error: CommandError) -> VolumeError {
    match error {
        CommandError::Unavailable { .. } => {
            failed(format!("{program} is not installed or is not on PATH"))
        }
        CommandError::Failed { detail, .. } => failed(format!("could not run {program}: {detail}")),
    }
}

/// One point-in-time snapshot. A failing `lsblk` degrades to a mounts-only
/// snapshot with a warning instead of failing the whole request, which keeps
/// the panel usable in minimal containers.
pub fn collect(control_enabled: bool) -> VolumeSnapshot {
    collect_with(control_enabled, &SystemProbe::new())
}

/// The full inventory with the warnings kept separate, for callers that must
/// validate before anything is shortened. `collect_with` fits to the line cap
/// first; a guard reading that one could pass on a device or mount that
/// truncation dropped.
pub fn collect_for_mutation(
    control_enabled: bool,
    probe: &dyn Probe,
) -> (VolumeSnapshot, Vec<String>) {
    collect_unfitted(control_enabled, probe)
}

pub fn collect_with(control_enabled: bool, probe: &dyn Probe) -> VolumeSnapshot {
    let (mut snapshot, warnings) = collect_unfitted(control_enabled, probe);

    snapshot = fit_snapshot(snapshot);
    if !warnings.is_empty() {
        let joined = warnings.join("; ");
        snapshot.warning = Some(match snapshot.warning {
            Some(existing) => format!("{existing}; {joined}"),
            None => joined,
        });
    }
    snapshot
}

/// The full inventory, before the line cap shortens anything.
///
/// Mutations validate against this: a guard that read the fitted snapshot would
/// refuse a device or mount that truncation dropped, and the guards exist for
/// exactly the busy hosts where truncation happens. The firewall's `read_table`
/// follows the same rule.
fn collect_unfitted(control_enabled: bool, probe: &dyn Probe) -> (VolumeSnapshot, Vec<String>) {
    let mut warnings = Vec::new();

    let devices = match probe.lsblk() {
        Ok(text) => match parse_lsblk(&text) {
            Ok(devices) => devices,
            Err(error) => {
                warnings.push(format!("device inventory unavailable: {}", error.detail));
                Vec::new()
            }
        },
        Err(error) => {
            warnings.push(format!("device inventory unavailable: {}", error.detail));
            Vec::new()
        }
    };

    // An unreadable `/proc/swaps` is an unknown, not an empty list, and the
    // warning says so: a guard that silently saw "no swap" would allow a
    // delete on a live swap area.
    let mut swap_devices = Vec::new();
    match probe.swaps() {
        Some(paths) => swap_devices = paths,
        None => warnings.push("swap areas unavailable; in-use checks are incomplete".to_string()),
    }

    let mounts = match probe.mountinfo() {
        Ok(text) => {
            let mut mounts = parse_mountinfo(&text);
            for mount in &mut mounts {
                mount.usage = probe.usage(&mount.mountpoint);
            }
            mounts
        }
        Err(error) => {
            warnings.push(format!("mount inventory unavailable: {}", error.detail));
            Vec::new()
        }
    };

    let snapshot = VolumeSnapshot {
        devices,
        mounts,
        control_enabled,
        timestamp_ms: now_ms(),
        total: 0,
        truncated: false,
        warning: None,
        swap_devices,
    };
    (snapshot, warnings)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Default, Deserialize)]
struct RawDevice {
    #[serde(default)]
    name: String,
    #[serde(default)]
    path: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    size: Option<Value>,
    #[serde(default)]
    fstype: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    mountpoint: Option<String>,
    #[serde(default)]
    mountpoints: Option<Vec<Option<String>>>,
    #[serde(default)]
    ro: Option<Value>,
    #[serde(default)]
    rm: Option<Value>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    tran: Option<String>,
    #[serde(default)]
    children: Vec<RawDevice>,
}

/// Parses one `lsblk -J` document into a flat list with parent links.
///
/// Both the modern `mountpoints` array and the legacy `mountpoint` string are
/// accepted, because util-linux changed the field and different distros ship
/// different versions of it.
pub fn parse_lsblk(json: &str) -> Result<Vec<VolumeDevice>, VolumeError> {
    let document: Value = serde_json::from_str(json).map_err(|e| {
        VolumeError::new(VolumeErrorKind::Invalid, format!("invalid lsblk JSON: {e}"))
    })?;
    let devices = document
        .get("blockdevices")
        .ok_or_else(|| {
            VolumeError::new(
                VolumeErrorKind::Invalid,
                "lsblk JSON has no `blockdevices` array",
            )
        })?
        .clone();
    let devices: Vec<RawDevice> = serde_json::from_value(devices).map_err(|e| {
        VolumeError::new(
            VolumeErrorKind::Invalid,
            format!("unexpected lsblk shape: {e}"),
        )
    })?;

    let mut flattened = Vec::new();
    for device in &devices {
        flatten(device, None, &mut flattened);
    }
    Ok(flattened)
}

fn flatten(device: &RawDevice, parent: Option<&str>, out: &mut Vec<VolumeDevice>) {
    let name = device.name.trim();
    if name.is_empty() {
        return;
    }

    out.push(VolumeDevice {
        name: name.to_string(),
        path: device
            .path
            .clone()
            .filter(|path| !path.is_empty())
            .unwrap_or_else(|| format!("/dev/{name}")),
        kind: kind_from_str(device.kind.as_deref().unwrap_or_default()),
        parent: parent.map(str::to_string),
        size_bytes: as_u64(&device.size),
        fstype: clean(device.fstype.clone()),
        label: clean(device.label.clone()),
        uuid: clean(device.uuid.clone()),
        mountpoints: raw_mountpoints(device),
        read_only: as_bool(&device.ro),
        removable: as_bool(&device.rm),
        model: clean(device.model.clone()),
        transport: clean(device.tran.clone()),
    });

    for child in &device.children {
        flatten(child, Some(name), out);
    }
}

fn kind_from_str(value: &str) -> VolumeKind {
    let value = value.trim().to_ascii_lowercase();
    if value.starts_with("raid") {
        return VolumeKind::Raid;
    }
    match value.as_str() {
        "disk" => VolumeKind::Disk,
        "part" => VolumeKind::Part,
        "crypt" => VolumeKind::Crypt,
        "lvm" => VolumeKind::Lvm,
        "loop" => VolumeKind::Loop,
        "rom" => VolumeKind::Rom,
        _ => VolumeKind::Other,
    }
}

fn raw_mountpoints(device: &RawDevice) -> Vec<String> {
    let mut points: Vec<String> = match &device.mountpoints {
        Some(list) => list.iter().flatten().cloned().collect(),
        None => device.mountpoint.clone().into_iter().collect(),
    };
    points.retain(|point| !point.is_empty());
    points.dedup();
    points
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn as_u64(value: &Option<Value>) -> u64 {
    match value {
        Some(Value::Number(number)) => number.as_u64().unwrap_or(0),
        Some(Value::String(text)) => text.trim().parse().unwrap_or(0),
        _ => 0,
    }
}

fn as_bool(value: &Option<Value>) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        Some(Value::Number(number)) => number.as_u64().unwrap_or(0) != 0,
        Some(Value::String(text)) => matches!(text.trim(), "1" | "true"),
        _ => false,
    }
}

/// Parses `/proc/self/mountinfo`. Usage is filled in later, so this stays a
/// pure function over the file contents.
/// Parses `/proc/swaps` into the device paths that are live swap areas.
///
/// The first line is a header. A file-backed swap area is reported by path,
/// which is why the guard compares whole paths and not just names.
pub fn swaps_from(text: &str) -> Option<Vec<String>> {
    let mut paths = Vec::new();
    for line in text.lines().skip(1) {
        let mut fields = line.split_whitespace();
        if let Some(path) = fields.next() {
            paths.push(path.to_string());
        }
    }
    Some(paths)
}

pub fn parse_mountinfo(text: &str) -> Vec<VolumeMount> {
    text.lines().filter_map(parse_mountinfo_line).collect()
}

fn parse_mountinfo_line(line: &str) -> Option<VolumeMount> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 6 {
        return None;
    }
    let separator = fields.iter().position(|field| *field == "-")?;
    let mountpoint = unescape_mountinfo(fields[4]);
    let options = fields[5].to_string();
    let fstype = fields.get(separator + 1)?.to_string();
    let source = fields.get(separator + 2).map(|s| unescape_mountinfo(s))?;
    let super_options = fields.get(separator + 3).copied().unwrap_or_default();

    let read_only = has_option(&options, "ro") || has_option(super_options, "ro");

    Some(VolumeMount {
        mountpoint,
        source,
        virtual_fs: VIRTUAL_FILESYSTEMS.contains(&fstype.as_str()),
        fstype,
        options,
        read_only,
        usage: None,
    })
}

fn has_option(options: &str, wanted: &str) -> bool {
    options.split(',').any(|option| option == wanted)
}

/// `/proc/self/mountinfo` escapes space, tab, newline and backslash as octal.
///
/// The field is a byte string: decodes are collected as bytes and the result is
/// decoded once, so a multi-byte UTF-8 path survives both escaped and raw.
fn unescape_mountinfo(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 3 < bytes.len() {
            let digits = &bytes[index + 1..index + 4];
            if digits.iter().all(|digit| (b'0'..=b'7').contains(digit)) {
                let code = digits
                    .iter()
                    .fold(0u32, |acc, digit| acc * 8 + u32::from(digit - b'0'));
                if let Ok(code) = u8::try_from(code) {
                    out.push(code);
                    index += 4;
                    continue;
                }
            }
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Free-space counters for one mountpoint. Unreadable or vanished mountpoints
/// yield `None` rather than failing the snapshot.
pub fn usage_for(mountpoint: &str) -> Option<VolumeUsage> {
    let path = CString::new(mountpoint).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut stat) } != 0 {
        return None;
    }

    let block = stat.f_frsize.max(1) as u64;
    let total = block.saturating_mul(stat.f_blocks as u64);
    let free = block.saturating_mul(stat.f_bfree as u64);
    Some(VolumeUsage {
        total_bytes: total,
        used_bytes: total.saturating_sub(free),
        available_bytes: block.saturating_mul(stat.f_bavail as u64),
    })
}

/// Length of the framed response, not the bare snapshot: the tagged enum adds
/// the `{"type":...,"data":...}` envelope, and that is what has to fit the line.
fn snapshot_json_len(snapshot: &VolumeSnapshot) -> usize {
    anth_proto::Response::VolumeSnapshot(snapshot.clone())
        .to_json()
        .map(|json| json.len())
        .unwrap_or(usize::MAX)
}

/// Shortens the snapshot until one line fits `MAX_LINE_LEN`.
///
/// Diagnostic detail (model, transport, UUID) goes first, then the tail of the
/// device list, then the tail of the mount list. Truncation keeps a prefix, so
/// a leading disk survives with its partitions, and `total` always reports what
/// was found before shortening.
/// Exposed for the integration tests, which cannot reach a private helper.
#[doc(hidden)]
pub fn fit_snapshot_for_test(snapshot: VolumeSnapshot) -> VolumeSnapshot {
    fit_snapshot(snapshot)
}

fn fit_snapshot(mut snapshot: VolumeSnapshot) -> VolumeSnapshot {
    let total = snapshot.devices.len();
    snapshot.total = total;

    if snapshot_json_len(&snapshot) <= MAX_LINE_LEN {
        return snapshot;
    }

    for device in &mut snapshot.devices {
        device.model = None;
        device.transport = None;
        device.uuid = None;
    }
    if snapshot_json_len(&snapshot) <= MAX_LINE_LEN {
        snapshot.truncated = true;
        snapshot.warning = Some(shortened_warning(
            "model, transport and UUID fields",
            snapshot.devices.len(),
        ));
        return snapshot;
    }

    let base = snapshot.clone();
    let keep = largest_prefix(base.devices.len(), |count| {
        let mut candidate = base.clone();
        candidate.devices.truncate(count);
        candidate.warning = Some(devices_warning(count, total));
        snapshot_json_len(&candidate) <= MAX_LINE_LEN
    });
    snapshot.devices.truncate(keep);
    snapshot.truncated = true;
    snapshot.warning = Some(devices_warning(keep, total));

    if snapshot_json_len(&snapshot) > MAX_LINE_LEN {
        let mounts_total = snapshot.mounts.len();
        let base = snapshot.clone();
        let keep = largest_prefix(mounts_total, |count| {
            let mut candidate = base.clone();
            candidate.mounts.truncate(count);
            candidate.warning = Some(format!(
                "{}; {count} of {mounts_total} mounts shown",
                devices_warning(candidate.devices.len(), total)
            ));
            snapshot_json_len(&candidate) <= MAX_LINE_LEN
        });
        snapshot.mounts.truncate(keep);
        snapshot.warning = Some(format!(
            "{}; {keep} of {mounts_total} mounts shown",
            devices_warning(snapshot.devices.len(), total)
        ));
    }

    snapshot
}

/// Largest prefix length for which `fits` holds. `fits` must be monotonic,
/// which it is: shortening a list only makes the JSON smaller.
fn largest_prefix(len: usize, fits: impl Fn(usize) -> bool) -> usize {
    let mut low = 0usize;
    let mut high = len;
    while low < high {
        let mid = (low + high).div_ceil(2);
        if fits(mid) {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    low
}

fn devices_warning(kept: usize, total: usize) -> String {
    format!(
        "showing {kept} of {total} devices; the snapshot was shortened to fit the protocol line cap"
    )
}

fn shortened_warning(field: &str, kept: usize) -> String {
    format!("dropped {field} from {kept} devices to fit the protocol line cap")
}

/// Everything a mount request needs after validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedMount {
    pub device: String,
    pub target: PathBuf,
    pub fs_type: String,
    pub read_only: bool,
}

/// Extensions a filesystem type must not have. `mount -t` never receives a
/// shell, but an argument such as `--bind` would still be read as an option.
fn valid_fs_type(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// Absolute, normalised, no `..`. `files::normalize` already rejects `..` and
/// NUL bytes, and the result is a virtual path the caller joins onto `/`.
fn require_absolute(path: &str, field: &str) -> Result<PathBuf, VolumeError> {
    if !path.starts_with('/') {
        return Err(invalid(format!("{field} must be an absolute path")));
    }
    let normalised = crate::files::normalize(path)
        .map_err(|error| invalid(format!("invalid {field}: {}", error.detail)))?;
    Ok(PathBuf::from(normalised))
}

fn invalid(detail: impl Into<String>) -> VolumeError {
    VolumeError::new(VolumeErrorKind::Invalid, detail)
}

fn protected(detail: impl Into<String>) -> VolumeError {
    VolumeError::new(VolumeErrorKind::Protected, detail)
}

/// Resolves a path as far as it exists, so a symlinked target cannot escape its
/// parent. Falls back to the lexical path only when nothing resolves.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Paths the panel must never unmount, because doing so would take the panel
/// itself (or the data it serves) offline.
pub fn protected_paths(
    config_path: Option<&Path>,
    files_root: &Path,
    socket_path: &Path,
) -> Vec<PathBuf> {
    let mut paths = vec![
        canonical(files_root),
        canonical(socket_path),
        socket_path.to_path_buf(),
        files_root.to_path_buf(),
    ];
    if let Some(config) = config_path {
        paths.push(canonical(config));
        paths.push(config.to_path_buf());
    }
    if let Ok(executable) = std::env::current_exe() {
        paths.push(canonical(&executable));
    }
    paths
}

fn is_ancestor_or_equal(ancestor: &Path, path: &Path) -> bool {
    path.starts_with(ancestor)
}

/// Validates one mount request against a fresh snapshot.
///
/// `mount_root` is the configured ceiling for targets; `None` disables the
/// check and is the documented escape hatch.
pub fn validate_mount(
    snapshot: &VolumeSnapshot,
    device: &str,
    target: &str,
    fs_type: Option<&str>,
    read_only: bool,
    allow_removable: bool,
    mount_root: Option<&Path>,
) -> Result<ValidatedMount, VolumeError> {
    let device = device.trim();
    if device.is_empty() {
        return Err(invalid("device cannot be empty"));
    }

    let known = snapshot
        .devices
        .iter()
        .find(|candidate| candidate.path == device || candidate.name == device)
        .ok_or_else(|| {
            VolumeError::new(
                VolumeErrorKind::NotFound,
                format!("`{device}` is not a block device on this host"),
            )
        })?;

    if known.is_mounted() {
        return Err(invalid(format!(
            "`{}` is already mounted on {}",
            known.path,
            known.mountpoints.join(", ")
        )));
    }
    if known.removable && !allow_removable {
        return Err(protected(format!(
            "`{}` is removable; pass --allow-removable to mount it anyway",
            known.path
        )));
    }

    let target = require_absolute(target, "target")?;
    let metadata = std::fs::metadata(&target).map_err(|error| {
        VolumeError::new(
            VolumeErrorKind::NotFound,
            format!("`{}` is not usable: {error}", target.display()),
        )
    })?;
    if !metadata.is_dir() {
        return Err(invalid(format!(
            "`{}` is not a directory",
            target.display()
        )));
    }
    let resolved = canonical(&target);
    if snapshot
        .mounts
        .iter()
        .any(|mount| mount.mountpoint == *resolved.to_string_lossy())
    {
        return Err(invalid(format!(
            "`{}` is already a mountpoint",
            resolved.display()
        )));
    }

    if let Some(root) = mount_root {
        let root = canonical(root);
        if !resolved.starts_with(&root) {
            return Err(protected(format!(
                "`{}` is outside the allowed mount root `{}`",
                resolved.display(),
                root.display()
            )));
        }
    }

    let fs_type = fs_type
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| known.fstype.clone())
        .ok_or_else(|| {
            invalid(format!(
                "`{}` has no detected filesystem; pass --type",
                known.path
            ))
        })?;
    if !valid_fs_type(&fs_type) {
        return Err(invalid(format!("invalid filesystem type `{fs_type}`")));
    }

    Ok(ValidatedMount {
        device: known.path.clone(),
        target: resolved,
        fs_type,
        read_only,
    })
}

/// Validates one unmount request against a fresh snapshot.
pub fn validate_unmount(
    snapshot: &VolumeSnapshot,
    target: &str,
    protected_paths: &[PathBuf],
) -> Result<PathBuf, VolumeError> {
    let target = require_absolute(target, "target")?;
    if target.as_path() == Path::new("/") {
        return Err(protected("refusing to unmount `/`"));
    }
    let resolved = canonical(&target);

    let mount = snapshot
        .mounts
        .iter()
        .find(|mount| mount.mountpoint == *resolved.to_string_lossy())
        .ok_or_else(|| {
            VolumeError::new(
                VolumeErrorKind::NotFound,
                format!("`{}` is not a mountpoint", resolved.display()),
            )
        })?;

    if mount.virtual_fs {
        return Err(protected(format!(
            "`{}` is a `{}` filesystem and cannot be unmounted through the panel",
            mount.mountpoint, mount.fstype
        )));
    }

    for guarded in protected_paths {
        let guarded = canonical(guarded);
        if is_ancestor_or_equal(&resolved, &guarded) {
            return Err(protected(format!(
                "`{}` contains `{}`, which the panel needs",
                resolved.display(),
                guarded.display()
            )));
        }
    }

    Ok(resolved)
}

/// Mounts a validated device. Runs on a blocking worker.
pub fn mount(
    control_enabled: bool,
    sudo_enabled: bool,
    mount_root: Option<&Path>,
    args: &MountArgs<'_>,
) -> Result<String, VolumeError> {
    mount_with(
        control_enabled,
        &SystemProbe::new(),
        &SystemRunner::new(sudo_enabled),
        mount_root,
        args,
    )
}

pub struct MountArgs<'a> {
    pub device: &'a str,
    pub target: &'a str,
    pub fs_type: Option<&'a str>,
    pub read_only: bool,
    pub allow_removable: bool,
}

pub fn mount_with(
    control_enabled: bool,
    probe: &dyn Probe,
    runner: &dyn CommandRunner,
    mount_root: Option<&Path>,
    args: &MountArgs<'_>,
) -> Result<String, VolumeError> {
    if !control_enabled {
        return Err(VolumeError::new(
            VolumeErrorKind::Disabled,
            "volume control is disabled",
        ));
    }

    let (snapshot, _) = collect_unfitted(control_enabled, probe);
    let validated = validate_mount(
        &snapshot,
        args.device,
        args.target,
        args.fs_type,
        args.read_only,
        args.allow_removable,
        mount_root,
    )?;
    mount_validated(runner, &validated)
}

fn mount_validated(
    runner: &dyn CommandRunner,
    validated: &ValidatedMount,
) -> Result<String, VolumeError> {
    let mut args = vec![];
    if validated.read_only {
        args.push("-o");
        args.push("ro");
    }
    args.push("-t");
    args.push(&validated.fs_type);
    args.push(&validated.device);
    let target = validated.target.to_string_lossy().into_owned();
    args.push(&target);

    run_mutation(runner, "mount", &args)?;
    Ok(format!(
        "mounted {} on {} as {}",
        validated.device, target, validated.fs_type
    ))
}

/// Unmounts a validated path. Runs on a blocking worker.
pub fn unmount(
    control_enabled: bool,
    sudo_enabled: bool,
    protected_paths: &[PathBuf],
    target: &str,
) -> Result<String, VolumeError> {
    unmount_with(
        control_enabled,
        &SystemProbe::new(),
        &SystemRunner::new(sudo_enabled),
        protected_paths,
        target,
    )
}

pub fn unmount_with(
    control_enabled: bool,
    probe: &dyn Probe,
    runner: &dyn CommandRunner,
    protected_paths: &[PathBuf],
    target: &str,
) -> Result<String, VolumeError> {
    if !control_enabled {
        return Err(VolumeError::new(
            VolumeErrorKind::Disabled,
            "volume control is disabled",
        ));
    }

    let (snapshot, _) = collect_unfitted(control_enabled, probe);
    let resolved = validate_unmount(&snapshot, target, protected_paths)?;
    unmount_path(runner, &resolved)
}

fn unmount_path(runner: &dyn CommandRunner, target: &Path) -> Result<String, VolumeError> {
    let target = target.to_string_lossy().into_owned();
    run_mutation(runner, "umount", &[&target])?;
    Ok(format!("unmounted {target}"))
}

fn run_mutation(
    runner: &dyn CommandRunner,
    program: &str,
    args: &[&str],
) -> Result<(), VolumeError> {
    match runner.run(program, args) {
        Ok(CommandOutput { ok: true, .. }) => Ok(()),
        Ok(output) => Err(classified_failure(program, &output.stderr)),
        Err(CommandError::Unavailable { .. }) => Err(failed(format!(
            "{program} is not installed or is not on PATH"
        ))),
        Err(CommandError::Failed { detail, .. }) => {
            Err(failed(format!("could not run {program}: {detail}")))
        }
    }
}

/// The kernel reports `EBUSY` and permission failures as plain text, so the
/// message is the only signal available.
fn classified_failure(program: &str, stderr: &str) -> VolumeError {
    let lowercase = stderr.to_lowercase();
    let kind = if lowercase.contains("busy") || lowercase.contains("in use") {
        VolumeErrorKind::Busy
    } else if lowercase.contains("permission denied")
        || lowercase.contains("operation not permitted")
        || lowercase.contains("must be superuser")
        || lowercase.contains("only root")
    {
        VolumeErrorKind::PermissionDenied
    } else if lowercase.contains("does not exist")
        || lowercase.contains("no such file")
        || lowercase.contains("not found")
    {
        VolumeErrorKind::NotFound
    } else if lowercase.contains("wrong fs type")
        || lowercase.contains("unknown filesystem")
        || lowercase.contains("bad option")
        || lowercase.contains("invalid")
    {
        VolumeErrorKind::Invalid
    } else {
        VolumeErrorKind::Failed
    };
    VolumeError::new(
        kind,
        if stderr.is_empty() {
            format!("{program} failed")
        } else {
            stderr.to_string()
        },
    )
}
