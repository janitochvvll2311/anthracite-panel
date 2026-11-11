//! Account inventory: local `/etc` files merged with what NSS resolves.
//!
//! Local entries are what the service can change. `getent` answers what the
//! host resolves (LDAP, Active Directory, SSSD); those entries are read-only
//! and are marked with `source = nss`. A local entry always wins when both
//! sources know the same name.

use std::collections::HashMap;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// `getent` talks to NSS; a hung LDAP/SSSD backend must not hang the service.
const GETENT_TIMEOUT: Duration = Duration::from_secs(2);

/// What the read path needs from the host.
#[derive(Clone, Debug, Default)]
pub struct LocalFiles {
    pub passwd: String,
    pub group: String,
    /// `None` when `/etc/shadow` cannot be read; lock state is then unknown.
    pub shadow: Option<String>,
    pub gshadow: Option<String>,
    pub login_defs: Option<String>,
    /// Output of `getent passwd`, or `None` when the tool is missing.
    pub nss_passwd: Option<String>,
    /// Output of `getent group`, or `None` when the tool is missing.
    pub nss_group: Option<String>,
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
            nss_passwd: getent("passwd"),
            nss_group: getent("group"),
        }
    }
}

fn getent(database: &str) -> Option<String> {
    let child = Command::new("getent")
        .arg(database)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    collect_output(child, GETENT_TIMEOUT)
}

/// Waits for the child while a reader thread drains stdout, so a response
/// larger than the pipe buffer (a big LDAP directory) does not deadlock.
fn collect_output(mut child: Child, timeout: Duration) -> Option<String> {
    let mut pipe = child.stdout.take()?;
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = pipe
            .read_to_end(&mut bytes)
            .map(|_| String::from_utf8_lossy(&bytes).into_owned());
        let _ = send.send(result);
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let remaining = timeout
                    .saturating_sub(started.elapsed())
                    .max(Duration::from_millis(50));
                return receive.recv_timeout(remaining).ok()?.ok();
            }
            Ok(None) => {}
            Err(_) => return None,
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Local,
    Nss,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Nss => "nss",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub group: String,
    pub gecos: String,
    pub home: String,
    pub shell: String,
    pub system: bool,
    /// `None` when the lock state cannot be read (NSS entries, or an
    /// unreadable `/etc/shadow`).
    pub locked: Option<bool>,
    pub source: Source,
    pub groups: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub name: String,
    pub gid: u32,
    pub members: Vec<String>,
    pub system: bool,
    pub source: Source,
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
    pub gecos: String,
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

/// Reads one consistent inventory. Malformed lines are skipped, not fatal.
pub fn read(files: &LocalFiles) -> Inventory {
    let passwd = parse_passwd(&files.passwd);
    let local_groups = parse_group(&files.group);
    let shadow = files.shadow.as_deref().map(parse_shadow);
    let gshadow = files.gshadow.as_deref().map(parse_gshadow_members);
    let id_ranges = files
        .login_defs
        .as_deref()
        .map(parse_login_defs)
        .unwrap_or_default();

    let mut groups: Vec<Group> = local_groups
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
                source: Source::Local,
            }
        })
        .collect();

    if let Some(nss_group) = files.nss_group.as_deref() {
        for entry in parse_group(nss_group).entries {
            if groups.iter().any(|group| group.name == entry.name) {
                continue;
            }
            groups.push(Group {
                name: entry.name,
                gid: entry.gid,
                members: entry.members,
                system: entry.gid < id_ranges.gid_min,
                source: Source::Nss,
            });
        }
    }

    let shadow_map: Option<HashMap<String, bool>> = shadow.map(|entries| {
        entries
            .into_iter()
            .map(|entry| (entry.name, entry.locked))
            .collect()
    });

    let mut users: Vec<User> = passwd
        .entries
        .into_iter()
        .map(|entry| {
            let locked = shadow_map.as_ref().map(|entries| {
                // A passwd entry without a shadow record has no password to
                // match; the safe reading is "not usable for login".
                entries.get(&entry.name).copied().unwrap_or(true)
            });
            User {
                name: entry.name,
                uid: entry.uid,
                gid: entry.gid,
                group: String::new(),
                gecos: entry.gecos,
                home: entry.home,
                shell: entry.shell,
                system: entry.uid < id_ranges.uid_min,
                locked,
                source: Source::Local,
                groups: Vec::new(),
            }
        })
        .collect();

    if let Some(nss_passwd) = files.nss_passwd.as_deref() {
        for entry in parse_passwd(nss_passwd).entries {
            if users.iter().any(|user| user.name == entry.name) {
                continue;
            }
            users.push(User {
                name: entry.name,
                uid: entry.uid,
                gid: entry.gid,
                group: String::new(),
                gecos: entry.gecos,
                home: entry.home,
                shell: entry.shell,
                system: entry.uid < id_ranges.uid_min,
                locked: None,
                source: Source::Nss,
                groups: Vec::new(),
            });
        }
    }

    for user in &mut users {
        user.group = groups
            .iter()
            .find(|group| group.gid == user.gid)
            .map(|group| group.name.clone())
            .unwrap_or_else(|| user.gid.to_string());
        user.groups = groups
            .iter()
            .filter(|group| group.members.iter().any(|member| member == &user.name))
            .map(|group| group.name.clone())
            .collect();
    }

    Inventory {
        users,
        groups,
        id_ranges,
    }
}

pub fn users_json(inventory: &Inventory) -> Value {
    Value::Array(
        inventory
            .users
            .iter()
            .map(|user| {
                json!({
                    "name": user.name,
                    "uid": user.uid,
                    "gid": user.gid,
                    "group": user.group,
                    "gecos": user.gecos,
                    "home": user.home,
                    "shell": user.shell,
                    "system": user.system,
                    "locked": user.locked,
                    "source": user.source.as_str(),
                    "groups": user.groups,
                })
            })
            .collect(),
    )
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
                    "system": group.system,
                    "source": group.source.as_str(),
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
                gecos: parts[4].to_string(),
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
            nss_passwd: None,
            nss_group: None,
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
        assert_eq!(operator.group, "operator");
        assert_eq!(operator.groups, vec!["sudo"]);
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
    fn an_unreadable_shadow_reports_unknown_lock_but_groups_work() {
        let mut files = files();
        files.shadow = None;
        let inventory = read(&files);
        assert!(users_json(&inventory)[0]["locked"].is_null());
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

    #[test]
    fn local_entries_win_and_nss_entries_are_read_only() {
        let mut files = files();
        files.nss_passwd = Some(
            "operator:x:1000:1000:Operator:/home/operator:/bin/bash\nana:x:2000:2000:Ana:/home/ana:/bin/bash\n"
                .to_string(),
        );
        files.nss_group = Some("sudo:x:27:operator\nldap:x:2000:ana\n".to_string());
        let inventory = read(&files);
        let operator = inventory
            .users
            .iter()
            .find(|user| user.name == "operator")
            .expect("operator");
        assert_eq!(operator.source, Source::Local);
        let ana = inventory
            .users
            .iter()
            .find(|user| user.name == "ana")
            .expect("ana");
        assert_eq!(ana.source, Source::Nss);
        assert_eq!(ana.locked, None);
        assert_eq!(ana.group, "ldap");
        let ldap = inventory
            .groups
            .iter()
            .find(|group| group.name == "ldap")
            .expect("ldap");
        assert_eq!(ldap.source, Source::Nss);
    }

    fn shell(script: &str) -> Child {
        Command::new("sh")
            .arg("-c")
            .arg(script)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn the fixture command")
    }

    #[test]
    fn getent_output_larger_than_the_pipe_buffer_is_drained() {
        let child = shell("head -c 131072 /dev/zero | tr '\\0' 'a'");
        let output = collect_output(child, Duration::from_secs(5)).expect("output");
        assert_eq!(output.len(), 131_072);
    }

    #[test]
    fn getent_output_is_decoded_lossily() {
        let child = shell("printf '\\377ok'");
        let output = collect_output(child, Duration::from_secs(5)).expect("output");
        assert!(output.ends_with("ok"), "{output:?}");
    }
}
