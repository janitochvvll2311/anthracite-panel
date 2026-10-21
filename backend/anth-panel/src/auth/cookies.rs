//! Cookie parsing and the two `Set-Cookie` headers the panel issues.
//!
//! The token cookies are `HttpOnly`, so page scripts cannot read them: an XSS
//! bug cannot exfiltrate a session. `SameSite=Strict` keeps another site from
//! driving the refresh endpoint. `Secure` comes from `[auth] cookie_secure`,
//! because a loopback deployment over plain HTTP would otherwise lose the
//! cookie.

use axum::http::header::SET_COOKIE;
use axum::http::{HeaderMap, HeaderValue};

pub const ACCESS_COOKIE: &str = "anth_access";
pub const REFRESH_COOKIE: &str = "anth_refresh";

/// Reads one cookie from the request headers.
pub fn read(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(axum::http::header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| key.trim() == name)
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// `Set-Cookie` for the short-lived access token.
pub fn access_cookie(token: &str, max_age: u64, secure: bool) -> HeaderValue {
    build(ACCESS_COOKIE, token, max_age, secure)
}

/// `Set-Cookie` for the refresh token.
pub fn refresh_cookie(token: &str, max_age: u64, secure: bool) -> HeaderValue {
    build(REFRESH_COOKIE, token, max_age, secure)
}

/// `Set-Cookie` that deletes both cookies.
pub fn clearing() -> [HeaderValue; 2] {
    [
        build(ACCESS_COOKIE, "", 0, false),
        build(REFRESH_COOKIE, "", 0, false),
    ]
}

pub fn append(headers: &mut HeaderMap, cookie: HeaderValue) {
    headers.append(SET_COOKIE, cookie);
}

fn build(name: &str, value: &str, max_age: u64, secure: bool) -> HeaderValue {
    let secure = if secure { "; Secure" } else { "" };
    HeaderValue::from_str(&format!(
        "{name}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{secure}"
    ))
    .expect("a token is base64url, so the header is valid ASCII")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cookie_is_read_from_a_shared_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            axum::http::header::COOKIE,
            HeaderValue::from_static("other=1; anth_access=abc.def.ghi"),
        );
        assert_eq!(
            read(&headers, ACCESS_COOKIE).as_deref(),
            Some("abc.def.ghi")
        );
        assert_eq!(read(&headers, REFRESH_COOKIE), None);
    }

    #[test]
    fn the_access_cookie_is_httponly_and_same_site() {
        let value = access_cookie("token", 90, false)
            .to_str()
            .unwrap()
            .to_string();
        assert!(value.contains("HttpOnly"));
        assert!(value.contains("SameSite=Strict"));
        assert!(!value.contains("Secure"));
    }

    #[test]
    fn a_secure_deployment_marks_the_cookie_secure() {
        let value = refresh_cookie("token", 60, true)
            .to_str()
            .unwrap()
            .to_string();
        assert!(value.contains("; Secure"));
    }

    #[test]
    fn clearing_sets_a_zero_max_age() {
        let headers = clearing();
        for header in headers {
            assert!(header.to_str().unwrap().contains("Max-Age=0"));
        }
    }
}
