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

    fn with_stdin(mut self, input: String) -> Self {
        self.stdin = Some(input);
        self
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
        if user.source != crate::inventory::Source::Local {
            return Err(Error::denied(format!(
                "`{name}` comes from a directory service and cannot be changed"
            )));
        }
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

    fn writable_group(&self, name: &str) -> Result<&'a crate::inventory::Group, Error> {
        validate_name(name)?;
        let group = self
            .inventory
            .groups
            .iter()
            .find(|group| group.name == name)
            .ok_or_else(|| Error::not_found(format!("`{name}` does not exist")))?;
        if group.gid == 0 {
            return Err(Error::denied("gid 0 is never modified through the panel"));
        }
        if group.source != crate::inventory::Source::Local {
            return Err(Error::denied(format!(
                "`{name}` comes from a directory service and cannot be changed"
            )));
        }
        if group.system && !self.policy.allow_system {
            return Err(Error::denied(format!(
                "`{name}` is a system group (gid {}); start the service with `--allow-system` to change it",
                group.gid
            )));
        }
        Ok(group)
    }

    /// Refuses a group delete that would leave no administrative group able to
    /// grant access. Mirrors the per-group user guard.
    fn keep_an_administrator(&self, group: &crate::inventory::Group) -> Result<(), Error> {
        if !ADMIN_GROUPS.contains(&group.name.as_str()) {
            return Ok(());
        }
        if self
            .inventory
            .users
            .iter()
            .any(|user| user.gid == group.gid)
        {
            return Ok(());
        }
        if group.members.is_empty() {
            return Ok(());
        }
        let another = self.inventory.groups.iter().any(|other| {
            other.name != group.name
                && ADMIN_GROUPS.contains(&other.name.as_str())
                && (!other.members.is_empty()
                    || self
                        .inventory
                        .users
                        .iter()
                        .any(|user| user.gid == other.gid))
        });
        if another {
            return Ok(());
        }
        Err(Error::denied(format!(
            "refusing to delete `{}`: no other administrative group would keep a member",
            group.name
        )))
    }

    fn not_self(&self, name: &str) -> Result<(), Error> {
        if name == self.self_name {
            return Err(Error::denied(
                "the account running the service cannot be changed; it would take the panel offline",
            ));
        }
        Ok(())
    }

    /// Refuses a delete or lock that would leave any administrative group
    /// with no other member. The check is per group: emptying `sudo` is not
    /// excused by `wheel` having another member.
    fn not_last_administrator(&self, name: &str) -> Result<(), Error> {
        for group in self
            .inventory
            .groups
            .iter()
            .filter(|group| ADMIN_GROUPS.contains(&group.name.as_str()))
        {
            let is_member = group.members.iter().any(|member| member == name)
                || self
                    .inventory
                    .users
                    .iter()
                    .any(|user| user.name == name && user.gid == group.gid);
            if !is_member {
                continue;
            }
            let mut remaining: HashSet<&str> = group
                .members
                .iter()
                .filter(|member| member.as_str() != name)
                .map(String::as_str)
                .collect();
            remaining.extend(
                self.inventory
                    .users
                    .iter()
                    .filter(|user| user.name != name && user.gid == group.gid)
                    .map(|user| user.name.as_str()),
            );
            if remaining.is_empty() {
                return Err(Error::denied(format!(
                    "refusing to change `{name}`: it is the last member of `{}`",
                    group.name
                )));
            }
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
    let uid = optional_number(input, "uid")?;
    let gid = optional_number(input, "gid")?;
    let gecos = optional_text(input, "gecos");
    validate_gecos(&gecos)?;
    let home = text(input, "home")?;
    let shell = text(input, "shell")?;
    let system = boolean(input, "system");
    if system && !guard.policy.allow_system {
        return Err(Error::denied(
            "creating a system account requires `--allow-system`",
        ));
    }
    if let Some(uid) = uid {
        if let Some(existing) = guard.inventory.users.iter().find(|user| user.uid == uid) {
            return Err(Error::conflict(format!(
                "uid {uid} is already used by `{}`",
                existing.name
            )));
        }
        guard.require_system_allowed(uid, "uid")?;
    }
    if let Some(gid) = gid {
        let group = guard.group_by_gid(gid)?;
        guard.require_privileged_group_allowed(&group.name)?;
        guard.require_system_allowed(gid, "gid")?;
    }
    let home = validate_home(&home, &guard.policy.home_root)?;
    let shells = guard.policy.allowed_shells();
    validate_shell(&shell, &shells)?;

    let mut args = vec!["--create-home".to_string()];
    if system {
        args.push("--system".to_string());
    }
    if let Some(uid) = uid {
        args.extend(["--uid".to_string(), uid.to_string()]);
    }
    if let Some(gid) = gid {
        args.extend(["--gid".to_string(), gid.to_string()]);
    }
    if !gecos.is_empty() {
        args.extend(["--comment".to_string(), gecos]);
    }
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
    let user = guard.writable_user(&name)?;
    guard.not_self(&name)?;
    guard.not_last_administrator(&name)?;
    let mut args = Vec::new();
    if boolean(input, "remove-home") {
        validate_home(&user.home, &guard.policy.home_root)?;
        args.push("--remove".to_string());
    }
    args.push(name);
    Ok(PlannedCall::new("userdel", args))
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

pub fn plan_set_password(guard: &Guard<'_>, input: &Value) -> Result<PlannedCall, Error> {
    if !guard.policy.passwords {
        return Err(Error::denied(
            "password changes are disabled; start the service with `--passwords`",
        ));
    }
    let name = text(input, "name")?;
    let password = text(input, "password")?;
    guard.writable_user(&name)?;
    let length = password.chars().count();
    if length < 8 {
        return Err(Error::invalid("a password must be at least 8 characters"));
    }
    if length > 256 {
        return Err(Error::invalid("a password must be at most 256 characters"));
    }
    if password.contains(':') || password.contains('\n') || password.contains('\r') {
        return Err(Error::invalid(
            "a password must not contain `:` or a newline",
        ));
    }
    // The pair travels on stdin; it never reaches argv or a log line.
    Ok(PlannedCall::new("chpasswd", Vec::new()).with_stdin(format!("{name}:{password}\n")))
}

pub fn plan_create_group(guard: &Guard<'_>, input: &Value) -> Result<PlannedCall, Error> {
    require_control(guard.policy)?;
    let name = text(input, "name")?;
    validate_name(&name)?;
    if guard
        .inventory
        .groups
        .iter()
        .any(|group| group.name == name)
    {
        return Err(Error::conflict(format!("group `{name}` already exists")));
    }
    guard.require_privileged_group_allowed(&name)?;
    let gid = optional_number(input, "gid")?;
    let system = boolean(input, "system");
    if system && !guard.policy.allow_system {
        return Err(Error::denied(
            "creating a system group requires `--allow-system`",
        ));
    }
    if let Some(gid) = gid {
        if let Some(existing) = guard.inventory.groups.iter().find(|group| group.gid == gid) {
            return Err(Error::conflict(format!(
                "gid {gid} is already used by `{}`",
                existing.name
            )));
        }
        guard.require_system_allowed(gid, "gid")?;
    }
    let mut args = Vec::new();
    if system {
        args.push("--system".to_string());
    }
    if let Some(gid) = gid {
        args.extend(["--gid".to_string(), gid.to_string()]);
    }
    args.push(name);
    Ok(PlannedCall::new("groupadd", args))
}

pub fn plan_delete_group(guard: &Guard<'_>, input: &Value) -> Result<PlannedCall, Error> {
    require_control(guard.policy)?;
    let name = text(input, "name")?;
    let group = guard.writable_group(&name)?;
    if let Some(owner) = guard
        .inventory
        .users
        .iter()
        .find(|user| user.gid == group.gid)
    {
        return Err(Error::conflict(format!(
            "`{}` is the primary group of `{}`",
            name, owner.name
        )));
    }
    guard.keep_an_administrator(group)?;
    Ok(PlannedCall::new("groupdel", vec![name]))
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

fn optional_number(input: &Value, key: &str) -> Result<Option<u32>, Error> {
    match input.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| *value > 0)
            .map(Some)
            .ok_or_else(|| Error::invalid(format!("`{key}` must be a positive u32"))),
        _ => Err(Error::invalid(format!("`{key}` must be a positive u32"))),
    }
}

fn optional_text(input: &Value, key: &str) -> String {
    input
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn boolean(input: &Value, key: &str) -> bool {
    match input.get(key) {
        Some(Value::Bool(value)) => *value,
        // DELETE routes carry their input as query strings.
        Some(Value::String(value)) => value == "true" || value == "1",
        _ => false,
    }
}

/// GECOS is a colon-separated field; a colon or newline would add a field.
pub fn validate_gecos(value: &str) -> Result<(), Error> {
    if value.chars().count() > 256 {
        return Err(Error::invalid(
            "a description must be at most 256 characters",
        ));
    }
    if value.contains(':') || value.contains('\n') || value.contains('\r') {
        return Err(Error::invalid(
            "a description must not contain `:` or a newline",
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(Error::invalid(
            "a description must not contain control characters",
        ));
    }
    Ok(())
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
    use crate::inventory::{Group, IdRanges, Source};

    fn inventory() -> Inventory {
        Inventory {
            id_ranges: IdRanges::default(),
            users: vec![
                User {
                    name: "root".to_string(),
                    uid: 0,
                    gid: 0,
                    group: "root".to_string(),
                    gecos: "root".to_string(),
                    home: "/root".to_string(),
                    shell: "/bin/bash".to_string(),
                    system: true,
                    locked: Some(true),
                    source: Source::Local,
                    groups: Vec::new(),
                },
                User {
                    name: "daemon".to_string(),
                    uid: 1,
                    gid: 1,
                    group: "daemon".to_string(),
                    gecos: "daemon".to_string(),
                    home: "/usr/sbin".to_string(),
                    shell: "/usr/sbin/nologin".to_string(),
                    system: true,
                    locked: Some(true),
                    source: Source::Local,
                    groups: Vec::new(),
                },
                User {
                    name: "operator".to_string(),
                    uid: 1000,
                    gid: 1000,
                    group: "operator".to_string(),
                    gecos: "Operator".to_string(),
                    home: "/home/operator".to_string(),
                    shell: "/bin/bash".to_string(),
                    system: false,
                    locked: Some(false),
                    source: Source::Local,
                    groups: vec!["sudo".to_string()],
                },
                User {
                    name: "alice".to_string(),
                    uid: 1001,
                    gid: 1001,
                    group: "alice".to_string(),
                    gecos: "Alice".to_string(),
                    home: "/home/alice".to_string(),
                    shell: "/bin/bash".to_string(),
                    system: false,
                    locked: Some(false),
                    source: Source::Local,
                    groups: Vec::new(),
                },
            ],
            groups: vec![
                Group {
                    name: "root".to_string(),
                    gid: 0,
                    members: Vec::new(),
                    system: true,
                    source: Source::Local,
                },
                Group {
                    name: "daemon".to_string(),
                    gid: 1,
                    members: Vec::new(),
                    system: true,
                    source: Source::Local,
                },
                Group {
                    name: "operator".to_string(),
                    gid: 1000,
                    members: Vec::new(),
                    system: false,
                    source: Source::Local,
                },
                Group {
                    name: "alice".to_string(),
                    gid: 1001,
                    members: Vec::new(),
                    system: false,
                    source: Source::Local,
                },
                Group {
                    name: "sudo".to_string(),
                    gid: 27,
                    members: vec!["operator".to_string()],
                    system: true,
                    source: Source::Local,
                },
            ],
        }
    }

    fn policy() -> Policy {
        Policy {
            control: true,
            passwords: true,
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

    fn create_input(name: &str, uid: Value, gid: Value) -> Value {
        serde_json::json!({
            "name": name,
            "uid": uid,
            "gid": gid,
            "gecos": "Review User",
            "home": format!("/home/{name}"),
            "shell": "/bin/bash",
            "system": false
        })
    }

    #[test]
    fn create_user_builds_an_argv_without_a_shell() {
        let plan = create(create_input(
            "review",
            serde_json::json!(1500),
            serde_json::json!(1001),
        ))
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
                "--comment",
                "Review User",
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
    fn optional_ids_are_left_to_useradd() {
        let plan = create(create_input(
            "review",
            serde_json::Value::Null,
            serde_json::Value::Null,
        ))
        .expect("plan");
        assert_eq!(plan.program, "useradd");
        assert_eq!(
            plan.args,
            vec![
                "--create-home",
                "--comment",
                "Review User",
                "--home-dir",
                "/home/review",
                "--shell",
                "/bin/bash",
                "review"
            ]
        );
    }

    #[test]
    fn read_only_policy_blocks_every_mutation() {
        let inventory = inventory();
        let mut policy = policy();
        policy.control = false;
        let guard = guard_for(&inventory, &policy);
        assert_eq!(
            plan_delete_user(
                &guard,
                &serde_json::json!({"name": "alice", "remove-home": false})
            )
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
        assert_eq!(
            plan_create_group(
                &guard,
                &serde_json::json!({"name": "builders", "gid": 1500, "system": false})
            )
            .expect_err("disabled")
            .kind(),
            anth_contract::ErrorKind::Denied
        );
    }

    #[test]
    fn passwords_need_their_own_flag() {
        let inventory = inventory();
        let mut policy = policy();
        policy.passwords = false;
        let guard = guard_for(&inventory, &policy);
        assert_eq!(
            plan_set_password(
                &guard,
                &serde_json::json!({"name": "alice", "password": "longpassword"})
            )
            .expect_err("disabled")
            .kind(),
            anth_contract::ErrorKind::Denied
        );
    }

    #[test]
    fn password_travels_on_stdin_and_is_validated() {
        let inventory = inventory();
        let policy = policy();
        let guard = guard_for(&inventory, &policy);
        let plan = plan_set_password(
            &guard,
            &serde_json::json!({"name": "alice", "password": "longpassword"}),
        )
        .expect("password");
        assert_eq!(plan.program, "chpasswd");
        assert!(plan.args.is_empty());
        assert_eq!(plan.stdin.as_deref(), Some("alice:longpassword\n"));

        for password in ["short", "has:colon", "line\nbreak"] {
            assert!(
                plan_set_password(
                    &guard,
                    &serde_json::json!({"name": "alice", "password": password})
                )
                .is_err(),
                "{password}"
            );
        }
    }

    #[test]
    fn root_and_self_are_protected() {
        let inventory = inventory();
        let policy = policy();
        let guard = guard_for(&inventory, &policy);
        assert_eq!(
            plan_delete_user(
                &guard,
                &serde_json::json!({"name": "root", "remove-home": false})
            )
            .expect_err("root")
            .kind(),
            anth_contract::ErrorKind::Denied
        );
        assert_eq!(
            plan_delete_user(
                &guard,
                &serde_json::json!({"name": "operator", "remove-home": false})
            )
            .expect_err("self")
            .kind(),
            anth_contract::ErrorKind::Denied
        );
    }

    #[test]
    fn every_administrative_group_must_keep_a_member() {
        let mut inventory = inventory();
        inventory.groups.push(Group {
            name: "wheel".to_string(),
            gid: 10,
            members: vec!["alice".to_string()],
            system: true,
            source: Source::Local,
        });
        let policy = policy();
        let guard = Guard {
            inventory: &inventory,
            policy: &policy,
            self_name: "root",
        };
        let error = plan_delete_user(
            &guard,
            &serde_json::json!({"name": "operator", "remove-home": false}),
        )
        .expect_err("sudo's last member");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        assert!(error.message().contains("sudo"), "{error}");

        for group in &mut inventory.groups {
            if group.name == "sudo" {
                group.members.push("bob".to_string());
            }
        }
        let guard = Guard {
            inventory: &inventory,
            policy: &policy,
            self_name: "root",
        };
        plan_delete_user(
            &guard,
            &serde_json::json!({"name": "operator", "remove-home": false}),
        )
        .expect("another sudo member remains");
    }

    #[test]
    fn a_group_with_the_same_name_does_not_block_a_user() {
        let mut inventory = inventory();
        inventory.groups.push(Group {
            name: "review".to_string(),
            gid: 1500,
            members: Vec::new(),
            system: false,
            source: Source::Local,
        });
        let policy = policy();
        let guard = guard_for(&inventory, &policy);
        let plan = plan_create_user(
            &guard,
            &create_input("review", serde_json::json!(1500), serde_json::json!(1500)),
        )
        .expect("user shares the group name");
        assert_eq!(plan.args.last().map(String::as_str), Some("review"));
    }

    #[test]
    fn system_accounts_and_ids_need_the_opt_in() {
        let inventory = inventory();
        let mut policy = policy();
        let guard = guard_for(&inventory, &policy);
        let error = plan_delete_user(
            &guard,
            &serde_json::json!({"name": "daemon", "remove-home": false}),
        )
        .expect_err("system account");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        let error = create(create_input(
            "sys",
            serde_json::json!(500),
            serde_json::json!(1000),
        ))
        .expect_err("system id");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);

        policy.allow_system = true;
        assert!(plan_create_user(
            &guard_for(&inventory, &policy),
            &create_input("sys", serde_json::json!(500), serde_json::json!(1000))
        )
        .is_ok());
    }

    #[test]
    fn privileged_groups_need_the_opt_in() {
        let inventory = inventory();
        let mut policy = policy();
        let guard = guard_for(&inventory, &policy);
        let error = plan_create_user(
            &guard,
            &create_input("admin2", serde_json::json!(1500), serde_json::json!(27)),
        )
        .expect_err("privileged group");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);

        policy.allow_privileged_groups = true;
        policy.allow_system = true;
        let guard = guard_for(&inventory, &policy);
        assert!(plan_create_user(
            &guard,
            &create_input("admin2", serde_json::json!(1500), serde_json::json!(27))
        )
        .is_ok());
        assert!(plan_create_group(
            &guard,
            &serde_json::json!({"name": "docker", "gid": 1500, "system": false})
        )
        .is_ok());
    }

    #[test]
    fn home_shell_and_gecos_are_validated() {
        let error = create(serde_json::json!({
            "name": "outside",
            "uid": 1500,
            "gid": 1000,
            "gecos": "Outside",
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
            "gecos": "Shell",
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
            "gecos": "Dots",
            "home": "/home/../etc",
            "shell": "/bin/sh",
            "system": false
        }))
        .expect_err("traversal");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Invalid);

        let error = create(serde_json::json!({
            "name": "gecos",
            "uid": 1500,
            "gid": 1000,
            "gecos": "root:0:0",
            "home": "/home/gecos",
            "shell": "/bin/sh",
            "system": false
        }))
        .expect_err("gecos");
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
    fn group_mutations_use_groupadd_and_groupdel() {
        let inventory = inventory();
        let policy = policy();
        let guard = guard_for(&inventory, &policy);
        let plan = plan_create_group(
            &guard,
            &serde_json::json!({"name": "builders", "gid": 1500, "system": false}),
        )
        .expect("create group");
        assert_eq!(plan.program, "groupadd");
        assert_eq!(plan.args, vec!["--gid", "1500", "builders"]);
        let plan = plan_create_group(
            &guard,
            &serde_json::json!({"name": "builders", "gid": null, "system": false}),
        )
        .expect("create group without gid");
        assert_eq!(plan.args, vec!["builders"]);
    }

    #[test]
    fn deleting_groups_respects_primary_and_admin_groups() {
        let mut inventory = inventory();
        inventory.groups.push(Group {
            name: "builders".to_string(),
            gid: 1500,
            members: Vec::new(),
            system: false,
            source: Source::Local,
        });
        let policy = policy();
        let guard = guard_for(&inventory, &policy);
        let error = plan_delete_group(&guard, &serde_json::json!({"name": "alice"}))
            .expect_err("primary group");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Conflict);

        let mut policy = policy;
        policy.allow_system = true;
        let guard = guard_for(&inventory, &policy);
        let error = plan_delete_group(&guard, &serde_json::json!({"name": "sudo"}))
            .expect_err("last admin group");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        let plan = plan_delete_group(&guard, &serde_json::json!({"name": "builders"}))
            .expect("empty ordinary group");
        assert_eq!(plan.program, "groupdel");
        assert_eq!(plan.args, vec!["builders"]);
    }

    #[test]
    fn remove_home_is_explicit_and_bounded() {
        let inventory = inventory();
        let policy = policy();
        let guard = guard_for(&inventory, &policy);
        let plan = plan_delete_user(
            &guard,
            &serde_json::json!({"name": "alice", "remove-home": true}),
        )
        .expect("remove home");
        assert_eq!(plan.args, vec!["--remove", "alice"]);
        let plan = plan_delete_user(
            &guard,
            &serde_json::json!({"name": "alice", "remove-home": false}),
        )
        .expect("keep home");
        assert_eq!(plan.args, vec!["alice"]);

        let mut inventory = inventory;
        inventory.users.push(User {
            name: "outside".to_string(),
            uid: 1500,
            gid: 1000,
            group: "operator".to_string(),
            gecos: "Outside".to_string(),
            home: "/etc/outside".to_string(),
            shell: "/bin/sh".to_string(),
            system: false,
            locked: Some(false),
            source: Source::Local,
            groups: Vec::new(),
        });
        let guard = guard_for(&inventory, &policy);
        let error = plan_delete_user(
            &guard,
            &serde_json::json!({"name": "outside", "remove-home": true}),
        )
        .expect_err("home outside root");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
    }

    #[test]
    fn delete_home_accepts_a_query_string() {
        let inventory = inventory();
        let policy = policy();
        let guard = guard_for(&inventory, &policy);
        let plan = plan_delete_user(
            &guard,
            &serde_json::json!({"name": "alice", "remove-home": "true"}),
        )
        .expect("query string");
        assert_eq!(plan.args, vec!["--remove", "alice"]);
    }

    #[test]
    fn nss_users_and_groups_are_not_writable() {
        let mut inventory = inventory();
        inventory.users.push(User {
            name: "ana".to_string(),
            uid: 2000,
            gid: 2000,
            group: "ldap".to_string(),
            gecos: "Ana".to_string(),
            home: "/home/ana".to_string(),
            shell: "/bin/bash".to_string(),
            system: false,
            locked: None,
            source: Source::Nss,
            groups: Vec::new(),
        });
        inventory.groups.push(Group {
            name: "ldap".to_string(),
            gid: 2000,
            members: vec!["ana".to_string()],
            system: false,
            source: Source::Nss,
        });
        let policy = policy();
        let guard = guard_for(&inventory, &policy);
        for function in [
            plan_delete_user(
                &guard,
                &serde_json::json!({"name": "ana", "remove-home": false}),
            ),
            plan_set_lock(&guard, &serde_json::json!({"name": "ana", "locked": true})),
            plan_set_password(
                &guard,
                &serde_json::json!({"name": "ana", "password": "longpassword"}),
            ),
        ] {
            assert_eq!(
                function.expect_err("nss").kind(),
                anth_contract::ErrorKind::Denied
            );
        }
        assert_eq!(
            plan_delete_group(&guard, &serde_json::json!({"name": "ldap"}))
                .expect_err("nss group")
                .kind(),
            anth_contract::ErrorKind::Denied
        );
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
