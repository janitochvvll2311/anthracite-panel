//! Synthetic account inventory and mutations for the demo build.
//!
//! No `getent`, `/etc/shadow`, `useradd` or `chpasswd` runs here. The table is
//! in memory, so the UI can create, delete and change a password without
//! touching the host.

use std::sync::Mutex;

use anth_wire::{
    AccountCreate, AccountDelete, AccountGroup, AccountGroupCreate, AccountGroupDelete,
    AccountSetPassword, AccountSnapshot, AccountSource, AccountUser, ErrorKind, WireError,
};

use crate::backend::AccountsService;
use crate::now_ms;

use crate::accounts::AccountsSettings;

const PRIVILEGED_GROUPS: &[&str] = &["sudo", "wheel", "admin", "root", "docker", "lxd"];
const ADMIN_GROUPS: &[&str] = &["sudo", "wheel", "admin"];
const SHELLS: &[&str] = &["/bin/sh", "/bin/bash", "/bin/ash", "/bin/zsh", "/bin/dash"];
const MIN_PASSWORD_LEN: usize = 8;
const MAX_PASSWORD_LEN: usize = 256;

pub struct DemoAccounts {
    settings: AccountsSettings,
    state: Mutex<DemoState>,
}

struct DemoState {
    users: Vec<AccountUser>,
    groups: Vec<AccountGroup>,
    next_uid: u32,
    next_gid: u32,
}

impl DemoAccounts {
    pub fn new(settings: AccountsSettings) -> Self {
        Self {
            settings,
            state: Mutex::new(seed()),
        }
    }

    fn view(&self, state: &DemoState) -> AccountSnapshot {
        AccountSnapshot {
            users: state.users.clone(),
            groups: state.groups.clone(),
            control_enabled: self.settings.control,
            password_enabled: self.settings.manage_passwords,
            allow_system: self.settings.allow_system,
            allow_privileged_groups: self.settings.allow_privileged_groups,
            timestamp_ms: now_ms(),
            warning: None,
        }
    }

    fn control(&self) -> Result<(), WireError> {
        if !self.settings.control {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "accounts_disabled",
                "account changes are disabled; set accounts.control to enable them",
            ));
        }
        Ok(())
    }

    fn privileged(&self, group: &str) -> Result<(), WireError> {
        if PRIVILEGED_GROUPS.contains(&group) && !self.settings.allow_privileged_groups {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "privileged_group",
                format!(
                    "`{group}` grants root-equivalent access; set accounts.allow_privileged_groups to touch it"
                ),
            ));
        }
        Ok(())
    }

    fn writable_user<'a>(
        &self,
        state: &'a DemoState,
        name: &str,
    ) -> Result<&'a AccountUser, WireError> {
        let user = state
            .users
            .iter()
            .find(|user| user.name == name)
            .ok_or_else(|| {
                WireError::new(
                    ErrorKind::NotFound,
                    "account_not_found",
                    format!("no such user `{name}`"),
                )
            })?;
        if !user.source.is_writable() {
            return Err(WireError::new(
                ErrorKind::Unsupported,
                "account_unsupported",
                format!("`{name}` comes from a directory service and cannot be changed"),
            ));
        }
        if user.uid == 0 {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "protected_account",
                "uid 0 is never modified through the panel",
            ));
        }
        if user.system && !self.settings.allow_system {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "protected_account",
                format!(
                    "`{name}` is a system account (uid {}); set accounts.allow_system to change it",
                    user.uid
                ),
            ));
        }
        Ok(user)
    }

    fn shell(&self, shell: Option<&str>) -> Result<String, WireError> {
        let shell = shell.unwrap_or(&self.settings.default_shell).to_string();
        if !shell.starts_with('/') {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "invalid_shell",
                "the shell must be an absolute path",
            ));
        }
        let ok = if self.settings.shells.is_empty() {
            SHELLS.contains(&shell.as_str())
        } else {
            self.settings.shells.iter().any(|entry| entry == &shell)
        };
        if !ok {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "invalid_shell",
                format!("`{shell}` is not an allowed login shell"),
            ));
        }
        Ok(shell)
    }

    fn home(&self, name: &str, home: Option<&str>) -> Result<String, WireError> {
        let home = home.map(str::to_string).unwrap_or_else(|| {
            format!("{}/{}", self.settings.home_root.trim_end_matches('/'), name)
        });
        let root = self.settings.home_root.trim_end_matches('/');
        if !home.starts_with(&format!("{root}/")) {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "invalid_home",
                format!("the home directory must be inside `{root}`"),
            ));
        }
        Ok(home)
    }

    fn next_uid(state: &mut DemoState) -> u32 {
        while state.users.iter().any(|user| user.uid == state.next_uid) {
            state.next_uid += 1;
        }
        let uid = state.next_uid;
        state.next_uid += 1;
        uid
    }

    fn next_gid(state: &mut DemoState) -> u32 {
        while state.groups.iter().any(|group| group.gid == state.next_gid) {
            state.next_gid += 1;
        }
        let gid = state.next_gid;
        state.next_gid += 1;
        gid
    }

    fn administrators(state: &DemoState) -> usize {
        state
            .users
            .iter()
            .filter(|user| {
                state.groups.iter().any(|group| {
                    group.gid == user.gid && ADMIN_GROUPS.contains(&group.name.as_str())
                }) || user.groups.iter().any(|name| {
                    ADMIN_GROUPS.contains(&name.as_str())
                        && state.groups.iter().any(|group| &group.name == name)
                })
            })
            .count()
    }
}

impl AccountsService for DemoAccounts {
    fn snapshot(&self) -> AccountSnapshot {
        let state = self.state.lock().expect("demo accounts lock");
        self.view(&state)
    }

    fn create_user(&self, request: &AccountCreate) -> Result<String, WireError> {
        self.control()?;
        crate::accounts::validate_name(&request.name)?;
        let mut state = self.state.lock().expect("demo accounts lock");
        if state.users.iter().any(|user| user.name == request.name) {
            return Err(WireError::new(
                ErrorKind::Conflict,
                "account_conflict",
                format!("user `{}` already exists", request.name),
            ));
        }
        if state.groups.iter().any(|group| group.name == request.name) {
            return Err(WireError::new(
                ErrorKind::Conflict,
                "account_conflict",
                format!(
                    "group `{}` already exists; `useradd` would refuse to create the user",
                    request.name
                ),
            ));
        }
        if request.system && !self.settings.allow_system {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "protected_account",
                "creating a system account requires accounts.allow_system",
            ));
        }
        if let Some(uid) = request.uid {
            if uid == 0 {
                return Err(WireError::new(
                    ErrorKind::Forbidden,
                    "protected_account",
                    "uid 0 is reserved",
                ));
            }
            if state.users.iter().any(|user| user.uid == uid) {
                return Err(WireError::new(
                    ErrorKind::Conflict,
                    "account_conflict",
                    format!("uid `{uid}` is already taken"),
                ));
            }
        }
        let group = match request.gid {
            Some(gid) => state
                .groups
                .iter()
                .find(|group| group.gid == gid)
                .cloned()
                .ok_or_else(|| {
                    WireError::new(
                        ErrorKind::NotFound,
                        "account_not_found",
                        format!("no such gid `{gid}`"),
                    )
                })?,
            None => {
                let gid = Self::next_gid(&mut state);
                let group = AccountGroup {
                    name: request.name.clone(),
                    gid,
                    members: vec![request.name.clone()],
                    system: request.system,
                    source: AccountSource::Local,
                };
                state.groups.push(group.clone());
                group
            }
        };
        self.privileged(&group.name)?;
        if let Some(gecos) = request.gecos.as_deref() {
            crate::accounts::validate_gecos(gecos)?;
        }
        let uid = request.uid.unwrap_or_else(|| Self::next_uid(&mut state));
        let home = self.home(&request.name, request.home.as_deref())?;
        let shell = self.shell(request.shell.as_deref())?;
        state.users.push(AccountUser {
            name: request.name.clone(),
            uid,
            gid: group.gid,
            group: group.name.clone(),
            gecos: request.gecos.clone().unwrap_or_default(),
            home,
            shell,
            locked: Some(true),
            system: request.system,
            source: AccountSource::Local,
            groups: if request.gid.is_none() {
                vec![group.name]
            } else {
                Vec::new()
            },
        });
        Ok(format!("created user {}", request.name))
    }

    fn delete_user(&self, request: &AccountDelete) -> Result<String, WireError> {
        self.control()?;
        crate::accounts::validate_name(&request.name)?;
        let mut state = self.state.lock().expect("demo accounts lock");
        self.writable_user(&state, &request.name)?;
        if request.name == "root" {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "protected_account",
                "the daemon's own account cannot be changed",
            ));
        }
        if ADMIN_GROUPS.iter().any(|group| {
            state
                .users
                .iter()
                .find(|user| user.name == request.name)
                .is_some_and(|user| {
                    state.groups.iter().any(|entry| {
                        entry.name == *group
                            && (entry.gid == user.gid || entry.members.contains(&user.name))
                    })
                })
        }) && Self::administrators(&state) <= 1
        {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "last_administrator",
                format!("`{}` is the last administrator", request.name),
            ));
        }
        state.users.retain(|user| user.name != request.name);
        for group in &mut state.groups {
            group.members.retain(|member| member != &request.name);
        }
        // `useradd` created a private group with the same name; drop it once it
        // is empty so the demo table does not collect orphans.
        let orphan = state
            .groups
            .iter()
            .any(|group| group.name == request.name && group.gid != 0 && group.members.is_empty());
        if orphan {
            state.groups.retain(|group| group.name != request.name);
        }
        Ok(format!("deleted user {}", request.name))
    }

    fn set_password(&self, request: &AccountSetPassword) -> Result<String, WireError> {
        if !self.settings.manage_passwords {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "account_passwords_disabled",
                "password changes are disabled; set accounts.manage_passwords to enable them",
            ));
        }
        let password = request.password.expose();
        if password.chars().count() < MIN_PASSWORD_LEN
            || password.chars().count() > MAX_PASSWORD_LEN
        {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "invalid_password",
                format!("a password must be {MIN_PASSWORD_LEN} to {MAX_PASSWORD_LEN} characters"),
            ));
        }
        if password.contains(':') || password.contains('\n') || password.contains('\r') {
            return Err(WireError::new(
                ErrorKind::Invalid,
                "invalid_password",
                "a password must not contain `:` or a newline",
            ));
        }
        let mut state = self.state.lock().expect("demo accounts lock");
        let name = request.name.clone();
        self.writable_user(&state, &name)?;
        if let Some(user) = state.users.iter_mut().find(|user| user.name == name) {
            user.locked = Some(false);
        }
        Ok(format!("set the password for {name}"))
    }

    fn create_group(&self, request: &AccountGroupCreate) -> Result<String, WireError> {
        self.control()?;
        crate::accounts::validate_name(&request.name)?;
        self.privileged(&request.name)?;
        let mut state = self.state.lock().expect("demo accounts lock");
        if state.groups.iter().any(|group| group.name == request.name) {
            return Err(WireError::new(
                ErrorKind::Conflict,
                "account_conflict",
                format!("group `{}` already exists", request.name),
            ));
        }
        if request.system && !self.settings.allow_system {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "protected_account",
                "creating a system group requires accounts.allow_system",
            ));
        }
        if let Some(gid) = request.gid {
            if gid == 0 {
                return Err(WireError::new(
                    ErrorKind::Forbidden,
                    "protected_account",
                    "gid 0 is reserved",
                ));
            }
            if state.groups.iter().any(|group| group.gid == gid) {
                return Err(WireError::new(
                    ErrorKind::Conflict,
                    "account_conflict",
                    format!("gid `{gid}` is already taken"),
                ));
            }
        }
        let gid = request.gid.unwrap_or_else(|| Self::next_gid(&mut state));
        state.groups.push(AccountGroup {
            name: request.name.clone(),
            gid,
            members: Vec::new(),
            system: request.system,
            source: AccountSource::Local,
        });
        Ok(format!("created group {}", request.name))
    }

    fn delete_group(&self, request: &AccountGroupDelete) -> Result<String, WireError> {
        self.control()?;
        crate::accounts::validate_name(&request.name)?;
        self.privileged(&request.name)?;
        let mut state = self.state.lock().expect("demo accounts lock");
        let group = state
            .groups
            .iter()
            .find(|group| group.name == request.name)
            .cloned()
            .ok_or_else(|| {
                WireError::new(
                    ErrorKind::NotFound,
                    "account_not_found",
                    format!("no such group `{}`", request.name),
                )
            })?;
        if group.gid == 0 {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "protected_account",
                "gid 0 is never deleted through the panel",
            ));
        }
        if group.system && !self.settings.allow_system {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "protected_account",
                format!(
                    "`{}` is a system group (gid {}); set accounts.allow_system to change it",
                    request.name, group.gid
                ),
            ));
        }
        if let Some(owner) = state.users.iter().find(|user| user.gid == group.gid) {
            return Err(WireError::new(
                ErrorKind::Conflict,
                "account_conflict",
                format!(
                    "`{}` is the primary group of `{}`",
                    request.name, owner.name
                ),
            ));
        }
        if ADMIN_GROUPS.contains(&request.name.as_str())
            && state.groups.iter().any(|entry| {
                ADMIN_GROUPS.contains(&entry.name.as_str()) && !entry.members.is_empty()
            })
            && state
                .groups
                .iter()
                .filter(|entry| {
                    ADMIN_GROUPS.contains(&entry.name.as_str()) && !entry.members.is_empty()
                })
                .count()
                <= 1
        {
            return Err(WireError::new(
                ErrorKind::Forbidden,
                "protected_group",
                format!(
                    "refusing to empty `{}`: no other administrative group would keep a member",
                    request.name
                ),
            ));
        }
        state.groups.retain(|group| group.name != request.name);
        for user in &mut state.users {
            user.groups.retain(|name| name != &request.name);
        }
        Ok(format!("deleted group {}", request.name))
    }
}

fn seed() -> DemoState {
    let group = |name: &str, gid: u32, members: &[&str], system: bool| AccountGroup {
        name: name.to_string(),
        gid,
        members: members.iter().map(|member| member.to_string()).collect(),
        system,
        source: AccountSource::Local,
    };
    let user = |name: &str,
                uid: u32,
                gid: u32,
                group: &str,
                gecos: &str,
                home: &str,
                shell: &str,
                locked: bool,
                system: bool,
                groups: &[&str]| AccountUser {
        name: name.to_string(),
        uid,
        gid,
        group: group.to_string(),
        gecos: gecos.to_string(),
        home: home.to_string(),
        shell: shell.to_string(),
        locked: Some(locked),
        system,
        source: AccountSource::Local,
        groups: groups.iter().map(|group| group.to_string()).collect(),
    };
    DemoState {
        users: vec![
            user(
                "root",
                0,
                0,
                "root",
                "root",
                "/root",
                "/bin/sh",
                true,
                true,
                &[],
            ),
            user(
                "demo",
                1000,
                1000,
                "demo",
                "Demo User",
                "/home/demo",
                "/bin/sh",
                false,
                false,
                &["demo", "users", "sudo"],
            ),
            user(
                "www-data",
                33,
                33,
                "www-data",
                "Web server",
                "/var/www",
                "/bin/sh",
                true,
                true,
                &[],
            ),
        ],
        groups: vec![
            group("root", 0, &[], true),
            group("sudo", 27, &["demo"], false),
            group("users", 100, &["demo"], false),
            group("demo", 1000, &["demo"], false),
            group("www-data", 33, &[], true),
        ],
        next_uid: 1001,
        next_gid: 1001,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> AccountsSettings {
        AccountsSettings {
            control: true,
            manage_passwords: true,
            allow_system: true,
            allow_privileged_groups: true,
            home_root: "/home".to_string(),
            shells: vec!["/bin/sh".to_string()],
            default_shell: "/bin/sh".to_string(),
        }
    }

    #[test]
    fn users_and_groups_can_be_created_and_deleted() {
        let accounts = DemoAccounts::new(settings());
        accounts
            .create_user(&AccountCreate {
                name: "alice".to_string(),
                uid: None,
                gid: None,
                gecos: Some("Alice".to_string()),
                home: None,
                shell: None,
                system: false,
            })
            .expect("create user");
        let snapshot = accounts.snapshot();
        let user = snapshot
            .users
            .iter()
            .find(|user| user.name == "alice")
            .expect("alice");
        assert_eq!(user.home, "/home/alice");
        assert_eq!(user.locked, Some(true));

        accounts
            .set_password(&AccountSetPassword {
                name: "alice".to_string(),
                password: "long-enough".into(),
            })
            .expect("password");
        assert_eq!(
            accounts
                .snapshot()
                .users
                .iter()
                .find(|user| user.name == "alice")
                .and_then(|user| user.locked),
            Some(false)
        );

        accounts
            .create_group(&AccountGroupCreate {
                name: "e2e".to_string(),
                gid: None,
                system: false,
            })
            .expect("create group");
        accounts
            .delete_group(&AccountGroupDelete {
                name: "e2e".to_string(),
            })
            .expect("delete group");
        accounts
            .delete_user(&AccountDelete {
                name: "alice".to_string(),
                remove_home: true,
            })
            .expect("delete user");
        assert!(
            !accounts
                .snapshot()
                .users
                .iter()
                .any(|user| user.name == "alice")
        );
    }

    #[test]
    fn policy_and_protected_accounts_are_enforced() {
        let accounts = DemoAccounts::new(AccountsSettings::default());
        let disabled = accounts.create_group(&AccountGroupCreate {
            name: "demo-group".to_string(),
            gid: None,
            system: false,
        });
        assert!(disabled.is_err());
        assert!(
            accounts
                .delete_user(&AccountDelete {
                    name: "root".to_string(),
                    remove_home: false,
                })
                .is_err()
        );
    }
}
