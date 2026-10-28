//! Allowlisted signals with the guard matrix.
//!
//! The daemon is the authority: the CLI, HTTP and the browser all reach this
//! function, and a direct socket client gets the same checks.

use std::fs;

use anth_wire::{ErrorKind, ProcessSignal, WireError};

use super::parse::{parse_stat, parse_status};

/// Sends one allowlisted signal after checking policy, ownership and identity.
///
/// `start_ticks` is the value the caller saw in the snapshot. A mismatch means
/// the number now belongs to a different process, so the request is refused
/// instead of signalling the wrong thing.
pub fn send(
    control: bool,
    pid: u32,
    start_ticks: u64,
    signal: ProcessSignal,
    group: bool,
) -> Result<(), WireError> {
    if !control {
        return Err(WireError::new(
            ErrorKind::Forbidden,
            "process_control_disabled",
            "process control is disabled; set `[process] control = true`",
        ));
    }
    if pid <= 1 || pid == std::process::id() {
        return Err(WireError::new(
            ErrorKind::Forbidden,
            "protected_process",
            "refusing to signal pid 1 or the daemon's own process",
        ));
    }

    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|text| parse_stat(&text))
        .ok_or_else(|| not_found(pid))?;
    if stat.start_ticks != start_ticks {
        return Err(WireError::new(
            ErrorKind::Conflict,
            "process_changed",
            format!("process {pid} changed since the snapshot; refresh and try again"),
        ));
    }

    let status = fs::read_to_string(format!("/proc/{pid}/status")).unwrap_or_default();
    let target_uid = parse_status(&status).uid.ok_or_else(|| not_found(pid))?;
    let own_uid = unsafe { libc::getuid() };
    if target_uid != own_uid {
        return Err(WireError::new(
            ErrorKind::Forbidden,
            "process_not_owned",
            "the process is not owned by the daemon user",
        ));
    }

    if group && stat.pgrp != pid {
        return Err(WireError::new(
            ErrorKind::Invalid,
            "not_group_leader",
            format!("process {pid} does not lead its own process group"),
        ));
    }

    let target = if group { -(pid as i32) } else { pid as i32 };
    if unsafe { libc::kill(target, raw_signal(signal)) } == 0 {
        return Ok(());
    }

    let error = std::io::Error::last_os_error();
    Err(match error.raw_os_error() {
        Some(libc::ESRCH) => not_found(pid),
        Some(libc::EPERM) => WireError::new(
            ErrorKind::Forbidden,
            "process_permission",
            format!("permission denied while signalling process {pid}"),
        ),
        _ => WireError::internal(format!("failed to signal process {pid}: {error}")),
    })
}

fn not_found(pid: u32) -> WireError {
    WireError::not_found(format!("no process `{pid}`"))
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
