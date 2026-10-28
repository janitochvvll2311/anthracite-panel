//! Virtual terminal for the demo build.
//!
//! It never opens a PTY, reads a shell or sends a signal. Input is echoed and a
//! small set of commands returns canned output, which is enough for the browser
//! bridge and the mobile key bar to exercise their real paths.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anth_wire::{ErrorKind, MAX_TERMINAL_INPUT, MAX_TERMINAL_WAIT_MS, TerminalSize, WireError};

use crate::backend::TerminalService;
use crate::terminal::{Chunk, OpenError};

const PROMPT: &str = "demo@anthracite:~$ ";
const POLL: Duration = Duration::from_millis(50);

pub struct DemoTerminal {
    enabled: bool,
    max: usize,
    next: AtomicU64,
    sessions: Mutex<HashMap<String, DemoSession>>,
}

struct DemoSession {
    owner: String,
    seq: u64,
    pending: VecDeque<Vec<u8>>,
    line: String,
    escape: bool,
    closed: bool,
    exit_code: Option<i32>,
}

impl DemoTerminal {
    pub fn new(enabled: bool, max: usize) -> Self {
        Self {
            enabled,
            max,
            next: AtomicU64::new(1),
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn sessions(&self) -> std::sync::MutexGuard<'_, HashMap<String, DemoSession>> {
        self.sessions
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    fn session<'a>(
        sessions: &'a mut HashMap<String, DemoSession>,
        owner: &str,
        id: &str,
    ) -> Result<&'a mut DemoSession, WireError> {
        match sessions.get_mut(id) {
            Some(session) if session.owner == owner => Ok(session),
            _ => Err(WireError::new(
                ErrorKind::NotFound,
                "terminal_not_found",
                format!("no terminal `{id}`"),
            )),
        }
    }
}

impl TerminalService for DemoTerminal {
    fn enabled(&self) -> bool {
        self.enabled
    }

    fn len(&self) -> usize {
        self.sessions().len()
    }

    fn open(&self, owner: &str, _program: &str, size: TerminalSize) -> Result<String, OpenError> {
        if !self.enabled {
            return Err(OpenError::Disabled);
        }
        size.validate().map_err(|_| OpenError::BadSize)?;
        let mut sessions = self.sessions();
        if sessions.len() >= self.max {
            return Err(OpenError::AtCapacity);
        }
        let id = format!("d{:x}", self.next.fetch_add(1, Ordering::Relaxed));
        let banner = concat!(
            "\u{1b}[1;32mAnthracite demo shell\u{1b}[0m\r\n",
            "Virtual terminal: no command touches the host.\r\n",
            "Type `help` for the demo commands.\r\n",
            "\r\ndemo@anthracite:~$ "
        );
        let mut pending = VecDeque::new();
        pending.push_back(banner.as_bytes().to_vec());
        sessions.insert(
            id.clone(),
            DemoSession {
                owner: owner.to_string(),
                seq: 0,
                pending,
                line: String::new(),
                escape: false,
                closed: false,
                exit_code: None,
            },
        );
        Ok(id)
    }

    fn read<'a>(
        &'a self,
        owner: &'a str,
        id: &'a str,
        wait_ms: u32,
    ) -> Pin<Box<dyn Future<Output = Result<Chunk, WireError>> + Send + 'a>> {
        Box::pin(async move {
            let wait = Duration::from_millis(u64::from(wait_ms.min(MAX_TERMINAL_WAIT_MS)));
            let started = tokio::time::Instant::now();
            loop {
                {
                    let mut sessions = self.sessions();
                    let session = Self::session(&mut sessions, owner, id)?;
                    if let Some(bytes) = session.pending.pop_front() {
                        session.seq += 1;
                        return Ok(Chunk {
                            seq: session.seq,
                            bytes,
                            eof: false,
                            exit_code: None,
                        });
                    }
                    if session.closed {
                        return Ok(Chunk {
                            seq: session.seq,
                            bytes: Vec::new(),
                            eof: true,
                            exit_code: session.exit_code.or(Some(0)),
                        });
                    }
                }
                let waited = started.elapsed();
                if waited >= wait {
                    let mut sessions = self.sessions();
                    let session = Self::session(&mut sessions, owner, id)?;
                    return Ok(Chunk {
                        seq: session.seq,
                        bytes: Vec::new(),
                        eof: false,
                        exit_code: None,
                    });
                }
                let step = POLL.min(wait - waited);
                tokio::time::sleep(step).await;
            }
        })
    }

    fn write(&self, owner: &str, id: &str, bytes: &[u8]) -> Result<(), WireError> {
        if bytes.len() > MAX_TERMINAL_INPUT {
            return Err(WireError::too_large(format!(
                "terminal input is {} bytes, over the {MAX_TERMINAL_INPUT} byte limit",
                bytes.len()
            )));
        }
        let mut sessions = self.sessions();
        let session = Self::session(&mut sessions, owner, id)?;
        if session.closed {
            return Ok(());
        }
        let text = String::from_utf8_lossy(bytes);
        let mut output = Vec::new();
        for character in text.chars() {
            if session.escape {
                // Swallow one ANSI/CSI sequence; arrow keys arrive as `ESC [ A`.
                if character.is_ascii_alphabetic() || character == '~' {
                    session.escape = false;
                }
                continue;
            }
            match character {
                '\u{1b}' => session.escape = true,
                '\r' | '\n' => {
                    output.extend_from_slice(b"\r\n");
                    run_command(session, &mut output);
                }
                '\u{8}' | '\u{7f}' => {
                    if session.line.pop().is_some() {
                        output.extend_from_slice(b"\x08 \x08");
                    }
                }
                '\u{3}' => {
                    session.line.clear();
                    output.extend_from_slice(b"^C\r\n");
                    output.extend_from_slice(PROMPT.as_bytes());
                }
                '\u{4}' => {
                    session.line.clear();
                    output.extend_from_slice(b"logout\r\n");
                    session.closed = true;
                    session.exit_code = Some(0);
                }
                character if !character.is_control() => {
                    session.line.push(character);
                    output.extend_from_slice(character.to_string().as_bytes());
                }
                _ => {}
            }
        }
        if !output.is_empty() {
            session.pending.push_back(output);
        }
        Ok(())
    }

    fn resize(&self, owner: &str, id: &str, size: TerminalSize) -> Result<(), WireError> {
        size.validate().map_err(WireError::invalid)?;
        let mut sessions = self.sessions();
        Self::session(&mut sessions, owner, id)?;
        Ok(())
    }

    fn close(&self, owner: &str, id: &str) -> Result<(), WireError> {
        let mut sessions = self.sessions();
        match sessions.get(id) {
            Some(session) if session.owner == owner => {
                sessions.remove(id);
                Ok(())
            }
            Some(_) => Err(WireError::new(
                ErrorKind::NotFound,
                "terminal_not_found",
                format!("no terminal `{id}`"),
            )),
            None => Ok(()),
        }
    }

    fn close_owner(&self, owner: &str) -> usize {
        let mut sessions = self.sessions();
        let ids: Vec<String> = sessions
            .iter()
            .filter(|(_, session)| session.owner == owner)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &ids {
            sessions.remove(id);
        }
        ids.len()
    }

    fn reap(&self) -> usize {
        0
    }
}

fn run_command(session: &mut DemoSession, output: &mut Vec<u8>) {
    let command = session.line.trim().to_string();
    session.line.clear();
    let reply = match command.as_str() {
        "" => String::new(),
        "help" => concat!(
            "demo commands:\r\n",
            "  help          show this list\r\n",
            "  whoami        current demo user\r\n",
            "  pwd           current directory\r\n",
            "  ls            files in the sandbox\r\n",
            "  uname -a      synthetic kernel\r\n",
            "  echo <text>   print text\r\n",
            "  date          fixed demo time\r\n",
            "  clear         clear the screen\r\n",
            "  exit          end the session"
        )
        .to_string(),
        "whoami" => "demo".to_string(),
        "pwd" => "/home/demo".to_string(),
        "ls" => "README.md  data  docs  logs".to_string(),
        "uname" | "uname -a" => {
            "Linux anthracite 6.1.0-demo #1 SMP PREEMPT_DYNAMIC x86_64 GNU/Linux".to_string()
        }
        "date" => "Thu Jan  1 00:00:00 UTC 2026".to_string(),
        "clear" => {
            output.extend_from_slice(b"\x1b[2J\x1b[H");
            output.extend_from_slice(PROMPT.as_bytes());
            return;
        }
        "exit" | "logout" => {
            output.extend_from_slice(b"logout\r\n");
            session.closed = true;
            session.exit_code = Some(0);
            return;
        }
        other => match other.strip_prefix("echo ") {
            Some(rest) => rest.to_string(),
            None => format!(
                "demo: command not found: {}",
                other.split_whitespace().next().unwrap_or(other)
            ),
        },
    };
    if !reply.is_empty() {
        output.extend_from_slice(reply.as_bytes());
        output.extend_from_slice(b"\r\n");
    }
    output.extend_from_slice(PROMPT.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal() -> DemoTerminal {
        DemoTerminal::new(true, 4)
    }

    fn size() -> TerminalSize {
        TerminalSize { cols: 80, rows: 24 }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("runtime")
            .block_on(future)
    }

    fn drain(terminal: &DemoTerminal, owner: &str, id: &str) -> String {
        let mut text = String::new();
        loop {
            let chunk = block_on(terminal.read(owner, id, 0)).expect("read");
            if chunk.bytes.is_empty() {
                break;
            }
            text.push_str(&String::from_utf8_lossy(&chunk.bytes));
        }
        text
    }

    #[test]
    fn commands_are_virtual_and_echoed() {
        let terminal = terminal();
        let id = terminal.open("owner", "/bin/sh", size()).expect("open");
        let banner = drain(&terminal, "owner", &id);
        assert!(banner.contains("Anthracite demo shell"));

        terminal
            .write("owner", &id, b"whoami\r")
            .expect("write command");
        let output = drain(&terminal, "owner", &id);
        assert!(output.contains("whoami"));
        assert!(output.contains("demo\r\n"));
    }

    #[test]
    fn exit_ends_the_session() {
        let terminal = terminal();
        let id = terminal.open("owner", "/bin/sh", size()).expect("open");
        drain(&terminal, "owner", &id);
        terminal.write("owner", &id, b"exit\r").expect("exit");
        let output = drain(&terminal, "owner", &id);
        assert!(output.contains("logout"));
        let chunk = block_on(terminal.read("owner", &id, 0)).expect("read");
        assert!(chunk.eof);
        assert_eq!(chunk.exit_code, Some(0));
    }

    #[test]
    fn owners_are_isolated_and_sessions_are_capped() {
        let terminal = DemoTerminal::new(true, 1);
        let id = terminal.open("one", "/bin/sh", size()).expect("open");
        assert!(terminal.open("two", "/bin/sh", size()).is_err());
        assert!(terminal.write("two", &id, b"ls\r").is_err());
        assert_eq!(terminal.close_owner("one"), 1);
        assert_eq!(terminal.close_owner("one"), 0);
    }
}
