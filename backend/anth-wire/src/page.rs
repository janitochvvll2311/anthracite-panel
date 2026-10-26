//! Explicit pagination for every list.
//!
//! A list that can be long is never shortened to fit a frame. It returns a
//! page, and `next` is the only end-of-list signal.

use serde::{Deserialize, Serialize};

/// Default number of items in one page.
pub const DEFAULT_PAGE_LIMIT: u16 = 100;

/// Largest page a caller may request.
pub const MAX_PAGE_LIMIT: u16 = 1000;

/// Longest cursor accepted. A cursor is opaque to the client.
pub const MAX_CURSOR_LEN: usize = 512;

/// The page a list request asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}

impl PageRequest {
    /// The effective limit, with `0` and oversized values rejected by the
    /// caller rather than silently clamped.
    pub fn limit(&self) -> u16 {
        self.limit.unwrap_or(DEFAULT_PAGE_LIMIT)
    }

    /// Whether the requested limit is valid.
    pub fn valid(&self) -> bool {
        self.limit
            .is_none_or(|limit| (1..=MAX_PAGE_LIMIT).contains(&limit))
            && self
                .cursor
                .as_ref()
                .is_none_or(|cursor| cursor.len() <= MAX_CURSOR_LEN)
    }
}

/// One page of a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    /// Opaque cursor for the next page, or `None` at the end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    /// Only present when the feature can compute it cheaply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

impl<T> Page<T> {
    pub fn new(items: Vec<T>) -> Self {
        Self {
            items,
            next: None,
            total: None,
        }
    }

    pub fn with_next(items: Vec<T>, next: impl Into<String>) -> Self {
        Self {
            items,
            next: Some(next.into()),
            total: None,
        }
    }

    pub fn with_total(items: Vec<T>, total: u64) -> Self {
        Self {
            items,
            next: None,
            total: Some(total),
        }
    }
}
