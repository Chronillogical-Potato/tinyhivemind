//! The stigmergic grammar: what a message deposits, and how it is read back.
//!
//! Coordination here is stigmergic in Grassé's sense — work leaves a trace in
//! a shared medium, and the trace is the stimulus for the next unit of work.
//! The transcript is the medium. No agent addresses another, and nothing in
//! this module dispatches anything.

#[cfg(test)]
mod test;

mod types;

pub use types::{TopicId, Trace, TraceKind};

use crate::masking::{code_ranges, is_masked};
use crate::runtime::{Sequence, SessionAuthor, SessionMessage};

/// Maximum number of traces read from one message body.
///
/// A body that deposits more than this keeps the first `TRACE_CAP` in reading
/// order. The bound exists for the same reason [`MENTION_CAP`] does: one
/// message must not be able to grow the fold without limit.
///
/// [`MENTION_CAP`]: crate::mention::MENTION_CAP
pub const TRACE_CAP: usize = 16;

/// Read traces from an authored body, or revalidate a supplied list.
///
/// `None` extracts from `body`; `Some`, including an empty vector, *selects*
/// from what extraction finds there. Parsing is fully determined by `body` --
/// unlike `mention::resolve`, there is no host-side resolution step a
/// supplied trace could legitimately carry -- so a supplied entry can only
/// name which extracted trace to keep, by `(offset, kind)`; anything else it
/// claims (a different topic, target, citation, or text) is discarded in
/// favor of what the body actually says, and a repeated offset is rejected
/// rather than selecting its trace twice. A body carrying no marker yields no
/// trace: ordinary conversation is never coerced into a vote.
///
/// Extraction recognises a marker only at the start of a line, ignoring
/// leading whitespace, and only outside code. Code is found by
/// [`masking::code_ranges`], the one scanner every grammar in this workspace
/// shares, so a span this crate reads as code is the same span the mention
/// grammar reads as code. Inline spans are masked here too: one opened on an
/// earlier line quotes whole lines below it, and a marker on such a line is
/// line-leading yet still inside quoted code.
///
/// [`masking::code_ranges`]: crate::masking::code_ranges
///
/// The grammar of one marker line is:
///
/// ```text
/// !<kind> [#topic] [>target] [^cite ...] [free text]
/// ```
///
/// Two markers are stricter. `!refute` requires both a `#topic` and at least
/// one `^cite`, and `!defer` requires a `#topic`; either yields no trace at all
/// without them, rather than a trace that could cap a topic on nothing or
/// abstain from nothing in particular.
#[must_use]
pub fn resolve(
    body: &str,
    supplied: Option<Vec<Trace>>,
    author: &SessionAuthor,
    sequence: Sequence,
) -> Vec<Trace> {
    let mut traces = match supplied {
        None => extract(body, author, sequence),
        Some(supplied) => revalidate(body, supplied, author, sequence),
    };
    traces.truncate(TRACE_CAP);
    traces
}

/// Fold a projected transcript into traces, in sequence order.
///
/// Messages carrying no marker contribute nothing, so a transcript of ordinary
/// conversation folds to an empty medium.
///
/// So does a message addressed to an aside, and for a different reason: **an
/// aside carries information, never support.** A marker written where the room
/// cannot read it adds no supporter, silences no advocate and earns no
/// directory credit — identically for every reader, because the filter is on
/// the row's audience rather than on who is folding. To make an aside count, a
/// member spends a desk-visible turn saying so in the open.
///
/// The test is here, in the one place a transcript becomes a medium, rather
/// than only inside the episode: a host that folds its own standings gets the
/// same answer as `step` does, which is the property that keeps quorum
/// single-valued. See [ADR 0010][adr].
///
/// [adr]: https://github.com/tinyhumansai/tinyhivemind/blob/main/docs/adr/0010-an-aside-carries-information-never-support.md
#[must_use]
pub fn read(messages: &[SessionMessage]) -> Vec<Trace> {
    read_each(messages.iter())
}

/// Fold a borrowed projection into traces, in sequence order.
///
/// Identical to [`read`], for a caller that already holds references rather
/// than owned messages. The episode filters the transcript on every step, and
/// cloning that filtered slice each time would allocate a copy of the whole
/// conversation for a fold that only ever borrows it.
pub(crate) fn read_borrowed(messages: &[&SessionMessage]) -> Vec<Trace> {
    read_each(messages.iter().copied())
}

fn read_each<'a>(messages: impl Iterator<Item = &'a SessionMessage>) -> Vec<Trace> {
    let mut traces: Vec<Trace> = messages
        .filter(|message| message.audience.is_desk())
        .flat_map(|message| resolve(&message.content, None, &message.author, message.sequence))
        .collect();
    traces.sort_by_key(|trace| (trace.sequence, trace.offset));
    traces
}

fn extract(body: &str, author: &SessionAuthor, sequence: Sequence) -> Vec<Trace> {
    // Every marker begins with `!`, so a body without one cannot deposit a
    // trace. Most messages in a real transcript are ordinary conversation, and
    // this check keeps the fold from scanning and allocating over all of them.
    if !body.contains('!') {
        return Vec::new();
    }
    let masked = code_ranges(body);
    let mut traces = Vec::new();
    let mut offset = 0;
    for line in body.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        if is_masked(start, &masked) {
            continue;
        }
        let trimmed = line.trim_end_matches(['\n', '\r']);
        let indent = trimmed.len() - trimmed.trim_start().len();
        let Some(trace) = parse_line(trimmed.trim_start(), author, sequence, start + indent) else {
            continue;
        };
        traces.push(trace);
    }
    traces
}

fn revalidate(
    body: &str,
    supplied: Vec<Trace>,
    author: &SessionAuthor,
    sequence: Sequence,
) -> Vec<Trace> {
    let extracted = extract(body, author, sequence);
    let mut seen_offsets: Vec<usize> = Vec::new();
    let mut traces = Vec::new();
    for trace in supplied {
        if seen_offsets.contains(&trace.offset) {
            continue;
        }
        seen_offsets.push(trace.offset);
        if let Some(found) = extracted
            .iter()
            .find(|found| found.offset == trace.offset && found.kind == trace.kind)
        {
            traces.push(found.clone());
        }
    }
    traces
}

fn parse_line(
    line: &str,
    author: &SessionAuthor,
    sequence: Sequence,
    offset: usize,
) -> Option<Trace> {
    let rest = line.strip_prefix('!')?;
    let mut words = rest.split_whitespace();
    let kind = match words.next()? {
        "propose" => TraceKind::Propose,
        "support" => TraceKind::Support,
        "object" => TraceKind::Object,
        "refute" => TraceKind::Refute,
        "evidence" => TraceKind::Evidence,
        "question" => TraceKind::Question,
        "commit" => TraceKind::Commit,
        "defer" => TraceKind::Defer,
        _ => return None,
    };

    let mut topic = None;
    let mut target = None;
    let mut cites = Vec::new();
    for word in words {
        if let Some(value) = word.strip_prefix('#') {
            if topic.is_none() && !value.is_empty() {
                topic = Some(TopicId(value.to_owned()));
            }
        } else if let Some(value) = word.strip_prefix('>') {
            if target.is_none() {
                target = value.parse().ok().map(Sequence);
            }
        } else if let Some(value) = word.strip_prefix('^')
            && let Ok(parsed) = value.parse()
        {
            let ground = Sequence(parsed);
            if !cites.contains(&ground) {
                cites.push(ground);
            }
        }
    }

    // A refutation names a hypothesis and points at a fact. Missing either, it
    // is not a refutation, and the line yields no trace at all rather than a
    // trace that could cap a topic on nothing -- the same fail-closed rule the
    // rest of the grammar uses for an unrecognised marker.
    if kind == TraceKind::Refute && (topic.is_none() || cites.is_empty()) {
        return None;
    }

    // A deferral says "this one is not mine". Without a topic it says nothing
    // and would still zero a directory weight and promote a contested topic,
    // so it fails closed for the same reason a refutation does.
    if kind == TraceKind::Defer && topic.is_none() {
        return None;
    }

    Some(Trace {
        sequence,
        author: author.clone(),
        kind,
        topic,
        target,
        cites,
        text: line.to_owned(),
        offset,
    })
}
