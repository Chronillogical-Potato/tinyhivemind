//! Speakers for packages without a simulation: a replayed `script.json`, or a
//! seeded default script that picks propose, support and object from the seed.

use std::collections::{BTreeMap, VecDeque};

use tinyhivemind_core::hive::{
    ConsensusState, EpisodePolicy, HiveTurn, Phase, SessionMessage, TopicStanding, TraceKind,
    Visibility,
    quorum::{consensus, standings},
    trace::read,
};

use crate::episode::Speaker;
use crate::rng::Rng;

/// What a seat with nothing left to say writes.
pub(crate) const QUIET: &str = "!question Nothing further from me.";

/// Topics the seeded default script argues over.
const OPTIONS: [&str; 3] = ["option-a", "option-b", "option-c"];

/// The commit line for whatever topic carried in `visible`, if one did.
///
/// Once the room is in [`Phase::Commit`] the authorized seat must actually
/// write `!commit` for the episode to converge; the phase alone records
/// nothing. This reads the carried topic from the seat's own projection with
/// core's folds, as `hive.rs` does.
pub(crate) fn commit_line(visible: &[SessionMessage], policy: &EpisodePolicy) -> Option<String> {
    let standing = view(visible, policy);
    match consensus(&standing, &policy.quorum) {
        ConsensusState::Quorum { topic } => Some(format!("!commit #{topic} Recording the decision.")),
        _ => None,
    }
}

/// Standings over one projection, empty when the policy is malformed.
pub(crate) fn view(visible: &[SessionMessage], policy: &EpisodePolicy) -> Vec<TopicStanding> {
    let traces = read(visible);
    let at = visible.last().map_or(tinyhivemind_core::hive::Sequence(0), |message| message.sequence);
    standings(&traces, at, &policy.quorum).unwrap_or_default()
}

/// The first message in `visible` that proposed or supported `topic`, by a
/// seat other than `seat`.
pub(crate) fn advocacy_of(visible: &[SessionMessage], topic: &str, seat: &str) -> Option<u64> {
    read(visible)
        .into_iter()
        .find(|trace| {
            matches!(trace.kind, TraceKind::Propose | TraceKind::Support)
                && trace.topic.as_ref().is_some_and(|id| id.0 == topic)
                && trace.agent_id() != Some(seat)
        })
        .map(|trace| trace.sequence.0)
}

/// Replays `script.json`: each seat's lines in order, then the default.
#[derive(Debug)]
pub(crate) struct Scripted {
    lines: BTreeMap<String, VecDeque<String>>,
}

impl Scripted {
    /// A speaker over a decoded `script.json`.
    pub(crate) fn new(script: &BTreeMap<String, Vec<String>>) -> Self {
        Self {
            lines: script
                .iter()
                .map(|(seat, lines)| (seat.clone(), lines.iter().cloned().collect()))
                .collect(),
        }
    }
}

impl Speaker for Scripted {
    fn speak(&mut self, turn: &HiveTurn, visible: &[SessionMessage], policy: &EpisodePolicy) -> String {
        if turn.phase == Phase::Commit
            && let Some(line) = commit_line(visible, policy)
        {
            return line;
        }
        self.lines
            .get_mut(&turn.agent_id)
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| QUIET.to_owned())
    }
}

/// The seeded default script.
///
/// In the blind round each seat proposes one of three options, chosen from the
/// seed. Once the room can see itself a seat supports the leading option
/// (60%), objects to a rival's advocate (25%), or adds evidence (15%). The
/// draws are per seat, so a seed fixes the whole run and different seeds end
/// differently.
#[derive(Debug)]
pub(crate) struct Seeded {
    seed: u64,
    streams: BTreeMap<String, Rng>,
    options: usize,
}

impl Seeded {
    /// A default script for a room of `seats`.
    pub(crate) fn new(seed: u64, seats: usize) -> Self {
        Self {
            seed,
            streams: BTreeMap::new(),
            options: if seats > 4 { 3 } else { 2 },
        }
    }
}

impl Speaker for Seeded {
    fn speak(&mut self, turn: &HiveTurn, visible: &[SessionMessage], policy: &EpisodePolicy) -> String {
        if turn.phase == Phase::Commit {
            return commit_line(visible, policy).unwrap_or_else(|| QUIET.to_owned());
        }
        let seed = self.seed;
        let rng = self
            .streams
            .entry(turn.agent_id.clone())
            .or_insert_with(|| Rng::stream(seed, &turn.agent_id));
        let pick = usize::try_from(rng.below(self.options as u64)).unwrap_or(0);
        let own = OPTIONS[pick % OPTIONS.len()];
        if turn.visibility == Visibility::Blind {
            return format!("!propose #{own} ^1 My first read favours {own}.");
        }
        let standing = view(visible, policy);
        let leader = standing
            .iter()
            .max_by_key(|standing| standing.supporters.len())
            .map(|standing| standing.topic.0.clone());
        let roll = rng.below(100);
        if roll < 60
            && let Some(leader) = &leader
            && let Some(cited) = advocacy_of(visible, leader, &turn.agent_id)
        {
            return format!("!support #{leader} ^{cited} The argument at {cited} holds up.");
        }
        if roll < 85
            && let Some(rival) = standing
                .iter()
                .find(|standing| standing.topic.0 != own && !standing.supporters.is_empty())
            && let Some(target) = advocacy_of(visible, &rival.topic.0, &turn.agent_id)
        {
            return format!("!object >{target} ^1 That case for {} skips the brief.", rival.topic);
        }
        format!("!evidence ^1 Noting a consideration for {own}.")
    }
}
