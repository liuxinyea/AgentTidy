//! Streaming JSONL reading (`Start.md` §2.1, §18.1).
//!
//! Session transcripts are append-only JSONL, and the provider docs
//! (claude-code.md / codex.md / workbuddy.md) all observed the same
//! shape: one JSON object per line, with the session being written
//! *while we scan* — so the final line may be truncated mid-write.
//! This reader therefore yields per-line events instead of failing the
//! stream: malformed lines become [`JsonlEvent::Malformed`] (skip and
//! record), and only an I/O error ends iteration
//! ([`JsonlEvent::Fatal`]). Fail-closed is reserved for cleanup
//! (§3.5); reporting degrades.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::de::DeserializeOwned;

/// One item produced by [`open_jsonl`].
#[derive(Debug, PartialEq)]
pub enum JsonlEvent<T> {
    /// A well-formed line, deserialized into `T`.
    Item { line: u64, value: T },
    /// A line that failed to parse. Contains the raw error; the line is
    /// consumed and iteration continues.
    Malformed { line: u64, error: String },
    /// Unrecoverable I/O error; the iterator yields this once and then
    /// ends.
    Fatal { error: String },
}

/// Open a JSONL file for streaming deserialization.
///
/// Lines are counted from 1. Empty lines are reported as
/// [`JsonlEvent::Malformed`] — real transcripts do not contain them,
/// and treating them as data would hide corruption.
pub fn open_jsonl<T: DeserializeOwned>(path: &Path) -> std::io::Result<JsonlReader<T>> {
    let file = File::open(path)?;
    Ok(JsonlReader {
        lines: BufReader::new(file).lines(),
        line: 0,
        _marker: std::marker::PhantomData,
    })
}

/// Streaming iterator over a JSONL file. See [`open_jsonl`].
pub struct JsonlReader<T> {
    lines: std::io::Lines<BufReader<File>>,
    line: u64,
    /// `T` appears only in the Iterator impl; PhantomData (via `fn()`
    /// so the reader stays `Send` regardless of `T`) carries it.
    _marker: std::marker::PhantomData<fn() -> T>,
}

impl<T: DeserializeOwned> Iterator for JsonlReader<T> {
    type Item = JsonlEvent<T>;

    fn next(&mut self) -> Option<Self::Item> {
        let raw = match self.lines.next()? {
            Ok(raw) => raw,
            Err(e) => {
                // Read error (not a parse error): terminal.
                return Some(JsonlEvent::Fatal {
                    error: e.to_string(),
                });
            }
        };
        self.line += 1;
        let line = self.line;

        if raw.trim().is_empty() {
            // Blank lines are corruption signals in append-only
            // transcripts, not padding.
            return Some(JsonlEvent::Malformed {
                line,
                error: "empty line".into(),
            });
        }

        match serde_json::from_str::<T>(&raw) {
            Ok(value) => Some(JsonlEvent::Item { line, value }),
            Err(e) => Some(JsonlEvent::Malformed {
                line,
                error: e.to_string(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Line {
        #[allow(dead_code)]
        kind: String,
        n: u64,
    }

    fn temp_file(tag: &str, content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "agenttidy-jsonl-{}-{tag}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn reads_items_and_counts_lines() {
        let path = temp_file("ok", "{\"kind\":\"a\",\"n\":1}\n{\"kind\":\"b\",\"n\":2}\n");
        let events: Vec<JsonlEvent<Line>> = open_jsonl::<Line>(&path).unwrap().collect();
        assert_eq!(
            events,
            vec![
                JsonlEvent::Item {
                    line: 1,
                    value: Line {
                        kind: "a".into(),
                        n: 1
                    }
                },
                JsonlEvent::Item {
                    line: 2,
                    value: Line {
                        kind: "b".into(),
                        n: 2
                    }
                },
            ]
        );
    }

    #[test]
    fn malformed_lines_are_events_not_errors() {
        // Third line is truncated JSON, fourth is empty — both must be
        // surfaced as Malformed, and the good line after them still
        // arrives (a scan never aborts on a bad line).
        let path = temp_file(
            "mixed",
            "{\"kind\":\"a\",\"n\":1}\n{broken\n\n{\"kind\":\"b\",\"n\":2}\n",
        );
        let events: Vec<JsonlEvent<Line>> = open_jsonl::<Line>(&path).unwrap().collect();
        assert_eq!(events.len(), 4);
        assert!(matches!(events[0], JsonlEvent::Item { line: 1, .. }));
        assert!(matches!(events[1], JsonlEvent::Malformed { line: 2, .. }));
        assert!(matches!(events[2], JsonlEvent::Malformed { line: 3, .. }));
        assert!(matches!(events[3], JsonlEvent::Item { line: 4, .. }));
    }

    #[test]
    fn missing_file_is_an_open_error() {
        assert!(open_jsonl::<Line>(Path::new("definitely-not-here.jsonl")).is_err());
    }
}
