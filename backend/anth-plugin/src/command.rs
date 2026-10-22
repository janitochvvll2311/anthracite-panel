//! One command, whichever transport delivered it.
//!
//! The wire has two envelopes — an HTTP anchor and a CLI call — and a plugin
//! must not care which one arrived. [`Command`] is the single form both are
//! folded into: a method, a path, a query and an input.
//!
//! This is the same shape the daemon uses: one `dispatch` site that takes a
//! request it can compare, rather than one branch per transport. A plugin that
//! matched on the envelope instead would need a second table for the CLI, which
//! is how a route ends up reachable from the browser and not from `plugin call`.

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::envelope::{Reply, Request};

/// A request with its transport folded away.
///
/// `method` and `path` are what a route matches on. `query` and `input` are what
/// a handler reads. A CLI call has no query and turns its positional arguments
/// into `{"args": [...]}`, so the same handler serves both.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    method: String,
    path: String,
    query: BTreeMap<String, String>,
    input: Value,
}

impl Command {
    /// Builds a command directly.
    ///
    /// A plugin's own tests use this to call a handler with the shape a transport
    /// would have produced, without hand-writing an envelope. It is also how a
    /// handler is exercised for a method the caller cannot reach otherwise.
    pub fn new(
        method: impl Into<String>,
        path: impl Into<String>,
        query: BTreeMap<String, String>,
        input: Value,
    ) -> Self {
        Self {
            method: method.into().to_uppercase(),
            path: with_leading_slash(path.into()),
            query,
            input,
        }
    }

    /// The method, uppercased. `"GET"` for a CLI call that names none.
    pub fn method(&self) -> &str {
        &self.method
    }

    /// The operation, with a leading slash. This is the anchor path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// One query value. Empty for a CLI call, which has no query string.
    pub fn query(&self, key: &str) -> Option<&str> {
        self.query.get(key).map(String::as_str)
    }

    /// The arguments, as JSON.
    ///
    /// An empty body and a CLI call with no arguments both yield `{}`, so a
    /// handler never has to treat "absent" and "empty" as different.
    pub fn input(&self) -> &Value {
        &self.input
    }

    /// Parses the input into the shape a handler expects.
    ///
    /// A mismatch is a `400`: the plugin is fine, the caller sent the wrong
    /// shape. Returning `Err` rather than panicking keeps the panel connected
    /// and shows the operator what was expected.
    pub fn input_as<T: DeserializeOwned>(&self) -> Result<T, Reply> {
        serde_json::from_value(self.input.clone()).map_err(|error| {
            Reply::bad_request(alloc::format!(
                "the request input has the wrong shape: {error}"
            ))
        })
    }

    /// The CLI's positional arguments, in order.
    ///
    /// Empty for an HTTP call. A handler that serves both reads this instead of
    /// sniffing the input, so one route can accept `plugin call <id> '/status'`
    /// and `GET /status` alike.
    pub fn args(&self) -> Vec<String> {
        self.input
            .get("args")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|item| item.as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl Request {
    /// Folds the envelope into the one command form.
    ///
    /// An HTTP request keeps its method, path and query, and its body becomes
    /// the input: parsed when it is JSON, kept as a string when it is not,
    /// because an anchor is not required to speak JSON.
    ///
    /// A CLI call is `argv`: an optional method word, the operation, then the
    /// positional arguments. The operation is the same path the manifest
    /// declares, which is what lets `plugin call` reach every route rather than
    /// one default one.
    pub fn into_command(self) -> Result<Command, Reply> {
        match self {
            Request::Http(request) => {
                let input = match request.body {
                    None => json!({}),
                    Some(text) if text.trim().is_empty() => json!({}),
                    Some(text) => serde_json::from_str(&text).unwrap_or(Value::String(text)),
                };
                Ok(Command {
                    method: request.method.to_uppercase(),
                    path: with_leading_slash(request.path),
                    query: request.query,
                    input,
                })
            }
            Request::Cli(call) => cli_command(call.argv),
        }
    }
}

/// `["POST", "/run", "a"]` names an operation and its arguments.
///
/// A CLI call that names no operation is refused rather than defaulted: guessing
/// which route the caller meant is how a `plugin call` silently runs the wrong
/// thing.
fn cli_command(argv: Vec<String>) -> Result<Command, Reply> {
    let mut items = argv.into_iter();
    let first = items.next().ok_or_else(|| {
        Reply::bad_request(
            "a CLI call names no operation: pass the anchor path as the first argv item",
        )
    })?;

    let (method, path) = match crate::exec::Method::parse(&first) {
        Some(method) => {
            let path = items.next().ok_or_else(|| {
                Reply::bad_request(alloc::format!(
                    "`{first}` is followed by no operation; name an anchor path"
                ))
            })?;
            (method.as_str().to_string(), path)
        }
        None => ("GET".to_string(), first),
    };

    let rest: Vec<String> = items.collect();
    let input = if rest.is_empty() {
        json!({})
    } else {
        json!({ "args": rest })
    };

    Ok(Command {
        method,
        path: with_leading_slash(path),
        query: BTreeMap::new(),
        input,
    })
}

/// Normalizes the one thing both transports must agree on.
///
/// The host strips its own prefix and hands the guest a path it compares against
/// the anchors its manifest declares, so the leading slash has to survive either
/// way: `plugin call <id> '/status'` and `GET /status` must be the same route.
fn with_leading_slash(path: String) -> String {
    if path.starts_with('/') {
        path
    } else {
        alloc::format!("/{path}")
    }
}
