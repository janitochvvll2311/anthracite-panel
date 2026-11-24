//! The proxy's one document: listeners, upstreams, hosts and routes.
//!
//! `apply` validates the whole document before anything swaps, so a bad edit
//! leaves the previous config running. Route precedence is fixed here and
//! unit-tested: exact beats prefix, the longest prefix wins, and the declared
//! order breaks a tie.

use std::collections::{BTreeMap, BTreeSet};

use anth_contract::Error;
use http::{HeaderName, HeaderValue, Uri};
use serde::{Deserialize, Serialize};

/// Body cap when a route does not declare one.
pub const DEFAULT_BODY_LIMIT: u64 = 10 * 1024 * 1024;
/// Hard ceiling for a per-route body limit.
pub const MAX_BODY_LIMIT: u64 = 1024 * 1024 * 1024;
const MAX_TIMEOUT_MS: u32 = 300_000;

/// The binds frozen in the manifest argv. A config may select one or both but
/// cannot invent a new port: the grant shows the listeners the operator
/// approved, and the document only maps hosts onto them.
#[derive(Debug, Clone)]
pub struct Endpoints {
    pub http: String,
    pub https: String,
}

impl Endpoints {
    pub fn allows(&self, bind: &str, tls: bool) -> bool {
        if tls {
            bind == self.https
        } else {
            bind == self.http
        }
    }

    pub fn contains(&self, bind: &str) -> bool {
        bind == self.http || bind == self.https
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    pub version: u64,
    pub listeners: Vec<Listener>,
    pub upstreams: Vec<Upstream>,
    pub hosts: Vec<Host>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Listener {
    pub id: String,
    pub bind: String,
    pub tls: bool,
    pub default_host: Option<String>,
    /// Add `X-Forwarded-*` and trust inbound ones. Off by default: the proxy
    /// must not forge a forwarded host for a remote client (design §6).
    pub forwarded: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Upstream {
    pub id: String,
    pub url: String,
    pub connect_timeout_ms: u32,
    pub read_timeout_ms: u32,
    pub tls_verify: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Header {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Host {
    pub id: String,
    pub names: Vec<String>,
    pub certificate: Option<String>,
    pub acme: bool,
    /// Redirect plain HTTP for this host to the TLS listener.
    pub redirect_https: bool,
    /// Send HSTS on TLS responses for this host.
    pub hsts: bool,
    pub routes: Vec<Route>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Route {
    pub id: String,
    /// `/` for a prefix or `= /health` for an exact match.
    #[serde(rename = "match")]
    pub r#match: String,
    pub upstream: Option<String>,
    pub strip_prefix: bool,
    pub websocket: bool,
    pub redirect: Option<String>,
    pub headers: Vec<Header>,
    pub body_limit: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Matcher {
    Exact(String),
    Prefix(String),
}

impl Route {
    /// The parsed form of `match`; validation has already refused anything
    /// else.
    pub fn matcher(&self) -> Matcher {
        match self.r#match.strip_prefix("= ") {
            Some(path) => Matcher::Exact(path.to_string()),
            None => Matcher::Prefix(self.r#match.clone()),
        }
    }

    pub fn body_limit(&self) -> u64 {
        self.body_limit.unwrap_or(DEFAULT_BODY_LIMIT)
    }
}

impl Config {
    pub fn host(&self, id: &str) -> Option<&Host> {
        self.hosts.iter().find(|host| host.id == id)
    }

    pub fn upstream(&self, id: &str) -> Option<&Upstream> {
        self.upstreams.iter().find(|upstream| upstream.id == id)
    }

    pub fn listener(&self, bind: &str) -> Option<&Listener> {
        self.listeners.iter().find(|listener| listener.bind == bind)
    }

    /// Validates the whole document against the endpoints the manifest froze
    /// and the certificates currently published. The error names the field.
    pub fn validate(
        &self,
        endpoints: &Endpoints,
        certificates: &BTreeSet<String>,
    ) -> Result<(), Error> {
        self.validate_listeners(endpoints)?;
        self.validate_upstreams()?;
        self.validate_hosts(certificates)?;
        if !self.listeners.is_empty() && self.hosts.is_empty() {
            return Err(invalid(
                "hosts",
                "a configured listener needs at least one host",
            ));
        }
        Ok(())
    }

    fn validate_listeners(&self, endpoints: &Endpoints) -> Result<(), Error> {
        let mut ids = BTreeSet::new();
        let mut binds = BTreeSet::new();
        for (index, listener) in self.listeners.iter().enumerate() {
            let at = format!("listeners[{index}]");
            if listener.id.trim().is_empty() {
                return Err(invalid(&format!("{at}.id"), "an id is required"));
            }
            if !ids.insert(listener.id.clone()) {
                return Err(invalid(
                    &format!("{at}.id"),
                    format!("`{}` is declared twice", listener.id),
                ));
            }
            if !endpoints.contains(&listener.bind) {
                return Err(invalid(
                    &format!("{at}.bind"),
                    format!(
                        "`{}` is not one of the manifest listeners ({} or {})",
                        listener.bind, endpoints.http, endpoints.https
                    ),
                ));
            }
            if !endpoints.allows(&listener.bind, listener.tls) {
                return Err(invalid(
                    &format!("{at}.tls"),
                    "the bind and the TLS flag disagree with the manifest listener",
                ));
            }
            if !binds.insert(listener.bind.clone()) {
                return Err(invalid(
                    &format!("{at}.bind"),
                    format!("`{}` is declared twice", listener.bind),
                ));
            }
            if let Some(host) = &listener.default_host {
                let target = self.host(host).ok_or_else(|| {
                    invalid(
                        &format!("{at}.default-host"),
                        format!("`{host}` is not a declared host"),
                    )
                })?;
                if listener.tls && target.certificate.is_none() {
                    return Err(invalid(
                        &format!("{at}.default-host"),
                        "a TLS listener needs a certificate on its default host",
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_upstreams(&self) -> Result<(), Error> {
        let mut ids = BTreeSet::new();
        for (index, upstream) in self.upstreams.iter().enumerate() {
            let at = format!("upstreams[{index}]");
            if upstream.id.trim().is_empty() {
                return Err(invalid(&format!("{at}.id"), "an id is required"));
            }
            if !ids.insert(upstream.id.clone()) {
                return Err(invalid(
                    &format!("{at}.id"),
                    format!("`{}` is declared twice", upstream.id),
                ));
            }
            parse_upstream_url(&upstream.url)
                .map_err(|message| invalid(&format!("{at}.url"), message))?;
            for (field, value) in [
                ("connect-timeout-ms", upstream.connect_timeout_ms),
                ("read-timeout-ms", upstream.read_timeout_ms),
            ] {
                if value == 0 || value > MAX_TIMEOUT_MS {
                    return Err(invalid(
                        &format!("{at}.{field}"),
                        format!("must be between 1 and {MAX_TIMEOUT_MS}"),
                    ));
                }
            }
        }
        Ok(())
    }

    fn validate_hosts(&self, certificates: &BTreeSet<String>) -> Result<(), Error> {
        let mut ids = BTreeSet::new();
        let mut names = BTreeMap::<String, String>::new();
        for (index, host) in self.hosts.iter().enumerate() {
            let at = format!("hosts[{index}]");
            if host.id.trim().is_empty() {
                return Err(invalid(&format!("{at}.id"), "an id is required"));
            }
            if !ids.insert(host.id.clone()) {
                return Err(invalid(
                    &format!("{at}.id"),
                    format!("`{}` is declared twice", host.id),
                ));
            }
            if host.names.is_empty() {
                return Err(invalid(
                    &format!("{at}.names"),
                    "at least one name is required",
                ));
            }
            for (name_index, name) in host.names.iter().enumerate() {
                let normalized = normalize_name(name).ok_or_else(|| {
                    invalid(
                        &format!("{at}.names[{name_index}]"),
                        format!("`{name}` is not a host name"),
                    )
                })?;
                if let Some(other) = names.insert(normalized.clone(), host.id.clone()) {
                    if other != host.id {
                        return Err(invalid(
                            &format!("{at}.names[{name_index}]"),
                            format!("`{normalized}` is already used by `{other}`"),
                        ));
                    }
                }
            }
            if let Some(id) = &host.certificate {
                if id.trim().is_empty() || !certificates.contains(id) {
                    return Err(invalid(
                        &format!("{at}.certificate"),
                        format!("`{id}` is not published under the TLS directory"),
                    ));
                }
            }
            let mut route_ids = BTreeSet::new();
            for (route_index, route) in host.routes.iter().enumerate() {
                let rat = format!("{at}.routes[{route_index}]");
                if route.id.trim().is_empty() {
                    return Err(invalid(&format!("{rat}.id"), "an id is required"));
                }
                if !route_ids.insert(route.id.clone()) {
                    return Err(invalid(
                        &format!("{rat}.id"),
                        format!("`{}` is declared twice", route.id),
                    ));
                }
                validate_match(&route.r#match)
                    .map_err(|message| invalid(&format!("{rat}.match"), message))?;
                match (&route.upstream, &route.redirect) {
                    (None, None) => {
                        return Err(invalid(
                            &rat,
                            "a route needs exactly one of `upstream` or `redirect`",
                        ))
                    }
                    (Some(_), Some(_)) => {
                        return Err(invalid(
                            &rat,
                            "a route needs exactly one of `upstream` or `redirect`",
                        ))
                    }
                    (Some(upstream), None) => {
                        if self.upstream(upstream).is_none() {
                            return Err(invalid(
                                &format!("{rat}.upstream"),
                                format!("`{upstream}` is not a declared upstream"),
                            ));
                        }
                    }
                    (None, Some(redirect)) => {
                        validate_redirect(redirect)
                            .map_err(|message| invalid(&format!("{rat}.redirect"), message))?;
                    }
                }
                if route.strip_prefix && route.r#match.starts_with("= ") {
                    return Err(invalid(
                        &format!("{rat}.strip-prefix"),
                        "an exact route has no prefix to strip",
                    ));
                }
                if route.websocket && route.redirect.is_some() {
                    return Err(invalid(
                        &format!("{rat}.websocket"),
                        "a redirect route cannot upgrade",
                    ));
                }
                if let Some(limit) = route.body_limit {
                    if limit == 0 || limit > MAX_BODY_LIMIT {
                        return Err(invalid(
                            &format!("{rat}.body-limit"),
                            format!("must be between 1 and {MAX_BODY_LIMIT}"),
                        ));
                    }
                }
                for (header_index, header) in route.headers.iter().enumerate() {
                    let hat = format!("{rat}.headers[{header_index}]");
                    HeaderName::from_bytes(header.name.as_bytes())
                        .map_err(|_| invalid(&format!("{hat}.name"), "not a valid header name"))?;
                    HeaderValue::from_str(&header.value).map_err(|_| {
                        invalid(
                            &format!("{hat}.value"),
                            "not a valid header value (control characters?)",
                        )
                    })?;
                }
            }
        }
        Ok(())
    }
}

/// Selects the route for `path`: exact wins, then the longest prefix, then the
/// declared order.
pub fn match_route<'a>(routes: &'a [Route], path: &str) -> Option<&'a Route> {
    let mut exact: Option<&Route> = None;
    let mut prefix: Option<(&Route, usize)> = None;
    for route in routes {
        match route.matcher() {
            Matcher::Exact(pattern) => {
                if exact.is_none() && pattern == path {
                    exact = Some(route);
                }
            }
            Matcher::Prefix(pattern) => {
                if path.starts_with(&pattern) {
                    let length = pattern.len();
                    if prefix.map(|(_, best)| length > best).unwrap_or(true) {
                        prefix = Some((route, length));
                    }
                }
            }
        }
    }
    exact.or_else(|| prefix.map(|(route, _)| route))
}

fn validate_match(pattern: &str) -> Result<(), String> {
    let path = pattern.strip_prefix("= ").unwrap_or(pattern);
    if !path.starts_with('/') {
        return Err("a match starts with `/`".to_string());
    }
    if path
        .chars()
        .any(|character| character.is_whitespace() || character.is_control())
    {
        return Err("a match cannot contain whitespace or control characters".to_string());
    }
    if pattern.starts_with("= ") && pattern.len() < 3 {
        return Err("an exact match needs a path".to_string());
    }
    Ok(())
}

fn validate_redirect(target: &str) -> Result<(), String> {
    if target.starts_with('/') {
        HeaderValue::from_str(target).map_err(|_| "not a valid redirect value".to_string())?;
        return Ok(());
    }
    let uri: Uri = target
        .parse()
        .map_err(|_| "a redirect needs an absolute http(s) URL or a path".to_string())?;
    match uri.scheme_str() {
        Some("http") | Some("https") if uri.authority().is_some() => Ok(()),
        _ => Err("a redirect needs an absolute http(s) URL or a path".to_string()),
    }
}

/// Normalizes and validates a configured host name. Wildcards are not part of
/// v0.7: an unmatched name goes to the listener default, never a guess.
pub fn normalize_name(name: &str) -> Option<String> {
    let name = name.trim().trim_end_matches('.').to_ascii_lowercase();
    if name.is_empty() || name.len() > 253 || name.contains('*') || name.contains('/') {
        return None;
    }
    if name.contains(':') {
        return None;
    }
    let labels: Vec<&str> = name.split('.').collect();
    if labels
        .iter()
        .any(|label| label.is_empty() || label.len() > 63)
    {
        return None;
    }
    if labels.iter().any(|label| {
        label.starts_with('-')
            || label.ends_with('-')
            || !label
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || character == '-')
    }) {
        return None;
    }
    Some(name)
}

/// Strips an optional port and trailing dot, lowercases.
pub fn normalize_host_header(raw: &str) -> String {
    let raw = raw.trim();
    let host = if let Some(rest) = raw.strip_prefix('[') {
        rest.split(']').next().unwrap_or(rest)
    } else {
        raw.split(':').next().unwrap_or(raw)
    };
    host.trim_end_matches('.').to_ascii_lowercase()
}

/// The upstream origin as `scheme://authority`, with no path.
pub fn parse_upstream_url(url: &str) -> Result<(String, String), String> {
    let uri: Uri = url.parse().map_err(|_| "not a valid URL".to_string())?;
    let scheme = uri.scheme_str().ok_or("a URL needs a scheme")?;
    if scheme != "http" && scheme != "https" {
        return Err("an upstream scheme is http or https".to_string());
    }
    let authority = uri.authority().ok_or("a URL needs a host")?;
    if authority.host().is_empty() {
        return Err("a URL needs a host".to_string());
    }
    let path = uri.path();
    if !path.is_empty() && path != "/" {
        return Err("an upstream URL carries no path".to_string());
    }
    if uri.query().is_some() {
        return Err("an upstream URL carries no query".to_string());
    }
    Ok((scheme.to_string(), authority.to_string()))
}

fn invalid(path: &str, message: impl Into<String>) -> Error {
    Error::invalid(format!("{path}: {}", message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn endpoints() -> Endpoints {
        Endpoints {
            http: ":8080".to_string(),
            https: ":8443".to_string(),
        }
    }

    fn route(id: &str, pattern: &str, upstream: &str) -> Route {
        Route {
            id: id.to_string(),
            r#match: pattern.to_string(),
            upstream: Some(upstream.to_string()),
            strip_prefix: false,
            websocket: false,
            redirect: None,
            headers: Vec::new(),
            body_limit: None,
        }
    }

    fn config(routes: Vec<Route>) -> Config {
        Config {
            version: 0,
            listeners: vec![Listener {
                id: "plain".to_string(),
                bind: ":8080".to_string(),
                tls: false,
                default_host: None,
                forwarded: false,
            }],
            upstreams: vec![Upstream {
                id: "app".to_string(),
                url: "http://127.0.0.1:3000".to_string(),
                connect_timeout_ms: 1000,
                read_timeout_ms: 5000,
                tls_verify: true,
            }],
            hosts: vec![Host {
                id: "site".to_string(),
                names: vec!["Example.COM.".to_string()],
                certificate: None,
                acme: true,
                redirect_https: false,
                hsts: false,
                routes,
            }],
        }
    }

    #[test]
    fn exact_beats_a_longer_prefix() {
        let routes = vec![
            route("api", "/api", "app"),
            route("health", "= /api/health", "app"),
            route("everything", "/", "app"),
        ];
        let chosen = match_route(&routes, "/api/health").expect("a route");
        assert_eq!(chosen.id, "health");
    }

    #[test]
    fn the_longest_prefix_wins() {
        let routes = vec![
            route("everything", "/", "app"),
            route("api", "/api", "app"),
            route("v2", "/api/v2", "app"),
        ];
        assert_eq!(match_route(&routes, "/api/v2/things").unwrap().id, "v2");
        assert_eq!(match_route(&routes, "/api/v1").unwrap().id, "api");
        assert_eq!(match_route(&routes, "/other").unwrap().id, "everything");
        assert!(match_route(&routes, "/x").is_some());
    }

    #[test]
    fn the_declared_order_breaks_a_tie() {
        let routes = vec![route("first", "/a", "app"), route("second", "/a", "app")];
        assert_eq!(match_route(&routes, "/a/b").unwrap().id, "first");
    }

    #[test]
    fn a_document_round_trips_through_json() {
        let document = config(vec![route("root", "/", "app")]);
        let json = serde_json::to_string(&document).expect("json");
        let parsed: Config = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed, document);
        assert!(json.contains("\"match\":\"/\""));
    }

    #[test]
    fn a_manifest_listener_is_the_only_allowed_bind() {
        let mut document = config(vec![route("root", "/", "app")]);
        document.listeners[0].bind = ":9000".to_string();
        let error = document
            .validate(&endpoints(), &BTreeSet::new())
            .expect_err("refused");
        assert!(error.message().contains("listeners[0].bind"), "{error:?}");
    }

    #[test]
    fn the_tls_flag_must_match_the_manifest_bind() {
        let mut document = config(vec![route("root", "/", "app")]);
        document.listeners[0].tls = true;
        let error = document
            .validate(&endpoints(), &BTreeSet::new())
            .expect_err("refused");
        assert!(error.message().contains("listeners[0].tls"), "{error:?}");
    }

    #[test]
    fn duplicate_host_names_are_refused_across_hosts() {
        let mut document = config(vec![route("root", "/", "app")]);
        let mut second = document.hosts[0].clone();
        second.id = "other".to_string();
        second.names = vec!["example.com".to_string()];
        document.hosts.push(second);
        let error = document
            .validate(&endpoints(), &BTreeSet::new())
            .expect_err("refused");
        assert!(error.message().contains("already used"), "{error:?}");
    }

    #[test]
    fn a_route_needs_exactly_one_target() {
        let mut missing = config(vec![route("root", "/", "app")]);
        missing.hosts[0].routes[0].upstream = None;
        let error = missing
            .validate(&endpoints(), &BTreeSet::new())
            .expect_err("no target is refused");
        assert!(error.message().contains("exactly one"), "{error:?}");

        let mut both = config(vec![route("root", "/", "app")]);
        both.hosts[0].routes[0].redirect = Some("/elsewhere".to_string());
        assert!(
            both.validate(&endpoints(), &BTreeSet::new()).is_err(),
            "two targets are refused"
        );
    }

    #[test]
    fn an_unknown_certificate_is_a_validation_error() {
        let mut document = config(vec![route("root", "/", "app")]);
        document.hosts[0].certificate = Some("site".to_string());
        let error = document
            .validate(&endpoints(), &BTreeSet::new())
            .expect_err("refused");
        assert!(
            error.message().contains("hosts[0].certificate"),
            "{error:?}"
        );

        let published = BTreeSet::from(["site".to_string()]);
        document
            .validate(&endpoints(), &published)
            .expect("the published id is accepted");
    }

    #[test]
    fn an_upstream_carries_no_path_or_query() {
        assert!(parse_upstream_url("http://127.0.0.1:3000").is_ok());
        assert!(parse_upstream_url("https://app:8443/").is_ok());
        assert!(parse_upstream_url("http://127.0.0.1:3000/api").is_err());
        assert!(parse_upstream_url("ftp://127.0.0.1").is_err());
        assert!(parse_upstream_url("unix:/run/app.sock").is_err());
    }

    #[test]
    fn host_names_are_normalized_and_wildcards_are_refused() {
        assert_eq!(
            normalize_name("Example.COM."),
            Some("example.com".to_string())
        );
        assert_eq!(normalize_name("localhost"), Some("localhost".to_string()));
        assert_eq!(normalize_name("*.example.com"), None);
        assert_eq!(normalize_name("bad name"), None);
        assert_eq!(normalize_name("bad/name"), None);
        assert_eq!(normalize_name("-bad.example"), None);
    }

    #[test]
    fn a_host_header_drops_its_port_and_case() {
        assert_eq!(normalize_host_header("Example.COM:8080"), "example.com");
        assert_eq!(normalize_host_header("example.com."), "example.com");
        assert_eq!(normalize_host_header("[::1]:8080"), "::1");
    }

    #[test]
    fn a_tls_default_host_needs_a_certificate() {
        let mut document = config(vec![route("root", "/", "app")]);
        document.listeners = vec![Listener {
            id: "secure".to_string(),
            bind: ":8443".to_string(),
            tls: true,
            default_host: Some("site".to_string()),
            forwarded: false,
        }];
        let error = document
            .validate(&endpoints(), &BTreeSet::new())
            .expect_err("refused");
        assert!(error.message().contains("certificate"), "{error:?}");
    }
}
