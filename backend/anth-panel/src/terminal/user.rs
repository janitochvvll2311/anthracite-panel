//! Resolve the account a terminal session should run as.
//!
//! `auto` (and an empty value) means the daemon's effective user. A name or
//! numeric uid is resolved through `/etc/passwd`; switching to a different
//! account requires the daemon to run as root and `setpriv`.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalUser {
    pub name: String,
    pub uid: u32,
    pub gid: u32,
    pub home: String,
}

impl fmt::Display for TerminalUser {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.name)
    }
}

pub fn resolve(configured: &str) -> Result<TerminalUser, String> {
    let configured = configured.trim();
    if configured.is_empty() || configured.eq_ignore_ascii_case("auto") {
        let uid = unsafe { libc::geteuid() };
        let gid = unsafe { libc::getegid() };
        let passwd = std::fs::read_to_string("/etc/passwd").unwrap_or_default();
        return Ok(entry_by_uid(&passwd, uid).unwrap_or_else(|| TerminalUser {
            name: std::env::var("USER")
                .or_else(|_| std::env::var("LOGNAME"))
                .unwrap_or_else(|_| format!("uid-{uid}")),
            uid,
            gid,
            home: std::env::var("HOME").unwrap_or_else(|_| "/".to_string()),
        }));
    }

    let passwd = std::fs::read_to_string("/etc/passwd")
        .map_err(|error| format!("could not read /etc/passwd: {error}"))?;
    let entry = if let Ok(uid) = configured.parse::<u32>() {
        entry_by_uid(&passwd, uid)
    } else {
        entry_by_name(&passwd, configured)
    };

    entry.ok_or_else(|| format!("terminal user `{configured}` does not exist"))
}

fn entry_by_name(passwd: &str, name: &str) -> Option<TerminalUser> {
    passwd
        .lines()
        .filter_map(parse_passwd_line)
        .find(|entry| entry.name == name)
}

fn entry_by_uid(passwd: &str, uid: u32) -> Option<TerminalUser> {
    passwd
        .lines()
        .filter_map(parse_passwd_line)
        .find(|entry| entry.uid == uid)
}

fn parse_passwd_line(line: &str) -> Option<TerminalUser> {
    let fields: Vec<&str> = line.split(':').collect();
    if fields.len() < 7 {
        return None;
    }

    Some(TerminalUser {
        name: fields[0].to_string(),
        uid: fields[2].parse().ok()?,
        gid: fields[3].parse().ok()?,
        home: if fields[5].is_empty() {
            "/".to_string()
        } else {
            fields[5].to_string()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_uses_the_effective_user() {
        let user = resolve("auto").unwrap();
        assert_eq!(user.uid, unsafe { libc::geteuid() });
        assert_eq!(user.gid, unsafe { libc::getegid() });
        assert!(!user.name.is_empty());
    }

    #[test]
    fn names_and_uids_are_resolved_from_passwd() {
        let passwd = concat!(
            "root:x:0:0:root:/root:/bin/sh\n",
            "anth:x:1000:1000:Anth:/home/anth:/bin/sh\n",
            "broken:x:1001:1001:Broken:/home/broken\n",
        );

        let anth = entry_by_name(passwd, "anth").unwrap();
        assert_eq!(anth.uid, 1000);
        assert_eq!(anth.gid, 1000);
        assert_eq!(anth.home, "/home/anth");
        assert_eq!(entry_by_uid(passwd, 1000), Some(anth));
        assert!(entry_by_name(passwd, "broken").is_none());
    }

    #[test]
    fn unknown_names_are_rejected() {
        assert!(resolve("definitely-not-a-real-user").is_err());
    }
}
