//! Which byte ranges of a message body are code, and so carry no grammar.
//!
//! The mention grammar here and the stigmergic trace grammar in
//! `hive` module read the *same* message body. While each carried its
//! own scanner they could disagree about which span of that body is code, and
//! a marker one grammar read as quoted documentation the other read as a live
//! instruction. This module is the one scanner they share.
//!
//! # Two levels of masking, and why
//!
//! [`fenced_ranges`] masks `CommonMark`'s two *block* code constructs, a
//! fenced block and an indented one. [`code_ranges`] masks those **and**
//! inline code spans, and is the level every grammar in this workspace reads:
//!
//! - A **mid-line** grammar — a mention, which may appear anywhere in a
//!   sentence — needs the spans for the obvious reason. `` `@alice` `` is a
//!   name in prose about a name, not a ping.
//! - A **line-leading** grammar — one whose marker only counts at the start
//!   of a line, like `!propose` or a pin directive — needs them for a less
//!   obvious one. A span opened on one line and closed on a later one quotes
//!   every line between them, so a marker with no backtick ahead of it *on
//!   its own line* can still sit inside quoted code. Masking on fences alone
//!   fires such a marker as a live directive.
//!
//! [`fenced_ranges`] is that block half on its own, for a caller that wants
//! `CommonMark`'s block code and nothing more.
//!
//! # Fence rules
//!
//! [`fenced_ranges`] covers both of `CommonMark`'s code block kinds, closely
//! enough that an author who formats a message for a Markdown renderer gets
//! the masking they see:
//!
//! - an opening fence is indented at most three spaces, and is a run of at
//!   least three backticks or at least three tildes;
//! - a backtick opener's info string may not contain a backtick, because that
//!   is an inline span in a paragraph rather than a block;
//! - a closing fence uses the same character as its opener, runs at least as
//!   long, and carries nothing but whitespace after the run;
//! - an unclosed fence masks to the end of the body, so a body cannot end
//!   mid-block with its tail read as grammar;
//! - a line indented four or more columns — a tab expands to the next
//!   multiple of four — opens an [indented code block][indented-spec] instead,
//!   which is why a fence at that indentation never opens a fenced one — but
//!   its content is still `CommonMark` code, so [`fenced_ranges`] masks it
//!   too, and a directive quoted inside one cannot reach a line-leading
//!   grammar;
//! - an indented block cannot interrupt a paragraph, so the indented *opener*
//!   must be a line with no paragraph open above it: the first line of the
//!   body, a line after a blank one, or a line after one that ends the block
//!   it belongs to — a fenced block, an ATX heading, a thematic break, or a
//!   setext underline. A wrapped, indented continuation line right after live
//!   text stays live rather than being read as a new block, and a quoted
//!   directive indented under a heading is masked rather than reaching a
//!   line-leading grammar.
//!
//! # Inline span rules
//!
//! [`code_ranges`] adds `CommonMark`'s [code spans][span-spec] to that:
//!
//! - a span opens on a run of backticks and closes on the next run of
//!   *exactly* the same length, so a two-backtick opener pairs only with a
//!   later two-backtick run, letting a single backtick appear as content
//!   inside it;
//! - a backslash escapes the backtick after it, so `` \` `` opens nothing —
//!   but a backslash inside an open span is content, and does not stop the
//!   span closing on the backtick it precedes;
//! - a span is inline content of one block, so pairing stops at the boundary
//!   the opener's paragraph ends on: a blank line, a fenced or indented code
//!   block, an ATX heading, a thematic break, or a setext underline. Without
//!   that stop, one stray backtick early in a message and another far below
//!   it would silence every mention in between.
//!
//! Block boundaries a container introduces — a list item or a blockquote
//! interrupting a paragraph — are not recognised, for the same reason the
//! indented-block rule ignores containers.
//!
//! The indented-block rule measures from the message's left margin, not from
//! inside a list or blockquote container the way a full `CommonMark` parser
//! would, so a body that nests one inside a list can, in principle, be masked
//! differently than a renderer would show it — see the private
//! `indented_block_ranges` helper's doc comment in this module's source for
//! why that trade is made rather than growing this into a block-level parser.
//!
//! For the same reason, the block kinds this scanner does not recognise at
//! all — an HTML block above an indented line, most of all — leave that line
//! read as paragraph continuation where `CommonMark` would call it code.
//!
//! [spec]: https://spec.commonmark.org/current/#fenced-code-blocks
//! [span-spec]: https://spec.commonmark.org/current/#code-spans
//! [indented-spec]: https://spec.commonmark.org/current/#indented-code-blocks

#[cfg(test)]
mod test;

/// Every byte range of `body` that is code: fenced blocks and inline spans.
///
/// Ranges are half-open and returned in ascending order, and may overlap
/// where a block and a span cover the same byte. This is the level an
/// authored grammar wants, line-leading or not; [`fenced_ranges`] is the
/// block-only half.
///
/// ```
/// use tinyhivemind_core::masking::code_ranges;
///
/// assert_eq!(code_ranges("`@alice` @bob"), vec![(0, 8)]);
/// ```
#[must_use]
pub fn code_ranges(body: &str) -> Vec<(usize, usize)> {
    let mut ranges = fenced_ranges(body);
    ranges.extend(inline_ranges(body, &ranges));
    ranges.sort_unstable();
    ranges
}

/// Whether the byte at `offset` falls inside one of `ranges`.
///
/// `ranges` are half-open, as [`code_ranges`] and [`fenced_ranges`] return
/// them, so a range's `end` is not masked.
///
/// ```
/// use tinyhivemind_core::masking::{code_ranges, is_masked};
///
/// let body = "`@alice` @bob";
/// let masked = code_ranges(body);
/// assert!(is_masked(1, &masked));
/// assert!(!is_masked(9, &masked));
/// ```
#[must_use]
pub fn is_masked(offset: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|(start, end)| *start <= offset && offset < *end)
}

/// The byte ranges of `body` covered by a fenced or indented code block.
///
/// Each range is half-open — `start` is masked, `end` is not — and they are
/// returned in ascending order. This is block code alone; a grammar reading
/// an authored body wants [`code_ranges`], which masks inline spans too.
///
/// ```
/// use tinyhivemind_core::masking::fenced_ranges;
///
/// // A ```rust line inside an open block is content, not a closing fence.
/// let body = "```\nhidden\n```rust\nalso hidden\n```\n";
/// assert_eq!(fenced_ranges(body), vec![(0, body.len())]);
/// ```
#[must_use]
pub fn fenced_ranges(body: &str) -> Vec<(usize, usize)> {
    let fences = backtick_or_tilde_fence_ranges(body);
    let mut ranges = indented_block_ranges(body, &fences);
    ranges.extend(fences);
    ranges.sort_unstable();
    ranges
}

fn backtick_or_tilde_fence_ranges(body: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut open: Option<(usize, u8, usize)> = None;
    let mut line_start = 0;
    for line in body.split_inclusive('\n') {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        if indent <= 3 {
            let marker = trimmed.as_bytes().first().copied();
            if matches!(marker, Some(b'`' | b'~')) {
                let marker = marker.unwrap_or_default();
                let run = trimmed
                    .as_bytes()
                    .iter()
                    .take_while(|byte| **byte == marker)
                    .count();
                if run >= 3 {
                    match open {
                        None if marker == b'~' || !trimmed[run..].contains('`') => {
                            open = Some((line_start, marker, run));
                        }
                        Some((start, open_marker, open_run))
                            if marker == open_marker
                                && run >= open_run
                                && is_blank_line(trimmed[run..].trim_end_matches(['\n', '\r'])) =>
                        {
                            ranges.push((start, line_start + line.len()));
                            open = None;
                        }
                        None | Some(_) => {}
                    }
                }
            }
        }
        line_start += line.len();
    }
    if let Some((start, _, _)) = open {
        ranges.push((start, body.len()));
    }
    ranges
}

/// A maximal run of lines each indented at least four columns from the left
/// margin: `CommonMark`'s other code block, distinct from a backtick or
/// tilde fence.
///
/// A blank line does not end the run — only a later non-blank, under-indented
/// line or the end of the body does — because `CommonMark` lets an indented
/// block continue across blank lines.
///
/// This deliberately does not track list or blockquote container context.
/// `CommonMark` measures an indented block's four columns *inside* its
/// container — a line under a `- ` list marker only needs indentation past
/// the marker's own width, not four literal columns from the message's left
/// margin — so a marker's author quoting an example inside a list at exactly
/// four raw columns can, in principle, get masking a real renderer would not
/// give it. Recognizing container context correctly needs a block-level
/// parser this module is not; the trade this scanner makes is the same one
/// its two-level design already commits to (see the module docs): stay a
/// small, dependency-free heuristic over `CommonMark`'s two most common,
/// unnested constructs rather than grow into a full parser for every corner
/// of the specification.
fn indented_block_ranges(body: &str, fenced: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut open: Option<usize> = None;
    let mut open_end = 0;
    let mut line_start = 0;
    // An indented code block cannot interrupt a paragraph, so its *opening*
    // line must be one no paragraph is open above: the first line of the
    // body, a line after a blank one, or a line after one that closed the
    // paragraph it ended. `paragraph` starts `false` so a block may open at
    // the very start.
    let mut paragraph = false;
    for line in body.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        if is_masked(line_start, fenced) {
            // A fenced block is not a paragraph and closes any paragraph
            // above it, so the line after one may open an indented block --
            // and its opening fence ends an indented block already open.
            if let Some(start) = open.take() {
                ranges.push((start, open_end));
            }
            paragraph = false;
            line_start += line.len();
            continue;
        }
        if is_blank_line(content) {
            paragraph = false;
            line_start += line.len();
            continue;
        }
        if indentation_width(content) >= 4 && (open.is_some() || !paragraph) {
            open.get_or_insert(line_start);
            open_end = line_start + line.len();
        } else {
            if let Some(start) = open.take() {
                ranges.push((start, open_end));
            }
            // A heading, a thematic break, and a setext heading's underline
            // each end the block they close, leaving no paragraph for the
            // next line to continue. Everything else -- including a line
            // that only looks like one of them, such as `#tag`, or a `===`
            // with nothing above it to underline -- is paragraph text.
            paragraph = !(is_atx_heading(content)
                || is_thematic_break(content)
                || (paragraph && is_setext_underline(content)));
        }
        line_start += line.len();
    }
    if let Some(start) = open {
        ranges.push((start, open_end));
    }
    ranges
}

/// `CommonMark`'s indentation width of a line's leading whitespace: a space
/// advances one column and a tab advances to the next multiple of four, the
/// same rule fence detection above does not need because a fence marker is
/// never itself whitespace.
fn indentation_width(content: &str) -> usize {
    let mut width = 0;
    for byte in content.bytes() {
        match byte {
            b' ' => width += 1,
            b'\t' => width += 4 - (width % 4),
            _ => break,
        }
    }
    width
}

/// Whether a line, with its line ending already stripped, is `CommonMark`
/// blank: nothing but spaces and tabs. `str::trim` is not this rule — it
/// treats any Unicode whitespace as blank, including a non-breaking space,
/// which `CommonMark` does not.
fn is_blank_line(content: &str) -> bool {
    content.bytes().all(|byte| matches!(byte, b' ' | b'\t'))
}

fn inline_ranges(body: &str, fenced: &[(usize, usize)]) -> Vec<(usize, usize)> {
    let bytes = body.as_bytes();
    let breaks = paragraph_breaks(body);
    let mut ranges = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        if let Some((_, end)) = fenced
            .iter()
            .find(|(start, end)| *start <= offset && offset < *end)
        {
            offset = *end;
            continue;
        }
        if bytes[offset] == b'\\' {
            // A backslash escapes the character after it, so `\`` is a literal
            // backtick rather than the opener of a span. Skipping the whole
            // escaped character keeps every offset on a character boundary.
            offset += 1 + body[offset + 1..].chars().next().map_or(0, char::len_utf8);
            continue;
        }
        if bytes[offset] != b'`' {
            offset += 1;
            continue;
        }
        let run = bytes[offset..]
            .iter()
            .take_while(|byte| **byte == b'`')
            .count();
        // A code span is inline content of one block, so the search for a
        // closing run stops where the opener's paragraph does. Pairing past
        // that boundary would mask the live text of every block in between.
        // Backslash escapes are deliberately not honoured inside the span:
        // `` `a\` `` closes on the escaped backtick, as `CommonMark` says.
        let limit = breaks
            .iter()
            .copied()
            .find(|boundary| *boundary > offset)
            .unwrap_or(bytes.len());
        let mut candidate = offset + run;
        let mut closing = None;
        while candidate < limit {
            // A fenced block ends the paragraph this opener lives in, so the
            // search for a closing run must not cross it: pairing across a
            // fenced block would mask live text after the block as if it
            // were still inside this opener's inline span.
            if is_masked(candidate, fenced) {
                break;
            }
            if bytes[candidate] != b'`' {
                candidate += 1;
                continue;
            }
            let closing_run = bytes[candidate..]
                .iter()
                .take_while(|byte| **byte == b'`')
                .count();
            if closing_run == run {
                closing = Some(candidate + closing_run);
                break;
            }
            candidate += closing_run;
        }
        if let Some(end) = closing {
            ranges.push((offset, end));
            offset = end;
        } else {
            offset += run;
        }
    }
    ranges
}

/// The byte offsets at which a block boundary closes an open paragraph: the
/// start and the end of every blank line, ATX heading, thematic break, and
/// setext heading underline in `body`.
///
/// Each of those is a one-line block, so both its edges are boundaries — an
/// opener above one cannot pair below it, and an opener inside a heading
/// cannot pair on the line after it.
///
/// A `=` or `-` underline is only a setext heading under an open paragraph;
/// elsewhere it is ordinary text. Treating it as a boundary regardless is
/// safe here because a boundary only ever *stops* a search that began above
/// it, and an opener above such a line is by construction inside the
/// paragraph the line underlines.
fn paragraph_breaks(body: &str) -> Vec<usize> {
    let mut breaks = Vec::new();
    let mut line_start = 0;
    for line in body.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']);
        if is_blank_line(content)
            || is_atx_heading(content)
            || is_thematic_break(content)
            || is_setext_underline(content)
        {
            breaks.push(line_start);
            breaks.push(line_start + line.len());
        }
        line_start += line.len();
    }
    breaks
}

/// A line's content past at most three columns of indentation, or `None` when
/// it is indented far enough to be a code block instead. A tab always reaches
/// the fourth column from any of the first four, so only spaces can survive
/// this trim.
fn under_four_columns(content: &str) -> Option<&str> {
    (indentation_width(content) <= 3).then(|| content.trim_start_matches(' '))
}

/// Whether `content` is an ATX heading: one to six `#` characters followed by
/// a space, a tab, or the end of the line.
fn is_atx_heading(content: &str) -> bool {
    let Some(rest) = under_four_columns(content) else {
        return false;
    };
    let hashes = rest.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes) && matches!(rest.as_bytes().get(hashes), None | Some(b' ' | b'\t'))
}

/// Whether `content` is a thematic break: three or more of `-`, `_`, or `*`,
/// all the same character, with nothing but spaces and tabs between them.
fn is_thematic_break(content: &str) -> bool {
    let Some(rest) = under_four_columns(content) else {
        return false;
    };
    let Some(marker) = rest
        .bytes()
        .next()
        .filter(|byte| matches!(byte, b'-' | b'_' | b'*'))
    else {
        return false;
    };
    let mut markers = 0;
    for byte in rest.bytes() {
        if byte == marker {
            markers += 1;
        } else if !matches!(byte, b' ' | b'\t') {
            return false;
        }
    }
    markers >= 3
}

/// Whether `content` could underline a setext heading: a run of `=` or a run
/// of `-`, then only spaces and tabs. It is one *only* under an open
/// paragraph, which every caller here checks for itself.
fn is_setext_underline(content: &str) -> bool {
    let Some(rest) = under_four_columns(content) else {
        return false;
    };
    let Some(marker) = rest
        .bytes()
        .next()
        .filter(|byte| matches!(byte, b'=' | b'-'))
    else {
        return false;
    };
    let run = rest.bytes().take_while(|byte| *byte == marker).count();
    is_blank_line(&rest[run..])
}
