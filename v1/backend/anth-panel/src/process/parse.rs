use super::prelude::*;

pub(crate) fn read_users(path: &Path) -> HashMap<u32, String> {
    fs::read_to_string(path)
        .ok()
        .map(|text| {
            text.lines()
                .filter_map(|line| {
                    let mut fields = line.split(':');
                    let name = fields.next()?;
                    let _password = fields.next()?;
                    let uid = fields.next()?.parse().ok()?;
                    Some((uid, name.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn read_processes(
    proc_dir: &Path,
    users: &HashMap<u32, String>,
) -> (Vec<ProcessInfo>, bool) {
    let Ok(entries) = fs::read_dir(proc_dir) else {
        return (Vec::new(), false);
    };

    let mut processes = Vec::new();
    let mut command_truncated = false;

    for entry in entries.flatten() {
        let Some(pid) = parse_pid(entry.file_name().as_os_str()) else {
            continue;
        };
        let directory = entry.path();

        let Ok(stat) = fs::read_to_string(directory.join("stat")) else {
            continue;
        };
        let Some(stat) = parse_stat(&stat) else {
            continue;
        };
        let status = fs::read_to_string(directory.join("status")).unwrap_or_default();
        let uid = parse_uid(&status).unwrap_or(0);
        let rss_kib = parse_rss(&status);
        let name = fs::read_to_string(directory.join("comm"))
            .ok()
            .map(|name| name.trim().to_string())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| stat.comm.clone());
        let command =
            read_cmdline(&directory.join("cmdline")).unwrap_or_else(|| format!("[{name}]"));
        let (command, truncated) = truncate_chars(&command, MAX_COMMAND_CHARS);
        command_truncated |= truncated;

        processes.push(ProcessInfo {
            pid,
            ppid: stat.ppid,
            uid,
            user: users.get(&uid).cloned().unwrap_or_else(|| uid.to_string()),
            name,
            command,
            state: stat.state,
            cpu_ticks: stat.cpu_ticks,
            rss_kib,
            threads: stat.threads,
            start_ticks: stat.start_ticks,
        });
    }

    (processes, command_truncated)
}

pub(crate) fn parse_pid(name: &OsStr) -> Option<u32> {
    name.to_str()?.parse().ok()
}

pub(crate) struct Stat {
    pub(crate) comm: String,
    pub(crate) state: String,
    pub(crate) ppid: u32,
    pub(crate) cpu_ticks: u64,
    pub(crate) threads: u32,
    pub(crate) start_ticks: u64,
}

/// Parses `/proc/<pid>/stat`. The command name is the only field that may contain
/// spaces and parentheses, so the numeric fields are read after the last `)`.
pub(crate) fn parse_stat(text: &str) -> Option<Stat> {
    let open = text.find('(')?;
    let close = text.rfind(')')?;
    if close <= open {
        return None;
    }

    let comm = text.get(open + 1..close)?.to_string();
    let fields: Vec<&str> = text.get(close + 1..)?.split_whitespace().collect();
    if fields.len() < 22 {
        return None;
    }

    let cpu_ticks = fields[11]
        .parse::<u64>()
        .ok()?
        .saturating_add(fields[12].parse::<u64>().ok()?);

    Some(Stat {
        comm,
        state: fields[0].to_string(),
        ppid: fields[1].parse().ok()?,
        cpu_ticks,
        threads: fields[17].parse().ok()?,
        start_ticks: fields[19].parse().ok()?,
    })
}

pub(crate) fn parse_uid(status: &str) -> Option<u32> {
    status.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        if fields.next()? != "Uid:" {
            return None;
        }
        fields.next()?.parse().ok()
    })
}

pub(crate) fn parse_rss(status: &str) -> u64 {
    status
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            if fields.next()? != "VmRSS:" {
                return None;
            }
            fields.next()?.parse().ok()
        })
        .unwrap_or(0)
}

pub(crate) fn read_cmdline(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let arguments: Vec<String> = bytes
        .split(|byte| *byte == 0)
        .filter(|argument| !argument.is_empty())
        .map(|argument| String::from_utf8_lossy(argument).into_owned())
        .collect();

    if arguments.is_empty() {
        None
    } else {
        Some(arguments.join(" "))
    }
}

pub(crate) fn truncate_chars(text: &str, limit: usize) -> (String, bool) {
    let mut chars = text.chars();
    let shortened: String = chars.by_ref().take(limit).collect();
    (shortened, chars.next().is_some())
}

pub(crate) fn read_process_uid(pid: u32) -> Option<u32> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    parse_uid(&status)
}
