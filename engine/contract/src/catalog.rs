//! The closed native capability set (§9.2) and the risk each interface carries
//! (§9.3).
//!
//! The set is closed on purpose: adding an entry is a design change, not a
//! configuration option. Every interface here is mechanism-adjacent and every
//! app needs at least one of them, so the engine provides them directly instead
//! of shipping an app that would only wrap the host.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::id::InterfaceId;

/// What granting an interface authorizes, from least to most.
///
/// The order is the policy: an app's risk is the maximum over what it can reach,
/// so `Read < Write < Admin < Host < Package` is the comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    /// Reading environment facts, the clock, logs. The baseline: no flag needed.
    Read,
    /// Mutating the app's own state.
    Write,
    /// Sockets, devices, network configuration.
    Admin,
    /// Executing a binary or spawning a process.
    Host,
    /// Installing a system package.
    Package,
}

impl Risk {
    /// Every level, least to most, so the approval flags can be derived.
    pub const ALL: &'static [Risk] = &[
        Risk::Read,
        Risk::Write,
        Risk::Admin,
        Risk::Host,
        Risk::Package,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Risk::Read => "read",
            Risk::Write => "write",
            Risk::Admin => "admin",
            Risk::Host => "host",
            Risk::Package => "package",
        }
    }

    /// The `--accept-<level>` flag that admits this level. `read` is the
    /// baseline and needs none: a flag for everything teaches operators to pass
    /// flags without reading them.
    pub const fn approval_flag(self) -> Option<&'static str> {
        match self {
            Risk::Read => None,
            Risk::Write => Some("--accept-write"),
            Risk::Admin => Some("--accept-admin"),
            Risk::Host => Some("--accept-host"),
            Risk::Package => Some("--accept-package"),
        }
    }
}

impl fmt::Display for Risk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Risk {
    type Err = crate::error::Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            .copied()
            .find(|risk| risk.as_str() == input)
            .ok_or_else(|| {
                crate::error::Error::invalid(format!(
                    "`{input}` is not a risk level; use read, write, admin, host or package"
                ))
            })
    }
}

/// One interface the engine provides, with the risk granting it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeInterface {
    pub id: &'static str,
    pub risk: Risk,
}

/// The closed set of §9.2, in the order the document lists them.
///
/// `anth:error` is absent because it is a package of types, not a capability,
/// and `anth:store/blobs` is absent until transfers land in v0.6.0.
pub const NATIVE: &[NativeInterface] = &[
    NativeInterface {
        id: "anth:env/info@1.0.0",
        risk: Risk::Read,
    },
    NativeInterface {
        id: "anth:env/time@1.0.0",
        risk: Risk::Read,
    },
    NativeInterface {
        id: "anth:env/log@1.0.0",
        risk: Risk::Read,
    },
    NativeInterface {
        id: "anth:env/random@1.0.0",
        risk: Risk::Read,
    },
    NativeInterface {
        id: "anth:store/kv@1.0.0",
        risk: Risk::Write,
    },
];

/// The risk of a host interface, or `None` when it is not in the native set.
pub fn risk_of(interface: &InterfaceId) -> Option<Risk> {
    let id = interface.to_string();
    NATIVE
        .iter()
        .find(|entry| entry.id == id)
        .map(|entry| entry.risk)
}

/// Whether the engine provides this interface.
pub fn is_native(interface: &InterfaceId) -> bool {
    risk_of(interface).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::Namespace;

    #[test]
    fn every_native_id_parses_and_lives_under_anth() {
        for entry in NATIVE {
            let id = InterfaceId::parse(entry.id).expect("native id parses");
            assert_eq!(id.namespace_kind(), Namespace::Engine, "{}", entry.id);
        }
    }

    #[test]
    fn the_set_is_exactly_the_five_interfaces_of_the_design() {
        // A guard against silent growth: changing this list is a design change.
        let ids: Vec<&str> = NATIVE.iter().map(|entry| entry.id).collect();
        assert_eq!(
            ids,
            [
                "anth:env/info@1.0.0",
                "anth:env/time@1.0.0",
                "anth:env/log@1.0.0",
                "anth:env/random@1.0.0",
                "anth:store/kv@1.0.0",
            ]
        );
    }

    #[test]
    fn environment_reads_are_read_and_the_store_is_write() {
        for id in [
            "anth:env/info@1.0.0",
            "anth:env/time@1.0.0",
            "anth:env/log@1.0.0",
            "anth:env/random@1.0.0",
        ] {
            assert_eq!(risk_of(&InterfaceId::parse(id).unwrap()), Some(Risk::Read));
        }
        assert_eq!(
            risk_of(&InterfaceId::parse("anth:store/kv@1.0.0").unwrap()),
            Some(Risk::Write)
        );
    }

    #[test]
    fn an_app_interface_is_not_native() {
        let id = InterfaceId::parse("example:notes/list@1.0.0").unwrap();
        assert!(!is_native(&id));
        assert_eq!(risk_of(&id), None);
    }

    #[test]
    fn another_version_of_a_native_interface_is_not_native() {
        let id = InterfaceId::parse("anth:env/time@1.0.1").unwrap();
        assert_eq!(risk_of(&id), None);
    }

    #[test]
    fn risk_orders_from_read_to_package() {
        let mut risks = vec![
            Risk::Package,
            Risk::Read,
            Risk::Host,
            Risk::Write,
            Risk::Admin,
        ];
        risks.sort();
        assert_eq!(risks, Risk::ALL.to_vec());
    }

    #[test]
    fn every_level_but_read_has_an_approval_flag() {
        assert_eq!(Risk::Read.approval_flag(), None);
        for (risk, flag) in [
            (Risk::Write, "--accept-write"),
            (Risk::Admin, "--accept-admin"),
            (Risk::Host, "--accept-host"),
            (Risk::Package, "--accept-package"),
        ] {
            assert_eq!(risk.approval_flag(), Some(flag));
        }
    }

    #[test]
    fn a_risk_round_trips_through_its_name() {
        for risk in Risk::ALL {
            assert_eq!(Risk::from_str(risk.as_str()).expect("parse"), *risk);
            assert_eq!(risk.to_string(), risk.as_str());
        }
        assert!(Risk::from_str("root").is_err());
    }
}
