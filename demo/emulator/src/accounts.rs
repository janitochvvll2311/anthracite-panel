//! Deterministic users and groups fixture for the demo (§19 §2, `accounts`).

use anth_contract::Error;
use serde_json::{json, Value};

use crate::{boolean, text};

#[derive(serde::Serialize, serde::Deserialize)]
struct User {
    name: String,
    uid: u32,
    gid: u32,
    #[serde(default)]
    gecos: String,
    home: String,
    shell: String,
    system: bool,
    locked: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Group {
    name: String,
    gid: u32,
    members: Vec<String>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct AccountsFixture {
    users: Vec<User>,
    groups: Vec<Group>,
}

impl AccountsFixture {
    pub fn new() -> Self {
        Self {
            users: crate::world::users()
                .iter()
                .map(|user| User {
                    name: user.name.to_string(),
                    uid: user.uid,
                    gid: user.gid,
                    gecos: String::new(),
                    home: user.home.to_string(),
                    shell: user.shell.to_string(),
                    system: user.system,
                    locked: user.locked,
                })
                .collect(),
            groups: crate::world::groups()
                .iter()
                .map(|group| Group {
                    name: group.name.to_string(),
                    gid: group.gid,
                    members: group
                        .members
                        .iter()
                        .map(|member| member.to_string())
                        .collect(),
                })
                .collect(),
        }
    }

    pub fn home_for(&self, name: &str) -> Option<String> {
        self.users
            .iter()
            .find(|user| user.name == name)
            .map(|user| user.home.clone())
    }

    pub fn handle(&mut self, function: &str, input: &Value) -> Result<Value, Error> {
        match function {
            "users" => Ok(Value::Array(
                self.users
                    .iter()
                    .map(|user| {
                        let group = self
                            .groups
                            .iter()
                            .find(|group| group.gid == user.gid)
                            .map(|group| group.name.clone())
                            .unwrap_or_else(|| user.gid.to_string());
                        let groups: Vec<&str> = self
                            .groups
                            .iter()
                            .filter(|group| group.members.iter().any(|member| member == &user.name))
                            .map(|group| group.name.as_str())
                            .collect();
                        json!({
                            "name": user.name,
                            "uid": user.uid,
                            "gid": user.gid,
                            "group": group,
                            "gecos": user.gecos,
                            "home": user.home,
                            "shell": user.shell,
                            "system": user.system,
                            "locked": user.locked,
                            "source": "local",
                            "groups": groups,
                        })
                    })
                    .collect(),
            )),
            "groups" => Ok(Value::Array(
                self.groups
                    .iter()
                    .map(|group| {
                        json!({
                            "name": group.name,
                            "gid": group.gid,
                            "members": group.members,
                            "system": group.gid < 1000,
                            "source": "local",
                        })
                    })
                    .collect(),
            )),
            "policy" => Ok(json!({
                "control": true,
                "passwords": true,
                "allow-system": true,
                "allow-privileged-groups": true,
                "home-root": "/home",
                "shells": ["/bin/sh", "/bin/bash"],
                "default-shell": "/bin/sh",
            })),
            "create-user" => {
                let name = text(input, "name")?;
                if !valid_name(&name) {
                    return Err(Error::invalid(
                        "`name` must be 1-32 lowercase letters, digits, `_` or `-`",
                    ));
                }
                if self.users.iter().any(|user| user.name == name) {
                    return Err(Error::conflict(format!("`{name}` already exists")));
                }
                let uid = match optional_number(input, "uid")? {
                    Some(uid) => {
                        if self.users.iter().any(|user| user.uid == uid) {
                            return Err(Error::conflict(format!("uid {uid} is already used")));
                        }
                        uid
                    }
                    None => self.next_uid(),
                };
                let gid = match optional_number(input, "gid")? {
                    Some(gid) => {
                        if !self.groups.iter().any(|group| group.gid == gid) {
                            return Err(Error::not_found(format!("gid {gid} does not exist")));
                        }
                        gid
                    }
                    None => match self.groups.iter().find(|group| group.name == name) {
                        Some(group) => group.gid,
                        None => {
                            let gid = self.next_gid();
                            self.groups.push(Group {
                                name: name.clone(),
                                gid,
                                members: Vec::new(),
                            });
                            gid
                        }
                    },
                };
                let gecos = input
                    .get("gecos")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                if gecos.contains(':') || gecos.contains('\n') || gecos.contains('\r') {
                    return Err(Error::invalid("`gecos` must not contain `:` or a newline"));
                }
                let home = text(input, "home")?;
                if !home.starts_with('/') || home.contains("..") {
                    return Err(Error::invalid("`home` must be an absolute path"));
                }
                let shell = input
                    .get("shell")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|shell| !shell.is_empty())
                    .unwrap_or("/bin/sh")
                    .to_string();
                let system = boolean(input, "system");
                self.users.push(User {
                    name,
                    uid,
                    gid,
                    gecos,
                    home,
                    shell,
                    system,
                    // A new account has no password yet, like `useradd`.
                    locked: true,
                });
                Ok(Value::Null)
            }
            "delete-user" => {
                let name = text(input, "name")?;
                if name == "root" || name == "operator" {
                    return Err(Error::denied(
                        "the root and the operator accounts are protected",
                    ));
                }
                let index = self
                    .users
                    .iter()
                    .position(|user| user.name == name)
                    .ok_or_else(|| Error::not_found(format!("`{name}` does not exist")))?;
                if self.users[index].system && name != "operator" {
                    return Err(Error::denied("system accounts cannot be deleted"));
                }
                self.users.remove(index);
                for group in &mut self.groups {
                    group.members.retain(|member| member != &name);
                }
                Ok(Value::Null)
            }
            "set-lock" => {
                let name = text(input, "name")?;
                if name == "root" {
                    return Err(Error::denied("the root account cannot be locked"));
                }
                let locked = boolean(input, "locked");
                let user = self
                    .users
                    .iter_mut()
                    .find(|user| user.name == name)
                    .ok_or_else(|| Error::not_found(format!("`{name}` does not exist")))?;
                user.locked = locked;
                Ok(Value::Null)
            }
            "set-password" => {
                let name = text(input, "name")?;
                if name == "root" || name == "operator" {
                    return Err(Error::denied(
                        "the root and the operator accounts are protected",
                    ));
                }
                let password = text(input, "password")?;
                if password.chars().count() < 8
                    || password.chars().count() > 256
                    || password.contains(':')
                    || password.contains('\n')
                    || password.contains('\r')
                {
                    return Err(Error::invalid(
                        "a password must be 8-256 characters without `:` or a newline",
                    ));
                }
                let user = self
                    .users
                    .iter_mut()
                    .find(|user| user.name == name)
                    .ok_or_else(|| Error::not_found(format!("`{name}` does not exist")))?;
                // The fixture never stores a password; setting one clears the
                // lock bit the way `chpasswd` does.
                user.locked = false;
                Ok(Value::Null)
            }
            "create-group" => {
                let name = text(input, "name")?;
                if !valid_name(&name) {
                    return Err(Error::invalid(
                        "`name` must be 1-32 lowercase letters, digits, `_` or `-`",
                    ));
                }
                if self.groups.iter().any(|group| group.name == name) {
                    return Err(Error::conflict(format!("`{name}` already exists")));
                }
                let gid = match optional_number(input, "gid")? {
                    Some(gid) => {
                        if self.groups.iter().any(|group| group.gid == gid) {
                            return Err(Error::conflict(format!("gid {gid} is already used")));
                        }
                        gid
                    }
                    None => self.next_gid(),
                };
                self.groups.push(Group {
                    name,
                    gid,
                    members: Vec::new(),
                });
                Ok(Value::Null)
            }
            "delete-group" => {
                let name = text(input, "name")?;
                if name == "root" {
                    return Err(Error::denied("the root group is protected"));
                }
                let group = self
                    .groups
                    .iter()
                    .find(|group| group.name == name)
                    .ok_or_else(|| Error::not_found(format!("`{name}` does not exist")))?;
                if self.users.iter().any(|user| user.gid == group.gid) {
                    return Err(Error::conflict(format!("`{name}` is a primary group")));
                }
                self.groups.retain(|candidate| candidate.name != name);
                Ok(Value::Null)
            }
            other => Err(Error::not_found(format!(
                "`anthracite:accounts/accounts#{other}` is not implemented"
            ))),
        }
    }

    fn next_uid(&self) -> u32 {
        self.users.iter().map(|user| user.uid).max().unwrap_or(999) + 1
    }

    fn next_gid(&self) -> u32 {
        self.groups
            .iter()
            .map(|group| group.gid)
            .max()
            .unwrap_or(999)
            + 1
    }
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

fn valid_name(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    name.len() <= 32
        && (first.is_ascii_lowercase() || first == '_')
        && characters.all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || character == '_'
                || character == '-'
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(fixture: &mut AccountsFixture, function: &str, input: Value) -> Value {
        fixture
            .handle(function, &input)
            .expect("the fixture answers")
    }

    #[test]
    fn users_and_groups_are_stable() {
        let mut fixture = AccountsFixture::new();
        let users = call(&mut fixture, "users", Value::Null);
        assert_eq!(users[2]["name"], "operator");
        assert_eq!(users[2]["uid"], 1000);
        assert_eq!(users[2]["source"], "local");
        assert_eq!(users[2]["group"], "operator");
        assert_eq!(users[1]["locked"], true);
        let groups = call(&mut fixture, "groups", Value::Null);
        assert_eq!(groups[2]["name"], "sudo");
        assert_eq!(groups[2]["members"][0], "operator");
        assert_eq!(groups[2]["source"], "local");
    }

    #[test]
    fn a_user_can_be_created_locked_and_deleted() {
        let mut fixture = AccountsFixture::new();
        call(
            &mut fixture,
            "create-user",
            json!({"name": "review", "uid": 1500, "gid": null, "gecos": "Review", "home": "/home/review", "shell": "/bin/sh", "system": false}),
        );
        let users = call(&mut fixture, "users", Value::Null);
        assert!(users.as_array().expect("users").iter().any(|user| {
            user["name"] == "review" && user["locked"] == true && user["group"] == "review"
        }));
        call(
            &mut fixture,
            "set-password",
            json!({"name": "review", "password": "longpassword"}),
        );
        let users = call(&mut fixture, "users", Value::Null);
        assert!(users
            .as_array()
            .expect("users")
            .iter()
            .any(|user| user["name"] == "review" && user["locked"] == false));
        call(
            &mut fixture,
            "set-lock",
            json!({"name": "review", "locked": true}),
        );
        let users = call(&mut fixture, "users", Value::Null);
        assert!(users
            .as_array()
            .expect("users")
            .iter()
            .any(|user| user["name"] == "review" && user["locked"] == true));
        call(
            &mut fixture,
            "delete-user",
            json!({"name": "review", "remove-home": false}),
        );
        let users = call(&mut fixture, "users", Value::Null);
        assert!(!users
            .as_array()
            .expect("users")
            .iter()
            .any(|user| user["name"] == "review"));
    }

    #[test]
    fn groups_can_be_created_and_deleted() {
        let mut fixture = AccountsFixture::new();
        call(
            &mut fixture,
            "create-group",
            json!({"name": "builders", "gid": 1500, "system": false}),
        );
        let groups = call(&mut fixture, "groups", Value::Null);
        assert!(groups
            .as_array()
            .expect("groups")
            .iter()
            .any(|group| group["name"] == "builders" && group["gid"] == 1500));
        call(&mut fixture, "delete-group", json!({"name": "builders"}));
        let groups = call(&mut fixture, "groups", Value::Null);
        assert!(!groups
            .as_array()
            .expect("groups")
            .iter()
            .any(|group| group["name"] == "builders"));
    }

    #[test]
    fn old_accounts_state_without_gecos_still_loads() {
        let user: User = serde_json::from_value(json!({
            "name": "operator",
            "uid": 1000,
            "gid": 1000,
            "home": "/home/operator",
            "shell": "/bin/sh",
            "system": false,
            "locked": false
        }))
        .expect("old user");
        assert_eq!(user.gecos, "");
    }

    #[test]
    fn protected_accounts_stay() {
        let mut fixture = AccountsFixture::new();
        for name in ["root", "operator", "daemon"] {
            let error = fixture
                .handle("delete-user", &json!({"name": name, "remove-home": false}))
                .expect_err("protected");
            assert_eq!(error.kind(), anth_contract::ErrorKind::Denied, "{name}");
        }
        let error = fixture
            .handle("set-lock", &json!({"name": "root", "locked": true}))
            .expect_err("root lock");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
        for name in ["root", "operator"] {
            let error = fixture
                .handle(
                    "set-password",
                    &json!({"name": name, "password": "longpassword"}),
                )
                .expect_err("protected password");
            assert_eq!(error.kind(), anth_contract::ErrorKind::Denied, "{name}");
        }
    }
}
