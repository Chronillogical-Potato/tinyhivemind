//! What a live seat is told, and how its reply becomes one transcript line.
//!
//! A seat's call is its persona (the seat prompt), its role charter, its
//! desk and the desks it may refer to, the trace grammar, and the problem,
//! followed by **that seat's projection** of its own desk's journal — the
//! rows `project_for` lets this turn see, so a blind round stays blind.
//! Referral rows from other desks are rows of that journal, marked with the
//! desk they came from.
//!
//! A reply is reduced deterministically to at most one trace line (the first
//! line core's own trace reader accepts, with the required fields present)
//! plus at most one `@#desk` line. A reply with no valid trace line becomes a
//! plain `note:` utterance, which core reads as no trace at all, so an
//! unparseable answer never votes.

use std::collections::BTreeMap;

use tinyhivemind_core::hive::{
    HiveTurn, Phase, SessionAuthor, SessionMessage, TraceKind, Visibility, trace::read,
};

use crate::episode::speech;

/// Rows of a projection shown to a model, the opening always included.
const SHOWN_ROWS: usize = 80;
/// Characters kept of one kept line.
const LINE_CHARS: usize = 400;

/// Everything about one seat a prompt needs.
#[derive(Clone, Debug, Default)]
pub(crate) struct Brief {
    pub(crate) seat: String,
    /// The seat prompt: the persona line and the seat's job.
    pub(crate) persona: String,
    pub(crate) role: String,
    pub(crate) charter: String,
    pub(crate) desk: String,
    pub(crate) desk_name: String,
    pub(crate) desk_description: String,
    /// Desks this seat's desk links to, by id.
    pub(crate) links: Vec<String>,
}

/// The trace grammar, as core reads it.
const GRAMMAR: &str = "\
Your reply is appended to your desk's shared transcript. Write exactly ONE line \
in this trace grammar; the line must START with the marker:
  !propose #<answer> ^<n> <one-sentence reason>   put forward an answer
  !support #<answer> ^<n> <reason>                back an answer; ^<n> is the sequence of a message that proposed it
  !object ><n> ^<m> <reason>                      object to the message at sequence n, grounded in your own message m
  !evidence ^<n> <text>                           share a partial result (e.g. a residue) without voting
  !commit #<answer> <reason>                      only when told the desk is recording its decision
Rules: <answer> is a bare integer with no commas, spaces or punctuation (#924, not #924. or #9,24). \
^<n> cites a sequence number from the transcript; ^1 is the problem statement. Every proposal and \
support must cite at least one ^<n>. Only members of your desk count toward its decision.";

/// The system prompt for one seat.
pub(crate) fn system(brief: &Brief) -> String {
    let links = if brief.links.is_empty() {
        "Your desk links to no other desk.".to_owned()
    } else {
        format!(
            "Your desk may refer a question to a linked desk by adding a second line \
             `@#<desk> <question>` (linked: {}). Use it rarely; a desk may ask at most twice.",
            brief
                .links
                .iter()
                .map(|desk| format!("@#{desk}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!(
        "{persona}\n\nRole ({role}): {charter}\n\nYou sit on the {name} desk ({id}): {about}. {links}\n\n\
         You have no tools and no web or internet access. Work from the problem and the transcript alone, \
         and do the arithmetic yourself.\n\n{GRAMMAR}",
        persona = brief.persona.trim(),
        role = brief.role,
        charter = brief.charter.trim(),
        name = brief.desk_name,
        id = brief.desk,
        about = brief.desk_description,
    )
}

/// The user prompt for one authorized turn.
pub(crate) fn turn(
    brief: &Brief,
    turn: &HiveTurn,
    visible: &[SessionMessage],
    home: &BTreeMap<String, String>,
    carried: Option<&str>,
) -> String {
    let situation = match (turn.phase, turn.visibility) {
        (Phase::Commit, _) => match carried {
            Some(topic) => format!(
                "Your desk has reached quorum on #{topic}. Record it now: `!commit #{topic} <reason>`."
            ),
            None => "Your desk is recording its decision; commit the answer that carried.".into(),
        },
        (_, Visibility::Blind) => "This is the blind opening round: you cannot see your \
            desk-mates' answers yet. Propose your own answer (or post evidence)."
            .into(),
        (_, Visibility::Full) => "The desk can now see itself. Support the answer you can \
            reproduce, object to advocates of answers you can show are wrong, or propose."
            .into(),
    };
    format!(
        "{situation}\n\nTranscript of your desk as you can see it:\n{}\n\nYou are {}. Reply with one trace line.",
        transcript(visible, home, &brief.desk),
        brief.seat
    )
}

/// The user prompt for answering a referral from another desk.
pub(crate) fn answer(
    brief: &Brief,
    incoming: &str,
    visible: &[SessionMessage],
    home: &BTreeMap<String, String>,
) -> String {
    format!(
        "Another desk referred this to you:\n{}\n\nTranscript of your desk:\n{}\n\nYou are {}. \
         Answer it with one trace line on your desk (for example `!evidence ^<n> ...` or \
         `!propose #<answer> ^<n> ...`). Do not add an @# line.",
        incoming.trim(),
        transcript(visible, home, &brief.desk),
        brief.seat
    )
}

/// The projection as numbered lines; rows from another desk say so.
pub(crate) fn transcript(
    visible: &[SessionMessage],
    home: &BTreeMap<String, String>,
    desk: &str,
) -> String {
    let skip = visible.len().saturating_sub(SHOWN_ROWS);
    visible
        .iter()
        .enumerate()
        .filter(|(index, _)| *index == 0 || *index > skip)
        .map(|(_, message)| {
            let author = match &message.author {
                SessionAuthor::Agent { id, .. } => match home.get(id) {
                    Some(from) if from != desk => format!("{id} (referral from desk {from})"),
                    _ => id.clone(),
                },
                SessionAuthor::Operator => "problem".to_owned(),
                SessionAuthor::Person { .. } | SessionAuthor::System { .. } => "host".to_owned(),
            };
            format!(
                "^{} {author}: {}",
                message.sequence.0,
                message.content.replace('\n', " / ")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// A reply reduced to the line the transcript records.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Parsed {
    /// What is appended.
    pub(crate) content: String,
    /// Whether it carries a valid trace line.
    pub(crate) valid: bool,
}

/// Reduce a reply to one trace line, plus one `@#desk` line if it has one.
///
/// Deterministic repairs only: markdown bullets, quotes, emphasis and code
/// fences around a line are stripped, and trailing punctuation or digit
/// separators are removed from a `#topic` (the grammar would otherwise read
/// `#924.` as a different topic from `#924`). Validity is core's own reader:
/// the first line that reads as a trace with what its kind requires
/// (`#topic` for propose, support and commit; `>target` for object).
pub(crate) fn parse(reply: &str) -> Parsed {
    let lines: Vec<String> = reply
        .lines()
        .map(clean)
        .filter(|line| !line.is_empty() && !line.starts_with("```"))
        .collect();
    let trace = lines
        .iter()
        .map(|line| repair(line))
        .find(|line| valid(line));
    let mention = lines
        .iter()
        .find(|line| !line.starts_with('!') && line.contains("@#"))
        .map(|line| clip(line));
    if let Some(trace) = trace {
        let mut content = clip(&trace);
        if let Some(mention) = mention {
            content.push('\n');
            content.push_str(&mention);
        }
        return Parsed {
            content,
            valid: true,
        };
    }
    let first = lines.first().map_or("(empty reply)", String::as_str);
    let mut content = format!("note: {}", clip(first.trim_start_matches('!')));
    if let Some(mention) = mention.filter(|mention| mention != first) {
        content.push('\n');
        content.push_str(&mention);
    }
    Parsed {
        content,
        valid: false,
    }
}

fn clean(line: &str) -> String {
    let mut line = line.trim();
    loop {
        let before = line;
        for prefix in ["- ", "* ", "> ", "`", "\"", "**", "Reply:", "reply:"] {
            line = line.strip_prefix(prefix).unwrap_or(line).trim_start();
        }
        for suffix in ["`", "\"", "**"] {
            line = line.strip_suffix(suffix).unwrap_or(line).trim_end();
        }
        if line == before {
            return line.to_owned();
        }
    }
}

/// Normalize every `#topic` word: strip emphasis, trailing punctuation and
/// digit separators.
fn repair(line: &str) -> String {
    line.split(' ')
        .map(|word| {
            let Some(topic) = word.strip_prefix('#') else {
                return word.to_owned();
            };
            let trimmed = topic
                .trim_matches(|c: char| matches!(c, '*' | '_' | '`'))
                .trim_end_matches(|c: char| ".,;:!?)]}'\"".contains(c));
            let digits: String = trimmed.chars().filter(|c| *c != ',' && *c != '_').collect();
            if !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                format!("#{digits}")
            } else {
                format!("#{trimmed}")
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn valid(line: &str) -> bool {
    let message = speech(1, "seat", line);
    read(std::slice::from_ref(&message))
        .first()
        .is_some_and(|trace| match trace.kind {
            TraceKind::Propose | TraceKind::Support | TraceKind::Commit => trace.topic.is_some(),
            TraceKind::Object => trace.target.is_some(),
            _ => true,
        })
}

fn clip(line: &str) -> String {
    line.chars().take(LINE_CHARS).collect()
}
