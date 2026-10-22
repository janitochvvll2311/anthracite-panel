use super::parse::truncate_chars;
use super::prelude::*;

pub(crate) fn fit_snapshot(
    mut snapshot: ProcessSnapshot,
    command_truncated: bool,
) -> ProcessSnapshot {
    let total = snapshot.total;
    let mut command_truncated = command_truncated;
    let mut command_limit = MAX_COMMAND_CHARS;
    let rows_truncated = snapshot.processes.len() < total;

    snapshot.truncated = rows_truncated || command_truncated;
    snapshot.warning = warning(snapshot.processes.len(), total, rows_truncated);
    let mut encoded_len = snapshot_json_len(&snapshot);

    // Commands are the bulk of a snapshot. Shorten them first so the useful
    // process rows survive, then drop the least busy rows only if necessary.
    while encoded_len > MAX_LINE_LEN && command_limit > MIN_COMMAND_CHARS {
        command_limit = (command_limit / 2).max(MIN_COMMAND_CHARS);
        for process in &mut snapshot.processes {
            let (command, truncated) = truncate_chars(&process.command, command_limit);
            process.command = command;
            command_truncated |= truncated;
        }

        snapshot.truncated = rows_truncated || command_truncated;
        snapshot.warning = warning(snapshot.processes.len(), total, rows_truncated);
        encoded_len = snapshot_json_len(&snapshot);
    }

    if encoded_len > MAX_LINE_LEN {
        // Rows are sorted by CPU descending, so the tail is the least busy.
        // Binary-search the largest prefix that still fits the frame.
        let processes = snapshot.processes.clone();
        let mut low = 0;
        let mut high = processes.len();

        while low < high {
            let mid = (low + high).div_ceil(2);
            let mut candidate = snapshot.clone();
            candidate.processes.truncate(mid);
            candidate.truncated = true;
            candidate.warning = warning(mid, total, true);

            if snapshot_json_len(&candidate) <= MAX_LINE_LEN {
                low = mid;
            } else {
                high = mid - 1;
            }
        }

        snapshot.processes.truncate(low);
        snapshot.truncated = true;
        snapshot.warning = warning(low, total, true);
    }

    snapshot
}

pub(crate) fn snapshot_json_len(snapshot: &ProcessSnapshot) -> usize {
    Response::ProcessSnapshot(snapshot.clone())
        .to_json()
        .map(|json| json.len())
        .unwrap_or(usize::MAX)
}

pub(crate) fn warning(shown: usize, total: usize, rows_truncated: bool) -> Option<String> {
    rows_truncated.then(|| format!("showing {shown} of {total} processes, highest CPU first"))
}

pub(crate) fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn clock_ticks() -> u64 {
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    if ticks > 0 {
        ticks as u64
    } else {
        CLOCK_TICKS_FALLBACK
    }
}
