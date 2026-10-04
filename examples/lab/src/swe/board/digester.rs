//! A digester that spends no tokens.
//!
//! Core's digest port is meant to be backed by a model. In this lab a model
//! fold would add tokens to the very arm whose tokens are being measured, so
//! the fold is extractive: one clipped line per row, newest kept when the
//! account is over budget. Pinned rows are marked so a reader sees them.

use tinyhivemind_core::runtime::digest::{DigestFuture, DigestRequest, Digester};
use tinyhivemind_core::runtime::render_row;

/// Characters of one row kept in the account.
const LINE_CHARS: usize = 160;

/// Clips each row to one line; calls no model.
pub struct ExtractiveDigester;

impl Digester for ExtractiveDigester {
    fn digest<'a>(&'a self, request: &'a DigestRequest) -> DigestFuture<'a> {
        Box::pin(async move { Ok(fold(request)) })
    }
}

/// The account text for one request, within its budget.
pub(super) fn fold(request: &DigestRequest) -> String {
    let mut lines: Vec<String> = request
        .prior
        .as_deref()
        .map(|prior| prior.lines().map(str::to_owned).collect())
        .unwrap_or_default();
    for row in &request.messages {
        let Some(rendered) = render_row(row) else {
            continue;
        };
        let first = rendered.lines().next().unwrap_or_default();
        let clipped: String = first.chars().take(LINE_CHARS).collect();
        let mark = if request.pinned.contains(&row.sequence) {
            "*"
        } else {
            "-"
        };
        lines.push(format!("{mark} ^{} {clipped}", row.sequence));
    }
    // Drop the oldest lines until the account fits, and always keep one.
    let mut total: usize = lines.iter().map(|line| line.len() + 1).sum();
    let mut start = 0;
    while total > request.budget_chars && start + 1 < lines.len() {
        total -= lines[start].len() + 1;
        start += 1;
    }
    let mut text = lines[start..].join("\n");
    if text.len() > request.budget_chars {
        let mut end = request.budget_chars;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    text
}
