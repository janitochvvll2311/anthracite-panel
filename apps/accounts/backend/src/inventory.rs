//! Local account inventory: `/etc/passwd`, `/etc/group`, `/etc/shadow`,
//! `/etc/gshadow` and the id ranges in `/etc/login.defs`.
//!
//! The service reads the files through a `HostProbe` so tests can script the
//! host without touching `/etc`. Directory accounts (`getent`/NSS) are the
//! next contract phase; this phase lists local entries only.

use std::collections::HashMap;

use anth_contract::Error;
use serde_json::{json, Value};

/// What the read path needs from the host.
#[derive(Clone, Debug, Default)]
pub struct LocalFiles {
    pub passwd: String,
    pub group: String,
    /// `None` when `/etc/shadow` cannot be read; lock state is then unknown
    /// instead of guessed.
    pub shadow: Option<String>,
    pub gshadow: Option<String>,
    pub login_defs: Option<String>,
}

pub trait HostProbe: Send + Sync {
    fn read(&self) -> LocalFiles;
}

pub struct SystemProbe;

impl HostProbe for SystemProbe {
    fn read(&self) -> LocalFiles {
        LocalFiles {
            passwd: std::fs::read_to_string("/etc/passwd").unwrap_or_default(),
            group: std::fs::read_to_string("/etc/group").unwrap_or_default(),
            shadow: std::fs::read_to_string("/etc/shadow").ok(),
            gshadow: std::fs::read_to_string("/etc/gshadow").ok(),
            login_defs: std::fs::read_to_string("/etc/login.defs").ok(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub home: String,
    pub shell: String,
    pub system: bool,
    /// `None` when `/etc/shadow` is unreadable.
    pub locked: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub gid: u32,
    pub members: Vec<String>,
    pub system: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inventory {
    pub users: Vec<User>,
    pub groups: Vec<Group>,
    pub id_ranges: IdRanges,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdRanges {
    pub uid_min: u32,
    pub gid_min: u32,
}

impl Default for IdRanges {
    fn default() -> Self {
        Self {
            uid_min: 1000,
            gid_min: 1000,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed<T> {
    pub entries: Vec<T>,
    pub skipped: usize,
}

impl<T> Default for Parsed<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            skipped: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasswdEntry {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub home: String,
    pub shell: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupEntry {
    pub name: String,
    pub gid: u32,
    pub members: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShadowEntry {
    pub name: String,
    pub locked: bool,
}

/// Reads one consistent inventory. Malformed lines are skipped, not fatal:
/// a snapshot taken while a tool rewrites a file should still list what parsed.
pub fn read(files: &LocalFiles) -> Inventory {
    let passwd = parse_passwd(&files.passwd);
    let groups = parse_group(&files.group);
    let shadow = files.shadow.as_deref().map(parse_shadow);
    let gshadow = files.gshadow.as_deref().map(parse_gshadow_members);
    let id_ranges = files
        .login_defs
        .as_deref()
        .map(parse_login_defs)
        .unwrap_or_default();

    let groups: Vec<Group> = groups
        .entries
        .into_iter()
        .map(|entry| {
            let mut members = entry.members;
            if let Some(extra) = gshadow.as_ref().and_then(|map| map.get(&entry.name)) {
                for member in extra {
                    if !members.iter().any(|existing| existing == member) {
                        members.push(member.clone());
                    }
                }
            }
            Group {
                name: entry.name,
                gid: entry.gid,
                members,
                system: entry.gid < id_ranges.gid_min,
            }
        })
        .collect();

    let users: Vec<User> = passwd
        .entries
        .into_iter()
        .map(|entry| {
            let locked = shadow.as_ref().map(|entries| {
                entries
                    .iter()
                    .find(|shadow| shadow.name == entry.name)
                    .map(|shadow| shadow.locked)
                    // A passwd entry without a shadow record has no password
                    // to match; the safe reading is "not usable for login".
                    .unwrap_or(true)
            });
            User {
                name: entry.name,
                uid: entry.uid,
                gid: entry.gid,
                home: entry.home,
                shell: entry.shell,
                system: entry.uid < id_ranges.uid_min,
                locked,
            }
        })
        .collect();

    Inventory {
        users,
        groups,
        id_ranges,
    }
}

/// The contract's user list. `locked` is not optional in 1.1.0, so an
/// unreadable shadow file is refused rather than reported as unlocked.
pub fn users_json(inventory: &Inventory) -> Result<Value, Error> {
    if inventory.users.iter().any(|user| user.locked.is_none()) {
        return Err(Error::denied(
            "cannot read `/etc/shadow`; the accounts service needs root to report the lock state",
        ));
    }
    Ok(Value::Array(
        inventory
            .users
            .iter()
            .map(|user| {
                json!({
                    "name": user.name,
                    "uid": user.uid,
                    "gid": user.gid,
                    "home": user.home,
                    "shell": user.shell,
                    "system": user.system,
                    "locked": user.locked.unwrap_or(false),
                })
            })
            .collect(),
    ))
}

pub fn groups_json(inventory: &Inventory) -> Value {
    Value::Array(
        inventory
            .groups
            .iter()
            .map(|group| {
                json!({
                    "name": group.name,
                    "gid": group.gid,
                    "members": group.members,
                })
            })
            .collect(),
    )
}

/// The name of the account running this process, used for the self-delete
/// guard. Falls back to `USER` and then `root`; it never invents permissions.
pub fn process_user(files: &LocalFiles) -> String {
    if let Some(uid) = effective_uid() {
        let passwd = parse_passwd(&files.passwd);
        if let Some(entry) = passwd.entries.iter().find(|entry| entry.uid == uid) {
            return entry.name.clone();
        }
    }
    std::env::var("USER").unwrap_or_else(|_| "root".to_string())
}

fn effective_uid() -> Option<u32> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|line| line.starts_with("Uid:"))?;
    line.split_whitespace().nth(1)?.parse().ok()
}

fn fields(line: &str) -> Vec<&str> {
    line.trim_end_matches(['\n', '\r']).split(':').collect()
}

fn is_content(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && !trimmed.starts_with('#')
}

fn split_members(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|member| !member.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_passwd(text: &str) -> Parsed<PasswdEntry> {
    let mut parsed = Parsed::default();
    for line in text.lines().filter(|line| is_content(line)) {
        let parts = fields(line);
        let entry = (|| {
            if parts.len() < 7 {
                return None;
            }
            Some(PasswdEntry {
                name: parts[0].trim().to_string(),
                uid: parts[2].trim().parse().ok()?,
                gid: parts[3].trim().parse().ok()?,
                home: parts[5].to_string(),
                shell: parts[6].to_string(),
            })
        })();
        match entry.filter(|entry: &PasswdEntry| !entry.name.is_empty()) {
            Some(entry) => parsed.entries.push(entry),
            None => parsed.skipped += 1,
        }
    }
    parsed
}

fn parse_group(text: &str) -> Parsed<GroupEntry> {
    let mut parsed = Parsed::default();
    for line in text.lines().filter(|line| is_content(line)) {
        let parts = fields(line);
        let entry = (|| {
            if parts.len() < 4 {
                return None;
            }
            Some(GroupEntry {
                name: parts[0].trim().to_string(),
                gid: parts[2].trim().parse().ok()?,
                members: split_members(parts[3]),
            })
        })();
        match entry.filter(|entry: &GroupEntry| !entry.name.is_empty()) {
            Some(entry) => parsed.entries.push(entry),
            None => parsed.skipped += 1,
        }
    }
    parsed
}

fn parse_shadow(text: &str) -> Vec<ShadowEntry> {
    text.lines()
        .filter(|line| is_content(line))
        .filter_map(|line| {
            let parts = fields(line);
            if parts.len() < 2 || parts[0].trim().is_empty() {
                return None;
            }
            Some(ShadowEntry {
                name: parts[0].trim().to_string(),
                locked: hash_is_locked(parts[1]),
            })
        })
        .collect()
}

/// `!` is a deliberate lock, `*` a password that can never match, and `!!` the
/// usual "no password has been set". All three mean no login.
fn hash_is_locked(hash: &str) -> bool {
    hash.starts_with('!') || hash.starts_with('*')
}

fn parse_gshadow_members(text: &str) -> HashMap<String, Vec<String>> {
    text.lines()
        .filter(|line| is_content(line))
        .filter_map(|line| {
            let parts = fields(line);
            if parts.len() < 4 || parts[0].trim().is_empty() {
                return None;
            }
            Some((parts[0].trim().to_string(), split_members(parts[3])))
        })
        .collect()
}

fn parse_login_defs(text: &str) -> IdRanges {
    let mut ranges = IdRanges::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let Some(value) = parts.next() else {
            continue;
        };
        match key {
            "UID_MIN" => {
                if let Ok(value) = value.parse() {
                    ranges.uid_min = value;
                }
            }
            "GID_MIN" => {
                if let Ok(value) = value.parse() {
                    ranges.gid_min = value;
                }
            }
            _ => {}
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD: &str = "\
root:x:0:0:root:/root:/bin/bash
daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin
operator:x:1000:1000:Operator:/home/operator:/bin/bash
alice:x:1001:1001:Alice:/home/alice:/bin/bash
broken line
";
    const GROUP: &str = "\
root:x:0:
daemon:x:1:
operator:x:1000:
alice:x:1001:
sudo:x:27:operator
";
    const SHADOW: &str = "\
root:!:19000:0:99999:7:::
daemon:*:19000:0:99999:7:::
operator:$6$hash:19000:0:99999:7:::
alice:!!:19000:0:99999:7:::
";
    const GSHADOW: &str = "\
sudo:!::operator,alice
";
    const LOGIN_DEFS: &str = "\
UID_MIN 1000
GID_MIN 1000
";

    fn files() -> LocalFiles {
        LocalFiles {
            passwd: PASSWD.to_string(),
            group: GROUP.to_string(),
            shadow: Some(SHADOW.to_string()),
            gshadow: Some(GSHADOW.to_string()),
            login_defs: Some(LOGIN_DEFS.to_string()),
        }
    }

    #[test]
    fn inventory_parses_users_groups_and_lock_bits() {
        let inventory = read(&files());
        assert_eq!(inventory.users.len(), 4);
        let operator = inventory
            .users
            .iter()
            .find(|user| user.name == "operator")
            .expect("operator");
        assert_eq!(operator.locked, Some(false));
        assert!(!operator.system);
        let daemon = inventory
            .users
            .iter()
            .find(|user| user.name == "daemon")
            .expect("daemon");
        assert_eq!(daemon.locked, Some(true));
        assert!(daemon.system);
        let sudo = inventory
            .groups
            .iter()
            .find(|group| group.name == "sudo")
            .expect("sudo");
        assert_eq!(sudo.members, vec!["operator", "alice"]);
    }

    #[test]
    fn malformed_lines_are_skipped() {
        let parsed = parse_passwd(PASSWD);
        assert_eq!(parsed.entries.len(), 4);
        assert_eq!(parsed.skipped, 1);
        assert_eq!(parse_shadow(SHADOW).len(), 4);
    }

    #[test]
    fn login_defs_override_the_builtin_ranges() {
        let ranges = parse_login_defs("UID_MIN 500\nGID_MIN 600\n");
        assert_eq!(ranges.uid_min, 500);
        assert_eq!(ranges.gid_min, 600);
    }

    #[test]
    fn an_unreadable_shadow_makes_users_unavailable_but_groups_work() {
        let mut files = files();
        files.shadow = None;
        let inventory = read(&files);
        assert!(users_json(&inventory).is_err());
        assert!(!groups_json(&inventory).as_array().unwrap().is_empty());
    }

    #[test]
    fn a_user_without_a_shadow_record_is_locked() {
        let mut files = files();
        files.shadow = Some("root:!:19000:0:99999:7:::\n".to_string());
        let inventory = read(&files);
        let alice = inventory
            .users
            .iter()
            .find(|user| user.name == "alice")
            .expect("alice");
        assert_eq!(alice.locked, Some(true));
    }
}
