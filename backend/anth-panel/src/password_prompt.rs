//! Interactive password prompts for the CLI.
//!
//! Shared by `account user password` and `auth set-password`, so echo is turned
//! off in one place and a password never reaches an argument vector.

use std::io::{BufRead, IsTerminal, Read};

use anth_proto::SecretString;

/// Reads a password without ever putting it on a command line.
///
/// Interaction is on `/dev/tty` rather than stdin when stdin is a terminal, so
/// `--stdin` and an interactive prompt cannot be confused. Echo is disabled for
/// the prompt and the terminal is restored even when reading fails.
pub fn read_password(from_stdin: bool) -> Result<SecretString, String> {
    if from_stdin {
        let mut input = String::new();
        std::io::stdin()
            .read_to_string(&mut input)
            .map_err(|error| format!("failed to read the password from stdin: {error}"))?;
        // One line, as `passwd --stdin` reads it. Reading the whole stream would
        // make a caller that pipes `printf 'pw\npw\n'` store the newline and
        // the repetition as part of the password, which then never matches.
        return password_from_stdin(&input);
    }

    if !std::io::stdin().is_terminal() {
        return Err(
            "refusing to prompt for a password when stdin is not a terminal; pipe it with --stdin"
                .to_string(),
        );
    }

    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|error| format!("could not open /dev/tty: {error}"))?;
    let mut reader = std::io::BufReader::new(&tty);
    let mut writer = &tty;

    let first = prompt_hidden(&mut reader, &mut writer, "Password: ")?;
    let second = prompt_hidden(&mut reader, &mut writer, "Repeat password: ")?;
    if first != second {
        return Err("the passwords do not match".to_string());
    }
    Ok(SecretString::new(first))
}

/// Picks the password out of a piped stream.
///
/// One line, as `passwd --stdin` reads it. Reading the whole stream would make
/// a caller that pipes `printf 'pw\npw\n'` store the newline and the
/// repetition as part of the password, which then never matches.
fn password_from_stdin(input: &str) -> Result<SecretString, String> {
    let password = input.lines().next().unwrap_or("");
    let password = password.strip_suffix('\r').unwrap_or(password);
    if password.is_empty() {
        return Err("the password read from stdin is empty".to_string());
    }
    Ok(SecretString::new(password))
}

fn prompt_hidden(
    reader: &mut impl BufRead,
    writer: &mut impl std::io::Write,
    prompt: &str,
) -> Result<String, String> {
    write!(writer, "{prompt}").map_err(|error| format!("failed to write the prompt: {error}"))?;
    writer.flush().ok();

    let echo_off = EchoGuard::disable();
    let mut line = String::new();
    let read = reader.read_line(&mut line);
    drop(echo_off);
    writeln!(writer).ok();

    read.map_err(|error| format!("failed to read the password: {error}"))?;
    let line = line
        .strip_suffix('\n')
        .unwrap_or(&line)
        .strip_suffix('\r')
        .unwrap_or(&line)
        .to_string();
    if line.is_empty() {
        return Err("the password is empty".to_string());
    }
    Ok(line)
}

/// Turns terminal echo off for as long as it is alive, including on unwind.
struct EchoGuard {
    original: Option<libc::termios>,
}

impl EchoGuard {
    fn disable() -> Self {
        let mut original: libc::termios = unsafe { std::mem::zeroed() };
        // fd 0 is the terminal in this path; a failure leaves echo on, which is
        // a privacy bug but not a hang, so it does not abort the prompt.
        if unsafe { libc::tcgetattr(libc::STDIN_FILENO, &mut original) } != 0 {
            return Self { original: None };
        }
        let mut hidden = original;
        hidden.c_lflag &= !libc::ECHO;
        if unsafe { libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &hidden) } != 0 {
            return Self { original: None };
        }
        Self {
            original: Some(original),
        }
    }
}

impl Drop for EchoGuard {
    fn drop(&mut self) {
        if let Some(original) = self.original {
            unsafe {
                libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &original);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_line_is_the_password() {
        let password = password_from_stdin("hunter2\n").unwrap();
        assert_eq!(password.expose(), "hunter2");
    }

    #[test]
    fn a_repeated_line_is_not_part_of_the_password() {
        let password = password_from_stdin("hunter2\nhunter2\n").unwrap();
        assert_eq!(password.expose(), "hunter2");
    }

    #[test]
    fn a_crlf_ending_is_stripped() {
        let password = password_from_stdin("hunter2\r\n").unwrap();
        assert_eq!(password.expose(), "hunter2");
    }

    #[test]
    fn an_empty_stream_is_refused() {
        assert!(password_from_stdin("").is_err());
        assert!(password_from_stdin("\n").is_err());
    }
}
