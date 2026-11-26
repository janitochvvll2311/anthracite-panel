//! The canonical message a bundle signature covers (D-Signature-Message).
//!
//! The format is written once here and pinned by a vector, because a signature
//! over "whatever bytes happen to be around" is a signature over nothing. The
//! set is exactly `manifest.toml` plus every file under `wit/`, sorted by name:
//!
//! ```text
//! "anth-manifest-v2\0"
//! for each file:
//!     name, "\0", length as little-endian u64, bytes
//! ```
//!
//! The wasm and UI modules are not in the message; their hashes live in the
//! manifest, which is in the message.

use anth_error::Error;

pub const MESSAGE_PREFIX: &[u8] = b"anth-manifest-v2\0";

/// Builds the canonical message. The caller supplies the files in any order;
/// sorting happens here so two equivalent sets always produce the same bytes.
pub fn message(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, Error> {
    if !files.iter().any(|(name, _)| name == "manifest.toml") {
        return Err(Error::invalid(
            "the signed set must contain `manifest.toml`",
        ));
    }
    framed(MESSAGE_PREFIX, files, check_name, "signed")
}

/// The prefix of the WIT-set hash (D-Wit-Hash).
pub const WIT_PREFIX: &[u8] = b"anth-wit-set-v1\0";

/// The WIT set hash covers exactly the files under `wit/`, sorted, with the same
/// `name \0 u64le(length) bytes` framing as the signed message, under its own
/// prefix. A grant stores this as `wit_sha256`.
pub fn wit_message(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, Error> {
    framed(WIT_PREFIX, files, check_wit_name, "WIT")
}

/// The one framing: sort by name, refuse duplicates, then `name \0 u64le(bytes)`.
fn framed(
    prefix: &[u8],
    files: &[(String, Vec<u8>)],
    validate: fn(&str) -> Result<(), Error>,
    label: &str,
) -> Result<Vec<u8>, Error> {
    let mut files: Vec<&(String, Vec<u8>)> = files.iter().collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    let mut message = prefix.to_vec();
    let mut previous: Option<&str> = None;
    for (name, bytes) in files {
        validate(name)?;
        if previous == Some(name.as_str()) {
            return Err(Error::invalid(format!(
                "`{name}` is in the {label} set twice"
            )));
        }
        previous = Some(name.as_str());
        message.extend_from_slice(name.as_bytes());
        message.push(0);
        message.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        message.extend_from_slice(bytes);
    }
    Ok(message)
}

fn check_wit_name(name: &str) -> Result<(), Error> {
    let valid = name
        .strip_prefix("wit/")
        .is_some_and(|rest| !rest.is_empty() && !rest.contains('/') && rest.ends_with(".wit"));
    if !valid {
        return Err(Error::invalid(format!(
            "`{name}` is not a WIT file; the set is `wit/*.wit`"
        )));
    }
    Ok(())
}

/// The signed set is closed: the manifest and the WIT files, nothing else.
///
/// Names are restricted to an unambiguous alphabet. The framing uses NUL as a
/// separator, so a name that contains NUL (or any other control byte) would let
/// two different sets produce the same message; refusing them keeps the
/// encoding injective.
fn check_name(name: &str) -> Result<(), Error> {
    let safe = !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'/'));
    let closed = name == "manifest.toml"
        || (name.starts_with("wit/")
            && name.len() > "wit/".len()
            && !name.ends_with('/')
            && !name
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == ".."));
    if !safe || !closed {
        return Err(Error::invalid(format!(
            "`{name}` is not part of the signed set; only `manifest.toml` and `wit/**` with plain names are"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<(String, Vec<u8>)> {
        vec![
            (
                "wit/notes.wit".to_string(),
                b"package example:notes;\n".to_vec(),
            ),
            (
                "manifest.toml".to_string(),
                b"manifest_version = 2\n".to_vec(),
            ),
        ]
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn the_message_is_pinned_by_a_vector() {
        // Fixed input, fixed bytes. Changing either the prefix or the framing
        // must fail here, not silently invalidate every existing signature.
        assert_eq!(
            hex(&message(&files()).expect("message")),
            "616e74682d6d616e69666573742d7632006d616e69666573742e746f6d6c0015000000000000006d616e69666573745f76657273696f6e203d20320a7769742f6e6f7465732e7769740017000000000000007061636b616765206578616d706c653a6e6f7465733b0a"
        );
    }

    #[test]
    fn the_order_of_the_input_does_not_matter() {
        let mut reordered = files();
        reordered.reverse();
        assert_eq!(
            message(&files()).expect("message"),
            message(&reordered).expect("message")
        );
    }

    #[test]
    fn a_missing_manifest_is_refused() {
        let error = message(&[("wit/notes.wit".to_string(), Vec::new())]).expect_err("manifest");
        assert!(error.message().contains("manifest.toml"), "{error}");
    }

    #[test]
    fn a_file_outside_the_set_is_refused() {
        for name in [
            "wasm/notes.wasm",
            "web/notes.js",
            "../manifest.toml",
            "wit/../x",
        ] {
            let files = vec![
                ("manifest.toml".to_string(), Vec::new()),
                (name.to_string(), Vec::new()),
            ];
            assert!(message(&files).is_err(), "`{name}` must not be signable");
        }
    }

    #[test]
    fn a_name_with_a_control_byte_is_refused() {
        // The framing uses NUL as a separator: a name containing it (or any
        // other control byte) would make two different sets sign the same bytes.
        for name in ["wit/a\0b", "wit/a\n", "wit/a b", "wit/a\\b", "wit/ñ"] {
            let files = vec![
                ("manifest.toml".to_string(), Vec::new()),
                (name.to_string(), Vec::new()),
            ];
            assert!(message(&files).is_err(), "`{name}` must not be signable");
        }
    }

    #[test]
    fn a_duplicate_name_is_refused() {
        let files = vec![
            ("manifest.toml".to_string(), Vec::new()),
            ("manifest.toml".to_string(), Vec::new()),
        ];
        let error = message(&files).expect_err("duplicate");
        assert!(error.message().contains("twice"), "{error}");
    }

    #[test]
    fn the_wit_message_is_pinned_by_a_vector() {
        let files = vec![("wit/a.wit".to_string(), b"package x:y@1.0.0;\n".to_vec())];
        assert_eq!(
            hex(&wit_message(&files).expect("wit message")),
            "616e74682d7769742d7365742d7631007769742f612e7769740013000000000000007061636b61676520783a7940312e302e303b0a"
        );
    }

    #[test]
    fn the_wit_set_order_does_not_matter() {
        let mut files = vec![
            ("wit/a.wit".to_string(), b"a".to_vec()),
            ("wit/b.wit".to_string(), b"b".to_vec()),
        ];
        let first = wit_message(&files).expect("wit message");
        files.reverse();
        assert_eq!(wit_message(&files).expect("wit message"), first);
    }

    #[test]
    fn the_wit_set_is_only_wit_files() {
        for name in ["wit/deps/a.wit", "wit/a/b.wit", "wasm/a.wasm", "wit/a.wasm"] {
            let files = vec![(name.to_string(), Vec::new())];
            assert!(wit_message(&files).is_err(), "`{name}` must be refused");
        }
    }
}
