//! Account inventory: local files merged with what NSS resolves.
//!
//! Two sources answer two different questions. `/etc/passwd` and `/etc/group`
//! are what the panel can change; `getent` is what the host actually resolves,
//! which on a domain-joined machine is strictly more. Every entry says where it
//! came from, because a local user must never silently shadow a directory
//! account.
//!
//! Nothing here reads or returns a password hash. The shadow file is read for
//! exactly one derived fact — whether the account is locked — and the hash is
//! dropped immediately.
//!
//! Mutations validate against the unfitted inventory, so truncation can never
//! hide an entry from a guard.

use std::collections::{BTreeMap, HashMap};
use std::time::{SystemTime, UNIX_EPOCH};

use anth_proto::{AccountGroup, AccountSnapshot, AccountSource, AccountUser, MAX_LINE_LEN};

use crate::command::{CommandRunner, SystemRunner};

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

pub trait HostProbe {
    fn read(&self) -> LocalFiles;
}

pub struct SystemProbe {
    runner: SystemRunner,
}

impl Default for SystemProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemProbe {
    pub fn new() -> Self {
        Self {
            runner: SystemRunner::new(false),
        }
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
            // `getent` is never run through sudo: it reads the same NSS
            // configuration the daemon already uses for its own lookups.
            nss_passwd: self.getent("passwd"),
            nss_group: self.getent("group"),
        }
    }
}

impl SystemProbe {
    fn getent(&self, database: &str) -> Option<String> {
        match self.runner.run("getent", &[database]) {
            Ok(output) if output.ok => Some(output.stdout),
            _ => None,
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
                members: parts[3]
                    .split(',')
                    .map(str::trim)
                    .filter(|member| !member.is_empty())
                    .map(str::to_string)
                    .collect(),
            })
        })();

        match entry.filter(|entry: &GroupEntry| !entry.name.is_empty()) {
            Some(entry) => parsed.entries.push(entry),
            None => parsed.skipped += 1,
        }
    }
    parsed
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
            Some((
                parts[0].trim().to_string(),
                parts[3]
                    .split(',')
                    .map(str::trim)
                    .filter(|member| !member.is_empty())
                    .map(str::to_string)
                    .collect(),
            ))
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

/// One point-in-time snapshot, with the line cap applied.
pub fn collect(control_enabled: bool, password_enabled: bool) -> AccountSnapshot {
    collect_with(control_enabled, password_enabled, &SystemProbe::new())
}

pub fn collect_with(
    control_enabled: bool,
    password_enabled: bool,
    probe: &dyn HostProbe,
) -> AccountSnapshot {
    let (mut snapshot, warnings) = collect_unfitted(control_enabled, password_enabled, probe);

    snapshot = fit_snapshot(snapshot);
    if !warnings.is_empty() {
        let joined = warnings.join("; ");
        snapshot.warning = Some(match snapshot.warning {
            Some(existing) => format!("{existing}; {joined}"),
            None => joined,
        });
    }
    snapshot
}

/// The full inventory, before the line cap shortens anything.
///
/// Mutations validate against this: a guard that read the fitted snapshot would
/// refuse to act on a user that truncation dropped, and a large host is exactly
/// where the feature is needed. `volumes::collect_unfitted` follows the same
/// rule.
pub fn collect_unfitted(
    control_enabled: bool,
    password_enabled: bool,
    probe: &dyn HostProbe,
) -> (AccountSnapshot, Vec<String>) {
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
        total_users: users.len(),
        total_groups: groups.len(),
        users,
        groups,
        control_enabled,
        password_enabled,
        timestamp_ms: now_ms(),
        truncated: false,
        warning: None,
    };
    (snapshot, warnings)
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
    let known: std::collections::HashSet<String> =
        merged.iter().map(|(entry, _)| entry.name.clone()).collect();
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
    let known: std::collections::HashSet<String> =
        merged.iter().map(|(entry, _)| entry.name.clone()).collect();
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

/// Length of the framed response, envelope included: that is the string the
/// socket has to fit into one line.
fn snapshot_json_len(snapshot: &AccountSnapshot) -> usize {
    anth_proto::Response::AccountSnapshot(snapshot.clone())
        .to_json()
        .map(|json| json.len())
        .unwrap_or(usize::MAX)
}

/// Shortens the snapshot until one line fits `MAX_LINE_LEN`, users first
/// because a group is cheaper and still useful without them.
fn fit_snapshot(mut snapshot: AccountSnapshot) -> AccountSnapshot {
    if snapshot_json_len(&snapshot) <= MAX_LINE_LEN {
        return snapshot;
    }

    let users_total = snapshot.users.len();
    let groups_total = snapshot.groups.len();

    let base = snapshot.clone();
    let keep_users = largest_prefix(users_total, |count| {
        let mut candidate = base.clone();
        candidate.users.truncate(count);
        candidate.warning = Some(users_warning(count, users_total));
        snapshot_json_len(&candidate) <= MAX_LINE_LEN
    });

    snapshot.users.truncate(keep_users);
    snapshot.truncated = true;
    snapshot.warning = Some(users_warning(keep_users, users_total));

    if snapshot_json_len(&snapshot) > MAX_LINE_LEN {
        let base = snapshot.clone();
        let keep_groups = largest_prefix(groups_total, |count| {
            let mut candidate = base.clone();
            candidate.groups.truncate(count);
            candidate.warning = Some(format!(
                "{}; {count} of {groups_total} groups shown",
                users_warning(keep_users, users_total)
            ));
            snapshot_json_len(&candidate) <= MAX_LINE_LEN
        });
        snapshot.groups.truncate(keep_groups);
        snapshot.warning = Some(format!(
            "{}; {keep_groups} of {groups_total} groups shown",
            users_warning(keep_users, users_total)
        ));
    }

    snapshot
}

fn largest_prefix(len: usize, fits: impl Fn(usize) -> bool) -> usize {
    let mut low = 0usize;
    let mut high = len;
    while low < high {
        let mid = (low + high).div_ceil(2);
        if fits(mid) {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    low
}

fn users_warning(kept: usize, total: usize) -> String {
    format!(
        "showing {kept} of {total} users; the snapshot was shortened to fit the protocol line cap"
    )
}

/// Exposed for the integration tests, which cannot reach a private helper.
#[doc(hidden)]
pub fn fit_snapshot_for_test(snapshot: AccountSnapshot) -> AccountSnapshot {
    fit_snapshot(snapshot)
}

/// Groups by gid then name, for a renderer that wants a stable order.
pub fn group_index(groups: &[AccountGroup]) -> BTreeMap<u32, String> {
    groups
        .iter()
        .map(|group| (group.gid, group.name.clone()))
        .collect()
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

    fn snapshot_with(files: LocalFiles) -> AccountSnapshot {
        let (snapshot, _) = collect_unfitted(true, true, &FakeProbe(files));
        snapshot
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

        // A higher floor moves a regular account into the system range: the
        // flag follows the host's own `login.defs`, not a hardcoded 1000.
        let mut files = files();
        files.login_defs = Some("UID_MIN 1500\n".to_string());
        let snapshot = snapshot_with(files);
        assert!(user(&snapshot, "alice").system);
        assert!(!user(&snapshot, "root").system || user(&snapshot, "root").uid == 0);
    }

    #[test]
    fn an_unreadable_shadow_degrades_to_unknown_with_a_warning() {
        let mut files = files();
        files.shadow = None;
        let (snapshot, warnings) = collect_unfitted(true, true, &FakeProbe(files));

        assert_eq!(user(&snapshot, "alice").locked, None);
        assert!(
            warnings.iter().any(|warning| warning.contains("shadow")),
            "{warnings:?}"
        );
    }

    #[test]
    fn a_password_field_outside_shadow_still_reports_lock_state() {
        let mut files = files();
        files.shadow = None;
        files.passwd =
            "root:x:0:0:root:/root:/bin/bash\nlocked:!$6$x:1001:1001::/home/locked:/bin/sh\n"
                .to_string();
        let snapshot = snapshot_with(files);

        assert_eq!(
            user(&snapshot, "root").locked,
            None,
            "x means look in shadow"
        );
        assert_eq!(user(&snapshot, "locked").locked, Some(true));
    }

    #[test]
    fn nss_only_accounts_are_listed_as_nss_and_read_only() {
        let mut files = files();
        files.nss_passwd = Some(format!(
            "{PASSWD}domain:x:2001:2001:Domain User:/home/domain:/bin/sh\n"
        ));
        files.nss_group = Some(format!("{GROUP}domain:x:2001:\n"));
        let snapshot = snapshot_with(files);

        let domain = user(&snapshot, "domain");
        assert_eq!(domain.source, AccountSource::Nss);
        assert_eq!(domain.uid, 2001);
        // No shadow entry and not local: the state is genuinely unknown.
        assert_eq!(domain.locked, None);
    }

    #[test]
    fn a_local_user_is_not_duplicated_when_nss_also_resolves_it() {
        let mut files = files();
        // getent resolves the local account too, as it does in real life.
        files.nss_passwd = Some(PASSWD.to_string());
        files.nss_group = Some(GROUP.to_string());
        let snapshot = snapshot_with(files);

        assert_eq!(snapshot.users.len(), 3);
        assert_eq!(user(&snapshot, "alice").source, AccountSource::Local);
    }

    #[test]
    fn a_missing_getent_warns_and_still_lists_local_accounts() {
        let (snapshot, warnings) = collect_unfitted(true, true, &FakeProbe(files()));
        assert!(!snapshot.users.is_empty());
        assert!(
            warnings.iter().any(|warning| warning.contains("getent")),
            "{warnings:?}"
        );
    }

    #[test]
    fn an_empty_host_does_not_panic() {
        let snapshot = snapshot_with(LocalFiles::default());
        assert!(snapshot.users.is_empty());
        assert!(snapshot.groups.is_empty());
    }

    #[test]
    fn control_and_password_flags_are_reported_separately() {
        let (snapshot, _) = collect_unfitted(false, true, &FakeProbe(files()));
        assert!(!snapshot.control_enabled);
        assert!(snapshot.password_enabled);
    }

    #[test]
    fn a_snapshot_over_the_cap_is_shortened_and_keeps_a_prefix() {
        let mut users = String::new();
        for index in 0..8000 {
            users.push_str(&format!(
                "user{index}:x:{}:1000:Some Name {index}:/home/user{index}:/bin/sh\n",
                10_000 + index
            ));
        }
        let mut files = files();
        files.passwd = users;
        files.group = "shared:x:1000:\n".to_string();
        files.shadow = None;
        files.gshadow = None;

        let snapshot = fit_snapshot(snapshot_with(files));

        assert!(snapshot.truncated);
        assert_eq!(snapshot.total_users, 8000);
        assert!(snapshot.users.len() < 8000);
        assert!(snapshot.warning.is_some());
        assert!(
            serde_json::to_string(&anth_proto::Response::AccountSnapshot(snapshot.clone()))
                .unwrap()
                .len()
                <= MAX_LINE_LEN
        );
    }

    #[test]
    fn a_snapshot_that_fits_is_untouched() {
        let snapshot = fit_snapshot(snapshot_with(files()));
        assert!(!snapshot.truncated);
        assert_eq!(snapshot.total_users, 3);
        assert_eq!(snapshot.users.len(), 3);
        assert!(snapshot.warning.is_none());
    }
}
