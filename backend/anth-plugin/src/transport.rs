//! The three pipes an invocation can arrive on, and nothing else.
//!
//! Each type here models exactly one transport, so a plugin's mapper reads the
//! fields that transport actually has. Both entry types hold their payload as a
//! raw `String` and resolve it through accessors: `HttpRequest` keeps its body as
//! text, and `CliRequest` keeps its command line as text. Nothing is pre-parsed
//! into a shared shape, because a shape the plugin did not ask for is a shape the
//! plugin then has to work around.
//!
//! Parsing returns a [`Response`] rather than panicking: a caller who sent
//! nonsense is an answer, not a trap.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde::Deserialize;
use serde_json::Value;

use crate::message::{Action, Request, Response, Status};

/// The HTTP method, typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    pub const fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Patch => "PATCH",
            Method::Delete => "DELETE",
        }
    }

    /// Parses a method name, case-insensitively.
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_uppercase().as_str() {
            "GET" => Some(Method::Get),
            "POST" => Some(Method::Post),
            "PUT" => Some(Method::Put),
            "PATCH" => Some(Method::Patch),
            "DELETE" => Some(Method::Delete),
            _ => None,
        }
    }
}

impl core::fmt::Display for Method {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One HTTP anchor, as the host sends it.
///
/// `body` is the raw bytes as text: the host deliberately does not parse it,
/// because an anchor is not required to speak JSON and a host that parsed first
/// would reject a payload the plugin could have handled.
#[derive(Debug, Deserialize)]
pub struct HttpRequest {
    pub method: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub query: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<String>,
}

impl HttpRequest {
    /// Decodes one `anth_http` payload.
    pub fn parse(text: &str) -> Result<Self, Response> {
        serde_json::from_str::<Self>(text)
            .map_err(|error| Response::bad_request(alloc::format!("not an HTTP request: {error}")))
    }

    /// The method, or a `400` naming what arrived.
    pub fn method(&self) -> Result<Method, Response> {
        Method::parse(&self.method).ok_or_else(|| {
            Response::bad_request(alloc::format!("unknown method `{}`", self.method))
        })
    }

    /// The path with exactly one leading slash, which is how the manifest spells
    /// an anchor. axum strips the slash from a `{*path}` capture, so a mapper
    /// comparing against `"/status"` would otherwise never match.
    pub fn path(&self) -> String {
        let trimmed = self.path.trim_start_matches('/');
        alloc::format!("/{trimmed}")
    }

    pub fn query(&self, key: &str) -> Option<&str> {
        self.query.get(key).map(String::as_str)
    }

    /// The body parsed as `T`, or `None` when the body is empty.
    pub fn body_json<T: serde::de::DeserializeOwned>(&self) -> Result<Option<T>, Response> {
        let Some(text) = self.body.as_deref().filter(|text| !text.is_empty()) else {
            return Ok(None);
        };
        serde_json::from_str::<T>(text).map(Some).map_err(|error| {
            Response::bad_request(alloc::format!("the body is not what this expects: {error}"))
        })
    }
}

/// One CLI invocation, as the host sends it: the command line, as text.
///
/// The payload is a JSON string — `"greet --name \"Ana Sol\""` — and the type
/// holds it verbatim, the same way [`HttpRequest`] holds its body verbatim. A
/// plugin that wants words asks for them; one that wants to treat the line as an
/// opaque argument keeps it. Pre-splitting it into a `Vec<String>` on arrival
/// would decide for the plugin that whitespace is the only separator, which is
/// false the moment an argument contains a space.
///
/// `argv` is text a caller wrote, never an argv the host will run: a plugin
/// executes only through a capability, so this is a request to the plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliRequest {
    line: String,
}

impl CliRequest {
    /// Wraps a command line without validating it.
    ///
    /// Construction cannot fail and [`CliRequest::words`] does the validating, so
    /// a mapper that only wants [`CliRequest::line`] never pays for a parse it
    /// will not read.
    pub fn new(line: impl Into<String>) -> Self {
        Self { line: line.into() }
    }

    /// Decodes one `anth_cli` payload, which is a JSON string.
    ///
    /// Strictly a string, and nothing else. Accepting a bare line would be
    /// friendly and would also swallow an old `{"argv": [...]}` payload as the
    /// literal command `{"argv": [...]}` — a confusing "no such subcommand"
    /// instead of a `400` naming the shape the entry expects.
    pub fn parse(text: &str) -> Result<Self, Response> {
        match serde_json::from_str::<String>(text) {
            Ok(line) => Ok(Self::new(line)),
            Err(_) => {
                let shown: String = text.chars().take(80).collect();
                Err(Response::bad_request(alloc::format!(
                    "not a CLI request: expected a JSON string, got `{shown}`"
                )))
            }
        }
    }

    /// The command line exactly as it arrived.
    pub fn line(&self) -> &str {
        &self.line
    }

    /// The words of the line, with quotes and escapes resolved.
    pub fn words(&self) -> Result<Vec<String>, Response> {
        split(&self.line)
    }

    /// The first word, which is the subcommand.
    pub fn command(&self) -> Result<String, Response> {
        let words = self.words()?;
        match words.first() {
            None => Err(Response::new(
                Status::BadRequest,
                serde_json::json!({ "error": "name a subcommand" }),
            )),
            Some(word) if word.starts_with('-') => Err(Response::bad_request(alloc::format!(
                "the first word must be a subcommand, not the flag `{word}`"
            ))),
            Some(word) => Ok(word.clone()),
        }
    }

    /// Whether `--name` (or `-n`) appeared as a bare flag.
    pub fn flag(&self, name: &str) -> bool {
        self.parsed().is_ok_and(|parsed| {
            parsed.flags.contains_key(name)
                || parsed
                    .options
                    .get(name)
                    .is_some_and(|value| value == "true")
        })
    }

    /// The value of `--name value`, `--name=value`, `-n value` or `-n=value`.
    pub fn option(&self, name: &str) -> Option<String> {
        self.parsed().ok()?.options.get(name).cloned()
    }

    /// The positional words, excluding the subcommand and every flag.
    pub fn positional(&self) -> Result<Vec<String>, Response> {
        Ok(self.parsed()?.positional)
    }

    /// Resolves the subcommand against the plugin's own table.
    ///
    /// The table is the only source of the alias and of the message naming the
    /// other subcommands, so neither can drift from the other.
    pub fn action(&self, commands: &[(&str, Action)]) -> Result<Action, Response> {
        let command = self.command()?;
        commands
            .iter()
            .find(|(name, _)| *name == command)
            .map(|(_, action)| *action)
            .ok_or_else(|| Response::unknown_command(&command, commands))
    }

    /// The `value` a [`Request`] carries.
    pub fn value(&self) -> Result<Value, Response> {
        let parsed = self.parsed()?;
        Ok(serde_json::json!({
            "command": parsed.command,
            "args": parsed.positional,
            "flags": parsed.flags,
            "options": parsed.options,
        }))
    }

    /// The whole mapping: alias to action, line to `value`.
    pub fn into_request(self, commands: &[(&str, Action)]) -> Result<Request, Response> {
        let action = self.action(commands)?;
        Ok(Request::new(action, self.value()?))
    }

    fn parsed(&self) -> Result<Parsed, Response> {
        Parsed::new(&self.line)
    }
}

/// The words of a line, resolved once, for the accessors that need the classes
/// kept apart. Private on purpose: this is how a line is read, not a second type
/// for a plugin to learn.
#[derive(Debug, Default)]
struct Parsed {
    command: String,
    positional: Vec<String>,
    flags: BTreeMap<String, bool>,
    options: BTreeMap<String, String>,
}

impl Parsed {
    fn new(line: &str) -> Result<Self, Response> {
        let mut words = split(line)?.into_iter();
        let Some(command) = words.next() else {
            return Err(Response::new(
                Status::BadRequest,
                serde_json::json!({ "error": "name a subcommand" }),
            ));
        };
        if command.starts_with('-') {
            return Err(Response::bad_request(alloc::format!(
                "the first word must be a subcommand, not the flag `{command}`"
            )));
        }

        let mut parsed = Self {
            command,
            ..Self::default()
        };
        let mut literal = false;
        while let Some(word) = words.next() {
            if literal {
                parsed.positional.push(word);
                continue;
            }
            if word == "--" {
                literal = true;
                continue;
            }
            if let Some(long) = word.strip_prefix("--") {
                if long.is_empty() {
                    literal = true;
                    continue;
                }
                match long.split_once('=') {
                    Some((name, value)) => {
                        parsed.options.insert(name.to_string(), value.to_string());
                    }
                    None => {
                        // `--name value` consumes the next word unless it is a
                        // flag itself, so `--verbose --other` stays a boolean.
                        let boolean = words
                            .clone()
                            .next()
                            .is_none_or(|next| next.starts_with('-'));
                        if boolean {
                            parsed.flags.insert(long.to_string(), true);
                        } else {
                            let value = words.next().unwrap_or_default();
                            parsed.options.insert(long.to_string(), value);
                        }
                    }
                }
                continue;
            }
            if let Some(short) = word.strip_prefix('-') {
                if short.is_empty() || short.contains('=') {
                    // `-` alone is a positional (stdin by convention), and
                    // `-abc` is not split: a plugin may have a one-letter name.
                    if short.is_empty() {
                        parsed.positional.push(word);
                    } else {
                        let (name, value) = short.split_once('=').unwrap_or((short, ""));
                        parsed.options.insert(name.to_string(), value.to_string());
                    }
                    continue;
                }
                let boolean = words
                    .clone()
                    .next()
                    .is_none_or(|next| next.starts_with('-'));
                if boolean || short.len() > 1 {
                    parsed.flags.insert(short.to_string(), true);
                } else {
                    let value = words.next().unwrap_or_default();
                    parsed.options.insert(short.to_string(), value);
                }
                continue;
            }
            parsed.positional.push(word);
        }
        Ok(parsed)
    }
}

/// Splits a command line into words, resolving quotes and escapes.
///
/// Whitespace separates words, `'...'` is literal, `"..."` is literal except for
/// `\"` and `\\`, and a backslash outside quotes escapes the next character. A
/// plain whitespace split would be smaller, and wrong the first time an argument
/// contains a space: the host joins an `argv` into this line, so the quotes are
/// what keeps a word from being split in two on the way back.
fn split(line: &str) -> Result<Vec<String>, Response> {
    #[derive(PartialEq)]
    enum State {
        Plain,
        Single,
        Double,
    }

    let mut words = Vec::new();
    let mut word = String::new();
    let mut started = false;
    let mut state = State::Plain;
    let mut characters = line.chars().peekable();

    while let Some(character) = characters.next() {
        match state {
            State::Plain => match character {
                '\'' => {
                    state = State::Single;
                    started = true;
                }
                '"' => {
                    state = State::Double;
                    started = true;
                }
                '\\' => {
                    if let Some(escaped) = characters.next() {
                        word.push(escaped);
                        started = true;
                    }
                }
                character if character.is_whitespace() => {
                    if started {
                        words.push(core::mem::take(&mut word));
                        started = false;
                    }
                }
                character => {
                    word.push(character);
                    started = true;
                }
            },
            State::Single => match character {
                '\'' => state = State::Plain,
                character => word.push(character),
            },
            State::Double => match character {
                '"' => state = State::Plain,
                '\\' => match characters.next() {
                    Some(escaped @ ('"' | '\\')) => word.push(escaped),
                    // Inside double quotes a backslash before anything else is
                    // literal, as a shell treats it.
                    Some(other) => {
                        word.push('\\');
                        word.push(other);
                    }
                    None => word.push('\\'),
                },
                character => word.push(character),
            },
        }
    }

    if state != State::Plain {
        return Err(Response::bad_request(
            "the command line has an unterminated quote",
        ));
    }
    if started {
        words.push(word);
    }
    Ok(words)
}
