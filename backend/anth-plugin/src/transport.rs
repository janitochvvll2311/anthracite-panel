//! The three pipes an invocation can arrive on, and nothing else.
//!
//! Each type here models exactly one transport, so a plugin's mapper reads the
//! fields that transport actually has. Parsing returns a [`Response`] rather
//! than panicking: a caller who sent nonsense is an answer, not a trap.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde::Deserialize;
use serde_json::Value;

use crate::request::{Action, Request, Response, Status};

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
    pub query: alloc::collections::BTreeMap<String, String>,
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

/// One CLI invocation, as the host sends it.
///
/// `argv` is words a caller wrote, never an argv the host will run: a plugin
/// executes only through a capability, so these are a request to the plugin.
#[derive(Debug, Deserialize)]
pub struct CliRequest {
    #[serde(default)]
    pub argv: Vec<String>,
}

/// A parsed `argv`: a subcommand, positional words, and named options.
///
/// Built by [`CliRequest::parse`], which is where the flag forms are resolved
/// once so a plugin's mapper does not hand-roll a different rule per action.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CliArgs {
    pub command: String,
    pub positional: Vec<String>,
    pub flags: alloc::collections::BTreeMap<String, bool>,
    pub options: alloc::collections::BTreeMap<String, String>,
}

impl CliArgs {
    /// Resolves every supported form: `-v`, `--verbose`, `--verbose=true`,
    /// `--name value`, `-n value` and `--name=value`. `--` ends the flags.
    fn new(argv: &[String]) -> Result<Self, Response> {
        let mut words = argv.iter();
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
            command: command.clone(),
            ..Self::default()
        };
        let mut literal = false;
        while let Some(word) = words.next() {
            if literal {
                parsed.positional.push(word.clone());
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
                            let value = words.next().cloned().unwrap_or_default();
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
                        parsed.positional.push(word.clone());
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
                    let value = words.next().cloned().unwrap_or_default();
                    parsed.options.insert(short.to_string(), value);
                }
                continue;
            }
            parsed.positional.push(word.clone());
        }
        Ok(parsed)
    }

    pub fn positional(&self, index: usize) -> Option<&str> {
        self.positional.get(index).map(String::as_str)
    }

    pub fn flag(&self, name: &str) -> bool {
        self.flags.get(name).copied().unwrap_or(false)
    }

    pub fn option(&self, name: &str) -> Option<&str> {
        self.options.get(name).map(String::as_str)
    }

    /// The `value` a `Request` carries, so a mapper need not build one by hand.
    pub fn value(&self) -> Value {
        serde_json::json!({
            "command": self.command,
            "args": self.positional,
            "flags": self.flags,
            "options": self.options,
        })
    }

    /// Resolves the subcommand against the plugin's own table.
    ///
    /// The table is the only source of alias and of the message naming the other
    /// subcommands, so neither can drift from the other.
    pub fn action(&self, commands: &[(&str, Action)]) -> Result<Action, Response> {
        commands
            .iter()
            .find(|(name, _)| *name == self.command)
            .map(|(_, action)| *action)
            .ok_or_else(|| Response::unknown_command(&self.command, commands))
    }

    /// The whole mapping: alias to action, `argv` to `value`.
    pub fn into_request(self, commands: &[(&str, Action)]) -> Result<Request, Response> {
        let action = self.action(commands)?;
        Ok(Request::new(action, self.value()))
    }
}

impl CliRequest {
    pub fn parse(text: &str) -> Result<Self, Response> {
        serde_json::from_str::<Self>(text)
            .map_err(|error| Response::bad_request(alloc::format!("not a CLI request: {error}")))
    }

    /// The `argv`, resolved into flags, options and positionals.
    pub fn args(&self) -> Result<CliArgs, Response> {
        CliArgs::new(&self.argv)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> CliArgs {
        CliArgs::new(
            &words
                .iter()
                .map(|word| word.to_string())
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }

    fn refused(words: &[&str]) -> Response {
        CliArgs::new(
            &words
                .iter()
                .map(|word| word.to_string())
                .collect::<Vec<_>>(),
        )
        .unwrap_err()
    }

    #[test]
    fn a_subcommand_and_positionals_are_separated() {
        let parsed = argv(&["peers", "wg0", "wg1"]);

        assert_eq!(parsed.command, "peers");
        assert_eq!(parsed.positional, ["wg0", "wg1"]);
        assert!(parsed.flags.is_empty() && parsed.options.is_empty());
    }

    #[test]
    fn long_flags_are_booleans() {
        let parsed = argv(&["status", "--verbose"]);

        assert!(parsed.flag("verbose"));
        assert!(parsed.positional.is_empty());
    }

    /// A flag followed by another flag stays a boolean: consuming the next word
    /// as a value would swallow `--json` and leave the caller mystified.
    #[test]
    fn a_flag_before_another_flag_takes_no_value() {
        let parsed = argv(&["status", "--verbose", "--json"]);

        assert!(parsed.flag("verbose"));
        assert!(parsed.flag("json"));
        assert!(parsed.options.is_empty());
    }

    #[test]
    fn a_long_option_takes_the_next_word() {
        let parsed = argv(&["peers", "--interface", "wg0"]);

        assert_eq!(parsed.option("interface"), Some("wg0"));
        assert!(parsed.positional.is_empty());
    }

    #[test]
    fn a_long_option_takes_an_equals_form() {
        assert_eq!(
            argv(&["peers", "--interface=wg0"]).option("interface"),
            Some("wg0")
        );
    }

    #[test]
    fn a_long_flag_takes_an_equals_form_as_a_value() {
        assert_eq!(
            argv(&["status", "--verbose=true"]).option("verbose"),
            Some("true")
        );
    }

    #[test]
    fn a_short_option_takes_the_next_word() {
        let parsed = argv(&["peers", "-i", "wg0"]);

        assert_eq!(parsed.option("i"), Some("wg0"));
    }

    /// `-abc` is one flag, not `-a -b -c`: a plugin may name a subcommand with
    /// several letters and the parser must not invent a different meaning.
    #[test]
    fn a_bundled_short_word_stays_one_flag() {
        let parsed = argv(&["status", "-abc"]);

        assert!(parsed.flag("abc"));
        assert!(parsed.flags.len() == 1);
    }

    #[test]
    fn a_single_dash_is_a_positional() {
        let parsed = argv(&["upload", "-"]);

        assert_eq!(parsed.positional, ["-"]);
    }

    /// Everything after `--` is a word, even when it looks like a flag: that is
    /// how a caller passes a value that starts with a dash.
    #[test]
    fn a_double_dash_ends_the_flags() {
        let parsed = argv(&["peers", "--", "--not-a-flag"]);

        assert_eq!(parsed.positional, ["--not-a-flag"]);
        assert!(parsed.flags.is_empty());
    }

    #[test]
    fn an_empty_argv_is_a_bad_request() {
        assert_eq!(refused(&[]).status, Status::BadRequest);
    }

    #[test]
    fn a_leading_flag_is_a_bad_request_naming_it() {
        let response = refused(&["--verbose"]);

        assert_eq!(response.status, Status::BadRequest);
        assert!(
            response.value["error"]
                .as_str()
                .unwrap()
                .contains("--verbose")
        );
    }

    #[test]
    fn the_value_carries_every_class_of_word() {
        let parsed = argv(&["peers", "wg0", "--verbose", "--interface=wg1", "--", "-x"]);

        assert_eq!(
            parsed.value(),
            serde_json::json!({
                "command": "peers",
                "args": ["wg0", "-x"],
                "flags": { "verbose": true },
                "options": { "interface": "wg1" },
            })
        );
    }

    const STATUS: Action = Action::new("status");

    #[test]
    fn an_alias_resolves_to_its_action() {
        let parsed = argv(&["status"]);

        assert_eq!(parsed.action(&[("status", STATUS)]).unwrap(), STATUS);
    }

    #[test]
    fn an_unknown_alias_names_the_known_ones() {
        let response = argv(&["frobnicate"])
            .action(&[("status", STATUS)])
            .unwrap_err();

        assert_eq!(
            response.value["error"],
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

        let response = request.method().unwrap_err();

        assert!(response.value["error"].as_str().unwrap().contains("TRACE"));
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
