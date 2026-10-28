//! Account inventory and mutation requests.
//!
//! This is the transport contract only. The daemon owns the `/etc/passwd` and
//! `getent` reads, the guard policy and the `useradd`/`userdel`/`groupadd`/
//! `groupdel`/`chpasswd` calls.
//!
//! Nothing here carries a password hash. [`AccountUser::locked`] is the single
//! derived fact the panel exposes, and it is `Option<bool>` because an account
//! resolved from a directory service has no shadow entry the daemon can read.
//! `None` means "unknown", never "unlocked".

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Where an entry came from, and therefore whether the panel may change it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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
/// It serializes like a `String`, so the wire format is unchanged, but `Debug`
/// prints a placeholder. Requests derive `Debug`, so a single `eprintln!`
/// between the socket and the handler would otherwise write the password to
/// the journal.
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AccountSnapshot {
    pub users: Vec<AccountUser>,
    pub groups: Vec<AccountGroup>,
    /// `[accounts] control`: create and delete.
    pub control_enabled: bool,
    /// `[accounts] manage_passwords`. Reported separately so the UI can hide the
    /// password action when only inventory is enabled.
    pub password_enabled: bool,
    /// `[accounts] allow_system`. The UI greys out system entries instead of
    /// offering an action the daemon will refuse.
    #[serde(default)]
    pub allow_system: bool,
    /// `[accounts] allow_privileged_groups`, same reason for privileged groups.
    #[serde(default)]
    pub allow_privileged_groups: bool,
    pub timestamp_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// Create one locked local user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountCreate {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gecos: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<String>,
    #[serde(default)]
    pub system: bool,
}

/// Delete one local user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountDelete {
    pub name: String,
    #[serde(default)]
    pub remove_home: bool,
}

/// Set one local user's password.
///
/// The password travels in the request body, never in a command line. The
/// derived `Debug` is safe because [`SecretString`] redacts itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountSetPassword {
    pub name: String,
    pub password: SecretString,
}

/// Create one local group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountGroupCreate {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gid: Option<u32>,
    #[serde(default)]
    pub system: bool,
}

/// Delete one local group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountGroupDelete {
    pub name: String,
}

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
    fn a_password_is_redacted_in_debug_and_kept_on_the_wire() {
        let request = AccountSetPassword {
            name: "alice".to_string(),
            password: SecretString::new("correct horse battery staple"),
        };
        let debug = format!("{request:?}");
        assert!(!debug.contains("correct horse"), "{debug}");
        assert!(debug.contains("<redacted>"), "{debug}");

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("correct horse battery staple"));
        let decoded: AccountSetPassword = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.password.expose(), "correct horse battery staple");
    }

    #[test]
    fn an_empty_secret_debug_does_not_pretend_to_hide_a_value() {
        assert_eq!(
            format!("{:?}", SecretString::default()),
            "SecretString(\"\")"
        );
    }
}
