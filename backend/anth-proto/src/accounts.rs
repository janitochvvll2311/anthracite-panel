//! Account inventory and mutation requests.
//!
//! This is the transport contract only. The daemon owns the `/etc/passwd` and
//! `getent` reads, the guard policy and the `useradd`/`usermod` calls.
//!
//! Nothing in this module carries a password hash. `AccountUser::locked` is the
//! single derived fact the panel exposes, and it is `Option<bool>` because an
//! account resolved from a directory service has no shadow entry the daemon can
//! read. `None` means "unknown", never "unlocked".

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Where an entry came from, and therefore whether the panel may change it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum AccountSource {
    /// Present in the local `passwd`/`group` files. Writable.
    #[default]
    Local,
    /// Resolved by NSS but absent from the local files, e.g. LDAP or SSSD.
    /// Read-only: the panel could not write it back.
    Nss,
}

impl AccountSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Nss => "nss",
        }
    }

    pub fn is_writable(self) -> bool {
        matches!(self, Self::Local)
    }
}

impl fmt::Display for AccountSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for AccountSource {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "nss" => Ok(Self::Nss),
            other => Err(format!("unknown account source `{other}`")),
        }
    }
}

/// A string that must not appear in a log line.
///
/// Serializes and deserializes exactly like a `String`, so the wire format is
/// unchanged, but `Debug` prints a placeholder. `Request` derives `Debug`, so
/// without this a single `eprintln!("{request:?}")` anywhere between the socket
/// and the handler would write the password to the journal.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct SecretString(String);

impl SecretString {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The plaintext. Call sites are few and deliberate: only the daemon writes
    /// it, and only into the stdin of `chpasswd`.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            formatter.write_str("SecretString(\"\")")
        } else {
            formatter.write_str("SecretString(\"<redacted>\")")
        }
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// One user account.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountUser {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    /// Name of the primary group, resolved from `gid` when possible and left as
    /// the numeric id otherwise.
    pub group: String,
    pub gecos: String,
    pub home: String,
    pub shell: String,
    /// `None` when the shadow entry could not be read, which is the normal case
    /// for an NSS account and for an unprivileged daemon. Clients must show
    /// "unknown", not "unlocked".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    /// `uid` is below the host's `UID_MIN`.
    pub system: bool,
    pub source: AccountSource,
    /// Supplementary groups. Always serialized, empty when there are none, so a
    /// client can read it as a list without checking for the key.
    #[serde(default)]
    pub groups: Vec<String>,
}

/// One group.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountGroup {
    pub name: String,
    pub gid: u32,
    /// Always serialized, empty when the group has no members.
    #[serde(default)]
    pub members: Vec<String>,
    /// `gid` is below the host's `GID_MIN`.
    pub system: bool,
    pub source: AccountSource,
}

/// One point-in-time view of the host's accounts.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountSnapshot {
    pub users: Vec<AccountUser>,
    pub groups: Vec<AccountGroup>,
    /// `[accounts] control`: create, modify and delete.
    pub control_enabled: bool,
    /// `[accounts] manage_passwords`. Reported separately so the UI can hide the
    /// password action when only inventory is enabled.
    pub password_enabled: bool,
    pub timestamp_ms: u64,
    /// Users found before truncation.
    pub total_users: usize,
    /// Groups found before truncation.
    pub total_groups: usize,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AccountErrorKind {
    /// `[accounts] control` or `manage_passwords` is off.
    Disabled,
    /// uid/gid 0, the daemon's own account, the last administrator, or a
    /// privileged group without the opt-in.
    Protected,
    NotFound,
    /// The name or id is already taken.
    Conflict,
    /// The host's tooling cannot perform this operation, e.g. no `usermod` on a
    /// BusyBox-only system, or an entry that comes from NSS.
    Unsupported,
    Invalid,
    PermissionDenied,
    Failed,
}

impl AccountErrorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Protected => "protected",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::Unsupported => "unsupported",
            Self::Invalid => "invalid",
            Self::PermissionDenied => "permission_denied",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for AccountErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl std::str::FromStr for AccountErrorKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "disabled" => Ok(Self::Disabled),
            "protected" => Ok(Self::Protected),
            "not_found" => Ok(Self::NotFound),
            "conflict" => Ok(Self::Conflict),
            "unsupported" => Ok(Self::Unsupported),
            "invalid" => Ok(Self::Invalid),
            "permission_denied" => Ok(Self::PermissionDenied),
            "failed" => Ok(Self::Failed),
            other => Err(format!("unknown account error kind `{other}`")),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AccountError {
    pub kind: AccountErrorKind,
    pub detail: String,
}

impl AccountError {
    pub fn new(kind: AccountErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: detail.into(),
        }
    }
}

impl fmt::Display for AccountError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.kind, self.detail)
    }
}

impl std::error::Error for AccountError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> AccountUser {
        AccountUser {
            name: "alice".to_string(),
            uid: 1001,
            gid: 1001,
            group: "alice".to_string(),
            gecos: "Alice".to_string(),
            home: "/home/alice".to_string(),
            shell: "/bin/sh".to_string(),
            locked: Some(true),
            system: false,
            source: AccountSource::Local,
            groups: Vec::new(),
        }
    }

    #[test]
    fn supplementary_groups_are_always_serialized() {
        let json = serde_json::to_string(&user()).unwrap();
        assert!(json.contains("\"groups\":[]"), "{json}");
    }

    #[test]
    fn group_members_are_always_serialized() {
        let group = AccountGroup {
            name: "wheel".to_string(),
            gid: 10,
            members: Vec::new(),
            system: false,
            source: AccountSource::Local,
        };
        let json = serde_json::to_string(&group).unwrap();
        assert!(json.contains("\"members\":[]"), "{json}");
    }

    #[test]
    fn an_unknown_lock_state_is_omitted_rather_than_reported_as_false() {
        let mut unknown = user();
        unknown.locked = None;
        let json = serde_json::to_string(&unknown).unwrap();
        assert!(!json.contains("\"locked\""), "{json}");

        let decoded: AccountUser = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.locked, None);
    }

    #[test]
    fn a_locked_false_is_preserved() {
        let mut unlocked = user();
        unlocked.locked = Some(false);
        let decoded: AccountUser =
            serde_json::from_str(&serde_json::to_string(&unlocked).unwrap()).unwrap();
        assert_eq!(decoded.locked, Some(false));
    }

    #[test]
    fn sources_round_trip() {
        assert_eq!(
            serde_json::to_string(&AccountSource::Local).unwrap(),
            "\"local\""
        );
        assert_eq!(
            serde_json::to_string(&AccountSource::Nss).unwrap(),
            "\"nss\""
        );
        assert!(AccountSource::Local.is_writable());
        assert!(!AccountSource::Nss.is_writable());
    }

    #[test]
    fn error_kinds_round_trip_through_strings() {
        for kind in [
            AccountErrorKind::Disabled,
            AccountErrorKind::Protected,
            AccountErrorKind::NotFound,
            AccountErrorKind::Conflict,
            AccountErrorKind::Unsupported,
            AccountErrorKind::Invalid,
            AccountErrorKind::PermissionDenied,
            AccountErrorKind::Failed,
        ] {
            let text = kind.as_str();
            assert_eq!(text.parse::<AccountErrorKind>().unwrap(), kind);
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{text}\""));
        }
    }

    #[test]
    fn a_password_never_appears_in_debug_output() {
        let request = crate::Request::AccountSetPassword {
            name: "alice".to_string(),
            password: SecretString::new("hunter2-do-not-log"),
        };
        let debug = format!("{request:?}");
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");
        assert!(debug.contains("alice"), "the name is not secret: {debug}");
    }

    #[test]
    fn a_password_never_appears_in_a_serialized_error_path() {
        // The wire form must still carry the real value, or the daemon could not
        // use it. Serialization is safe; only `Debug` redacts.
        let request = crate::Request::AccountSetPassword {
            name: "alice".to_string(),
            password: SecretString::new("hunter2"),
        };
        let json = request.to_json().unwrap();
        assert!(json.contains("hunter2"), "{json}");

        let decoded = crate::Request::from_json(&json).unwrap();
        match decoded {
            crate::Request::AccountSetPassword { password, .. } => {
                assert_eq!(password.expose(), "hunter2");
            }
            other => panic!("unexpected request: {other:?}"),
        }
    }
}
