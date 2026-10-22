//! Guards the user-facing docs against the most expensive kind of staleness:
//! claiming the panel has no authentication after it gained one.
//!
//! The claim was true for most of the project's life, so it is the sentence a
//! reader is most likely to still find and act on. A comment in the source
//! cannot catch it; a test over the shipped markdown can.

use std::path::Path;

/// Phrases that assert an absent authentication layer.
const STALE: &[&str] = &[
    "no authentication yet",
    "Authentication is still unimplemented",
    "has no authentication",
    "no authentication at all",
];

fn docs() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs");
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read docs directory") {
            let path = entry.expect("read entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "md") {
                let text = std::fs::read_to_string(&path).expect("read doc");
                out.push((path.display().to_string(), text));
            }
        }
    }
    out
}

#[test]
fn no_doc_claims_the_panel_has_no_authentication() {
    let mut stale = Vec::new();
    for (path, text) in docs() {
        let mut in_historical = false;
        for (n, line) in text.lines().enumerate() {
            // Two deliberate exceptions: the daemon socket relies on file
            // permissions and says so, and a design note is a record of what
            // the author was working around, not a claim about today's build.
            if line.contains("daemon socket has no authentication") {
                continue;
            }
            if line.trim_start().starts_with('>') {
                continue;
            }
            // Design notes quote the state they were written against. Marking
            // the span explicitly keeps this test honest instead of guessing
            // from wording.
            if line.trim_start().starts_with("<!-- historical") {
                in_historical = true;
                continue;
            }
            if line.trim_start().starts_with("<!-- /historical") {
                in_historical = false;
                continue;
            }
            if in_historical {
                continue;
            }
            if line.contains("at the time") {
                continue;
            }
            if STALE.iter().any(|phrase| line.contains(phrase)) {
                stale.push(format!("{path}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(stale.is_empty(), "stale auth claims:\n{}", stale.join("\n"));
}
