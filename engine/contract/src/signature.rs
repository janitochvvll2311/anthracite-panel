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

use crate::error::Error;

pub const MESSAGE_PREFIX: &[u8] = b"anth-manifest-v2\0";

/// Builds the canonical message. The caller supplies the files in any order;
/// sorting happens here so two equivalent sets always produce the same bytes.
pub fn message(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, Error> {
    let mut files: Vec<&(String, Vec<u8>)> = files.iter().collect();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    if !files.iter().any(|(name, _)| name == "manifest.toml") {
        return Err(Error::invalid(
            "the signed set must contain `manifest.toml`",
        ));
    }
    let mut message = MESSAGE_PREFIX.to_vec();
    let mut previous: Option<&str> = None;
    for (name, bytes) in files {
        check_name(name)?;
        if previous == Some(name.as_str()) {
            return Err(Error::invalid(format!(
                "`{name}` is in the signed set twice"
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

/// The signed set is closed: the manifest and the WIT files, nothing else.
fn check_name(name: &str) -> Result<(), Error> {
    let valid = name == "manifest.toml"
        || (name.starts_with("wit/")
            && name.len() > "wit/".len()
            && !name.ends_with('/')
            && !name
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == ".."));
    if !valid {
        return Err(Error::invalid(format!(
            "`{name}` is not part of the signed set; only `manifest.toml` and `wit/**` are"
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
    fn a_duplicate_name_is_refused() {
        let files = vec![
            ("manifest.toml".to_string(), Vec::new()),
            ("manifest.toml".to_string(), Vec::new()),
        ];
        let error = message(&files).expect_err("duplicate");
        assert!(error.message().contains("twice"), "{error}");
    }

    #[test]
    fn the_length_is_little_endian() {
        let message = message(&[
            ("manifest.toml".to_string(), b"abc".to_vec()),
            ("wit/a.wit".to_string(), Vec::new()),
        ])
        .expect("message");
        let prefix = MESSAGE_PREFIX.len();
        // name is 13 bytes, then the NUL, then the u64 length.
        assert_eq!(&message[prefix..prefix + 13], b"manifest.toml");
        assert_eq!(message[prefix + 13], 0);
        assert_eq!(
            &message[prefix + 14..prefix + 22],
            &[3, 0, 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(&message[prefix + 22..prefix + 25], b"abc");
    }
}
