//! Account mutations.
//!
//! Every request is validated into a [`PlannedCall`] before anything runs. That
//! split is what makes the security properties testable: the guards are pure
//! functions over a snapshot, and the tests assert the exact argument vector
//! that would reach the system tools. Two properties depend on it directly:
//! no password ever appears in argv, and nothing is ever interpolated into a
//! shell.
//!
//! The tools are `useradd`/`userdel`/`groupadd`/`groupdel`/`chpasswd`. They are
//! not reimplemented by editing `/etc/passwd`: `useradd` takes the lock,
//! applies `login.defs`, creates the home from `skel` and maintains
//! `subuid`/`subgid`, and getting any of that wrong corrupts the host.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use anth_wire::{
    AccountCreate, AccountDelete, AccountGroup, AccountGroupCreate, AccountGroupDelete,
    AccountSetPassword, AccountSnapshot, AccountUser, ErrorKind, SecretString, WireError,
};

use super::AccountsSettings;
use super::inventory::IdRanges;

/// Groups that grant root-equivalent power. `docker` and `lxd` are on the list
/// because a member can mount the host filesystem inside a container.
pub const PRIVILEGED_GROUPS: &[&str] = &["sudo", "wheel", "admin", "root", "docker", "lxd"];

/// Administrative groups whose last member must never be removed.
pub const ADMIN_GROUPS: &[&str] = &["sudo", "wheel", "admin"];

/// Fallback allowlist when neither the config nor `/etc/shells` yields one.
const BUILTIN_SHELLS: &[&str] = &["/bin/sh", "/bin/bash", "/bin/ash", "/bin/zsh", "/bin/dash"];

const MAX_NAME_LEN: usize = 32;
const MAX_GECOS_LEN: usize = 256;
const MIN_PASSWORD_LEN: usize = 8;
const MAX_PASSWORD_LEN: usize = 256;

/// One validated invocation.
///
/// `stdin` rather than an argument is the whole point for a password: argv is
/// readable in `/proc` by every account on the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCall {
    pub program: &'static str,
    pub args: Vec<String>,
    pub stdin: Option<SecretString>,
}

impl PlannedCall {
    fn new(program: &'static str, args: Vec<String>) -> Self {
        Self {
            program,
            args,
            stdin: None,
        }
    }

    fn with_stdin(mut self, input: impl Into<String>) -> Self {
        self.stdin = Some(SecretString::new(input));
        self
    }
}

/// The result of one tool invocation. `Err` means the program could not be
/// executed; a refusal is `Ok(AccountOutput { ok: false, .. })`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountOutput {
    pub ok: bool,
    pub stderr: String,
}

pub trait AccountRunner: Send + Sync {
    fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<&str>,
    ) -> Result<AccountOutput, String>;
}

/// The production runner. `LC_ALL=C` keeps the classified stderr stable across
/// distros, and stdout is discarded: every tool in this module reports through
/// the exit status and stderr.
pub struct SystemAccountRunner;

impl AccountRunner for SystemAccountRunner {
    fn run(
        &self,
        program: &str,
        args: &[String],
        stdin: Option<&str>,
    ) -> Result<AccountOutput, String> {
        let mut command = Command::new(program);
        command.args(args).env("LC_ALL", "C");
        let output = match stdin {
            Some(input) => {
                command
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped());
                let mut child = command.spawn().map_err(|error| error.to_string())?;
                if let Some(mut pipe) = child.stdin.take() {
                    pipe.write_all(input.as_bytes())
                        .map_err(|error| error.to_string())?;
                }
                child
                    .wait_with_output()
                    .map_err(|error| error.to_string())?
            }
            None => command.output().map_err(|error| error.to_string())?,
        };
        Ok(AccountOutput {
            ok: output.status.success(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        })
    }
}

fn disabled(code: &'static str, detail: impl Into<String>) -> WireError {
    WireError::new(ErrorKind::Forbidden, code, detail)
}

fn protected(detail: impl Into<String>) -> WireError {
    WireError::new(ErrorKind::Forbidden, "account_protected", detail)
}

fn invalid(detail: impl Into<String>) -> WireError {
    WireError::new(ErrorKind::Invalid, "invalid_account", detail)
}

fn not_found(detail: impl Into<String>) -> WireError {
    WireError::new(ErrorKind::NotFound, "account_not_found", detail)
}

fn conflict(detail: impl Into<String>) -> WireError {
    WireError::new(ErrorKind::Conflict, "account_conflict", detail)
}

fn unsupported(detail: impl Into<String>) -> WireError {
    WireError::new(ErrorKind::Unsupported, "account_unsupported", detail)
}

/// Validates every mutation against the full snapshot and the policy.
///
/// `daemon_user` is the account the daemon runs as, so it cannot delete
/// itself. `id_ranges` come from the same read as the snapshot.
pub struct Guard<'a> {
    pub snapshot: &'a AccountSnapshot,
    pub config: &'a AccountsSettings,
    pub daemon_user: String,
    pub id_ranges: IdRanges,
}

impl<'a> Guard<'a> {
    pub fn new(
        snapshot: &'a AccountSnapshot,
        config: &'a AccountsSettings,
        daemon_user: impl Into<String>,
        id_ranges: IdRanges,
    ) -> Self {
        Self {
            snapshot,
            config,
            daemon_user: daemon_user.into(),
            id_ranges,
        }
    }

    fn user(&self, name: &str) -> Result<&'a AccountUser, WireError> {
        self.snapshot
            .users
            .iter()
            .find(|user| user.name == name)
            .ok_or_else(|| not_found(format!("no such user `{name}`")))
    }

    fn group(&self, name: &str) -> Result<&'a AccountGroup, WireError> {
        self.snapshot
            .groups
            .iter()
            .find(|group| group.name == name)
            .ok_or_else(|| not_found(format!("no such group `{name}`")))
    }

    /// Shared preamble for every mutation on an existing user.
    fn writable_user(&self, name: &str) -> Result<&'a AccountUser, WireError> {
        validate_name(name)?;
        let user = self.user(name)?;
        if !user.source.is_writable() {
            return Err(unsupported(format!(
                "`{name}` comes from a directory service and cannot be changed from the panel"
            )));
        }
        if user.uid == 0 {
            return Err(protected("uid 0 is never modified through the panel"));
        }
        if user.system && !self.config.allow_system {
            return Err(protected(format!(
                "`{name}` is a system account (uid {}); set accounts.allow_system to change it",
                user.uid
            )));
        }
        Ok(user)
    }

    /// Shared preamble for every mutation on an existing group.
    ///
    /// It mirrors `writable_user` on purpose. Without the system check the
    /// `allow_system` switch only guarded users, so a group below `GID_MIN`
    /// that is not on the privileged list (`shadow`, `disk`, `kmem`) could be
    /// deleted with the switch off.
    fn writable_group(&self, name: &str) -> Result<&'a AccountGroup, WireError> {
        validate_name(name)?;
        let group = self.group(name)?;
        if !group.source.is_writable() {
            return Err(unsupported(format!(
                "`{name}` comes from a directory service and cannot be changed from the panel"
            )));
        }
        if group.gid == 0 {
            return Err(protected("gid 0 is never modified through the panel"));
        }
        if group.system && !self.config.allow_system {
            return Err(protected(format!(
                "`{name}` is a system group (gid {}); set accounts.allow_system to change it",
                group.gid
            )));
        }
        Ok(group)
    }

    /// Refuses to delete the account the daemon itself runs as.
    fn not_self(&self, name: &str) -> Result<(), WireError> {
        if name == self.daemon_user {
            return Err(protected(
                "the daemon's own account cannot be changed; it would take the panel offline",
            ));
        }
        Ok(())
    }

    /// Every account that is an administrator by either provisioning style:
    /// a supplementary member of an administrative group, or an account whose
    /// primary gid is one of those groups.
    fn administrator_names(&self) -> HashSet<&str> {
        let mut names = HashSet::new();
        for group in self
            .snapshot
            .groups
            .iter()
            .filter(|group| ADMIN_GROUPS.contains(&group.name.as_str()))
        {
            names.extend(group.members.iter().map(String::as_str));
            names.extend(
                self.snapshot
                    .users
                    .iter()
                    .filter(|user| user.gid == group.gid)
                    .map(|user| user.name.as_str()),
            );
        }
        names
    }

    /// Refuses a delete that would leave `sudo`/`wheel`/`admin` with no other
    /// member. This is the loopback guard: without it the panel can lock every
    /// operator out of the host it is meant to administer.
    fn not_last_administrator(&self, name: &str) -> Result<(), WireError> {
        let administrators = self.administrator_names();
        if !administrators.contains(name) {
            return Ok(());
        }
        // Count distinct accounts, not group entries: one user can be an
        // administrator through both a primary group and a supplementary one.
        let remaining = administrators
            .iter()
            .filter(|administrator| **administrator != name)
            .count();
        // uid 0 is deliberately not an escape hatch: on many hosts root is
        // locked and has no password, which is exactly the state that would
        // make this guard fail open.
        if remaining == 0 {
            return Err(protected(format!(
                "refusing to delete `{name}`: it is the last member of an administrative group"
            )));
        }
        Ok(())
    }

    /// Refuses to empty an administrative group through a delete.
    fn keep_an_administrator(&self, group: &AccountGroup) -> Result<(), WireError> {
        if !ADMIN_GROUPS.contains(&group.name.as_str()) {
            return Ok(());
        }
        // A primary-group member is still an administrator even though
        // `/etc/group`'s member field does not list them.
        if self.snapshot.users.iter().any(|user| user.gid == group.gid) {
            return Ok(());
        }
        if group.members.is_empty() {
            return Ok(());
        }
        let others: Vec<&AccountGroup> = self
            .snapshot
            .groups
            .iter()
            .filter(|other| other.name != group.name && ADMIN_GROUPS.contains(&other.name.as_str()))
            .collect();
        let other_gids: HashSet<u32> = others.iter().map(|other| other.gid).collect();
        let other_primary = self
            .snapshot
            .users
            .iter()
            .any(|user| other_gids.contains(&user.gid));
        if other_primary || others.iter().any(|other| !other.members.is_empty()) {
            return Ok(());
        }
        Err(protected(format!(
            "refusing to empty `{}`: no other administrative group would keep a member",
            group.name
        )))
    }

    fn require_privileged_group_allowed(&self, group: &str) -> Result<(), WireError> {
        if PRIVILEGED_GROUPS.contains(&group) && !self.config.allow_privileged_groups {
            return Err(protected(format!(
                "`{group}` grants root-equivalent access; set accounts.allow_privileged_groups to touch it"
            )));
        }
        Ok(())
    }
}

/// A user or group name reaches an argv vector and a `/etc/passwd` field, so it
/// is restricted to what every tool on every distro accepts. The leading
/// character rule is also what stops `-rf` from being read as a flag.
pub fn validate_name(name: &str) -> Result<(), WireError> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(invalid(format!(
            "a name must be 1 to {MAX_NAME_LEN} characters"
        )));
    }
    let mut chars = name.chars();
    let first = chars.next().expect("checked non-empty");
    if !(first.is_ascii_lowercase() || first == '_') {
        return Err(invalid(
            "a name must start with a lowercase letter or an underscore",
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
    {
        return Err(invalid(
            "a name may contain lowercase letters, digits, underscores and hyphens",
        ));
    }
    Ok(())
}

/// GECOS goes into a colon-separated record: a colon or a newline would add or
/// split a field, and a control character would corrupt the file.
pub fn validate_gecos(value: &str) -> Result<(), WireError> {
    if value.len() > MAX_GECOS_LEN {
        return Err(invalid(format!(
            "a description must be at most {MAX_GECOS_LEN} characters"
        )));
    }
    if value.contains(':') || value.contains('\n') || value.contains('\r') {
        return Err(invalid("a description must not contain `:` or a newline"));
    }
    if value.chars().any(|c| c.is_control()) {
        return Err(invalid("a description must not contain control characters"));
    }
    Ok(())
}

fn normalize(path: &str) -> Result<PathBuf, WireError> {
    if !path.starts_with('/') {
        return Err(invalid("the home directory must be an absolute path"));
    }
    let mut normalized = PathBuf::from("/");
    for component in Path::new(path).components() {
        match component {
            Component::RootDir => {}
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                return Err(invalid("the home directory must not contain `..`"));
            }
            Component::Prefix(_) => {
                return Err(invalid(
                    "the home directory contains a path component the panel refuses",
                ));
            }
        }
    }
    Ok(normalized)
}

/// Resolves a requested home directory and proves it stays inside `home_root`.
///
/// The parent is what exists at request time — `useradd` creates the leaf — so
/// the parent is what gets canonicalised. That is what stops `/home/../../etc`
/// and a symlinked parent.
pub fn validate_home(home: &str, home_root: &Path) -> Result<PathBuf, WireError> {
    let path = normalize(home)?;
    let root = std::fs::canonicalize(home_root).unwrap_or_else(|_| home_root.to_path_buf());
    let parent = path.parent().unwrap_or(Path::new("/"));
    let leaf = path
        .file_name()
        .ok_or_else(|| invalid("the home directory must name a folder, not `/`"))?;
    let parent = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    let resolved = parent.join(leaf);

    if !resolved.starts_with(&root) {
        return Err(protected(format!(
            "the home directory must be inside `{}`",
            root.display()
        )));
    }
    if resolved == root {
        return Err(protected(
            "the home directory cannot be the root folder itself",
        ));
    }
    Ok(resolved)
}

/// Shell must be on the allowlist. A user must not be able to name an arbitrary
/// binary as a login shell, which would be a code-execution path on next login.
pub fn validate_shell(shell: &str, allowed: &[String]) -> Result<(), WireError> {
    if !shell.starts_with('/') {
        return Err(invalid("the shell must be an absolute path"));
    }
    if allowed.iter().any(|entry| entry == shell) {
        return Ok(());
    }
    Err(invalid(format!("`{shell}` is not an allowed login shell")))
}

/// The effective shell allowlist: config first, then `/etc/shells`, then a
/// small built-in set, so an empty config never means "anything goes".
pub fn allowed_shells(configured: &[String]) -> Vec<String> {
    if !configured.is_empty() {
        return configured.to_vec();
    }
    if let Ok(text) = std::fs::read_to_string("/etc/shells") {
        let listed: Vec<String> = text
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with('/'))
            .map(str::to_string)
            .collect();
        if !listed.is_empty() {
            return listed;
        }
    }
    BUILTIN_SHELLS
        .iter()
        .map(|shell| (*shell).to_string())
        .collect()
}

fn validate_new_id(id: u32, taken_by: Option<&str>, kind: &str) -> Result<(), WireError> {
    if id == 0 {
        return Err(protected(format!("{kind} 0 is reserved")));
    }
    if let Some(existing) = taken_by {
        return Err(conflict(format!(
            "{kind} {id} is already used by `{existing}`"
        )));
    }
    Ok(())
}

/// Refuses an explicitly requested id that lands in the system range.
fn require_system_allowed(guard: &Guard<'_>, id: u32, kind: &str) -> Result<(), WireError> {
    let minimum = if kind == "uid" {
        guard.id_ranges.uid_min
    } else {
        guard.id_ranges.gid_min
    };
    if id >= minimum || guard.config.allow_system {
        return Ok(());
    }
    Err(protected(format!(
        "{kind} {id} is below {minimum}; set accounts.allow_system to use a system id"
    )))
}

fn user_with_uid(snapshot: &AccountSnapshot, uid: u32) -> Option<&str> {
    snapshot
        .users
        .iter()
        .find(|user| user.uid == uid)
        .map(|user| user.name.as_str())
}

fn group_with_gid(snapshot: &AccountSnapshot, gid: u32) -> Option<&str> {
    snapshot
        .groups
        .iter()
        .find(|group| group.gid == gid)
        .map(|group| group.name.as_str())
}

pub fn plan_create_user(
    guard: &Guard<'_>,
    request: &AccountCreate,
) -> Result<PlannedCall, WireError> {
    if !guard.config.control {
        return Err(disabled(
            "accounts_disabled",
            "account changes are disabled; set accounts.control to enable them",
        ));
    }
    validate_name(&request.name)?;
    if guard
        .snapshot
        .users
        .iter()
        .any(|user| user.name == request.name)
    {
        return Err(conflict(format!("user `{}` already exists", request.name)));
    }
    if guard
        .snapshot
        .groups
        .iter()
        .any(|group| group.name == request.name)
    {
        // `useradd` would fail anyway; saying why is more useful.
        return Err(conflict(format!(
            "group `{}` already exists; `useradd` would refuse to create the user",
            request.name
        )));
    }
    if request.system && !guard.config.allow_system {
        return Err(protected(
            "creating a system account requires accounts.allow_system",
        ));
    }

    if let Some(uid) = request.uid {
        validate_new_id(uid, user_with_uid(guard.snapshot, uid), "uid")?;
        require_system_allowed(guard, uid, "uid")?;
    }
    if let Some(gid) = request.gid {
        // An explicit primary gid must name a group that already exists: the
        // account would otherwise start with an unresolvable group.
        let group = guard
            .snapshot
            .groups
            .iter()
            .find(|group| group.gid == gid)
            .ok_or_else(|| not_found(format!("no such gid `{gid}`")))?
            .clone();
        // The privileged check runs first: a group like `sudo` is both below
        // GID_MIN and root-equivalent, and the root warning is the one an
        // operator needs to read.
        guard.require_privileged_group_allowed(&group.name)?;
        require_system_allowed(guard, gid, "gid")?;
    }
    if let Some(gecos) = request.gecos.as_deref() {
        validate_gecos(gecos)?;
    }

    let home_root = Path::new(&guard.config.home_root);
    let home = match request.home.as_deref() {
        Some(home) => validate_home(home, home_root)?,
        None => validate_home(
            &format!(
                "{}/{}",
                guard.config.home_root.trim_end_matches('/'),
                request.name
            ),
            home_root,
        )?,
    };

    let shells = allowed_shells(&guard.config.shells);
    let shell = request
        .shell
        .as_deref()
        .unwrap_or(&guard.config.default_shell);
    validate_shell(shell, &shells)?;

    let mut args = vec!["--create-home".to_string()];
    if request.system {
        args.push("--system".to_string());
    }
    if let Some(uid) = request.uid {
        args.extend(["--uid".to_string(), uid.to_string()]);
    }
    if let Some(gid) = request.gid {
        args.extend(["--gid".to_string(), gid.to_string()]);
    }
    if let Some(gecos) = request.gecos.as_deref() {
        args.extend(["--comment".to_string(), gecos.to_string()]);
    }
    args.extend([
        "--home-dir".to_string(),
        home.to_string_lossy().into_owned(),
        "--shell".to_string(),
        shell.to_string(),
    ]);
    // `useradd` creates the account locked unless a password is supplied, and
    // the protocol has no password field, so a new account can never log in
    // until `set_password` is called. That is the point.
    args.push(request.name.clone());
    Ok(PlannedCall::new("useradd", args))
}

pub fn plan_delete_user(
    guard: &Guard<'_>,
    request: &AccountDelete,
) -> Result<PlannedCall, WireError> {
    if !guard.config.control {
        return Err(disabled(
            "accounts_disabled",
            "account changes are disabled; set accounts.control to enable them",
        ));
    }
    let user = guard.writable_user(&request.name)?;
    guard.not_self(&request.name)?;
    guard.not_last_administrator(&request.name)?;

    let mut args = Vec::new();
    if request.remove_home {
        // Deleting a home is only ever explicit, and only inside home_root; the
        // validation proves that before `userdel` sees `--remove`. `--force`
        // is never passed: it would remove the user even while logged in.
        validate_home(&user.home, Path::new(&guard.config.home_root))?;
        args.push("--remove".to_string());
    }
    args.push(request.name.clone());
    Ok(PlannedCall::new("userdel", args))
}

pub fn plan_set_password(
    guard: &Guard<'_>,
    request: &AccountSetPassword,
) -> Result<PlannedCall, WireError> {
    if !guard.config.manage_passwords {
        return Err(disabled(
            "account_passwords_disabled",
            "password changes are disabled; set accounts.manage_passwords to enable them",
        ));
    }
    guard.writable_user(&request.name)?;

    let password = request.password.expose();
    if password.chars().count() < MIN_PASSWORD_LEN {
        return Err(invalid(format!(
            "a password must be at least {MIN_PASSWORD_LEN} characters"
        )));
    }
    if password.chars().count() > MAX_PASSWORD_LEN {
        return Err(invalid(format!(
            "a password must be at most {MAX_PASSWORD_LEN} characters"
        )));
    }
    // The line is `<name>:<password>\n`. A newline or colon in the password
    // would inject a second record or a second field, so they are refused
    // rather than escaped: `chpasswd` has no escaping to rely on.
    if password.contains('\n') || password.contains('\r') || password.contains(':') {
        return Err(invalid("a password must not contain `:` or a newline"));
    }

    Ok(PlannedCall::new("chpasswd", Vec::new())
        .with_stdin(format!("{}:{password}\n", request.name)))
}

pub fn plan_create_group(
    guard: &Guard<'_>,
    request: &AccountGroupCreate,
) -> Result<PlannedCall, WireError> {
    if !guard.config.control {
        return Err(disabled(
            "accounts_disabled",
            "account changes are disabled; set accounts.control to enable them",
        ));
    }
    validate_name(&request.name)?;
    if guard
        .snapshot
        .groups
        .iter()
        .any(|group| group.name == request.name)
    {
        return Err(conflict(format!("group `{}` already exists", request.name)));
    }
    guard.require_privileged_group_allowed(&request.name)?;
    if request.system && !guard.config.allow_system {
        return Err(protected(
            "creating a system group requires accounts.allow_system",
        ));
    }
    if let Some(gid) = request.gid {
        validate_new_id(gid, group_with_gid(guard.snapshot, gid), "gid")?;
        require_system_allowed(guard, gid, "gid")?;
    }

    let mut args = Vec::new();
    if request.system {
        args.push("--system".to_string());
    }
    if let Some(gid) = request.gid {
        args.extend(["--gid".to_string(), gid.to_string()]);
    }
    args.push(request.name.clone());
    Ok(PlannedCall::new("groupadd", args))
}

pub fn plan_delete_group(
    guard: &Guard<'_>,
    request: &AccountGroupDelete,
) -> Result<PlannedCall, WireError> {
    if !guard.config.control {
        return Err(disabled(
            "accounts_disabled",
            "account changes are disabled; set accounts.control to enable them",
        ));
    }
    validate_name(&request.name)?;
    // `writable_group` refuses gid 0 with the modify wording; a delete deserves
    // its own, so the check runs first to word it correctly.
    if guard.group(&request.name)?.gid == 0 {
        return Err(protected("gid 0 is never deleted through the panel"));
    }
    guard.require_privileged_group_allowed(&request.name)?;
    let group = guard.writable_group(&request.name)?;

    // A group that is somebody's primary group cannot be removed: those users
    // would be left with a gid that resolves to nothing.
    if let Some(owner) = guard
        .snapshot
        .users
        .iter()
        .find(|user| user.gid == group.gid)
    {
        return Err(conflict(format!(
            "`{}` is the primary group of `{}`",
            request.name, owner.name
        )));
    }
    guard.keep_an_administrator(group)?;

    Ok(PlannedCall::new("groupdel", vec![request.name.clone()]))
}

/// Runs one planned call and maps a tool failure onto the protocol error.
pub fn run_planned(runner: &dyn AccountRunner, call: &PlannedCall) -> Result<(), WireError> {
    let outcome = runner.run(
        call.program,
        &call.args,
        call.stdin.as_ref().map(SecretString::expose),
    );
    match outcome {
        Ok(AccountOutput { ok: true, .. }) => Ok(()),
        Ok(output) => Err(classified_failure(call.program, &output.stderr)),
        Err(detail) => Err(unsupported(format!(
            "`{}` is not installed or could not be executed: {detail}",
            call.program
        ))),
    }
}

/// The tools report failures as plain text. `LC_ALL=C` is set by the runner, so
/// these strings are stable on every distro.
fn classified_failure(program: &str, stderr: &str) -> WireError {
    let lowercase = stderr.to_lowercase();
    let (kind, code) = if lowercase.contains("already exists")
        || lowercase.contains("not unique")
        || lowercase.contains("is currently used")
    {
        (ErrorKind::Conflict, "account_conflict")
    } else if lowercase.contains("does not exist")
        || lowercase.contains("no such user")
        || lowercase.contains("no such group")
    {
        (ErrorKind::NotFound, "account_not_found")
    } else if lowercase.contains("permission denied")
        || lowercase.contains("operation not permitted")
        || lowercase.contains("must be root")
        || lowercase.contains("cannot open")
        || lowercase.contains("cannot lock")
    {
        (ErrorKind::Forbidden, "account_permission")
    } else if lowercase.contains("invalid")
        || lowercase.contains("not a valid")
        || lowercase.contains("bad name")
    {
        (ErrorKind::Invalid, "invalid_account")
    } else {
        (ErrorKind::Internal, "account_failed")
    };

    let detail = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    WireError::new(
        kind,
        code,
        if detail.is_empty() {
            format!("{program} refused the change")
        } else {
            format!("{program} refused the change: {detail}")
        },
    )
}

/// The account the daemon runs as, for the self-protection guard.
///
/// The real uid wins. `$USER` and `$LOGNAME` are inherited environment, so a
/// service unit or `docker exec` can hand the daemon a stale name; trusting it
/// first would protect an account that is not running the panel and leave the
/// daemon's own account deletable.
pub fn daemon_user() -> String {
    current_uid_name()
        .or_else(|| {
            std::env::var("USER")
                .ok()
                .or_else(|| std::env::var("LOGNAME").ok())
                .filter(|name| !name.is_empty())
        })
        .unwrap_or_else(|| "root".to_string())
}

fn current_uid_name() -> Option<String> {
    let passwd = std::fs::read_to_string("/etc/passwd").ok()?;
    let uid = unsafe { libc::getuid() };
    passwd
        .lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() < 3 {
                return None;
            }
            parts[2]
                .trim()
                .parse::<u32>()
                .ok()
                .filter(|found| *found == uid)
                .map(|_| parts[0].to_string())
        })
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use anth_wire::{AccountGroup, AccountSource};

    fn snapshot() -> AccountSnapshot {
        AccountSnapshot {
            users: vec![
                AccountUser {
                    name: "root".to_string(),
                    uid: 0,
                    gid: 0,
                    group: "root".to_string(),
                    gecos: "root".to_string(),
                    home: "/root".to_string(),
                    shell: "/bin/sh".to_string(),
                    locked: Some(true),
                    system: true,
                    source: AccountSource::Local,
                    groups: Vec::new(),
                },
                AccountUser {
                    name: "alice".to_string(),
                    uid: 1001,
                    gid: 1001,
                    group: "alice".to_string(),
                    gecos: "Alice".to_string(),
                    home: "/home/alice".to_string(),
                    shell: "/bin/sh".to_string(),
                    locked: Some(false),
                    system: false,
                    source: AccountSource::Local,
                    groups: vec!["wheel".to_string()],
                },
                AccountUser {
                    name: "bob".to_string(),
                    uid: 1002,
                    gid: 1002,
                    group: "bob".to_string(),
                    gecos: String::new(),
                    home: "/home/bob".to_string(),
                    shell: "/bin/sh".to_string(),
                    locked: Some(true),
                    system: false,
                    source: AccountSource::Local,
                    groups: Vec::new(),
                },
                AccountUser {
                    name: "dora".to_string(),
                    uid: 2001,
                    gid: 2001,
                    group: "dora".to_string(),
                    gecos: String::new(),
                    home: "/home/dora".to_string(),
                    shell: "/bin/sh".to_string(),
                    locked: None,
                    system: false,
                    source: AccountSource::Nss,
                    groups: Vec::new(),
                },
            ],
            groups: vec![
                AccountGroup {
                    name: "root".to_string(),
                    gid: 0,
                    members: Vec::new(),
                    system: true,
                    source: AccountSource::Local,
                },
                AccountGroup {
                    name: "wheel".to_string(),
                    gid: 10,
                    members: vec!["alice".to_string()],
                    system: true,
                    source: AccountSource::Local,
                },
                AccountGroup {
                    name: "alice".to_string(),
                    gid: 1001,
                    members: Vec::new(),
                    system: false,
                    source: AccountSource::Local,
                },
                AccountGroup {
                    name: "bob".to_string(),
                    gid: 1002,
                    members: Vec::new(),
                    system: false,
                    source: AccountSource::Local,
                },
                AccountGroup {
                    name: "docker".to_string(),
                    gid: 999,
                    members: Vec::new(),
                    system: true,
                    source: AccountSource::Local,
                },
                AccountGroup {
                    name: "dora".to_string(),
                    gid: 2001,
                    members: Vec::new(),
                    system: false,
                    source: AccountSource::Nss,
                },
            ],
            control_enabled: true,
            password_enabled: true,
            allow_system: false,
            allow_privileged_groups: false,
            timestamp_ms: 1,
            warning: None,
        }
    }

    fn base_config() -> AccountsSettings {
        AccountsSettings {
            control: true,
            manage_passwords: true,
            allow_system: true,
            allow_privileged_groups: true,
            home_root: "/home".to_string(),
            shells: vec!["/bin/sh".to_string(), "/bin/bash".to_string()],
            default_shell: "/bin/sh".to_string(),
        }
    }

    fn make_guard<'a>(snapshot: &'a AccountSnapshot, config: &'a AccountsSettings) -> Guard<'a> {
        Guard::new(snapshot, config, "panel", IdRanges::default())
    }

    fn create(name: &str) -> AccountCreate {
        AccountCreate {
            name: name.to_string(),
            uid: None,
            gid: None,
            gecos: None,
            home: None,
            shell: None,
            system: false,
        }
    }

    #[test]
    fn useradd_creates_a_locked_home_and_never_a_password() {
        let snapshot = snapshot();
        let config = base_config();
        let plan =
            plan_create_user(&make_guard(&snapshot, &config), &create("carol")).expect("plan");
        assert_eq!(plan.program, "useradd");
        assert_eq!(
            plan.args,
            vec![
                "--create-home",
                "--home-dir",
                "/home/carol",
                "--shell",
                "/bin/sh",
                "carol"
            ]
        );
        assert!(plan.stdin.is_none());
    }

    #[test]
    fn useradd_passes_ids_comment_home_and_shell_in_order() {
        let snapshot = snapshot();
        let config = base_config();
        let request = AccountCreate {
            name: "carol".to_string(),
            uid: Some(1500),
            gid: Some(1001),
            gecos: Some("Carol".to_string()),
            home: Some("/home/carol".to_string()),
            shell: Some("/bin/bash".to_string()),
            system: false,
        };
        let plan = plan_create_user(&make_guard(&snapshot, &config), &request).expect("plan");
        assert_eq!(
            plan.args,
            vec![
                "--create-home",
                "--uid",
                "1500",
                "--gid",
                "1001",
                "--comment",
                "Carol",
                "--home-dir",
                "/home/carol",
                "--shell",
                "/bin/bash",
                "carol"
            ]
        );
    }

    #[test]
    fn names_ids_homes_and_shells_are_validated() {
        let snapshot = snapshot();
        let config = base_config();
        let guard = make_guard(&snapshot, &config);

        assert_eq!(
            plan_create_user(&guard, &create("-rf"))
                .expect_err("leading dash")
                .code,
            "invalid_account"
        );
        assert_eq!(
            plan_create_user(&guard, &create("alice"))
                .expect_err("duplicate")
                .code,
            "account_conflict"
        );

        let mut system = create("carol");
        system.system = true;
        let strict = AccountsSettings {
            allow_system: false,
            ..base_config()
        };
        let strict_guard = make_guard(&snapshot, &strict);
        assert_eq!(
            plan_create_user(&strict_guard, &system)
                .expect_err("system")
                .code,
            "account_protected"
        );

        let mut outside = create("carol");
        outside.home = Some("/etc/carol".to_string());
        assert_eq!(
            plan_create_user(&guard, &outside).expect_err("home").code,
            "account_protected"
        );

        let mut shell = create("carol");
        shell.shell = Some("/usr/bin/python".to_string());
        assert_eq!(
            plan_create_user(&guard, &shell).expect_err("shell").code,
            "invalid_account"
        );

        let mut taken = create("carol");
        taken.uid = Some(1001);
        assert_eq!(
            plan_create_user(&guard, &taken)
                .expect_err("taken uid")
                .code,
            "account_conflict"
        );
    }

    #[test]
    fn system_ids_need_the_opt_in_even_without_the_system_flag() {
        let snapshot = snapshot();
        let config = AccountsSettings {
            allow_system: false,
            ..base_config()
        };
        let guard = make_guard(&snapshot, &config);
        let mut request = create("carol");
        request.uid = Some(999);
        assert_eq!(
            plan_create_user(&guard, &request)
                .expect_err("system uid")
                .code,
            "account_protected"
        );
    }

    #[test]
    fn uid_zero_self_nss_and_the_last_administrator_are_protected() {
        let snapshot = snapshot();
        let config = base_config();
        let guard = make_guard(&snapshot, &config);

        assert_eq!(
            plan_delete_user(
                &guard,
                &AccountDelete {
                    name: "root".to_string(),
                    remove_home: false,
                },
            )
            .expect_err("root")
            .code,
            "account_protected"
        );
        assert_eq!(
            plan_delete_user(
                &guard,
                &AccountDelete {
                    name: "panel".to_string(),
                    remove_home: false,
                },
            )
            .expect_err("missing self")
            .code,
            "account_not_found"
        );
        assert_eq!(
            plan_delete_user(
                &guard,
                &AccountDelete {
                    name: "dora".to_string(),
                    remove_home: false,
                },
            )
            .expect_err("nss")
            .code,
            "account_unsupported"
        );
        assert_eq!(
            plan_delete_user(
                &guard,
                &AccountDelete {
                    name: "alice".to_string(),
                    remove_home: false,
                },
            )
            .expect_err("last admin")
            .code,
            "account_protected"
        );

        let self_guard = Guard::new(&snapshot, &config, "bob", IdRanges::default());
        assert_eq!(
            plan_delete_user(
                &self_guard,
                &AccountDelete {
                    name: "bob".to_string(),
                    remove_home: false,
                },
            )
            .expect_err("self")
            .code,
            "account_protected"
        );
    }

    #[test]
    fn userdel_removes_a_home_only_inside_home_root() {
        let snapshot = snapshot();
        let config = base_config();
        let guard = make_guard(&snapshot, &config);
        let plan = plan_delete_user(
            &guard,
            &AccountDelete {
                name: "bob".to_string(),
                remove_home: true,
            },
        )
        .expect("delete");
        assert_eq!(plan.args, vec!["--remove", "bob"]);

        let mut outside = snapshot.clone();
        outside.users[2].home = "/srv/bob".to_string();
        let outside_guard = make_guard(&outside, &config);
        assert_eq!(
            plan_delete_user(
                &outside_guard,
                &AccountDelete {
                    name: "bob".to_string(),
                    remove_home: true,
                },
            )
            .expect_err("outside")
            .code,
            "account_protected"
        );
    }

    #[test]
    fn chpasswd_takes_the_pair_on_stdin_and_rejects_injection() {
        let snapshot = snapshot();
        let config = base_config();
        let guard = make_guard(&snapshot, &config);
        let request = AccountSetPassword {
            name: "bob".to_string(),
            password: SecretString::new("long enough"),
        };
        let plan = plan_set_password(&guard, &request).expect("plan");
        assert_eq!(plan.program, "chpasswd");
        assert!(plan.args.is_empty());
        assert_eq!(
            plan.stdin.as_ref().map(SecretString::expose),
            Some("bob:long enough\n")
        );
        assert!(!format!("{plan:?}").contains("long enough"));

        for password in ["short", "has:colon", "has\nnewline"] {
            let request = AccountSetPassword {
                name: "bob".to_string(),
                password: SecretString::new(password),
            };
            assert_eq!(
                plan_set_password(&guard, &request)
                    .expect_err("bad password")
                    .code,
                "invalid_account"
            );
        }

        let disabled = AccountsSettings {
            manage_passwords: false,
            ..base_config()
        };
        let disabled_guard = make_guard(&snapshot, &disabled);
        assert_eq!(
            plan_set_password(&disabled_guard, &request)
                .expect_err("disabled")
                .code,
            "account_passwords_disabled"
        );
    }

    #[test]
    fn group_create_and_delete_guards_and_argv() {
        let snapshot = snapshot();
        let config = base_config();
        let guard = make_guard(&snapshot, &config);

        let plan = plan_create_group(
            &guard,
            &AccountGroupCreate {
                name: "builders".to_string(),
                gid: Some(1500),
                system: false,
            },
        )
        .expect("create");
        assert_eq!(plan.program, "groupadd");
        assert_eq!(plan.args, vec!["--gid", "1500", "builders"]);

        let strict = AccountsSettings {
            allow_privileged_groups: false,
            ..base_config()
        };
        let strict_guard = make_guard(&snapshot, &strict);
        assert_eq!(
            plan_create_group(
                &strict_guard,
                &AccountGroupCreate {
                    name: "lxd".to_string(),
                    gid: None,
                    system: false,
                },
            )
            .expect_err("privileged")
            .code,
            "account_protected"
        );
        assert_eq!(
            plan_delete_group(
                &guard,
                &AccountGroupDelete {
                    name: "root".to_string(),
                },
            )
            .expect_err("gid zero")
            .code,
            "account_protected"
        );
        assert_eq!(
            plan_delete_group(
                &guard,
                &AccountGroupDelete {
                    name: "alice".to_string(),
                },
            )
            .expect_err("primary group")
            .code,
            "account_conflict"
        );

        let plan = plan_delete_group(
            &guard,
            &AccountGroupDelete {
                name: "docker".to_string(),
            },
        )
        .expect("delete");
        assert_eq!(plan.program, "groupdel");
        assert_eq!(plan.args, vec!["docker"]);
    }

    #[test]
    fn privileged_groups_need_the_opt_in() {
        let snapshot = snapshot();
        let config = AccountsSettings {
            allow_privileged_groups: false,
            ..base_config()
        };
        let guard = make_guard(&snapshot, &config);
        assert_eq!(
            plan_delete_group(
                &guard,
                &AccountGroupDelete {
                    name: "wheel".to_string(),
                },
            )
            .expect_err("wheel")
            .code,
            "account_protected"
        );
    }

    #[test]
    fn a_disabled_control_switch_refuses_every_mutation() {
        let snapshot = snapshot();
        let config = AccountsSettings {
            control: false,
            ..base_config()
        };
        let guard = make_guard(&snapshot, &config);
        assert_eq!(
            plan_create_user(&guard, &create("carol"))
                .expect_err("create")
                .code,
            "accounts_disabled"
        );
        assert_eq!(
            plan_delete_user(
                &guard,
                &AccountDelete {
                    name: "bob".to_string(),
                    remove_home: false,
                },
            )
            .expect_err("delete")
            .code,
            "accounts_disabled"
        );
        assert_eq!(
            plan_create_group(
                &guard,
                &AccountGroupCreate {
                    name: "builders".to_string(),
                    gid: None,
                    system: false,
                },
            )
            .expect_err("group create")
            .code,
            "accounts_disabled"
        );
    }

    #[test]
    fn tool_failures_are_classified() {
        assert_eq!(
            classified_failure("useradd", "useradd: user 'x' already exists").code,
            "account_conflict"
        );
        assert_eq!(
            classified_failure("userdel", "userdel: user 'x' does not exist").code,
            "account_not_found"
        );
        assert_eq!(
            classified_failure("chpasswd", "chpasswd: cannot lock /etc/passwd").code,
            "account_permission"
        );
        assert_eq!(
            classified_failure("groupadd", "groupadd: 'x' is not a valid group name").code,
            "invalid_account"
        );
        assert_eq!(
            classified_failure("useradd", "something unexpected").code,
            "account_failed"
        );
    }
}
