//! L1: per-service filesystem confinement and the port capability.
//!
//! The supervisor applies this between `fork` and `exec`, after the L0 knobs:
//!
//! - a per-service uid when the engine runs as root, so one service cannot read
//!   another's `{data}` or the engine state even with a broad root;
//! - a Landlock ruleset built from the declared roots plus the implicit
//!   directories, so the filesystem view is what the grant froze;
//! - `CAP_NET_BIND_SERVICE` only, and only when a declared listener is
//!   privileged. Every other capability is dropped from the bounding set.
//!
//! Every step is fail-closed: a kernel without Landlock, a root the process
//! cannot open, or a privileged port without the capability refuses the launch
//! with a typed denial instead of running the service unconfined.

use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// The first uid the allocator hands out. Above any system or login user.
const UID_BASE: u32 = 1_000_000;

/// A stable, per-app uid. The derivation lives in the composer so the broker
/// and the sandbox agree on the value.
pub fn uid_for(app: &str) -> u32 {
    anth_compose::service_uid(app)
}

/// The uid the service actually gets: the derived one, or the next free id when
/// the host has a real account (SSSD/AD often allocate from 1,000,000 upward).
/// `getpwuid_r` queries NSS with a caller-owned buffer, so concurrent launches
/// do not share a static one.
pub fn allocate_uid(app: &str) -> io::Result<u32> {
    let mut uid = uid_for(app);
    for _ in 0..1000 {
        if !uid_taken(uid)? {
            return Ok(uid);
        }
        uid = uid.wrapping_add(1);
        if uid < UID_BASE {
            uid = UID_BASE;
        }
    }
    Err(io::Error::other(format!(
        "no free service uid near {}",
        uid_for(app)
    )))
}

/// Whether a real account already owns `uid`.
pub fn uid_taken(uid: u32) -> io::Result<bool> {
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buffer = [0u8; 1024];
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: the entry, buffer and result pointers are valid for the call.
    let code = unsafe {
        libc::getpwuid_r(
            uid,
            &mut entry,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if code != 0 {
        return Err(io::Error::from_raw_os_error(code));
    }
    Ok(!result.is_null())
}

/// The engine's effective gid. Services keep it so `{shared}` stays the trust
/// domain; their own directories are `0700` and owned by their uid.
pub fn engine_gid() -> u32 {
    // SAFETY: `getegid` has no preconditions.
    unsafe { libc::getegid() }
}

pub fn is_root() -> bool {
    // SAFETY: `geteuid` has no preconditions.
    unsafe { libc::geteuid() == 0 }
}

/// Whether the engine's effective set carries `CAP_NET_BIND_SERVICE` (bit 10).
pub fn has_net_bind_service() -> bool {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return false;
    };
    status
        .lines()
        .find(|line| line.starts_with("CapEff:"))
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| u64::from_str_radix(value, 16).ok())
        .is_some_and(|caps| caps & (1 << 10) != 0)
}

/// One Landlock rule: a directory and the access the service keeps under it.
#[derive(Debug, Clone)]
pub struct Rule {
    pub path: PathBuf,
    pub access: u64,
}

/// Read and execute/traverse under the path.
pub const READ: u64 =
    LANDLOCK_ACCESS_FS_EXECUTE | LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_READ_DIR;
/// A file rule cannot carry directory rights: Landlock answers EINVAL.
pub const READ_FILE: u64 = LANDLOCK_ACCESS_FS_READ_FILE;
/// The rights that are valid on a non-directory: execute, write, read and
/// truncate (the last one only when the ABI handles it).
const FILE_ACCESS: u64 = LANDLOCK_ACCESS_FS_EXECUTE
    | LANDLOCK_ACCESS_FS_WRITE_FILE
    | LANDLOCK_ACCESS_FS_READ_FILE
    | LANDLOCK_ACCESS_FS_TRUNCATE;

const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
const LANDLOCK_ACCESS_FS_WRITE_FILE: u64 = 1 << 1;
const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;
const LANDLOCK_ACCESS_FS_READ_DIR: u64 = 1 << 3;
const LANDLOCK_ACCESS_FS_REMOVE_DIR: u64 = 1 << 4;
const LANDLOCK_ACCESS_FS_REMOVE_FILE: u64 = 1 << 5;
const LANDLOCK_ACCESS_FS_MAKE_CHAR: u64 = 1 << 6;
const LANDLOCK_ACCESS_FS_MAKE_DIR: u64 = 1 << 7;
const LANDLOCK_ACCESS_FS_MAKE_REG: u64 = 1 << 8;
const LANDLOCK_ACCESS_FS_MAKE_SOCK: u64 = 1 << 9;
const LANDLOCK_ACCESS_FS_MAKE_FIFO: u64 = 1 << 10;
const LANDLOCK_ACCESS_FS_MAKE_BLOCK: u64 = 1 << 11;
const LANDLOCK_ACCESS_FS_MAKE_SYM: u64 = 1 << 12;
const LANDLOCK_ACCESS_FS_REFER: u64 = 1 << 13;
const LANDLOCK_ACCESS_FS_TRUNCATE: u64 = 1 << 14;

const LANDLOCK_CREATE_RULESET_VERSION: u32 = 1;
const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;

const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;
const SYS_LANDLOCK_ADD_RULE: libc::c_long = 445;
const SYS_LANDLOCK_RESTRICT_SELF: libc::c_long = 446;

#[repr(C)]
struct RulesetAttr {
    handled_access_fs: u64,
}

#[repr(C, packed)]
struct PathBeneathAttr {
    allowed_access: u64,
    parent_fd: i32,
}

/// The Landlock ABI the running kernel speaks, or `None` when it has none.
pub fn abi() -> Option<u32> {
    // SAFETY: a null ruleset with the VERSION flag is the documented probe.
    let value = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            std::ptr::null::<RulesetAttr>(),
            0usize,
            LANDLOCK_CREATE_RULESET_VERSION,
        )
    };
    (value >= 1).then_some(value as u32)
}

fn handled(abi: u32) -> u64 {
    let base = LANDLOCK_ACCESS_FS_EXECUTE
        | LANDLOCK_ACCESS_FS_WRITE_FILE
        | LANDLOCK_ACCESS_FS_READ_FILE
        | LANDLOCK_ACCESS_FS_READ_DIR
        | LANDLOCK_ACCESS_FS_REMOVE_DIR
        | LANDLOCK_ACCESS_FS_REMOVE_FILE
        | LANDLOCK_ACCESS_FS_MAKE_CHAR
        | LANDLOCK_ACCESS_FS_MAKE_DIR
        | LANDLOCK_ACCESS_FS_MAKE_REG
        | LANDLOCK_ACCESS_FS_MAKE_SOCK
        | LANDLOCK_ACCESS_FS_MAKE_FIFO
        | LANDLOCK_ACCESS_FS_MAKE_BLOCK
        | LANDLOCK_ACCESS_FS_MAKE_SYM;
    let base = if abi >= 2 {
        base | LANDLOCK_ACCESS_FS_REFER
    } else {
        base
    };
    if abi >= 3 {
        base | LANDLOCK_ACCESS_FS_TRUNCATE
    } else {
        base
    }
}

/// Access for a writable root: everything the ruleset handles.
pub fn read_write(abi: u32) -> u64 {
    handled(abi)
}

/// Builds the ruleset and restricts the calling process. Must run in the child
/// after `PR_SET_NO_NEW_PRIVS`; it does not need privileges.
pub fn restrict(rules: &[Rule]) -> Result<(), io::Error> {
    let ruleset = build(rules)?;
    let result = restrict_with(ruleset);
    // SAFETY: closing our own fd.
    unsafe {
        libc::close(ruleset);
    }
    result
}

/// Creates the ruleset and adds every rule. Called while the child still has
/// the engine's privileges, so a root the service cannot open itself (the
/// bundle directory, for example) can still be referenced.
pub fn build(rules: &[Rule]) -> Result<i32, io::Error> {
    let abi = abi().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "landlock: the kernel has no Landlock (5.13+)",
        )
    })?;
    let handled = handled(abi);
    let attr = RulesetAttr {
        handled_access_fs: handled,
    };
    // SAFETY: the attribute matches the struct the syscall reads.
    let ruleset = unsafe {
        libc::syscall(
            SYS_LANDLOCK_CREATE_RULESET,
            &attr,
            std::mem::size_of::<RulesetAttr>(),
            0u32,
        )
    };
    if ruleset < 0 {
        return Err(io::Error::last_os_error());
    }
    let ruleset = ruleset as i32;
    for rule in rules {
        if let Err(error) = add_rule(ruleset, rule, handled) {
            // SAFETY: closing our own fd.
            unsafe {
                libc::close(ruleset);
            }
            return Err(io::Error::new(
                error.kind(),
                format!("landlock: {}: {error}", rule.path.display()),
            ));
        }
    }
    Ok(ruleset)
}

/// Restricts the calling process to a ruleset built by [`build`].
pub fn restrict_with(ruleset: i32) -> Result<(), io::Error> {
    // SAFETY: the fd is a ruleset we built and the syscall takes no pointer.
    if unsafe { libc::syscall(SYS_LANDLOCK_RESTRICT_SELF, ruleset, 0u32) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn add_rule(ruleset: i32, rule: &Rule, handled: u64) -> Result<(), io::Error> {
    // Stat before opening the O_PATH fd: a root that disappears between the
    // two would otherwise leak the descriptor on every supervisor retry.
    let metadata = std::fs::metadata(&rule.path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("stat {}: {error}", rule.path.display()),
        )
    })?;
    let path = std::ffi::CString::new(rule.path.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "the root path contains a NUL"))?;
    // SAFETY: O_PATH opens a reference for Landlock without read permission on
    // the final component; the CString is NUL-terminated.
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_PATH | libc::O_CLOEXEC) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    // A runtime socket or a file root is not a directory: Landlock answers
    // EINVAL for directory rights on it. Keep only the file rights.
    let access = if metadata.is_dir() {
        rule.access
    } else {
        rule.access & FILE_ACCESS
    };
    let attr = PathBeneathAttr {
        allowed_access: access & handled,
        parent_fd: fd,
    };
    // SAFETY: the packed attribute matches the kernel ABI.
    let value = unsafe {
        libc::syscall(
            SYS_LANDLOCK_ADD_RULE,
            ruleset,
            LANDLOCK_RULE_PATH_BENEATH,
            &attr,
            0u32,
        )
    };
    let error = if value < 0 {
        Some(io::Error::last_os_error())
    } else {
        None
    };
    // SAFETY: closing our own fd.
    unsafe {
        libc::close(fd);
    }
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Drops every capability from the bounding set except `CAP_NET_BIND_SERVICE`
/// when the service declared a privileged listener. Called while still root.
pub fn drop_bounding_set(keep_bind: bool) -> Result<(), io::Error> {
    for capability in 0..64 {
        if keep_bind && capability == 10 {
            continue;
        }
        // SAFETY: prctl with PR_CAPBSET_DROP only reads its integer argument.
        if unsafe { libc::prctl(libc::PR_CAPBSET_DROP, capability, 0, 0, 0) } == -1 {
            let error = io::Error::last_os_error();
            // Numbers above `CAP_LAST_CAP` do not exist; the kernel answers
            // EINVAL and there is nothing left to drop.
            if error.raw_os_error() == Some(libc::EINVAL) {
                break;
            }
            return Err(error);
        }
    }
    Ok(())
}

/// Switches to the service uid, keeps the engine gid, and installs
/// `CAP_NET_BIND_SERVICE` as the only capability (ambient, so it survives the
/// `exec` under `no_new_privs`).
pub fn become_service(uid: u32, gid: u32, keep_bind: bool) -> Result<(), io::Error> {
    // SAFETY: every call is async-signal-safe and runs in the child.
    unsafe {
        if libc::prctl(libc::PR_SET_KEEPCAPS, 1, 0, 0, 0) == -1 {
            return Err(io::Error::last_os_error());
        }
        if libc::setresgid(gid, gid, gid) == -1 {
            return Err(io::Error::last_os_error());
        }
        if libc::setresuid(uid, uid, uid) == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    let mut header = CapHeader {
        version: LINUX_CAPABILITY_VERSION_3,
        pid: 0,
    };
    let mut data = [CapData::default(); 2];
    if keep_bind {
        data[0].effective = 1 << 10;
        data[0].permitted = 1 << 10;
        data[0].inheritable = 1 << 10;
    }
    // SAFETY: the header and data match the kernel ABI for _VERSION_3.
    if unsafe {
        libc::syscall(
            libc::SYS_capset,
            &mut header as *mut CapHeader,
            data.as_mut_ptr(),
        )
    } < 0
    {
        return Err(io::Error::last_os_error());
    }
    if keep_bind {
        // SAFETY: raising an ambient capability we already hold is permitted.
        if unsafe { libc::prctl(libc::PR_CAP_AMBIENT, libc::PR_CAP_AMBIENT_RAISE, 10, 0, 0) } == -1
        {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;

#[repr(C)]
struct CapHeader {
    version: u32,
    pid: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CapData {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

/// Marks an engine-owned ancestor as traversable (`0711`) without making it
/// listable. A service uid needs this on the state root and `run/` to reach the
/// directory it owns.
pub fn make_traversable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o711))
}

/// The shared trust domain: `0770`, owned by the engine and its group. Every
/// service keeps that gid, so a published file is reachable by its peers.
pub fn prepare_shared(path: &Path, gid: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o770))?;
    if is_root() {
        let owned = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path with NUL"))?;
        // SAFETY: chown takes a NUL-terminated path.
        if unsafe { libc::chown(owned.as_ptr(), 0, gid) } == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Copies the verified binary into the service's own directory. The bundle tree
/// stays engine-owned and unreadable; the child executes its private copy.
pub fn stage_binary(source: &Path, dir: &Path, uid: u32, gid: u32) -> io::Result<PathBuf> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::os::unix::io::AsRawFd;
    let target = dir.join("service");
    // The service owns `dir`, so it could point `service` at a host file. Copy
    // into a fresh O_EXCL file, fchmod/fchown the descriptor, then rename over
    // the name: rename replaces a symlink instead of following it.
    let staged = dir.join(format!("service.{}", std::process::id()));
    let _ = std::fs::remove_file(&staged);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o700)
        .open(&staged)?;
    let mut source = std::fs::File::open(source)?;
    std::io::copy(&mut source, &mut file)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o700))?;
    if is_root() {
        // SAFETY: `fchown` takes an fd we own.
        if unsafe { libc::fchown(file.as_raw_fd(), uid, gid) } == -1 {
            let error = io::Error::last_os_error();
            let _ = std::fs::remove_file(&staged);
            return Err(error);
        }
    }
    file.sync_all()?;
    drop(file);
    if let Err(error) = std::fs::rename(&staged, &target) {
        let _ = std::fs::remove_file(&staged);
        return Err(error);
    }
    Ok(target)
}

/// Prepares a directory the service owns: created if missing, `0700`, and
/// chowned to the service uid when the engine is root.
pub fn prepare_owned(path: &Path, uid: u32, gid: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    if is_root() {
        let owned = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "path with NUL"))?;
        // SAFETY: chown takes a NUL-terminated path.
        if unsafe { libc::chown(owned.as_ptr(), uid, gid) } == -1 {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

/// The implicit Landlock rules for one service: its data, socket directory,
/// broker directory, the shared trust domain and the binary's directory.
pub fn implicit_rules(
    abi: u32,
    data: &Path,
    socket_dir: &Path,
    broker: &Path,
    shared: &Path,
    binary: &Path,
) -> Vec<Rule> {
    let write = read_write(abi);
    let mut rules = Vec::with_capacity(8);
    for path in [data, socket_dir, shared] {
        rules.push(Rule {
            path: path.to_path_buf(),
            access: write,
        });
    }
    // The broker listener belongs to the engine. The service connects to the
    // socket; it must not be able to replace or unlink it.
    rules.push(Rule {
        path: broker.to_path_buf(),
        access: READ,
    });
    if let Some(parent) = binary.parent() {
        rules.push(Rule {
            path: parent.to_path_buf(),
            access: READ,
        });
    }
    // Name resolution is part of the network contract, but Landlock hides the
    // resolver files by default. A service that resolves a host would otherwise
    // block until its caller times out. Only the lookup files are readable;
    // /etc/shadow and friends stay outside the view.
    for path in [
        "/etc/resolv.conf",
        "/etc/hosts",
        "/etc/nsswitch.conf",
        "/etc/host.conf",
        "/etc/services",
    ] {
        let path = Path::new(path);
        if path.exists() {
            rules.push(Rule {
                path: path.to_path_buf(),
                access: READ_FILE,
            });
        }
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_root_uses_only_file_rights() {
        if abi().is_none() {
            return;
        }
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("sandbox-file-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        let socket = dir.join("runtime.sock");
        std::fs::write(&socket, b"file").expect("file");
        let rules = vec![Rule {
            path: socket,
            access: READ | LANDLOCK_ACCESS_FS_WRITE_FILE,
        }];
        build(&rules).expect("a file root is a valid Landlock rule");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolver_files_are_part_of_the_implicit_view() {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("sandbox-resolver-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("data")).expect("data");
        std::fs::create_dir_all(dir.join("socket")).expect("socket");
        std::fs::create_dir_all(dir.join("shared")).expect("shared");
        std::fs::write(dir.join("service"), b"binary").expect("binary");
        let rules = implicit_rules(
            1,
            &dir.join("data"),
            &dir.join("socket"),
            &dir.join("broker"),
            &dir.join("shared"),
            &dir.join("service"),
        );
        for path in ["/etc/resolv.conf", "/etc/hosts", "/etc/nsswitch.conf"] {
            let path = std::path::Path::new(path);
            if path.exists() {
                assert!(
                    rules.iter().any(|rule| rule.path == path),
                    "{path:?} is not readable by a service"
                );
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn uids_are_stable_and_in_range() {
        let first = uid_for("org.anthracite.fs");
        assert_eq!(first, uid_for("org.anthracite.fs"));
        assert_ne!(first, uid_for("org.anthracite.net"));
        assert!(first >= UID_BASE);
        assert!(first < UID_BASE + anth_compose::SERVICE_UID_RANGE);
    }

    #[test]
    fn the_allocated_uid_has_no_account_and_is_stable() {
        let uid = allocate_uid("org.example.sandbox-uid").expect("uid");
        assert!(!uid_taken(uid).expect("probe"));
        assert_eq!(uid, allocate_uid("org.example.sandbox-uid").expect("uid"));
    }

    #[test]
    fn a_planted_symlink_is_replaced_not_followed() {
        let dir = std::env::temp_dir()
            .join("anthracite-panel")
            .join(format!("sandbox-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        let source = dir.join("source");
        std::fs::write(&source, b"bundle bytes").expect("source");
        let victim = dir.join("victim");
        std::fs::write(&victim, b"must not change").expect("victim");
        std::os::unix::fs::symlink(&victim, dir.join("service")).expect("symlink");
        let staged =
            stage_binary(&source, &dir, uid_for("org.example"), engine_gid()).expect("stage");
        assert_eq!(std::fs::read(&victim).expect("victim"), b"must not change");
        assert_eq!(std::fs::read(&staged).expect("staged"), b"bundle bytes");
        assert!(
            std::fs::symlink_metadata(&staged)
                .expect("metadata")
                .file_type()
                .is_file(),
            "the symlink is replaced by the copied file"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_abi_probe_does_not_change_the_process() {
        // The probe is read-only: whatever the kernel answers, this process can
        // still touch its own state directory.
        let _ = abi();
        let dir = std::env::temp_dir().join("anthracite-panel");
        std::fs::create_dir_all(&dir).expect("scratch");
        assert!(std::fs::metadata(&dir).is_ok());
    }
}
