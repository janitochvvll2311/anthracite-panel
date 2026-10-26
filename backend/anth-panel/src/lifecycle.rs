//! The two irreversible actions, kept in one place and behind one seam.
//!
//! They are neighbours in the protocol only because both are "make it come back
//! up". Everything else about them is different, and the difference is what this
//! module exists to keep visible:
//!
//! | | [`Actions::restart`] | [`Actions::reboot`] |
//! |---|---|---|
//! | Replaced | this process | the whole machine |
//! | Re-reads the settings | yes | no |
//! | Needs a privilege the daemon lacks | no | yes (`CAP_SYS_BOOT`) |
//! | Refused unless | — | `--allow-reboot` |
//! | Recovery if the new start fails | run the binary again | a supervisor, if any |
//!
//! A restart therefore applies an edit to `config.toml`; a reboot does not, and
//! is offered because an operator asked for both, not because one implies the
//! other.
//!
//! The seam is a trait rather than free functions because both actions replace
//! the running system, and a test that called the real one would not report a
//! failure: it would reboot the machine the suite is running on. [`Recorded`]
//! captures the argv or the delay instead, so the tests assert what *would* have
//! been executed.

use std::ffi::OsString;
use std::io;

use anth_wire::WireError;

/// The longest a caller may defer a reboot.
///
/// The delay exists so the answer reaches the caller before the host goes down.
/// A minute is far more than that needs; five is the point where a request stops
/// being "reboot soon" and starts being a way to park a machine in a state
/// nobody remembers asking for.
pub const MAX_REBOOT_DELAY_S: u32 = 300;

/// What the daemon can do to bring itself or the host back up.
pub trait Actions: Send + Sync + 'static {
    /// Replaces this process, keeping its pid, its working directory and its
    /// environment.
    ///
    /// Returns only if the replacement failed, because a successful one does not
    /// come back here: the new program is already running.
    fn restart(&self, argv: &[OsString]) -> io::Error;

    /// Asks the host to reboot after `delay_s`.
    fn reboot(&self, delay_s: u32) -> Result<(), WireError>;
}

/// The real actions, against the running system.
pub struct System;

impl Actions for System {
    fn restart(&self, argv: &[OsString]) -> io::Error {
        use std::os::unix::process::CommandExt;

        // `execv` rather than a spawn: the pid stays, so nothing is orphaned and
        // a container keeps seeing exactly one process. The new image is read
        // from disk, which is what makes a restart pick up an upgraded binary
        // as well as an edited `config.toml`.
        let Some(program) = argv.first() else {
            return io::Error::new(io::ErrorKind::InvalidInput, "no program to restart");
        };
        std::process::Command::new(program).args(&argv[1..]).exec()
    }

    fn reboot(&self, delay_s: u32) -> Result<(), WireError> {
        if delay_s > 0 {
            std::thread::sleep(std::time::Duration::from_secs(delay_s.into()));
        }
        // `systemctl` first because it is what a distro with systemd expects and
        // it records the request; `reboot` covers the rest. Either way the
        // kernel refuses an unprivileged caller, and that refusal is reported
        // rather than swallowed: a `500` naming `reboot` is better than an
        // answer that says the host is going down when it is not.
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

/// An [`Actions`] that records instead of acting.
///
/// `cfg(test)` so no production caller can reach it: an implementation that
/// silently does nothing is the vacuous test this project's rules forbid, and it
/// must not be something a wrong wiring could select.
#[cfg(test)]
#[derive(Default)]
pub struct Recorded {
    pub restarts: std::sync::Mutex<Vec<Vec<OsString>>>,
    pub reboots: std::sync::Mutex<Vec<u32>>,
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

/// Whether this process may reboot the host, and how.
///
/// One value rather than two arguments, because the permission and the action it
/// gates must be decided together: a caller that could pass `true` to one place
/// and forget the other would have a route that reboots without the switch.
pub struct Lifecycle {
    actions: std::sync::Arc<dyn Actions>,
    reboot_allowed: bool,
}

impl Lifecycle {
    pub fn new(actions: std::sync::Arc<dyn Actions>, reboot_allowed: bool) -> Self {
        Self {
            actions,
            reboot_allowed,
        }
    }

    /// A lifecycle that may restart but never reboot, for a daemon started
    /// without `--allow-reboot`.
    pub fn without_reboot() -> Self {
        Self::new(std::sync::Arc::new(System), false)
    }

    /// A lifecycle that records everything and allows everything, with no
    /// handle to the record.
    ///
    /// For a daemon built by a test that is not asserting on the action: what
    /// matters is that the test binary is not replaced and the machine is not
    /// rebooted.
    #[cfg(test)]
    pub fn empty_recorder() -> Self {
        Self::new(std::sync::Arc::new(Recorded::default()), true)
    }

    /// Records an action instead of performing it.
    #[cfg(test)]
    pub fn recorded(reboot_allowed: bool) -> (Self, std::sync::Arc<Recorded>) {
        let recorded = std::sync::Arc::new(Recorded::default());
        let lifecycle = Self::new(recorded.clone(), reboot_allowed);
        (lifecycle, recorded)
    }

    /// Replaces this process with a fresh copy of the same command line.
    ///
    /// Called on a task of its own, after the answer has been written, so the
    /// caller learns the request was taken rather than watching a connection go
    /// quiet.
    pub fn restart(&self) {
        let argv: Vec<OsString> = std::env::args_os().collect();
        let error = self.actions.restart(&argv);
        // Reaching here means the replacement failed and this process is still
        // the daemon. Saying so is the only honest thing left.
        eprintln!("anth-panel: restart failed: {error}");
    }

    /// Checks that a reboot is permitted and sane, without performing it.
    ///
    /// Separate from [`Self::perform_reboot`] so the caller can answer the
    /// request first: a refusal is an immediate error the caller must see, while
    /// an accepted one has to reach the client before the host goes down.
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

    /// Reboots the host. Only reachable after [`Self::authorize_reboot`], which
    /// is what a caller checks on the request path.
    pub fn perform_reboot(&self, delay_s: u32) -> Result<(), WireError> {
        self.actions.reboot(delay_s)
    }
}
