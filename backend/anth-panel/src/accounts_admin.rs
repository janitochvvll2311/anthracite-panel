//! Account mutations.
//!
//! Every request is validated into a `PlannedCall` before anything runs. That
//! split is what makes the security properties testable: the guards are pure
//! functions over a snapshot, and the tests assert the exact argument vector
//! that would reach the system tools. Two properties depend on it directly:
//! no password ever appears in argv, and nothing is ever interpolated into a
//! shell.
//!
//! The tools are `useradd`/`usermod`/`userdel`/`groupadd`/`groupmod`/
//! `groupdel`/`gpasswd`/`chpasswd`. They are not reimplemented by editing
//! `/etc/passwd`: `useradd` takes the lock, applies `login.defs`, creates the
//! home from `skel` and maintains `subuid`/`subgid`, and getting any of that
//! wrong corrupts the host.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anth_proto::{
    AccountError, AccountErrorKind, AccountGroup, AccountSnapshot, AccountUser, SecretString,
};

use crate::accounts::IdRanges;
use crate::command::{CommandError, CommandOutput, CommandRunner, SystemRunner};
use crate::config::AccountsConfig;

/// Groups that grant root-equivalent power. `docker` and `lxd` are on the list
/// because a member can mount the host filesystem inside a container.
pub const PRIVILEGED_GROUPS: &[&str] = &["sudo", "wheel", "admin", "root", "docker", "lxd"];

/// Administrative groups whose last member must never be removed.
pub const ADMIN_GROUPS: &[&str] = &["sudo", "wheel", "admin"];

/// Fallback allowlist when neither the config nor `/etc/shells` yields one.
const BUILTIN_SHELLS: &[&str] = &["/bin/sh", "/bin/bash", "/bin/ash", "/bin/zsh", "/bin/dash"];

const MAX_NAME_LEN: usize = 32;
const MAX_GECOS_LEN: usize = 256;

/// `useradd` refuses to create a user when a group with the same name exists,
/// and creates a same-name group otherwise. Both cases are handled explicitly
/// so the caller knows which happened.
pub fn invalid(detail: impl Into<String>) -> AccountError {
    AccountError::new(AccountErrorKind::Invalid, detail)
}

fn protected(detail: impl Into<String>) -> AccountError {
    AccountError::new(AccountErrorKind::Protected, detail)
}

fn disabled(detail: impl Into<String>) -> AccountError {
    AccountError::new(AccountErrorKind::Disabled, detail)
}

fn unsupported(detail: impl Into<String>) -> AccountError {
    AccountError::new(AccountErrorKind::Unsupported, detail)
}

fn failed(detail: impl Into<String>) -> AccountError {
    AccountError::new(AccountErrorKind::Failed, detail)
}

fn not_found(detail: impl Into<String>) -> AccountError {
    AccountError::new(AccountErrorKind::NotFound, detail)
}

fn conflict(detail: impl Into<String>) -> AccountError {
    AccountError::new(AccountErrorKind::Conflict, detail)
}

/// One validated invocation.
///
/// `stdin` rather than an argument is the whole point for a password: argv is
/// readable in `/proc` by every account on the host.
#[derive(Clone, PartialEq, Eq)]
pub struct PlannedCall {
    pub program: String,
    pub args: Vec<String>,
    /// Hidden behind a type that redacts itself in `Debug`, so this struct can
    /// be logged safely. A derived `Debug` here would print the password.
    pub stdin: Option<SecretString>,
}

impl std::fmt::Debug for PlannedCall {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PlannedCall")
            .field("program", &self.program)
            .field("args", &self.args)
            .field("stdin", &self.stdin)
            .finish()
    }
}

impl PlannedCall {
    fn new(program: &str, args: Vec<String>) -> Self {
        Self {
            program: program.to_string(),
            args,
            stdin: None,
        }
    }

    fn with_stdin(mut self, input: impl Into<String>) -> Self {
        self.stdin = Some(SecretString::new(input));
        self
    }
}

/// Validates every mutation against the unfitted snapshot and the policy.
///
/// `config` is the `[accounts]` section; `daemon_user` is the account the
/// daemon runs as, so it cannot delete itself.
pub struct Guard<'a> {
    pub snapshot: &'a AccountSnapshot,
    pub config: &'a AccountsConfig,
    pub daemon_user: String,
    /// `UID_MIN`/`GID_MIN` from the host's `login.defs`. Held here so a plan can
    /// tell whether an explicitly requested id lands in system space; the
    /// snapshot classifies existing entries but carries no ranges.
    pub id_ranges: IdRanges,
}

impl<'a> Guard<'a> {
    pub fn new(
        snapshot: &'a AccountSnapshot,
        config: &'a AccountsConfig,
        daemon_user: impl Into<String>,
    ) -> Self {
        Self {
            snapshot,
            config,
            daemon_user: daemon_user.into(),
            id_ranges: IdRanges::default(),
        }
    }

    /// Supplies the ranges read from `login.defs`. Defaults match shadow's
    /// built-in `UID_MIN`/`GID_MIN`, so a host without the file is still guarded.
    pub fn with_id_ranges(mut self, ranges: IdRanges) -> Self {
        self.id_ranges = ranges;
        self
    }

    fn user(&self, name: &str) -> Result<&'a AccountUser, AccountError> {
        self.snapshot
            .users
            .iter()
            .find(|user| user.name == name)
            .ok_or_else(|| not_found(format!("no such user `{name}`")))
    }

    fn group(&self, name: &str) -> Result<&'a AccountGroup, AccountError> {
        self.snapshot
            .groups
            .iter()
            .find(|group| group.name == name)
            .ok_or_else(|| not_found(format!("no such group `{name}`")))
    }

    /// Shared preamble for every mutation on an existing user.
    fn writable_user(&self, name: &str) -> Result<&'a AccountUser, AccountError> {
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
        // The snapshot already classified the entry against the host's own
        // `login.defs` range, so the guard and the display agree by construction.
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
    /// that is *not* on the privileged list (`shadow`, `disk`, `kmem`) could be
    /// renamed or deleted with the switch off.
    fn writable_group(&self, name: &str) -> Result<&'a AccountGroup, AccountError> {
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

    /// Refuses to delete, lock or demote the account the daemon itself runs as.
    fn not_self(&self, name: &str) -> Result<(), AccountError> {
        if name == self.daemon_user {
            return Err(protected(
                "the daemon's own account cannot be changed; it would take the panel offline",
            ));
        }
        Ok(())
    }

    /// Refuses a member list change that would empty an administrative group.
    ///
    /// This is the loopback guard: without it the panel can lock every operator
    /// out of the host it is meant to administer.
    fn keep_an_administrator(
        &self,
        group: &AccountGroup,
        next: &[String],
    ) -> Result<(), AccountError> {
        if !ADMIN_GROUPS.contains(&group.name.as_str()) {
            return Ok(());
        }
        if !next.is_empty() || group.members.is_empty() {
            return Ok(());
        }
        // Emptying one administrative group is only fatal if no other
        // administrative group still has a member.
        let others: Vec<&AccountGroup> = self
            .snapshot
            .groups
            .iter()
            .filter(|other| other.name != group.name && ADMIN_GROUPS.contains(&other.name.as_str()))
            .collect();
        if others.iter().any(|other| !other.members.is_empty()) {
            return Ok(());
        }
        Err(protected(format!(
            "refusing to empty `{}`: no other administrative group would keep a member",
            group.name
        )))
    }

    fn require_privileged_group_allowed(&self, group: &str) -> Result<(), AccountError> {
        if PRIVILEGED_GROUPS.contains(&group) && !self.config.allow_privileged_groups {
            return Err(protected(format!(
                "`{group}` grants root-equivalent access; set accounts.allow_privileged_groups to touch it"
            )));
        }
        Ok(())
    }
}

/// A user name reaches an argv vector and a `/etc/passwd` field, so it is
/// restricted to what every tool on every distro accepts. The leading
/// character rule is also what stops `-rf` from being read as a flag.
pub fn validate_name(name: &str) -> Result<(), AccountError> {
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
pub fn validate_gecos(value: &str) -> Result<(), AccountError> {
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

/// Resolves a requested home directory and proves it stays inside `home_root`.
///
/// The parent is what exists at request time — `useradd` creates the leaf — so
/// the parent is what gets canonicalised. That is the same shape as
/// `files.root`, and it is what stops `/home/../../etc` and a symlinked parent.
pub fn validate_home(home: &str, home_root: &Path) -> Result<PathBuf, AccountError> {
    if !home.starts_with('/') {
        return Err(invalid("the home directory must be an absolute path"));
    }
    let normalised = crate::files::normalize(home)
        .map_err(|error| invalid(format!("invalid home directory: {}", error.detail)))?;
    let path = PathBuf::from(&normalised);

    let root = std::fs::canonicalize(home_root).unwrap_or_else(|_| home_root.to_path_buf());
    // The leaf does not exist yet; resolve its parent and re-attach the name.
    let parent = path.parent().unwrap_or(Path::new("/"));
    let leaf = path
        .file_name()
        .ok_or_else(|| invalid("the home directory must name a folder, not `/`"))?;
    if leaf == ".." {
        return Err(invalid("the home directory must not contain `..`"));
    }
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
pub fn validate_shell(shell: &str, allowed: &[String]) -> Result<(), AccountError> {
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

/// Refuse an id of 0, or one already taken by a different name.
fn validate_new_id(id: u32, taken_by: Option<&str>, kind: &str) -> Result<(), AccountError> {
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
///
/// The range comes from the host's `login.defs`, so this agrees with how the
/// snapshot already classifies existing entries. Reusing an id that is already
/// occupied is a `Conflict` and is checked before this.
fn require_system_allowed(guard: &Guard<'_>, id: u32, kind: &str) -> Result<(), AccountError> {
    let minimum = match kind {
        "uid" => guard.id_ranges.uid_min,
        _ => guard.id_ranges.gid_min,
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

// ---------------------------------------------------------------------------
// Create
// ---------------------------------------------------------------------------

pub struct CreateArgs<'a> {
    pub name: &'a str,
    pub uid: Option<u32>,
    pub gid: Option<u32>,
    pub gecos: Option<&'a str>,
    pub home: Option<&'a str>,
    pub shell: Option<&'a str>,
    pub system: bool,
}

pub fn plan_create(guard: &Guard<'_>, args: &CreateArgs<'_>) -> Result<PlannedCall, AccountError> {
    validate_name(args.name)?;
    if guard
        .snapshot
        .users
        .iter()
        .any(|user| user.name == args.name)
    {
        return Err(conflict(format!("user `{}` already exists", args.name)));
    }
    if guard
        .snapshot
        .groups
        .iter()
        .any(|group| group.name == args.name)
    {
        // `useradd` would fail anyway; saying why is more useful.
        return Err(conflict(format!(
            "group `{}` already exists; `useradd` would refuse to create the user",
            args.name
        )));
    }
    if args.system && !guard.config.allow_system {
        return Err(protected(
            "creating a system account requires accounts.allow_system",
        ));
    }

    if let Some(uid) = args.uid {
        validate_new_id(uid, user_with_uid(guard.snapshot, uid), "uid")?;
        // `--system` is not the only way into the system range: an explicit id
        // does it just as well, and without this the allow_system switch had a
        // documented hole big enough to walk a service account through.
        require_system_allowed(guard, uid, "uid")?;
    }
    if let Some(gid) = args.gid {
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
    if let Some(gecos) = args.gecos {
        validate_gecos(gecos)?;
    }

    let home_root = Path::new(&guard.config.home_root);
    let home = match args.home {
        Some(home) => validate_home(home, home_root)?,
        None => validate_home(&format!("/home/{}", args.name), home_root)?,
    };

    let shells = allowed_shells(&guard.config.shells);
    let shell = args.shell.unwrap_or(&guard.config.default_shell);
    validate_shell(shell, &shells)?;

    let mut call = PlannedCall::new("useradd", vec!["--create-home".to_string()]);
    if args.system {
        call.args.push("--system".to_string());
    }
    if let Some(uid) = args.uid {
        call.args.extend(["--uid".to_string(), uid.to_string()]);
    }
    if let Some(gid) = args.gid {
        call.args.extend(["--gid".to_string(), gid.to_string()]);
    }
    if let Some(gecos) = args.gecos {
        call.args
            .extend(["--comment".to_string(), gecos.to_string()]);
    }
    call.args.extend([
        "--home-dir".to_string(),
        home.to_string_lossy().into_owned(),
        "--shell".to_string(),
        shell.to_string(),
    ]);
    // `useradd` creates the account locked unless a password is supplied, and
    // the protocol has no password field, so a new account can never log in
    // until `set_password` is called. That is the point.
    call.args.push(args.name.to_string());
    Ok(call)
}

// ---------------------------------------------------------------------------
// Modify
// ---------------------------------------------------------------------------

pub struct ModifyArgs<'a> {
    pub name: &'a str,
    pub gecos: Option<&'a str>,
    pub shell: Option<&'a str>,
    pub home: Option<&'a str>,
    pub primary_gid: Option<u32>,
    pub locked: Option<bool>,
}

pub fn plan_modify(guard: &Guard<'_>, args: &ModifyArgs<'_>) -> Result<PlannedCall, AccountError> {
    let user = guard.writable_user(args.name)?;
    guard.not_self(args.name)?;

    if args.gecos.is_none()
        && args.shell.is_none()
        && args.home.is_none()
        && args.primary_gid.is_none()
        && args.locked.is_none()
    {
        return Err(invalid("no change was requested"));
    }

    let mut call = PlannedCall::new("usermod", Vec::new());
    if let Some(gecos) = args.gecos {
        validate_gecos(gecos)?;
        call.args
            .extend(["--comment".to_string(), gecos.to_string()]);
    }
    if let Some(shell) = args.shell {
        validate_shell(shell, &allowed_shells(&guard.config.shells))?;
        call.args.extend(["--shell".to_string(), shell.to_string()]);
    }
    if let Some(home) = args.home {
        let home = validate_home(home, Path::new(&guard.config.home_root))?;
        // No `--move`: moving a populated home is a long, interruptible
        // operation that the panel should not attempt under a timeout.
        call.args
            .extend(["--home".to_string(), home.to_string_lossy().into_owned()]);
    }
    if let Some(gid) = args.primary_gid {
        // A primary gid *must* name an existing group: an occupied gid is the
        // normal case here, not the duplicate the group-rename path refuses.
        validate_new_id(gid, None, "gid")?;
        let group = guard
            .snapshot
            .groups
            .iter()
            .find(|group| group.gid == gid)
            .ok_or_else(|| not_found(format!("no such gid `{gid}`")))?;
        // Same order as `plan_create`: the root warning must win, then the
        // system-range switch. Without the second check this path could move a
        // user into `disk` or `shadow` with `allow_system = false`.
        guard.require_privileged_group_allowed(&group.name)?;
        require_system_allowed(guard, gid, "gid")?;
        call.args.extend(["--gid".to_string(), gid.to_string()]);
    }
    if let Some(locked) = args.locked {
        // Locking or unlocking an account that is currently the only
        // administrator is the same loopback hazard as deleting it.
        if locked {
            guard.not_last_administrator(args.name)?;
        }
        call.args.push(if locked {
            "--lock".to_string()
        } else {
            "--unlock".to_string()
        });
    }

    // A call with only flags and no target would modify the current user.
    call.args.push(user.name.clone());
    Ok(call)
}

impl Guard<'_> {
    /// Refuses a lock or delete that would leave `sudo`/`wheel`/`admin` with no
    /// other member.
    pub fn not_last_administrator(&self, name: &str) -> Result<(), AccountError> {
        let is_admin = self.snapshot.groups.iter().any(|group| {
            ADMIN_GROUPS.contains(&group.name.as_str())
                && group.members.iter().any(|member| member == name)
        });
        if !is_admin {
            return Ok(());
        }
        let remaining: usize = self
            .snapshot
            .groups
            .iter()
            .filter(|group| ADMIN_GROUPS.contains(&group.name.as_str()))
            .map(|group| {
                group
                    .members
                    .iter()
                    .filter(|member| member.as_str() != name)
                    .count()
            })
            .sum();
        // uid 0 is deliberately not an escape hatch: on many hosts root is
        // locked and has no password, which is exactly the state that would
        // make this guard fail open.
        if remaining == 0 {
            return Err(protected(format!(
                "refusing to disable `{name}`: it is the last member of an administrative group"
            )));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Delete
// ---------------------------------------------------------------------------

pub fn plan_delete(
    guard: &Guard<'_>,
    name: &str,
    remove_home: bool,
) -> Result<PlannedCall, AccountError> {
    let user = guard.writable_user(name)?;
    guard.not_self(name)?;
    guard.not_last_administrator(name)?;

    let mut call = PlannedCall::new("userdel", Vec::new());
    if remove_home {
        let home = validate_home(&user.home, Path::new(&guard.config.home_root))?;
        // Deleting a home is only ever explicit, and only inside home_root; the
        // guard above has already proven that. `--force` is never passed: it
        // would remove the user even while logged in.
        let _ = home;
        call.args.push("--remove".to_string());
    }
    call.args.push(name.to_string());
    Ok(call)
}

// ---------------------------------------------------------------------------
// Password
// ---------------------------------------------------------------------------

pub fn plan_set_password(
    guard: &Guard<'_>,
    password_control: bool,
    name: &str,
    password: &str,
) -> Result<PlannedCall, AccountError> {
    if !password_control {
        return Err(disabled(
            "password changes are disabled; set accounts.manage_passwords to enable them",
        ));
    }
    guard.writable_user(name)?;

    if password.chars().count() < 8 {
        return Err(invalid("a password must be at least 8 characters"));
    }
    if password.chars().count() > 256 {
        return Err(invalid("a password must be at most 256 characters"));
    }
    // The line is `<name>:<password>\n`. A newline or colon in the password
    // would inject a second record or a second field, so they are refused
    // rather than escaped: `chpasswd` has no escaping to rely on.
    if password.contains('\n') || password.contains('\r') || password.contains(':') {
        return Err(invalid("a password must not contain `:` or a newline"));
    }

    // `chpasswd` takes the pair on stdin. The plaintext never reaches argv.
    Ok(PlannedCall::new("chpasswd", Vec::new()).with_stdin(format!("{name}:{password}\n")))
}

// ---------------------------------------------------------------------------
// Groups
// ---------------------------------------------------------------------------

pub fn plan_group_create(
    guard: &Guard<'_>,
    name: &str,
    gid: Option<u32>,
    system: bool,
) -> Result<PlannedCall, AccountError> {
    validate_name(name)?;
    if guard.snapshot.groups.iter().any(|group| group.name == name) {
        return Err(conflict(format!("group `{name}` already exists")));
    }
    if system && !guard.config.allow_system {
        return Err(protected(
            "creating a system group requires accounts.allow_system",
        ));
    }
    if let Some(gid) = gid {
        validate_new_id(gid, group_with_gid(guard.snapshot, gid), "gid")?;
        require_system_allowed(guard, gid, "gid")?;
    }

    let mut call = PlannedCall::new("groupadd", Vec::new());
    if system {
        call.args.push("--system".to_string());
    }
    if let Some(gid) = gid {
        call.args.extend(["--gid".to_string(), gid.to_string()]);
    }
    call.args.push(name.to_string());
    Ok(call)
}

/// Plans every call a group change needs. `groupmod` and `gpasswd` are separate
/// tools, and a request that sets both fields must run both, so the plan is a
/// list rather than pretending one tool covers the request.
pub fn plan_group_modify(
    guard: &Guard<'_>,
    name: &str,
    gid: Option<u32>,
    members: Option<&[String]>,
) -> Result<Vec<PlannedCall>, AccountError> {
    // Privileged first: `sudo` is also below GID_MIN, and the root warning is
    // the one that explains why the action is dangerous. Same order as
    // `plan_create` and the component's `groupBlockReason`.
    guard.require_privileged_group_allowed(name)?;
    let group = guard.writable_group(name)?;

    if gid.is_none() && members.is_none() {
        return Err(invalid("no change was requested"));
    }

    let mut calls = Vec::new();
    if let Some(gid) = gid {
        // `groupmod --gid` accepts a gid another group already owns, which
        // leaves two groups on one gid: ambiguous ownership, and the state the
        // create path already refuses. Re-sending the group's own gid is fine.
        let taken_by = group_with_gid(guard.snapshot, gid).filter(|owner| *owner != name);
        validate_new_id(gid, taken_by, "gid")?;
        calls.push(PlannedCall::new(
            "groupmod",
            vec!["--gid".to_string(), gid.to_string(), name.to_string()],
        ));
    }
    if let Some(members) = members {
        let mut seen: HashSet<&str> = HashSet::new();
        for member in members {
            validate_name(member)?;
            if !seen.insert(member.as_str()) {
                return Err(invalid(format!("`{member}` is listed twice")));
            }
            let user = guard
                .snapshot
                .users
                .iter()
                .find(|user| &user.name == member)
                .ok_or_else(|| not_found(format!("no such user `{member}`")))?;
            if !user.source.is_writable() {
                return Err(unsupported(format!(
                    "`{member}` comes from a directory service and cannot join a local group"
                )));
            }
        }
        guard.keep_an_administrator(group, members)?;
        calls.push(PlannedCall::new(
            "gpasswd",
            vec!["--members".to_string(), members.join(","), name.to_string()],
        ));
    }
    Ok(calls)
}

pub fn plan_group_delete(guard: &Guard<'_>, name: &str) -> Result<PlannedCall, AccountError> {
    validate_name(name)?;
    // `writable_group` refuses gid 0 with the modify wording; a delete deserves
    // its own, so the check runs first to word it correctly.
    if guard.group(name)?.gid == 0 {
        return Err(protected("gid 0 is never deleted through the panel"));
    }
    guard.require_privileged_group_allowed(name)?;
    let group = guard.writable_group(name)?;

    // A group that is somebody's primary group cannot be removed: those users
    // would be left with a gid that resolves to nothing.
    if let Some(owner) = guard
        .snapshot
        .users
        .iter()
        .find(|user| user.gid == group.gid)
    {
        return Err(conflict(format!(
            "`{name}` is the primary group of `{}`",
            owner.name
        )));
    }
    guard.keep_an_administrator(group, &[])?;

    Ok(PlannedCall::new("groupdel", vec![name.to_string()]))
}

// ---------------------------------------------------------------------------
// Execution
// ---------------------------------------------------------------------------

/// Runs one planned call and maps a tool failure onto the protocol error.
pub fn run_planned(runner: &dyn CommandRunner, call: &PlannedCall) -> Result<String, AccountError> {
    let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
    let outcome = match &call.stdin {
        Some(input) => runner.run_with_stdin(&call.program, &args, input.expose()),
        None => runner.run(&call.program, &args),
    };

    match outcome {
        Ok(CommandOutput { ok: true, .. }) => Ok(call.program.clone()),
        Ok(output) => Err(classified_failure(&call.program, &output.stderr)),
        Err(CommandError::Unavailable { program }) => Err(unsupported(format!(
            "{program} is not installed on this host; the shadow-utils tools are required"
        ))),
        Err(CommandError::Failed { detail, .. }) => {
            Err(failed(format!("could not run {}: {detail}", call.program)))
        }
    }
}

/// The tools report failures as plain text. `LC_ALL=C` is set by the runner, so
/// these strings are stable on every distro.
fn classified_failure(program: &str, stderr: &str) -> AccountError {
    let lowercase = stderr.to_lowercase();
    let kind = if lowercase.contains("already exists") || lowercase.contains("not unique") {
        AccountErrorKind::Conflict
    } else if lowercase.contains("does not exist") || lowercase.contains("no such user") {
        AccountErrorKind::NotFound
    } else if lowercase.contains("permission denied")
        || lowercase.contains("operation not permitted")
        || lowercase.contains("must be root")
        || lowercase.contains("cannot open")
        || lowercase.contains("cannot lock")
    {
        AccountErrorKind::PermissionDenied
    } else if lowercase.contains("invalid") || lowercase.contains("not a valid") {
        AccountErrorKind::Invalid
    } else {
        AccountErrorKind::Failed
    };

    let detail = stderr.trim();
    let detail = if detail.is_empty() {
        format!("{program} failed")
    } else {
        detail.replace('\n', "; ")
    };
    AccountError::new(kind, format!("{program}: {detail}"))
}

/// The unit tests and the daemon both build one of these. `SystemRunner` is the
/// production path; a `false` argument means "try directly, without sudo".
pub fn system_runner(sudo: bool) -> SystemRunner {
    SystemRunner::new(sudo)
}

/// The name of the account the daemon runs as, for the self-protection guard.
///
/// The real uid wins. `$USER` and `$LOGNAME` are inherited environment, so a
/// service unit or `docker exec` can hand the daemon a stale name; trusting it
/// first would protect an account that is not the one running the panel and
/// leave the daemon's own account deletable.
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
