//! Deterministic users and groups fixture for the demo (§19 §2, `accounts`).

use anth_contract::Error;
use serde_json::{json, Value};

use crate::{boolean, number, text};

#[derive(serde::Serialize, serde::Deserialize)]
struct User {
    name: String,
    uid: u32,
    gid: u32,
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

    pub fn handle(&mut self, function: &str, input: &Value) -> Result<Value, Error> {
        match function {
            "users" => Ok(Value::Array(
                self.users
                    .iter()
                    .map(|user| {
                        json!({
                            "name": user.name,
                            "uid": user.uid,
                            "gid": user.gid,
                            "home": user.home,
                            "shell": user.shell,
                            "system": user.system,
                            "locked": user.locked,
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
                        })
                    })
                    .collect(),
            )),
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
                let uid = number(input, "uid")
                    .and_then(|value| u32::try_from(value).ok())
                    .filter(|value| *value > 0)
                    .ok_or_else(|| Error::invalid("`uid` must be a positive u32"))?;
                if self.users.iter().any(|user| user.uid == uid) {
                    return Err(Error::conflict(format!("uid {uid} is already used")));
                }
                let gid = number(input, "gid")
                    .and_then(|value| u32::try_from(value).ok())
                    .filter(|value| *value > 0)
                    .ok_or_else(|| Error::invalid("`gid` must be a positive u32"))?;
                let home = text(input, "home")?;
                if !home.starts_with('/') || home.contains("..") {
                    return Err(Error::invalid("`home` must be an absolute path"));
                }
                let shell = text(input, "shell")?;
                if shell.trim().is_empty() {
                    return Err(Error::invalid("`shell` cannot be empty"));
                }
                let system = boolean(input, "system");
                self.users.push(User {
                    name,
                    uid,
                    gid,
                    home,
                    shell,
                    system,
                    locked: false,
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
                if self.users[index].system {
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
            other => Err(Error::not_found(format!(
                "`anthracite:accounts/accounts#{other}` is not implemented"
            ))),
        }
    }
}

fn valid_name(name: &str) -> bool {
    let mut characters = name.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    name.len() <= 32
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
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
        assert_eq!(users[1]["locked"], true);
        let groups = call(&mut fixture, "groups", Value::Null);
        assert_eq!(groups[2]["name"], "sudo");
        assert_eq!(groups[2]["members"][0], "operator");
    }

    #[test]
    fn a_user_can_be_created_locked_and_deleted() {
        let mut fixture = AccountsFixture::new();
        call(
            &mut fixture,
            "create-user",
            json!({"name": "review", "uid": 1500, "gid": 1500, "home": "/home/review", "shell": "/bin/sh", "system": false}),
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
        call(&mut fixture, "delete-user", json!({"name": "review"}));
        let users = call(&mut fixture, "users", Value::Null);
        assert!(!users
            .as_array()
            .expect("users")
            .iter()
            .any(|user| user["name"] == "review"));
    }

    #[test]
    fn protected_accounts_stay() {
        let mut fixture = AccountsFixture::new();
        for name in ["root", "operator", "daemon"] {
            let error = fixture
                .handle("delete-user", &json!({"name": name}))
                .expect_err("protected");
            assert_eq!(error.kind(), anth_contract::ErrorKind::Denied, "{name}");
        }
        let error = fixture
            .handle("set-lock", &json!({"name": "root", "locked": true}))
            .expect_err("root lock");
        assert_eq!(error.kind(), anth_contract::ErrorKind::Denied);
    }
}
