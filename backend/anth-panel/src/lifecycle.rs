//! The two irreversible actions, behind one seam.
//!
//! They travel together because both make the panel come back up, but they
//! differ in blast radius, privilege and persistence:
//!
//! | | restart | reboot |
//! |---|---|---|
//! | Replaces | this process | the whole machine |
//! | Re-reads the config | yes | no |
//! | Needs a privilege the daemon lacks | no | yes (`CAP_SYS_BOOT`) |
//! | Refused unless | — | `--allow-reboot` |
//!
//! The seam is a trait so a test can assert the argv or the delay that *would*
//! have been executed instead of replacing the test binary or rebooting the
//! machine the suite runs on.

use std::ffi::OsString;
use std::io;
use std::sync::Arc;
use std::time::Duration;

use anth_wire::WireError;

/// The longest a caller may defer a reboot.
///
/// A reboot is answered before it happens, so a delay is a safety margin rather
/// than a scheduling feature. Five minutes is already a way to park a machine
/// in a state nobody remembers asking for.
pub const MAX_REBOOT_DELAY_S: u32 = 300;

/// How long the transports get to write the answer before the action starts.
///
/// A restart replaces the process that is still holding the response, and a
/// reboot may take the kernel down before a socket flush. The action runs on
/// its own task after this grace period so the caller sees the acknowledgement
/// first.
pub const ACTION_GRACE: Duration = Duration::from_millis(250);

/// What the daemon can do to bring itself or the host back up.
pub trait Actions: Send + Sync + 'static {
    /// Replaces this process, keeping its pid and environment.
    ///
    /// Returns only if the replacement failed; a successful one does not come
    /// back here.
    fn restart(&self, argv: &[OsString]) -> io::Error;

    /// Asks the host to reboot after `delay_s` seconds.
    fn reboot(&self, delay_s: u32) -> Result<(), WireError>;
}

/// The real actions, against the running system.
pub struct System;

impl Actions for System {
    fn restart(&self, argv: &[OsString]) -> io::Error {
        use std::os::unix::process::CommandExt;

        // `execv` keeps the pid, so a container still sees one process and a
        // supervisor does not race a new one. The image is read from disk,
        // which is what makes a restart pick up an upgraded binary as well as
        // an edited `config.toml`.
        let Some(program) = argv.first() else {
            return io::Error::new(io::ErrorKind::InvalidInput, "no program to restart");
        };
        std::process::Command::new(program).args(&argv[1..]).exec()
    }

    fn reboot(&self, delay_s: u32) -> Result<(), WireError> {
        if delay_s > 0 {
            std::thread::sleep(Duration::from_secs(delay_s.into()));
        }
        // `systemctl` first because a systemd host records the request;
        // `reboot` covers the rest. The kernel refuses an unprivileged caller,
        // and that refusal is reported instead of silently swallowed.
        let attempts: [&str; 2] = ["systemctl", "reboot"];
        let mut last = String::from("no reboot command found");
        for program in attempts {
            let args: &[&str] = if program == "systemctl" {
                &["reboot"]
            } else {
                &[]
            };
            match std::process::Command::new(program).args(args).status() {
                Ok(status) if status.success() => return Ok(()),
                Ok(status) => last = format!("{program} exited with {status}"),
                Err(error) => last = format!("{program}: {error}"),
            }
        }
        Err(WireError::internal(format!(
            "the host refused the reboot: {last}"
        )))
    }
}

/// A [`Lifecycle`] with its permission and its actions decided together.
pub struct Lifecycle {
    actions: Arc<dyn Actions>,
    reboot_allowed: bool,
}

impl Lifecycle {
    pub fn new<A: Actions>(actions: Arc<A>, reboot_allowed: bool) -> Self {
        Self {
            actions,
            reboot_allowed,
        }
    }

    /// A lifecycle that may restart but never reboot, for a daemon started
    /// without `--allow-reboot`.
    pub fn without_reboot() -> Self {
        Self::new(Arc::new(System), false)
    }

    /// Whether `--allow-reboot` was given.
    pub fn reboot_allowed(&self) -> bool {
        self.reboot_allowed
    }

    /// Replaces this process with a fresh copy of the same command line.
    pub fn restart(&self) {
        let argv: Vec<OsString> = std::env::args_os().collect();
        let error = self.actions.restart(&argv);
        // Reaching here means the replacement failed and this process is still
        // the daemon. Saying so is the only honest thing left.
        eprintln!("anth-panel: restart failed: {error}");
    }

    /// Checks that a reboot is permitted and sane, without performing it.
    ///
    /// Separate from [`Self::perform_reboot`] so the caller answers the request
    /// first: a refusal is an immediate error, while an accepted one must reach
    /// the client before the host goes down.
    pub fn authorize_reboot(&self, delay_s: u32) -> Result<(), WireError> {
        if !self.reboot_allowed {
            return Err(WireError::forbidden(
                "this daemon may not reboot the host: start it with --allow-reboot",
            ));
        }
        if delay_s > MAX_REBOOT_DELAY_S {
            return Err(WireError::invalid(format!(
                "the delay is at most {MAX_REBOOT_DELAY_S} seconds"
            )));
        }
        Ok(())
    }

    /// Reboots the host. Only reachable after [`Self::authorize_reboot`].
    pub fn perform_reboot(&self, delay_s: u32) -> Result<(), WireError> {
        self.actions.reboot(delay_s)
    }
}

#[cfg(test)]
pub struct Recorded {
    pub restarts: std::sync::Mutex<Vec<Vec<OsString>>>,
    pub reboots: std::sync::Mutex<Vec<u32>>,
}

#[cfg(test)]
impl Default for Recorded {
    fn default() -> Self {
        Self {
            restarts: std::sync::Mutex::new(Vec::new()),
            reboots: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[cfg(test)]
impl Actions for Recorded {
    fn restart(&self, argv: &[OsString]) -> io::Error {
        self.restarts.lock().unwrap().push(argv.to_vec());
        io::Error::other("recorded")
    }

    fn reboot(&self, delay_s: u32) -> Result<(), WireError> {
        self.reboots.lock().unwrap().push(delay_s);
        Ok(())
    }
}

#[cfg(test)]
pub fn recorded(reboot_allowed: bool) -> (Lifecycle, Arc<Recorded>) {
    let recorder = Arc::new(Recorded::default());
    (
        Lifecycle::new(Arc::clone(&recorder), reboot_allowed),
        recorder,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reboot_is_refused_without_the_switch() {
        let (lifecycle, _) = recorded(false);
        let error = lifecycle.authorize_reboot(0).expect_err("refused");
        assert_eq!(error.kind, anth_wire::ErrorKind::Forbidden);
        assert_eq!(error.code, "forbidden");
    }

    #[test]
    fn reboot_delay_is_capped() {
        let (lifecycle, _) = recorded(true);
        assert!(lifecycle.authorize_reboot(MAX_REBOOT_DELAY_S).is_ok());
        let error = lifecycle
            .authorize_reboot(MAX_REBOOT_DELAY_S + 1)
            .expect_err("capped");
        assert_eq!(error.kind, anth_wire::ErrorKind::Invalid);
    }

    #[test]
    fn an_authorized_reboot_records_the_delay() {
        let (lifecycle, recorder) = recorded(true);
        lifecycle.authorize_reboot(30).expect("allowed");
        lifecycle.perform_reboot(30).expect("recorded");
        assert_eq!(recorder.reboots.lock().unwrap().as_slice(), &[30]);
    }

    #[test]
    fn restart_records_the_current_command_line() {
        let (lifecycle, recorder) = recorded(true);
        lifecycle.restart();
        let recorded = recorder.restarts.lock().unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0], std::env::args_os().collect::<Vec<_>>());
    }
}
