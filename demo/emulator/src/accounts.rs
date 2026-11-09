//! Deterministic users and groups fixture for the demo (§19 §2, `accounts`).

use anth_contract::Error;
use serde_json::{json, Value};

pub struct AccountsFixture;

impl AccountsFixture {
    pub fn new() -> Self {
        Self
    }

    pub fn handle(&self, function: &str, _input: &Value) -> Result<Value, Error> {
        match function {
            "users" => Ok(json!([
                {
                    "name": "root",
                    "uid": 0,
                    "gid": 0,
                    "home": "/root",
                    "shell": "/bin/sh",
                    "system": true,
                    "locked": false,
                },
                {
                    "name": "daemon",
                    "uid": 1,
                    "gid": 1,
                    "home": "/usr/sbin",
                    "shell": "/usr/sbin/nologin",
                    "system": true,
                    "locked": true,
                },
                {
                    "name": "operator",
                    "uid": 1000,
                    "gid": 1000,
                    "home": "/home/operator",
                    "shell": "/bin/sh",
                    "system": false,
                    "locked": false,
                },
                {
                    "name": "backup",
                    "uid": 34,
                    "gid": 34,
                    "home": "/var/backups",
                    "shell": "/usr/sbin/nologin",
                    "system": true,
                    "locked": true,
                },
                {
                    "name": "postgres",
                    "uid": 999,
                    "gid": 999,
                    "home": "/var/lib/postgresql",
                    "shell": "/bin/sh",
                    "system": false,
                    "locked": false,
                },
            ])),
            "groups" => Ok(json!([
                { "name": "root", "gid": 0, "members": [] },
                { "name": "operator", "gid": 1000, "members": ["operator"] },
                {
                    "name": "sudo",
                    "gid": 27,
                    "members": ["operator"],
                },
                { "name": "docker", "gid": 999, "members": ["operator"] },
                {
                    "name": "postgres",
                    "gid": 999,
                    "members": ["postgres"],
                },
            ])),
            other => Err(Error::not_found(format!(
                "`anthracite:accounts/accounts#{other}` is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn users_and_groups_are_stable() {
        let fixture = AccountsFixture::new();
        let users = fixture.handle("users", &Value::Null).expect("users");
        assert_eq!(users[2]["name"], "operator");
        assert_eq!(users[2]["uid"], 1000);
        assert_eq!(users[1]["locked"], true);
        let groups = fixture.handle("groups", &Value::Null).expect("groups");
        assert_eq!(groups[2]["name"], "sudo");
        assert_eq!(groups[2]["members"][0], "operator");
    }
}
