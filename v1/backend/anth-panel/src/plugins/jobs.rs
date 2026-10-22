//! Host jobs a granted plugin may invoke.
//!
//! A job is the strong form of a capability: the host owns the program, the
//! argv, the file paths and any secret handling. The plugin supplies only the
//! values declared by the frozen recipe. That is what lets a job create a
//! persistent WireGuard interface without turning the guest into a shell.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::command::CommandRunner;

use super::wireguard;

/// One closed set of host-side operations. The runtime calls this only after it
/// has validated every argument against the grant.
pub trait PluginJobs {
    fn run(
        &self,
        job: &str,
        args: &BTreeMap<String, String>,
        runner: &dyn CommandRunner,
    ) -> Result<String, String>;
}

/// Used by callers that do not expose host jobs (unit tests, the guest probe).
pub struct NoJobs;

impl PluginJobs for NoJobs {
    fn run(
        &self,
        job: &str,
        _args: &BTreeMap<String, String>,
        _runner: &dyn CommandRunner,
    ) -> Result<String, String> {
        Err(format!("host job `{job}` is not enabled in this context"))
    }
}

/// The production job table.
///
/// The root is a constructor argument so the planner can be tested against a
/// temporary directory. Production always uses `/etc/wireguard`.
pub struct SystemJobs {
    wireguard_root: PathBuf,
}

impl Default for SystemJobs {
    fn default() -> Self {
        Self {
            wireguard_root: PathBuf::from(wireguard::DEFAULT_DIR),
        }
    }
}

impl SystemJobs {
    pub fn with_wireguard_root(root: impl Into<PathBuf>) -> Self {
        Self {
            wireguard_root: root.into(),
        }
    }
}

impl PluginJobs for SystemJobs {
    fn run(
        &self,
        job: &str,
        args: &BTreeMap<String, String>,
        runner: &dyn CommandRunner,
    ) -> Result<String, String> {
        match job {
            "wireguard.list"
            | "wireguard.create"
            | "wireguard.update"
            | "wireguard.delete"
            | "wireguard.add_peer"
            | "wireguard.remove_peer" => wireguard::run(job, args, runner, &self.wireguard_root),
            other => Err(format!("host job `{other}` is not implemented")),
        }
    }
}
