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

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: Action = Action::new("status");

    fn command_line(line: &str) -> CliRequest {
        CliRequest::new(line)
    }

    fn words(line: &str) -> Vec<String> {
        command_line(line).words().unwrap()
    }

    #[test]
    fn the_line_is_kept_exactly_as_it_arrived() {
        let request = command_line("greet --name \"Ana Sol\"");

        assert_eq!(request.line(), "greet --name \"Ana Sol\"");
    }

    /// The payload the host sends is a JSON string, so the quotes around it are
    /// the encoding rather than part of the line.
    #[test]
    fn a_json_string_payload_decodes_to_the_line_inside_it() {
        let request = CliRequest::parse(r#""peers --interface wg0""#).unwrap();

        assert_eq!(request.line(), "peers --interface wg0");
    }

    /// An object payload is what the previous contract sent, so it must be a
    /// loud `400` rather than a command named `{"argv":...}`.
    #[test]
    fn an_old_argv_payload_is_a_400_naming_the_expected_shape() {
        let refused = CliRequest::parse(r#"{"argv":["greet"]}"#).unwrap_err();

        assert_eq!(refused.status, Status::BadRequest);
        let message = refused.value["error"].as_str().unwrap();
        assert!(message.contains("JSON string"), "got: {message}");
    }

    #[test]
    fn a_subcommand_and_positionals_are_separated() {
        let request = command_line("peers wg0 wg1");

        assert_eq!(request.command().unwrap(), "peers");
        assert_eq!(request.positional().unwrap(), ["wg0", "wg1"]);
        assert!(!request.flag("verbose"));
        assert_eq!(request.option("interface"), None);
    }

    /// The reason the line is kept as text instead of arriving pre-split: a
    /// host that joined an `argv` loses the boundary, so the quotes are the only
    /// thing that keeps `"Ana Sol"` one word.
    #[test]
    fn a_quoted_argument_with_a_space_stays_one_word() {
        assert_eq!(
            words("greet --name \"Ana Sol\""),
            ["greet", "--name", "Ana Sol"]
        );
        assert_eq!(
            words("greet --name 'Ana Sol'"),
            ["greet", "--name", "Ana Sol"]
        );
    }

    #[test]
    fn an_escaped_space_stays_one_word() {
        assert_eq!(words("greet Ana\\ Sol"), ["greet", "Ana Sol"]);
    }

    #[test]
    fn single_quotes_are_literal() {
        assert_eq!(words("greet 'a \\n b'"), ["greet", "a \\n b"]);
    }

    #[test]
    fn double_quotes_resolve_only_the_two_escapes() {
        assert_eq!(words(r#"greet "a\"b""#), ["greet", "a\"b"]);
        assert_eq!(words(r#"greet "a\nb""#), ["greet", "a\\nb"]);
    }

    #[test]
    fn an_unterminated_quote_is_a_400() {
        let refused = command_line("greet \"Ana").words().unwrap_err();

        assert_eq!(refused.status, Status::BadRequest);
        assert!(refused.value["error"].as_str().unwrap().contains("quote"));
    }

    #[test]
    fn long_flags_are_booleans() {
        let request = command_line("status --verbose");

        assert!(request.flag("verbose"));
        assert!(request.positional().unwrap().is_empty());
    }

    /// A flag followed by another flag stays a boolean: consuming the next word
    /// as a value would swallow `--json` and leave the caller mystified.
    #[test]
    fn a_flag_before_another_flag_takes_no_value() {
        let request = command_line("status --verbose --json");

        assert!(request.flag("verbose"));
        assert!(request.flag("json"));
        assert_eq!(request.option("verbose"), None);
    }

    #[test]
    fn a_long_option_takes_the_next_word() {
        assert_eq!(
            command_line("peers --interface wg0").option("interface"),
            Some("wg0".to_string())
        );
    }

    #[test]
    fn a_long_option_takes_an_equals_form() {
        assert_eq!(
            command_line("peers --interface=wg0").option("interface"),
            Some("wg0".to_string())
        );
    }

    #[test]
    fn a_long_flag_takes_an_equals_form_as_a_value() {
        assert_eq!(
            command_line("status --verbose=true").option("verbose"),
            Some("true".to_string())
        );
    }

    #[test]
    fn a_short_option_takes_the_next_word() {
        assert_eq!(
            command_line("peers -i wg0").option("i"),
            Some("wg0".to_string())
        );
    }

    /// `-abc` is one flag, not `-a -b -c`: a plugin may name a subcommand with
    /// several letters and the parser must not invent a different meaning.
    #[test]
    fn a_bundled_short_word_stays_one_flag() {
        let request = command_line("status -abc");

        assert!(request.flag("abc"));
        assert!(!request.flag("a"));
    }

    #[test]
    fn a_single_dash_is_a_positional() {
        assert_eq!(command_line("upload -").positional().unwrap(), ["-"]);
    }

    /// Everything after `--` is a word, even when it looks like a flag: that is
    /// how a caller passes a value that starts with a dash.
    #[test]
    fn a_double_dash_ends_the_flags() {
        let request = command_line("peers -- --not-a-flag");

        assert_eq!(request.positional().unwrap(), ["--not-a-flag"]);
        assert!(!request.flag("not-a-flag"));
    }

    #[test]
    fn an_empty_line_is_a_bad_request() {
        assert_eq!(
            command_line("").command().unwrap_err().status,
            Status::BadRequest
        );
    }

    #[test]
    fn a_leading_flag_is_a_bad_request_naming_it() {
        let refused = command_line("--verbose").command().unwrap_err();

        assert_eq!(refused.status, Status::BadRequest);
        assert!(
            refused.value["error"]
                .as_str()
                .unwrap()
                .contains("--verbose")
        );
    }

    #[test]
    fn the_value_carries_every_class_of_word() {
        let request = command_line("peers wg0 --verbose --interface=wg1 -- -x");

        assert_eq!(
            request.value().unwrap(),
            serde_json::json!({
                "command": "peers",
                "args": ["wg0", "-x"],
                "flags": { "verbose": true },
                "options": { "interface": "wg1" },
            })
        );
    }

    #[test]
    fn an_alias_resolves_to_its_action() {
        let request = command_line("status");

        assert_eq!(request.action(&[("status", STATUS)]).unwrap(), STATUS);
    }

    #[test]
    fn an_unknown_alias_names_the_known_ones() {
        let refused = command_line("frobnicate")
            .action(&[("status", STATUS)])
            .unwrap_err();

        assert_eq!(
            refused.value["error"],
            serde_json::json!("no such subcommand `frobnicate`; try one of: status")
        );
    }

    #[test]
    fn the_http_path_gets_one_leading_slash() {
        let request = HttpRequest {
            method: "GET".to_string(),
            path: "status".to_string(),
            query: Default::default(),
            body: None,
        };
        assert_eq!(request.path(), "/status");

        let request = HttpRequest {
            method: "GET".to_string(),
            path: "//status".to_string(),
            query: Default::default(),
            body: None,
        };
        assert_eq!(
            request.path(),
            "/status",
            "a doubled slash is the same anchor"
        );
    }

    #[test]
    fn an_unknown_method_is_a_400_naming_it() {
        let request = HttpRequest {
            method: "TRACE".to_string(),
            path: "/status".to_string(),
            query: Default::default(),
            body: None,
        };

        assert!(
            request.method().unwrap_err().value["error"]
                .as_str()
                .unwrap()
                .contains("TRACE")
        );
    }

    #[test]
    fn a_body_is_parsed_only_when_it_is_there() {
        let request = HttpRequest {
            method: "POST".to_string(),
            path: "/peers".to_string(),
            query: Default::default(),
            body: Some(r#"{"interface":"wg0"}"#.to_string()),
        };

        #[derive(serde::Deserialize, PartialEq, Debug)]
        struct Body {
            interface: String,
        }

        assert_eq!(
            request.body_json::<Body>().unwrap(),
            Some(Body {
                interface: "wg0".to_string()
            })
        );
    }

    #[test]
    fn a_bad_body_is_a_400() {
        let request = HttpRequest {
            method: "POST".to_string(),
            path: "/peers".to_string(),
            query: Default::default(),
            body: Some("not json".to_string()),
        };

        assert_eq!(
            request.body_json::<serde_json::Value>().unwrap_err().status,
            Status::BadRequest
        );
    }

    #[test]
    fn a_query_value_is_read_by_name() {
        let request = HttpRequest::parse(
            r#"{"method":"GET","path":"/status","query":{"verbose":"1"},"body":null}"#,
        )
        .unwrap();

        assert_eq!(request.query("verbose"), Some("1"));
        assert_eq!(request.query("missing"), None);
    }
}
