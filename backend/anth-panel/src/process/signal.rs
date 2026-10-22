use super::parse::read_process_uid;
use super::prelude::*;

#[derive(Debug, PartialEq, Eq)]
pub enum SignalError {
    ControlDisabled,
    Protected,
    NotOwned,
    NotFound,
    PermissionDenied,
    Failed(String),
}

impl SignalError {
    pub fn detail(&self) -> String {
        match self {
            Self::ControlDisabled => "process control is disabled".to_string(),
            Self::Protected => "refusing to signal pid 1 or the daemon's own process".to_string(),
            Self::NotOwned => "the process is not owned by the daemon user".to_string(),
            Self::NotFound => "the process does not exist".to_string(),
            Self::PermissionDenied => "permission denied while signalling the process".to_string(),
            Self::Failed(detail) => detail.clone(),
        }
    }
}

/// Sends an allowlisted signal after checking the daemon-side policy.
///
/// The policy is not a UI concern: the daemon is the authority even when a
/// client talks to the socket directly.
pub fn send_signal(
    pid: u32,
    signal: ProcessSignal,
    policy: &ProcessPolicy,
) -> Result<(), SignalError> {
    if !policy.control {
        return Err(SignalError::ControlDisabled);
    }
    if pid <= 1 || pid == std::process::id() {
        return Err(SignalError::Protected);
    }

    let target_uid = read_process_uid(pid).ok_or(SignalError::NotFound)?;
    let own_uid = unsafe { libc::getuid() };
    if target_uid != own_uid {
        return Err(SignalError::NotOwned);
    }

    let result = unsafe { libc::kill(pid as i32, raw_signal(signal)) };
    if result == 0 {
        return Ok(());
    }

    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ESRCH) => Err(SignalError::NotFound),
        Some(libc::EPERM) => Err(SignalError::PermissionDenied),
        _ => Err(SignalError::Failed(format!(
            "failed to signal process {pid}: {error}"
        ))),
    }
}

fn raw_signal(signal: ProcessSignal) -> i32 {
    match signal {
        ProcessSignal::Term => libc::SIGTERM,
        ProcessSignal::Kill => libc::SIGKILL,
        ProcessSignal::Hup => libc::SIGHUP,
        ProcessSignal::Int => libc::SIGINT,
        ProcessSignal::Quit => libc::SIGQUIT,
        ProcessSignal::Stop => libc::SIGSTOP,
        ProcessSignal::Cont => libc::SIGCONT,
    }
}
