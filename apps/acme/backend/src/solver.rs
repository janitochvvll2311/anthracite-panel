//! Challenge solving: the HTTP-01 token file and the three DNS-01 solvers.
//!
//! HTTP-01 never binds a port. The client writes the key authorization under
//! `{shared}/acme/http-01/<token>` for the ingress to serve, then self-checks
//! `http://<name>/.well-known/acme-challenge/<token>` before telling the CA it
//! is ready. DNS-01 talks to a manual, RFC 2136 or webhook solver.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anth_contract::Error;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use http::Method;
use serde_json::json;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::config::Solver;
use crate::http::Http;

pub const HTTP_PREFIX: &str = "/.well-known/acme-challenge/";

#[derive(Debug, Clone)]
pub struct DnsRecord {
    pub name: String,
    pub value: String,
}

/// The TXT record for a DNS-01 challenge: `_acme-challenge.<name>` carrying the
/// base64url SHA-256 of the key authorization. A wildcard keeps only its base.
pub fn txt_record(name: &str, key_authorization: &str) -> DnsRecord {
    let base = name.trim().trim_end_matches('.').trim_start_matches("*.");
    let digest = Sha256::digest(key_authorization.as_bytes());
    DnsRecord {
        name: format!("_acme-challenge.{base}."),
        value: URL_SAFE_NO_PAD.encode(digest),
    }
}

pub async fn present(http: &Http, solver: &Solver, record: &DnsRecord) -> Result<(), Error> {
    match solver.kind.as_str() {
        "manual" => Ok(()),
        "webhook" => webhook(http, solver, "present", record).await,
        "rfc2136" => rfc2136(solver, record, false).await,
        other => Err(Error::invalid(format!("unknown solver kind `{other}`"))),
    }
}

pub async fn cleanup(http: &Http, solver: &Solver, record: &DnsRecord) -> Result<(), Error> {
    match solver.kind.as_str() {
        "manual" => Ok(()),
        "webhook" => webhook(http, solver, "cleanup", record).await,
        "rfc2136" => rfc2136(solver, record, true).await,
        other => Err(Error::invalid(format!("unknown solver kind `{other}`"))),
    }
}

async fn webhook(
    http: &Http,
    solver: &Solver,
    action: &str,
    record: &DnsRecord,
) -> Result<(), Error> {
    let url = solver
        .url
        .as_deref()
        .ok_or_else(|| Error::invalid(format!("solver `{}` has no url", solver.id)))?;
    let mut headers = vec![("content-type", "application/json".to_string())];
    if let Some(token) = solver.token.as_deref().filter(|token| !token.is_empty()) {
        headers.push(("authorization", format!("Bearer {token}")));
    }
    let body = serde_json::to_vec(&json!({
        "action": action,
        "name": record.name,
        "value": record.value,
        "ttl": 60,
    }))
    .map_err(|error| Error::internal(format!("encoding the webhook call: {error}")))?;
    // The webhook uses the operator's trust: an internal CA serves both the
    // ACME directory and the hook, so the `ca-bundle` applies here too.
    let response = http
        .request(Method::POST, url, &headers, body, false)
        .await?;
    if !response.status.is_success() {
        return Err(Error::unavailable(format!(
            "the DNS webhook answered {} for {action} {}",
            response.status, record.name
        )));
    }
    Ok(())
}

/// Writes the token atomically at 0640 and returns its path.
pub fn write_http_token(
    root: &Path,
    token: &str,
    key_authorization: &str,
) -> Result<PathBuf, Error> {
    if token.is_empty()
        || token.len() > 512
        || !token
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character))
    {
        return Err(Error::invalid("the CA returned an unusable HTTP-01 token"));
    }
    std::fs::create_dir_all(root)
        .map_err(|error| Error::unavailable(format!("creating {}: {error}", root.display())))?;
    let path = root.join(token);
    let temp = root.join(format!(".{token}.{}", std::process::id()));
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o640)
            .open(&temp)
            .map_err(|error| Error::unavailable(format!("writing {}: {error}", temp.display())))?;
        file.write_all(key_authorization.as_bytes())
            .map_err(|error| Error::unavailable(format!("writing {}: {error}", temp.display())))?;
        file.sync_all()
            .map_err(|error| Error::unavailable(format!("syncing {}: {error}", temp.display())))?;
    }
    // The service runs with umask 077, which masks the `mode` above down to
    // 0600. The proxy is another uid and needs the group bit, so chmod after
    // writing instead of trusting the creation mode.
    std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o640))
        .map_err(|error| Error::unavailable(format!("securing {}: {error}", temp.display())))?;
    std::fs::rename(&temp, &path)
        .map_err(|error| Error::unavailable(format!("publishing {}: {error}", path.display())))?;
    Ok(path)
}

pub fn remove_http_token(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Fetches the token through the ingress and compares it with the key
/// authorization. Retries because a reload can lag the write.
pub async fn self_check(
    http: &Http,
    name: &str,
    port: u16,
    token: &str,
    key_authorization: &str,
) -> Result<(), Error> {
    let base = name.trim_start_matches("*.");
    let url = format!("http://{base}:{port}{HTTP_PREFIX}{token}");
    let mut last = String::new();
    for attempt in 0..6 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        // The self-check reaches the ingress that owns the name, which may be a
        // private address; the CA-directory guard does not apply to it.
        match http.request(Method::GET, &url, &[], Vec::new(), true).await {
            Ok(response) if response.status.is_success() => {
                let body = String::from_utf8_lossy(&response.body);
                if body.trim() == key_authorization {
                    return Ok(());
                }
                last = "the ingress answered a different body".to_string();
            }
            Ok(response) => {
                last = format!("the ingress answered {}", response.status);
            }
            Err(error) => {
                last = error.to_string();
            }
        }
    }
    Err(Error::unavailable(format!(
        "HTTP-01 self-check for {base} failed: {last}. Check that the name resolves to the ingress and that its host has `acme = true`."
    )))
}

async fn rfc2136(solver: &Solver, record: &DnsRecord, delete: bool) -> Result<(), Error> {
    let server = solver
        .server
        .as_deref()
        .ok_or_else(|| Error::invalid(format!("solver `{}` has no server", solver.id)))?;
    let zone = solver
        .zone
        .as_deref()
        .ok_or_else(|| Error::invalid(format!("solver `{}` has no zone", solver.id)))?;
    let key_name = solver
        .key_name
        .as_deref()
        .ok_or_else(|| Error::invalid(format!("solver `{}` has no key-name", solver.id)))?;
    let secret = solver
        .secret
        .as_deref()
        .ok_or_else(|| Error::invalid(format!("solver `{}` has no secret", solver.id)))?;
    let algorithm = solver.algorithm.as_deref().unwrap_or("hmac-sha256");
    let message = update_message(zone, record, delete)?;
    let message = sign_tsig(&message, key_name, algorithm, secret)?;
    let mut stream = tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(server))
        .await
        .map_err(|_| Error::unavailable(format!("connecting to the DNS server {server}")))?
        .map_err(|error| Error::unavailable(format!("connecting to {server}: {error}")))?;
    let length = (message.len() as u16).to_be_bytes();
    stream
        .write_all(&length)
        .await
        .map_err(|error| Error::unavailable(format!("sending the DNS update: {error}")))?;
    stream
        .write_all(&message)
        .await
        .map_err(|error| Error::unavailable(format!("sending the DNS update: {error}")))?;
    stream
        .flush()
        .await
        .map_err(|error| Error::unavailable(format!("sending the DNS update: {error}")))?;
    let mut header = [0u8; 2];
    tokio::time::timeout(Duration::from_secs(10), stream.read_exact(&mut header))
        .await
        .map_err(|_| Error::unavailable("the DNS server did not answer the update"))?
        .map_err(|error| Error::unavailable(format!("reading the DNS answer: {error}")))?;
    let length = u16::from_be_bytes(header) as usize;
    if !(12..=65_535).contains(&length) {
        return Err(Error::unavailable(
            "the DNS server answered a broken message",
        ));
    }
    let mut answer = vec![0u8; length];
    stream
        .read_exact(&mut answer)
        .await
        .map_err(|error| Error::unavailable(format!("reading the DNS answer: {error}")))?;
    let rcode = answer[3] & 0x0f;
    if rcode != 0 {
        return Err(Error::unavailable(format!(
            "the DNS server refused the update with rcode {rcode}"
        )));
    }
    Ok(())
}

/// A tiny helper so both the length prefix and the body are written before the
/// timeout applies; kept separate to make the error mapping obvious.
/// One RFC 2136 UPDATE message without TSIG: zone section plus one TXT update.
pub fn update_message(zone: &str, record: &DnsRecord, delete: bool) -> Result<Vec<u8>, Error> {
    let zone = fqdn(zone)?;
    let name = fqdn(&record.name)?;
    let mut message = Vec::new();
    message.extend_from_slice(&(std::process::id() as u16).to_be_bytes());
    // Opcode UPDATE (5) is bits 11-14 of the flags.
    message.extend_from_slice(&(5u16 << 11).to_be_bytes());
    message.extend_from_slice(&1u16.to_be_bytes()); // zone
    message.extend_from_slice(&0u16.to_be_bytes()); // prerequisites
    message.extend_from_slice(&1u16.to_be_bytes()); // updates
    message.extend_from_slice(&0u16.to_be_bytes()); // additional (TSIG later)
    encode_name(&mut message, &zone)?;
    message.extend_from_slice(&6u16.to_be_bytes()); // SOA
    message.extend_from_slice(&1u16.to_be_bytes()); // IN
    encode_name(&mut message, &name)?;
    message.extend_from_slice(&16u16.to_be_bytes()); // TXT
    if delete {
        message.extend_from_slice(&254u16.to_be_bytes()); // NONE
        message.extend_from_slice(&0u32.to_be_bytes());
        let mut rdata = Vec::new();
        encode_txt(&mut rdata, &record.value)?;
        message.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        message.extend_from_slice(&rdata);
    } else {
        message.extend_from_slice(&1u16.to_be_bytes()); // IN
        message.extend_from_slice(&60u32.to_be_bytes());
        let mut rdata = Vec::new();
        encode_txt(&mut rdata, &record.value)?;
        message.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
        message.extend_from_slice(&rdata);
    }
    Ok(message)
}

/// Appends a TSIG RR and updates ARCOUNT. The MAC covers the message as it was
/// plus the RFC 2845 variables, never the secret.
pub fn sign_tsig(
    message: &[u8],
    key_name: &str,
    algorithm: &str,
    secret: &str,
) -> Result<Vec<u8>, Error> {
    let key_name = fqdn(key_name)?;
    let algorithm = fqdn(match algorithm {
        "hmac-sha1" => "hmac-sha1",
        "hmac-sha256" => "hmac-sha256",
        "hmac-sha512" => "hmac-sha512",
        other => return Err(Error::invalid(format!("`{other}` is not a TSIG algorithm"))),
    })?;
    let secret = STANDARD
        .decode(secret.trim())
        .or_else(|_| URL_SAFE_NO_PAD.decode(secret.trim()))
        .map_err(|error| Error::invalid(format!("the TSIG secret is not base64: {error}")))?;
    let time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let mut variables = Vec::new();
    encode_name(&mut variables, &key_name)?;
    variables.extend_from_slice(&255u16.to_be_bytes()); // ANY
    variables.extend_from_slice(&0u32.to_be_bytes()); // TTL
    encode_name(&mut variables, &algorithm)?;
    variables.extend_from_slice(&time_u48(time));
    variables.extend_from_slice(&300u16.to_be_bytes()); // fudge
    variables.extend_from_slice(&0u16.to_be_bytes()); // error
    variables.extend_from_slice(&0u16.to_be_bytes()); // other len

    let mut mac_input = message.to_vec();
    mac_input.extend_from_slice(&variables);
    let mac = match algorithm.as_str() {
        "hmac-sha1." => {
            let mut mac = Hmac::<Sha1>::new_from_slice(&secret)
                .map_err(|error| Error::internal(format!("TSIG key: {error}")))?;
            mac.update(&mac_input);
            mac.finalize().into_bytes().to_vec()
        }
        "hmac-sha512." => {
            let mut mac = Hmac::<Sha512>::new_from_slice(&secret)
                .map_err(|error| Error::internal(format!("TSIG key: {error}")))?;
            mac.update(&mac_input);
            mac.finalize().into_bytes().to_vec()
        }
        _ => {
            let mut mac = Hmac::<Sha256>::new_from_slice(&secret)
                .map_err(|error| Error::internal(format!("TSIG key: {error}")))?;
            mac.update(&mac_input);
            mac.finalize().into_bytes().to_vec()
        }
    };

    let mut message = message.to_vec();
    // ARCOUNT is the last 16-bit field of the header.
    let arcount = u16::from_be_bytes([message[10], message[11]]) + 1;
    message[10..12].copy_from_slice(&arcount.to_be_bytes());
    encode_name(&mut message, &key_name)?;
    message.extend_from_slice(&250u16.to_be_bytes()); // TSIG
    message.extend_from_slice(&255u16.to_be_bytes()); // ANY
    message.extend_from_slice(&0u32.to_be_bytes()); // TTL
    let mut rdata = Vec::new();
    encode_name(&mut rdata, &algorithm)?;
    rdata.extend_from_slice(&time_u48(time));
    rdata.extend_from_slice(&300u16.to_be_bytes());
    rdata.extend_from_slice(&(mac.len() as u16).to_be_bytes());
    rdata.extend_from_slice(&mac);
    rdata.extend_from_slice(&(std::process::id() as u16).to_be_bytes()); // original id
    rdata.extend_from_slice(&0u16.to_be_bytes()); // error
    rdata.extend_from_slice(&0u16.to_be_bytes()); // other len
    message.extend_from_slice(&(rdata.len() as u16).to_be_bytes());
    message.extend_from_slice(&rdata);
    Ok(message)
}

fn time_u48(time: u64) -> [u8; 6] {
    let bytes = time.to_be_bytes();
    [bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7]]
}

fn encode_txt(out: &mut Vec<u8>, value: &str) -> Result<(), Error> {
    let bytes = value.as_bytes();
    let mut rest = bytes;
    while !rest.is_empty() {
        let take = rest.len().min(255);
        out.push(take as u8);
        out.extend_from_slice(&rest[..take]);
        rest = &rest[take..];
    }
    Ok(())
}

fn fqdn(name: &str) -> Result<String, Error> {
    let name = name.trim().trim_end_matches('.').to_ascii_lowercase();
    if name.is_empty() || name.len() > 253 {
        return Err(Error::invalid(format!("`{name}` is not a DNS name")));
    }
    Ok(format!("{name}."))
}

fn encode_name(out: &mut Vec<u8>, name: &str) -> Result<(), Error> {
    for label in name.trim_end_matches('.').split('.') {
        if label.is_empty() || label.len() > 63 {
            return Err(Error::invalid(format!("`{name}` has an invalid label")));
        }
        out.push(label.len() as u8);
        out.extend_from_slice(label.as_bytes());
    }
    out.push(0);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_txt_record_uses_the_base_name_and_the_sha256() {
        let record = txt_record("*.example.com", "token.thumbprint");
        assert_eq!(record.name, "_acme-challenge.example.com.");
        let expected = URL_SAFE_NO_PAD.encode(Sha256::digest(b"token.thumbprint"));
        assert_eq!(record.value, expected);
        assert!(!record.value.contains('='));
    }

    #[test]
    fn an_update_message_has_a_zone_and_one_txt_rr() {
        let record = txt_record("a.example.com", "key");
        let add = update_message("example.com", &record, false).expect("add");
        assert_eq!(u16::from_be_bytes([add[4], add[5]]), 1);
        assert_eq!(u16::from_be_bytes([add[6], add[7]]), 0);
        assert_eq!(u16::from_be_bytes([add[8], add[9]]), 1);
        assert_eq!(u16::from_be_bytes([add[10], add[11]]), 0);
        assert_eq!(add[2] >> 3 & 0x0f, 5, "opcode UPDATE");
        let delete = update_message("example.com", &record, true).expect("delete");
        assert_ne!(add, delete);
    }

    #[test]
    fn a_tsig_message_increments_arcount_and_carries_the_mac() {
        let record = txt_record("a.example.com", "key");
        let message = update_message("example.com", &record, false).expect("message");
        let signed = sign_tsig(&message, "acme.", "hmac-sha256", "MDEyMzQ1Njc4OWFiY2RlZg==")
            .expect("signed");
        assert_eq!(u16::from_be_bytes([signed[10], signed[11]]), 1);
        assert!(signed.len() > message.len());
        assert!(!String::from_utf8_lossy(&signed).contains("MDEyMzQ1"));
    }

    #[test]
    fn a_token_path_cannot_escape_the_root() {
        let root = std::env::temp_dir()
            .join("anthracite-panel")
            .join("acme-token-test");
        assert!(write_http_token(&root, "../escape", "value").is_err());
        let path = write_http_token(&root, "ok-token", "value").expect("token");
        assert_eq!(std::fs::read_to_string(&path).expect("body"), "value");
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(&path).expect("meta").permissions(),
        );
        assert_eq!(mode & 0o777, 0o640);
        remove_http_token(&path);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_token_keeps_its_group_bit_under_the_service_umask() {
        use std::os::unix::fs::PermissionsExt;
        use std::sync::Mutex;

        static UMASK: Mutex<()> = Mutex::new(());
        struct Restore(libc::mode_t);
        impl Drop for Restore {
            fn drop(&mut self) {
                // SAFETY: the guard serializes this test with itself; restoring
                // the process umask cannot invalidate memory.
                unsafe {
                    libc::umask(self.0);
                }
            }
        }

        let _guard = UMASK.lock().unwrap_or_else(|poison| poison.into_inner());
        // SAFETY: same as above; the service runs with 077 and this is the
        // property the proxy depends on.
        let previous = unsafe { libc::umask(0o077) };
        let _restore = Restore(previous);
        let root = std::env::temp_dir()
            .join("anthracite-panel")
            .join("acme-token-umask");
        let path = write_http_token(&root, "group-token", "value").expect("token");
        assert_eq!(
            std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777,
            0o640
        );
        remove_http_token(&path);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_http_token_must_stay_inside_the_directory() {
        let root = std::env::temp_dir()
            .join("anthracite-panel")
            .join("acme-token-test-2");
        assert!(write_http_token(&root, "a/b", "value").is_err());
        assert!(write_http_token(&root, "", "value").is_err());
        assert!(write_http_token(&root, "a b", "value").is_err());
    }
}
