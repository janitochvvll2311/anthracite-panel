//! Account inventory: local files merged with what NSS resolves.
//!
//! Two sources answer two different questions. `/etc/passwd` and `/etc/group`
//! are what the panel can change; `getent` is what the host actually resolves,
//! which on a domain-joined machine is strictly more. Every entry says where
//! it came from, because a local user must never silently shadow a directory
//! account.
//!
//! Nothing here reads or returns a password hash. The shadow file is read for
//! exactly one derived fact — whether the account is locked — and the hash is
//! dropped immediately.

use std::collections::{HashMap, HashSet};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anth_wire::{AccountGroup, AccountSnapshot, AccountSource, AccountUser};

use super::AccountsSettings;

/// Fallback when `/etc/login.defs` is unreadable or silent. Matches the Debian,
/// Red Hat and Alpine default.
const DEFAULT_UID_MIN: u32 = 1000;
const DEFAULT_GID_MIN: u32 = 1000;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PasswdEntry {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub gecos: String,
    pub home: String,
    pub shell: String,
    /// The password field, which is `x` when the hash lives in `/etc/shadow`.
    pub password_field: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GroupEntry {
    pub name: String,
    pub gid: u32,
    pub members: Vec<String>,
}

/// One `shadow` line, reduced to the only fact the panel exposes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ShadowEntry {
    pub name: String,
    pub locked: bool,
}

/// Parsed contents plus how many lines could not be understood.
///
/// A malformed line is skipped and counted rather than failing the request:
/// `/etc/passwd` is rewritten in place by some tools, and a snapshot taken
/// mid-write should still list the entries that are intact.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Parsed<T> {
    pub entries: Vec<T>,
    pub skipped: usize,
}

/// Everything the read path needs from the host, behind a trait so tests can
/// script it without touching `/etc`.
#[derive(Debug, Clone, Default)]
pub struct LocalFiles {
    pub passwd: String,
    pub group: String,
    /// `None` when unreadable, which is the normal case for an unprivileged
    /// daemon. Lock state then degrades to "unknown".
    pub shadow: Option<String>,
    pub gshadow: Option<String>,
    pub login_defs: Option<String>,
    /// Output of `getent passwd` and `getent group`. `None` when the program is
    /// missing, which is the case on a BusyBox-only system.
    pub nss_passwd: Option<String>,
    pub nss_group: Option<String>,
}

pub trait HostProbe: Send + Sync {
    fn read(&self) -> LocalFiles;
}

pub struct SystemProbe;

impl Default for SystemProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemProbe {
    pub fn new() -> Self {
        Self
    }

    fn getent(&self, database: &str) -> Option<String> {
        let output = Command::new("getent").arg(database).output().ok()?;
        output
            .status
            .success()
            .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

impl HostProbe for SystemProbe {
    fn read(&self) -> LocalFiles {
        LocalFiles {
            passwd: std::fs::read_to_string("/etc/passwd").unwrap_or_default(),
            group: std::fs::read_to_string("/etc/group").unwrap_or_default(),
            shadow: std::fs::read_to_string("/etc/shadow").ok(),
            gshadow: std::fs::read_to_string("/etc/gshadow").ok(),
            login_defs: std::fs::read_to_string("/etc/login.defs").ok(),
            nss_passwd: self.getent("passwd"),
            nss_group: self.getent("group"),
        }
    }
}

/// Splits a colon-separated line, trimming the trailing newline only.
fn fields(line: &str) -> Vec<&str> {
    line.trim_end_matches(['\n', '\r']).split(':').collect()
}

/// Skips blank and comment lines, which are legal in `passwd`-format files.
fn is_content(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty() && !trimmed.starts_with('#')
}

fn parse_passwd_entries(text: &str) -> Parsed<PasswdEntry> {
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
                password_field: parts[1].to_string(),
            })
        })();

        match entry.filter(|entry: &PasswdEntry| !entry.name.is_empty()) {
            Some(entry) => parsed.entries.push(entry),
            None => parsed.skipped += 1,
        }
    }
    parsed
}

fn parse_group_entries(text: &str) -> Parsed<GroupEntry> {
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

fn split_members(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|member| !member.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_shadow_entries(text: &str) -> Parsed<ShadowEntry> {
    let mut parsed = Parsed::default();
    for line in text.lines().filter(|line| is_content(line)) {
        let parts = fields(line);
        if parts.len() < 2 || parts[0].trim().is_empty() {
            parsed.skipped += 1;
            continue;
        }
        parsed.entries.push(ShadowEntry {
            name: parts[0].trim().to_string(),
            locked: hash_is_locked(parts[1]),
        });
    }
    parsed
}

/// A shadow hash is locked when it is prefixed. `!` marks a deliberately
/// locked account, `*` a password that can never match, and `!!` the usual
/// "no password has ever been set", which is how every `useradd` creates a
/// user. All three mean the same thing to a caller: no login.
fn hash_is_locked(hash: &str) -> bool {
    hash.starts_with('!') || hash.starts_with('*')
}

/// `/etc/gshadow` lists members that `/etc/group` sometimes omits. Only the
/// member list is used; the password column is ignored.
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

/// The id ranges from `/etc/login.defs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdRanges {
    pub uid_min: u32,
    pub gid_min: u32,
}

impl Default for IdRanges {
    fn default() -> Self {
        Self {
            uid_min: DEFAULT_UID_MIN,
            gid_min: DEFAULT_GID_MIN,
        }
    }
}

pub fn read_login_defs(text: &str) -> IdRanges {
    let mut ranges = IdRanges::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(key), Some(value)) = (parts.next(), parts.next()) else {
            continue;
        };
        match key {
            "UID_MIN" => {
                if let Ok(parsed) = value.parse() {
                    ranges.uid_min = parsed;
                }
            }
            "GID_MIN" => {
                if let Ok(parsed) = value.parse() {
                    ranges.gid_min = parsed;
                }
            }
            _ => {}
        }
    }
    ranges
}

/// The full inventory plus the ranges from the same read. Mutations validate
/// against this, so a stale or shortened view can never hide an entry from a
/// guard.
pub fn collect(
    config: &AccountsSettings,
    probe: &dyn HostProbe,
) -> (AccountSnapshot, Vec<String>, IdRanges) {
    let files = probe.read();
    let mut warnings = Vec::new();

    let ranges = files
        .login_defs
        .as_deref()
        .map(read_login_defs)
        .unwrap_or_default();

    let local_passwd = parse_passwd_entries(&files.passwd);
    let local_group = parse_group_entries(&files.group);
    let mut group_names: HashMap<u32, String> = local_group
        .entries
        .iter()
        .map(|group| (group.gid, group.name.clone()))
        .collect();

    for reason in [
        skipped_warning("passwd", local_passwd.skipped),
        skipped_warning("group", local_group.skipped),
    ]
    .into_iter()
    .flatten()
    {
        warnings.push(reason);
    }

    let shadow_locked: HashMap<String, bool> = match &files.shadow {
        Some(text) => {
            let parsed = parse_shadow_entries(text);
            if let Some(warning) = skipped_warning("shadow", parsed.skipped) {
                warnings.push(warning);
            }
            parsed
                .entries
                .into_iter()
                .map(|entry| (entry.name, entry.locked))
                .collect()
        }
        None => {
            warnings.push(
                "lock state is unknown: /etc/shadow is not readable, so run the daemon as root to see it"
                    .to_string(),
            );
            HashMap::new()
        }
    };

    // Supplementary membership comes from both files: `/etc/group` is the
    // canonical list, but gshadow can carry entries group does not.
    let mut members: HashMap<String, Vec<String>> = local_group
        .entries
        .iter()
        .map(|group| (group.name.clone(), group.members.clone()))
        .collect();
    if let Some(text) = &files.gshadow {
        for (name, listed) in parse_gshadow_members(text) {
            members.entry(name).or_insert(listed);
        }
    }

    let merged = merge_users(
        local_passwd.entries,
        files.nss_passwd.as_deref(),
        &mut warnings,
    );
    let mut users: Vec<AccountUser> = merged
        .into_iter()
        .map(|(entry, source)| AccountUser {
            uid: entry.uid,
            gid: entry.gid,
            group: group_names
                .get(&entry.gid)
                .cloned()
                .unwrap_or_else(|| entry.gid.to_string()),
            gecos: entry.gecos.clone(),
            home: entry.home.clone(),
            shell: entry.shell.clone(),
            locked: lock_state(&entry, source, &shadow_locked),
            system: entry.uid < ranges.uid_min,
            source,
            groups: supplementary_groups(&entry.name, &members),
            name: entry.name,
        })
        .collect();

    let merged_groups = merge_groups(
        local_group.entries,
        files.nss_group.as_deref(),
        &mut warnings,
    );
    let mut groups: Vec<AccountGroup> = merged_groups
        .into_iter()
        .map(|(entry, source)| {
            // Keep a group name resolvable for the primary-group column of a
            // user that only exists in NSS.
            group_names
                .entry(entry.gid)
                .or_insert_with(|| entry.name.clone());
            AccountGroup {
                system: entry.gid < ranges.gid_min,
                members: members.get(&entry.name).cloned().unwrap_or_default(),
                name: entry.name,
                gid: entry.gid,
                source,
            }
        })
        .collect();

    users.sort_by(|left, right| left.uid.cmp(&right.uid).then(left.name.cmp(&right.name)));
    groups.sort_by(|left, right| left.gid.cmp(&right.gid).then(left.name.cmp(&right.name)));

    let snapshot = AccountSnapshot {
        users,
        groups,
        control_enabled: config.control,
        password_enabled: config.manage_passwords,
        allow_system: config.allow_system,
        allow_privileged_groups: config.allow_privileged_groups,
        timestamp_ms: now_ms(),
        warning: (!warnings.is_empty()).then(|| warnings.join("; ")),
    };
    (snapshot, warnings, ranges)
}

fn skipped_warning(file: &str, skipped: usize) -> Option<String> {
    (skipped > 0).then(|| format!("skipped {skipped} malformed line(s) in {file}"))
}

/// Local entries win, and NSS only adds what the files do not already have.
///
/// Deduplicating by name rather than by id matters: a local `alice` with uid
/// 1001 must not be listed twice when NSS also resolves a directory `alice`.
fn merge_users(
    local: Vec<PasswdEntry>,
    nss: Option<&str>,
    warnings: &mut Vec<String>,
) -> Vec<(PasswdEntry, AccountSource)> {
    let mut merged: Vec<(PasswdEntry, AccountSource)> = local
        .into_iter()
        .map(|entry| (entry, AccountSource::Local))
        .collect();
    let Some(nss) = nss else {
        warnings.push(
            "directory accounts are not listed: `getent` is not available (install musl-utils or glibc's getent)"
                .to_string(),
        );
        return merged;
    };

    let parsed = parse_passwd_entries(nss);
    if let Some(warning) = skipped_warning("getent passwd", parsed.skipped) {
        warnings.push(warning);
    }
    let known: HashSet<String> = merged.iter().map(|(entry, _)| entry.name.clone()).collect();
    for entry in parsed.entries {
        if !known.contains(&entry.name) {
            merged.push((entry, AccountSource::Nss));
        }
    }
    merged
}

fn merge_groups(
    local: Vec<GroupEntry>,
    nss: Option<&str>,
    warnings: &mut Vec<String>,
) -> Vec<(GroupEntry, AccountSource)> {
    let mut merged: Vec<(GroupEntry, AccountSource)> = local
        .into_iter()
        .map(|entry| (entry, AccountSource::Local))
        .collect();
    let Some(nss) = nss else {
        return merged;
    };

    let parsed = parse_group_entries(nss);
    if let Some(warning) = skipped_warning("getent group", parsed.skipped) {
        warnings.push(warning);
    }
    let known: HashSet<String> = merged.iter().map(|(entry, _)| entry.name.clone()).collect();
    for entry in parsed.entries {
        if !known.contains(&entry.name) {
            merged.push((entry, AccountSource::Nss));
        }
    }
    merged
}

/// Resolves the lock state, or `None` when it genuinely cannot be known.
fn lock_state(
    entry: &PasswdEntry,
    source: AccountSource,
    shadow: &HashMap<String, bool>,
) -> Option<bool> {
    if let Some(locked) = shadow.get(&entry.name) {
        return Some(*locked);
    }
    // An NSS account has no shadow entry the daemon can read. Reporting
    // "unlocked" would be a guess, and the wrong guess is the dangerous one.
    if source == AccountSource::Nss {
        return None;
    }
    match entry.password_field.as_str() {
        // `x` means "look in shadow", and shadow was unreadable.
        "x" | "" => None,
        field if hash_is_locked(field) => Some(true),
        // No password at all is not locked, but it is also not safe; the
        // inventory reports what is there rather than what it wishes.
        _ => Some(false),
    }
}

fn supplementary_groups(name: &str, members: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut groups: Vec<String> = members
        .iter()
        .filter(|(_, listed)| listed.iter().any(|member| member == name))
        .map(|(group, _)| group.clone())
        .collect();
    groups.sort();
    groups.dedup();
    groups
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PASSWD: &str = "\
root:x:0:0:root:/root:/bin/bash
alice:x:1001:1001:Alice Example:/home/alice:/bin/bash
bob:x:1002:1001::/home/bob:/usr/sbin/nologin
";

    const GROUP: &str = "\
root:x:0:
wheel:x:10:alice,carol
alice:x:1001:
shared:x:1002:alice,bob
";

    const SHADOW: &str = "\
root:!:19000:0:99999:7:::
alice:$6$saltsalt$hashhash:19000:0:99999:7:::
bob:!!:19000:0:99999:7:::
";

    const GSHADOW: &str = "\
wheel:!::alice,carol
alice:!::alice
";

    fn files() -> LocalFiles {
        LocalFiles {
            passwd: PASSWD.to_string(),
            group: GROUP.to_string(),
            shadow: Some(SHADOW.to_string()),
            gshadow: Some(GSHADOW.to_string()),
            login_defs: Some("UID_MIN 1000\nGID_MIN 1000\n".to_string()),
            nss_passwd: None,
            nss_group: None,
        }
    }

    struct FakeProbe(LocalFiles);

    impl HostProbe for FakeProbe {
        fn read(&self) -> LocalFiles {
            self.0.clone()
        }
    }

    fn policy() -> AccountsSettings {
        AccountsSettings {
            control: true,
            manage_passwords: true,
            ..AccountsSettings::default()
        }
    }

    fn snapshot_with(files: LocalFiles) -> AccountSnapshot {
        collect(&policy(), &FakeProbe(files)).0
    }

    fn user<'a>(snapshot: &'a AccountSnapshot, name: &str) -> &'a AccountUser {
        snapshot
            .users
            .iter()
            .find(|user| user.name == name)
            .unwrap_or_else(|| panic!("no user {name}"))
    }

    #[test]
    fn passwd_lines_are_parsed() {
        let parsed = parse_passwd_entries(PASSWD);
        assert_eq!(parsed.entries.len(), 3);
        assert_eq!(parsed.skipped, 0);
        assert_eq!(parsed.entries[1].name, "alice");
        assert_eq!(parsed.entries[1].uid, 1001);
        assert_eq!(parsed.entries[1].gecos, "Alice Example");
        assert_eq!(parsed.entries[1].shell, "/bin/bash");
    }

    #[test]
    fn a_malformed_line_is_skipped_and_counted() {
        let parsed =
            parse_passwd_entries("alice:x:1001:1001::/home/alice:/bin/sh\nbroken\n\n# comment\n");
        assert_eq!(parsed.entries.len(), 1);
        assert_eq!(parsed.skipped, 1);
    }

    #[test]
    fn a_non_numeric_id_is_malformed() {
        let parsed = parse_passwd_entries("alice:x:abc:1001::/home/alice:/bin/sh\n");
        assert!(parsed.entries.is_empty());
        assert_eq!(parsed.skipped, 1);
    }

    #[test]
    fn group_members_are_split_and_trimmed() {
        let parsed = parse_group_entries(GROUP);
        let wheel = parsed
            .entries
            .iter()
            .find(|group| group.name == "wheel")
            .unwrap();
        assert_eq!(wheel.gid, 10);
        assert_eq!(wheel.members, vec!["alice", "carol"]);
    }

    #[test]
    fn empty_group_member_list_is_empty() {
        let parsed = parse_group_entries("nogroup:x:65534:\n");
        assert!(parsed.entries[0].members.is_empty());
    }

    #[test]
    fn lock_state_comes_from_the_shadow_prefix() {
        let parsed = parse_shadow_entries(SHADOW);
        let states: HashMap<String, bool> = parsed
            .entries
            .into_iter()
            .map(|entry| (entry.name, entry.locked))
            .collect();
        assert!(states["root"]);
        assert!(!states["alice"]);
        assert!(states["bob"]);
    }

    #[test]
    fn a_single_bang_and_a_star_are_both_locked() {
        assert!(hash_is_locked("!$6$hash"));
        assert!(hash_is_locked("*"));
        assert!(hash_is_locked("!!"));
        assert!(!hash_is_locked("$6$hash"));
        assert!(!hash_is_locked(""));
    }

    #[test]
    fn login_defs_ranges_are_read_and_default_when_missing() {
        let ranges = read_login_defs("# comment\nUID_MIN  5000\nGID_MIN 6000\n");
        assert_eq!(ranges.uid_min, 5000);
        assert_eq!(ranges.gid_min, 6000);

        let fallback = read_login_defs("");
        assert_eq!(fallback, IdRanges::default());
        assert_eq!(fallback.uid_min, DEFAULT_UID_MIN);
    }

    #[test]
    fn the_snapshot_merges_users_groups_and_membership() {
        let snapshot = snapshot_with(files());

        assert_eq!(snapshot.users.len(), 3);
        let alice = user(&snapshot, "alice");
        assert_eq!(alice.uid, 1001);
        assert_eq!(alice.group, "alice");
        assert_eq!(alice.locked, Some(false));
        assert!(!alice.system);
        assert_eq!(alice.source, AccountSource::Local);
        // From /etc/group plus gshadow's own list.
        assert_eq!(alice.groups, vec!["shared", "wheel"]);
    }

    #[test]
    fn a_primary_group_name_is_resolved_and_falls_back_to_the_id() {
        let mut files = files();
        files.group = "root:x:0:\n".to_string();
        files.gshadow = None;
        let snapshot = snapshot_with(files);

        assert_eq!(user(&snapshot, "root").group, "root");
        // 1001 is not in the group file any more, so the number is all we have.
        assert_eq!(user(&snapshot, "alice").group, "1001");
    }

    #[test]
    fn users_are_sorted_by_uid() {
        let snapshot = snapshot_with(files());
        let uids: Vec<u32> = snapshot.users.iter().map(|user| user.uid).collect();
        assert_eq!(uids, vec![0, 1001, 1002]);
    }

    #[test]
    fn system_accounts_are_flagged_from_the_range() {
        let snapshot = snapshot_with(files());
        assert!(user(&snapshot, "root").system);
        assert!(!user(&snapshot, "alice").system);

        let mut files = files();
        files.login_defs = Some("UID_MIN 1500\n".to_string());
        let snapshot = snapshot_with(files);
        assert!(user(&snapshot, "alice").system);
    }

    #[test]
    fn an_unreadable_shadow_degrades_to_unknown_with_a_warning() {
        let mut files = files();
        files.shadow = None;
        let (snapshot, warnings, _) = collect(&policy(), &FakeProbe(files));

        assert_eq!(user(&snapshot, "alice").locked, None);
        assert!(warnings.iter().any(|warning| warning.contains("shadow")));
        assert!(snapshot.warning.is_some());
    }

    #[test]
    fn nss_entries_are_marked_read_only_and_never_shadow_local_ones() {
        let mut files = files();
        files.nss_passwd = Some(
            "alice:x:1001:1001::/home/alice:/bin/bash\ndora:x:2001:2001::/home/dora:/bin/sh\n"
                .to_string(),
        );
        files.nss_group = Some("alice:x:1001:\ndora:x:2001:\n".to_string());
        let snapshot = snapshot_with(files);

        assert_eq!(
            snapshot
                .users
                .iter()
                .filter(|user| user.name == "alice")
                .count(),
            1
        );
        assert_eq!(user(&snapshot, "alice").source, AccountSource::Local);
        assert_eq!(user(&snapshot, "dora").source, AccountSource::Nss);
        assert_eq!(user(&snapshot, "dora").locked, None);
    }

    #[test]
    fn a_missing_getent_degrades_to_local_only_with_a_warning() {
        let mut files = files();
        files.nss_passwd = None;
        files.nss_group = None;
        let (snapshot, warnings, _) = collect(&policy(), &FakeProbe(files));

        assert_eq!(snapshot.users.len(), 3);
        assert!(warnings.iter().any(|warning| warning.contains("getent")));
    }
}
