//! Account mutations: validate a request into an argv vector, then run it.
//!
//! No shell is involved. `useradd`, `userdel` and `usermod` receive direct argv
//! vectors; the tests assert the exact vector and never touch `/etc`.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use anth_contract::Error;
use serde_json::Value;

use crate::inventory::{Inventory, User};
use crate::policy::Policy;

/// Groups that grant root-equivalent power. `docker` and `lxd` are on the list
/// because a member can mount the host filesystem inside a container.
pub const PRIVILEGED_GROUPS: &[&str] = &["sudo", "wheel", "admin", "root", "docker", "lxd"];

/// Groups whose last member must never be removed or locked away.
pub const ADMIN_GROUPS: &[&str] = &["sudo", "wheel", "admin"];

const MAX_NAME_LEN: usize = 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlannedCall {
    pub program: &'static str,
    pub args: Vec<String>,
    pub stdin: Option<String>,
}

impl PlannedCall {
    fn new(program: &'static str, args: Vec<String>) -> Self {
        Self {
            program,
            args,
            stdin: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
/// distros and stdout is discarded.
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

/// The snapshot, the approved policy and the process account, together for
/// every mutation. Guards are pure functions over this view.
pub struct Guard<'a> {
    pub inventory: &'a Inventory,
    pub policy: &'a Policy,
    pub self_name: &'a str,
}

impl<'a> Guard<'a> {
    fn user(&self, name: &str) -> Result<&'a User, Error> {
        self.inventory
            .users
            .iter()
            .find(|user| user.name == name)
            .ok_or_else(|| Error::not_found(format!("`{name}` does not exist")))
    }

    fn group_by_gid(&self, gid: u32) -> Result<&'a crate::inventory::Group, Error> {
        self.inventory
            .groups
            .iter()
            .find(|group| group.gid == gid)
            .ok_or_else(|| Error::not_found(format!("gid {gid} does not exist")))
    }

    fn writable_user(&self, name: &str) -> Result<&'a User, Error> {
        validate_name(name)?;
        let user = self.user(name)?;
        if user.uid == 0 {
            return Err(Error::denied("uid 0 is never modified through the panel"));
        }
        if user.system && !self.policy.allow_system {
            return Err(Error::denied(format!(
                "`{name}` is a system account (uid {}); start the service with `--allow-system` to change it",
                user.uid
            )));
        }
        Ok(user)
    }

    fn not_self(&self, name: &str) -> Result<(), Error> {
        if name == self.self_name {
            return Err(Error::denied(
                "the account running the service cannot be changed; it would take the panel offline",
            ));
        }
        Ok(())
    }

    /// Administrators by either provisioning style: a supplementary member of
    /// an administrative group, or a user whose primary gid is that group.
    fn administrator_names(&self) -> HashSet<&str> {
        let mut names = HashSet::new();
        for group in self
            .inventory
            .groups
            .iter()
            .filter(|group| ADMIN_GROUPS.contains(&group.name.as_str()))
        {
            names.extend(group.members.iter().map(String::as_str));
            names.extend(
                self.inventory
                    .users
                    .iter()
                    .filter(|user| user.gid == group.gid)
                    .map(|user| user.name.as_str()),
            );
        }
        names
    }

    /// Refuses a delete or lock that would leave `sudo`/`wheel`/`admin` with no
    /// other member. uid 0 is deliberately not an escape hatch.
    fn not_last_administrator(&self, name: &str) -> Result<(), Error> {
        let administrators = self.administrator_names();
        if !administrators.contains(name) {
            return Ok(());
        }
        let remaining = administrators
            .iter()
            .filter(|administrator| **administrator != name)
            .count();
        if remaining == 0 {
            return Err(Error::denied(format!(
                "refusing to change `{name}`: it is the last member of an administrative group"
            )));
        }
        Ok(())
    }

    fn require_privileged_group_allowed(&self, group: &str) -> Result<(), Error> {
        if PRIVILEGED_GROUPS.contains(&group) && !self.policy.allow_privileged_groups {
            return Err(Error::denied(format!(
                "`{group}` grants root-equivalent access; start the service with `--allow-privileged-groups` to use it"
            )));
        }
        Ok(())
    }

    fn require_system_allowed(&self, id: u32, kind: &str) -> Result<(), Error> {
        let minimum = if kind == "uid" {
            self.inventory.id_ranges.uid_min
        } else {
            self.inventory.id_ranges.gid_min
        };
        if id >= minimum || self.policy.allow_system {
            return Ok(());
        }
        Err(Error::denied(format!(
            "{kind} {id} is below {minimum}; start the service with `--allow-system` to use a system id"
        )))
    }
}

pub fn plan_create_user(guard: &Guard<'_>, input: &Value) -> Result<PlannedCall, Error> {
    require_control(guard.policy)?;
    let name = text(input, "name")?;
    validate_name(&name)?;
    if guard.inventory.users.iter().any(|user| user.name == name) {
        return Err(Error::conflict(format!("user `{name}` already exists")));
    }
    if guard
        .inventory
        .groups
        .iter()
        .any(|group| group.name == name)
    {
        return Err(Error::conflict(format!(
            "group `{name}` already exists; `useradd` would refuse the user"
        )));
    }
    let uid = number(input, "uid")?;
    let gid = number(input, "gid")?;
    let home = text(input, "home")?;
    let shell = text(input, "shell")?;
    let system = boolean(input, "system");
    if system && !guard.policy.allow_system {
        return Err(Error::denied(
            "creating a system account requires `--allow-system`",
        ));
    }
    if let Some(existing) = guard.inventory.users.iter().find(|user| user.uid == uid) {
        return Err(Error::conflict(format!(
            "uid {uid} is already used by `{}`",
            existing.name
        )));
    }
    let group = guard.group_by_gid(gid)?;
    guard.require_privileged_group_allowed(&group.name)?;
    guard.require_system_allowed(uid, "uid")?;
    guard.require_system_allowed(gid, "gid")?;
    let home = validate_home(&home, &guard.policy.home_root)?;
    let shells = guard.policy.allowed_shells();
    validate_shell(&shell, &shells)?;

    let mut args = vec!["--create-home".to_string()];
    if system {
        args.push("--system".to_string());
    }
    args.extend(["--uid".to_string(), uid.to_string()]);
    args.extend(["--gid".to_string(), gid.to_string()]);
    args.extend([
        "--home-dir".to_string(),
        home.to_string_lossy().into_owned(),
        "--shell".to_string(),
        shell,
    ]);
    args.push(name);
    Ok(PlannedCall::new("useradd", args))
}

pub fn plan_delete_user(guard: &Guard<'_>, input: &Value) -> Result<PlannedCall, Error> {
    require_control(guard.policy)?;
    let name = text(input, "name")?;
    guard.writable_user(&name)?;
    guard.not_self(&name)?;
    guard.not_last_administrator(&name)?;
    Ok(PlannedCall::new("userdel", vec![name]))
}

pub fn plan_set_lock(guard: &Guard<'_>, input: &Value) -> Result<PlannedCall, Error> {
    require_control(guard.policy)?;
    let name = text(input, "name")?;
    let locked = boolean(input, "locked");
    guard.writable_user(&name)?;
    guard.not_self(&name)?;
    if locked {
        guard.not_last_administrator(&name)?;
    }
    Ok(PlannedCall::new(
        "usermod",
        vec![if locked { "--lock" } else { "--unlock" }.to_string(), name],
    ))
}

pub fn run_planned(runner: &dyn AccountRunner, call: &PlannedCall) -> Result<(), Error> {
    match runner.run(call.program, &call.args, call.stdin.as_deref()) {
        Ok(AccountOutput { ok: true, .. }) => Ok(()),
        Ok(output) => Err(classified_failure(call.program, &output.stderr)),
        Err(detail) => Err(Error::unavailable(format!(
            "`{}` is not installed or could not be executed: {detail}",
            call.program
        ))),
    }
}

fn require_control(policy: &Policy) -> Result<(), Error> {
    if policy.control {
        Ok(())
    } else {
        Err(Error::denied(
            "account changes are disabled; start the service with `--control`",
        ))
    }
}

fn classified_failure(program: &str, stderr: &str) -> Error {
    let lowercase = stderr.to_lowercase();
    let message = if stderr.is_empty() {
        format!("`{program}` failed without a message")
    } else {
        format!("`{program}` failed: {stderr}")
    };
    if lowercase.contains("already exists")
        || lowercase.contains("not unique")
        || lowercase.contains("is currently used")
    {
        return Error::conflict(message);
    }
    if lowercase.contains("does not exist")
        || lowercase.contains("no such user")
        || lowercase.contains("no such group")
    {
        return Error::not_found(message);
    }
    if lowercase.contains("permission denied")
        || lowercase.contains("operation not permitted")
        || lowercase.contains("cannot lock")
        || lowercase.contains("cannot open")
    {
        return Error::denied(format!(
            "the account change needs root or equivalent write access: {message}"
        ));
    }
    Error::unavailable(message)
}

fn text(input: &Value, key: &str) -> Result<String, Error> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| Error::invalid(format!("`{key}` is required")))
}

fn number(input: &Value, key: &str) -> Result<u32, Error> {
    input
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| Error::invalid(format!("`{key}` must be a positive u32")))
}

fn boolean(input: &Value, key: &str) -> bool {
    input.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// A name reaches argv and `/etc/passwd`: keep it to what every tool accepts.
/// The leading-character rule also stops `-rf` from being read as a flag.
pub fn validate_name(name: &str) -> Result<(), Error> {
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(Error::invalid(format!(
            "a name must be 1 to {MAX_NAME_LEN} characters"
        )));
    }
    let mut characters = name.chars();
    let first = characters.next().expect("checked non-empty");
    if !(first.is_ascii_lowercase() || first == '_') {
        return Err(Error::invalid(
            "a name must start with a lowercase letter or an underscore",
        ));
    }
    if !characters.all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || character == '_'
            || character == '-'
    }) {
        return Err(Error::invalid(
            "a name may contain lowercase letters, digits, underscores and hyphens",
        ));
    }
    Ok(())
}

fn normalize(path: &str) -> Result<PathBuf, Error> {
    if !path.starts_with('/') {
        return Err(Error::invalid("the home directory must be absolute"));
    }
    let mut normalized = PathBuf::from("/");
    for component in Path::new(path).components() {
        match component {
            Component::RootDir => {}
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                return Err(Error::invalid("the home directory must not contain `..`"));
            }
            Component::Prefix(_) => {
                return Err(Error::invalid(
                    "the home directory contains an unsupported component",
                ));
            }
        }
    }
    Ok(normalized)
}

/// Resolves a requested home and proves it stays inside `home_root`. The parent
/// is what exists at request time; `useradd` creates the leaf.
pub fn validate_home(home: &str, home_root: &Path) -> Result<PathBuf, Error> {
    let path = normalize(home)?;
    let root = std::fs::canonicalize(home_root).unwrap_or_else(|_| home_root.to_path_buf());
    let parent = path.parent().unwrap_or(Path::new("/"));
    let leaf = path
        .file_name()
        .ok_or_else(|| Error::invalid("the home directory must name a folder, not `/`"))?;
    let parent = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
    let resolved = parent.join(leaf);
    if !resolved.starts_with(&root) {
        return Err(Error::denied(format!(
            "the home directory must be inside `{}`",
            root.display()
        )));
    }
    if resolved == root {
        return Err(Error::denied(
            "the home directory cannot be the root folder itself",
        ));
    }
    Ok(resolved)
}

pub fn validate_shell(shell: &str, allowed: &[String]) -> Result<(), Error> {
    if !shell.starts_with('/') {
        return Err(Error::invalid("the shell must be an absolute path"));
    }
    if allowed.iter().any(|entry| entry == shell) {
        return Ok(());
    }
    Err(Error::invalid(format!(
        "`{shell}` is not an allowed login shell"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{Group, IdRanges};

    fn inventory() -> Inventory {
        Inventory {
            id_ranges: IdRanges::default(),
            users: vec![
                User {
                    name: "root".to_string(),
                    uid: 0,
                    gid: 0,
                    home: "/root".to_string(),
                    shell: "/bin/bash".to_string(),
                    system: false,
                    locked: Some(true),
                },
                User {
                    name: "daemon".to_string(),
                    uid: 1,
                    gid: 1,
                    home: "/usr/sbin".to_string(),
                    shell: "/usr/sbin/nologin".to_string(),
                    system: true,
                    locked: Some(true),
                },
                User {
                    name: "operator".to_string(),
                    uid: 1000,
                    gid: 1000,
                    home: "/home/operator".to_string(),
                    shell: "/bin/bash".to_string(),
                    system: false,
                    locked: Some(false),
                },
                User {
                    name: "alice".to_string(),
                    uid: 1001,
                    gid: 1001,
                    home: "/home/alice".to_string(),
                    shell: "/bin/bash".to_string(),
                    system: false,
                    locked: Some(false),
                },
            ],
            groups: vec![
                Group {
                    name: "root".to_string(),
                    gid: 0,
                    members: Vec::new(),
                    system: true,
                },
                Group {
                    name: "daemon".to_string(),
                    gid: 1,
                    members: Vec::new(),
                    system: true,
                },
                Group {
                    name: "operator".to_string(),
                    gid: 1000,
                    members: Vec::new(),
                    system: false,
                },
                Group {
                    name: "alice".to_string(),
                    gid: 1001,
                    members: Vec::new(),
                    system: false,
                },
                Group {
                    name: "sudo".to_string(),
                    gid: 27,
                    members: vec!["operator".to_string()],
                    system: true,
                },
            ],
        }
    }

    fn policy() -> Policy {
        Policy {
            control: true,
            allow_system: false,
            allow_privileged_groups: false,
            home_root: PathBuf::from("/home"),
            shells: vec!["/bin/sh".to_string(), "/bin/bash".to_string()],
            default_shell: "/bin/sh".to_string(),
        }
    }

    fn guard_for<'a>(inventory: &'a Inventory, policy: &'a Policy) -> Guard<'a> {
        Guard {
            inventory,
            policy,
            self_name: "operator",
        }
    }

    fn create(input: Value) -> Result<PlannedCall, Error> {
        let inventory = inventory();
        let policy = policy();
        plan_create_user(&guard_for(&inventory, &policy), &input)
    }

    #[test]
    fn create_user_builds_an_argv_without_a_shell() {
        let plan = create(serde_json::json!({
            "name": "review",
            "uid": 1500,
            "gid": 1001,
            "home": "/home/review",
            "shell": "/bin/bash",
            "system": false
        }))
        .expect("plan");
        assert_eq!(plan.program, "useradd");
        assert_eq!(
            plan.args,
            vec![
                "--create-home",
                "--uid",
                "1500",
                "--gid",
                "1001",
                "--home-dir",
                "/home/review",
                "--shell",
                "/bin/bash",
                "review"
            ]
        );
        assert!(plan.stdin.is_none());
    }

    #[test]
    fn read_only_policy_blocks_every_mutation() {
        let inventory = inventory();
        let mut policy = policy();
        policy.control = false;
        let guard = guard_for(&inventory, &policy);
        assert_eq!(
            plan_delete_user(&guard, &serde_json::json!({"name": "alice"}))
                .expect_err("disabled")
                .kind(),
            anth_contract::ErrorKind::Denied
        );
        assert_eq!(
            plan_set_lock(
                &guard,
                &serde_json::json!({"name": "alice", "locked": true})
            )
            .expect_err("disabled")
            .kind(),
            anth_contract::ErrorKind::Denied
        );
    }

    #[test]
    fn root_self_and_the_last_administrator_are_protected() {
        let inventory = inventory();
        let policy = policy();
        let guard = guard_for(&inventory, &policy);
        assert_eq!(
            plan_delete_user(&guard, &serde_json::json!({"name": "root"}))
                .expect_err("root")
                .kind(),
            anth_contract::ErrorKind::Denied
        );
        assert_eq!(
            plan_delete_user(&guard, &serde_json::json!({"name": "operator"}))
                .expect_err("self")
                .kind(),
            anth_contract::ErrorKind::Denied
        );
        // With operator gone and sudo's only member being alice, deleting
        // alice would leave no administrator.
        let mut inventory = inventory;
        inventory.users.retain(|user| user.name != "operator");
        for group in &mut inventory.groups {
            if group.name == "sudo" {
                group.members = vec!["alice".to_string()];
            }
        }
        let guard = Guard {
            inventory: &inventory,
            policy: &policy,
            self_name: "root",
        };
        let error = plan_delete_user(&guard, &serde_json::json!({"name": "alice"}))
            .expect_err("last administrator");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
    }

    #[test]
    fn system_accounts_and_ids_need_the_opt_in() {
        let inventory = inventory();
        let mut policy = policy();
        let guard = guard_for(&inventory, &policy);
        let error = plan_delete_user(&guard, &serde_json::json!({"name": "daemon"}))
            .expect_err("system account");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        let error = create(serde_json::json!({
            "name": "sys",
            "uid": 500,
            "gid": 1000,
            "home": "/home/sys",
            "shell": "/bin/sh",
            "system": true
        }))
        .expect_err("system id");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);

        policy.allow_system = true;
        assert!(plan_create_user(
            &guard_for(&inventory, &policy),
            &serde_json::json!({
                "name": "sys",
                "uid": 500,
                "gid": 1000,
                "home": "/home/sys",
                "shell": "/bin/sh",
                "system": true
            })
        )
        .is_ok());
    }

    #[test]
    fn privileged_groups_need_the_opt_in() {
        let inventory = inventory();
        let policy = policy();
        let error = create(serde_json::json!({
            "name": "admin2",
            "uid": 1500,
            "gid": 27,
            "home": "/home/admin2",
            "shell": "/bin/sh",
            "system": false
        }))
        .expect_err("privileged group");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);

        let mut allowed = policy;
        allowed.allow_privileged_groups = true;
        allowed.allow_system = true;
        assert!(plan_create_user(
            &guard_for(&inventory, &allowed),
            &serde_json::json!({
                "name": "admin2",
                "uid": 1500,
                "gid": 27,
                "home": "/home/admin2",
                "shell": "/bin/sh",
                "system": false
            })
        )
        .is_ok());
    }

    #[test]
    fn home_and_shell_are_validated() {
        let error = create(serde_json::json!({
            "name": "outside",
            "uid": 1500,
            "gid": 1000,
            "home": "/etc/outside",
            "shell": "/bin/sh",
            "system": false
        }))
        .expect_err("home root");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);

        let error = create(serde_json::json!({
            "name": "shell",
            "uid": 1500,
            "gid": 1000,
            "home": "/home/shell",
            "shell": "/usr/bin/curl",
            "system": false
        }))
        .expect_err("shell");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);

        let error = create(serde_json::json!({
            "name": "dotdot",
            "uid": 1500,
            "gid": 1000,
            "home": "/home/../etc",
            "shell": "/bin/sh",
            "system": false
        }))
        .expect_err("traversal");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);
    }

    #[test]
    fn names_reject_flags_and_injection() {
        for name in ["-rf", "Alice", "a b", "a/b", ""] {
            assert!(validate_name(name).is_err(), "{name}");
        }
        assert!(validate_name("alice_2").is_ok());
    }

    #[test]
    fn lock_uses_usermod_and_protects_the_last_admin_when_locking() {
        let inventory = inventory();
        let policy = policy();
        let guard = guard_for(&inventory, &policy);
        let plan = plan_set_lock(
            &guard,
            &serde_json::json!({"name": "alice", "locked": true}),
        )
        .expect("lock");
        assert_eq!(plan.program, "usermod");
        assert_eq!(plan.args, vec!["--lock", "alice"]);
        let plan = plan_set_lock(
            &guard,
            &serde_json::json!({"name": "alice", "locked": false}),
        )
        .expect("unlock");
        assert_eq!(plan.args, vec!["--unlock", "alice"]);

        let error = plan_set_lock(
            &guard,
            &serde_json::json!({"name": "operator", "locked": true}),
        )
        .expect_err("self");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
    }

    #[test]
    fn failures_are_classified() {
        let conflict = classified_failure("useradd", "useradd: user 'alice' already exists");
        assert_eq!(conflict.kind(), anth_contract::ErrorKind::Conflict);
        let missing = classified_failure("userdel", "userdel: user 'x' does not exist");
        assert_eq!(missing.kind(), anth_contract::ErrorKind::NotFound);
        let denied = classified_failure("userdel", "userdel: Permission denied");
        assert_eq!(denied.kind(), anth_contract::ErrorKind::Denied);
        let other = classified_failure("useradd", "something else");
        assert_eq!(other.kind(), anth_contract::ErrorKind::Unavailable);
    }
}
