//! Capabilities the panel provides to apps.
//!
//! The catalog is deliberately small: an operation is added when an app needs
//! it, with its schema, its risk and the policy switch it obeys. A capability
//! that no app consumes is surface with no test.

use super::grant_v3::Risk;
use super::schema::{Signature, Type};

/// One host-provided capability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostExport {
    pub signature: Signature,
    /// The Rust operation the broker dispatches to.
    pub operation: &'static str,
    pub risk: Risk,
}

/// The host catalog for the first migration phase.
pub fn exports() -> Vec<HostExport> {
    vec![HostExport {
        signature: Signature {
            id: "org.anth.host.info".to_string(),
            version: 1,
            input: Type::record([]),
            output: host_info(),
        },
        operation: "host.info",
        risk: Risk::Read,
    }]
}

fn host_info() -> Type {
    Type::record([
        ("hostname", Type::string()),
        (
            "os",
            Type::record([
                ("name", Type::string()),
                ("version", Type::string()),
                ("kernel", Type::string()),
                ("arch", Type::string()),
            ]),
        ),
        ("uptime_seconds", Type::int(Some(0), None)),
        ("cpu_model", Type::string()),
        ("logical_cores", Type::int(Some(1), None)),
        (
            "load",
            Type::record([
                ("one", Type::Float),
                ("five", Type::Float),
                ("fifteen", Type::Float),
            ]),
        ),
        (
            "memory",
            Type::record([
                ("total_kib", Type::int(Some(0), None)),
                ("available_kib", Type::int(Some(0), None)),
                ("swap_total_kib", Type::int(Some(0), None)),
                ("swap_free_kib", Type::int(Some(0), None)),
            ]),
        ),
        ("collected_at_ms", Type::Timestamp),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_host_export_is_in_the_reserved_namespace_and_valid() {
        let exports = exports();
        assert!(!exports.is_empty());
        for export in exports {
            assert!(
                export.signature.id.starts_with("org.anth.host."),
                "{}",
                export.signature.id
            );
            assert_eq!(export.signature.version, 1);
            export.signature.validate().expect("schema");
        }
    }

    #[test]
    fn a_host_info_value_matches_the_catalog() {
        let export = exports().into_iter().next().expect("host.info");
        let value = serde_json::json!({
            "hostname": "example",
            "os": { "name": "Alpine", "version": "3.24", "kernel": "6.12", "arch": "x86_64" },
            "uptime_seconds": 10,
            "cpu_model": "Example CPU",
            "logical_cores": 4,
            "load": { "one": 0.1, "five": 0.2, "fifteen": 0.3 },
            "memory": {
                "total_kib": 1024,
                "available_kib": 512,
                "swap_total_kib": 0,
                "swap_free_kib": 0
            },
            "collected_at_ms": 1_700_000_000_000u64
        });
        export
            .signature
            .output
            .validate_value(&value)
            .expect("value");
    }
}
