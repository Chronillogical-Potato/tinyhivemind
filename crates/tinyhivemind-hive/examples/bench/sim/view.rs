//! What one turn can actually see, and the self-checks built on it.
//!
//! [`View`] is the read-only projection a turn composes against: it folds the
//! transcript a turn was shown through the library's own [`resolve`] and
//! [`standings`] once, so nothing in [`super::agent`] reimplements the fold or
//! guesses at what still counts as a supporter. The parsing helpers below it
//! read the same private-check grammar [`super::agent`] writes, and
//! [`check_selfcheck`] is this example's stand-in for `cargo test` coverage —
//! `sim` is an example binary, so a `#[test]` placed in it never runs.

use tinyhivemind_hive::quorum::{TopicStanding, standings};
use tinyhivemind_hive::trace::{TopicId, Trace, TraceKind, resolve};
use tinyhivemind_hive::{QuorumPolicy, Sequence, SessionAuthor, SessionMessage};

use super::agent::{CheckStyle, SimAgent};
use super::{ASIDE_READS, CONCESSION, Expertise, GROUNDS_WEIGHT, RULES_OUT, Room, SOCIAL_WEIGHT};
use crate::run::ASIDE_MARKER;

/// What one turn can actually see, folded once through the library's own reads.
///
/// The participant does not reimplement the fold. It calls [`resolve`] to read
/// the traces out of the messages it was shown and [`standings`] to see who is
/// still counted as a supporter after cross-inhibition — the same two functions
/// the episode uses. A participant that guessed at the standings instead would
/// be benchmarking the guess.
pub(crate) struct View {
    /// Every trace resolved out of the visible transcript, in sequence order.
    traces: Vec<Trace>,
    /// Who the library still counts as backing each topic, after cross-inhibition.
    standings: Vec<TopicStanding>,
    /// The room's quorum threshold, as a plain count.
    threshold: usize,
}

impl View {
    /// Fold the transcript one turn was shown into a [`View`] of it.
    pub(crate) fn fold(visible: &[SessionMessage], quorum: QuorumPolicy) -> Self {
        let mut traces: Vec<Trace> = visible
            .iter()
            .flat_map(|message| resolve(&message.content, None, &message.author, message.sequence))
            .collect();
        traces.sort_by_key(|trace| (trace.sequence, trace.offset));
        let at = visible
            .last()
            .map_or(Sequence(0), |message| message.sequence);
        let standings = standings(&traces, at, &quorum).unwrap_or_default();
        Self {
            traces,
            standings,
            threshold: usize::try_from(quorum.threshold).unwrap_or(2),
        }
    }

    /// Who this transcript shows has said something grounded about a topic.
    ///
    /// The room's own record of who knows what, read off the traces rather
    /// than from anything a participant is not entitled to see: a member that
    /// deposited `!evidence` on a topic, or was cited for it, is the member to
    /// ask about it. This is the same signal the folded directory is built
    /// from, taken directly because a participant here needs one name rather
    /// than a ranking.
    pub(crate) fn grounded_by(&self, topic: &TopicId, excluding: &str) -> Option<String> {
        // A deposit that *argues against* the topic first, and any deposit at
        // all only after that.
        //
        // Taking the first depositor outright is what this did before, and it
        // did not aim. Under `--blind-evidence` every lay member opens by
        // depositing its own reading of whatever it rates highest, which
        // under a hidden profile is the planted decoy: four such deposits and
        // one refutation land on the same topic, the refutation is rarely
        // first, and "whoever the room has heard ground this option" resolved
        // to a member holding nothing but the error every member shares. The
        // measured hit rate on the one member who knows something was 22%
        // against 25% for drawing a peer at random, so the informed arm was
        // not aiming at anything.
        let depositors = || {
            self.traces
                .iter()
                .filter(|trace| trace.topic.as_ref() == Some(topic))
                .filter(|trace| matches!(trace.kind, TraceKind::Evidence))
        };
        depositors()
            .filter(|trace| trace.text.contains(RULES_OUT))
            .chain(depositors())
            .filter_map(|trace| match &trace.author {
                SessionAuthor::Agent { id, .. } => Some(id.clone()),
                _ => None,
            })
            .find(|author| author != excluding)
    }

    /// The sequence that first proposed a topic, if it is on the floor.
    pub(crate) fn proposal(&self, topic: &TopicId) -> Option<Sequence> {
        self.traces
            .iter()
            .find(|trace| trace.kind == TraceKind::Propose && trace.topic.as_ref() == Some(topic))
            .map(|trace| trace.sequence)
    }

    /// Every topic on the floor, with the sequence that put it there.
    fn floor(&self) -> Vec<(&TopicId, Sequence)> {
        let mut floor: Vec<(&TopicId, Sequence)> = Vec::new();
        for trace in &self.traces {
            if trace.kind != TraceKind::Propose {
                continue;
            }
            let Some(topic) = trace.topic.as_ref() else {
                continue;
            };
            if !floor.iter().any(|(held, _)| *held == topic) {
                floor.push((topic, trace.sequence));
            }
        }
        floor
    }

    /// The supporters the library still counts for a topic.
    fn backing(&self, topic: &TopicId) -> &[String] {
        self.standings
            .iter()
            .find(|standing| &standing.topic == topic)
            .map_or(&[][..], |standing| &standing.supporters)
    }

    /// How many distinct agents the library still counts for a topic.
    fn backers(&self, topic: &TopicId) -> usize {
        self.backing(topic).len()
    }

    /// A participant's private score for an option, updated by how many *other*
    /// members independently back it, and discounted by how many *other*
    /// members have deposited grounds against it.
    fn posterior(&self, agent: &SimAgent, topic: &TopicId) -> i32 {
        let peers = self
            .backing(topic)
            .iter()
            .filter(|backer| **backer != agent.id)
            .count();
        let peers = i32::try_from(peers).unwrap_or(0);
        let refuters = self.refuters_of(agent, topic);
        agent
            .score(topic)
            .saturating_add(peers.saturating_mul(SOCIAL_WEIGHT))
            .saturating_sub(refuters.saturating_mul(GROUNDS_WEIGHT))
    }

    /// Whether the library still counts this agent as backing a topic.
    pub(crate) fn has_backed(&self, agent_id: &str, topic: &TopicId) -> bool {
        self.backing(topic).iter().any(|held| held == agent_id)
    }

    /// Whether this agent has already deposited an `!evidence` trace naming
    /// `topic`.
    ///
    /// Depositing the same grounds twice would spend a turn saying nothing
    /// new, so a hidden-profile member checks this before repeating its one
    /// fact.
    pub(crate) fn has_deposited(&self, agent_id: &str, topic: &TopicId) -> bool {
        self.traces.iter().any(|trace| {
            trace.kind == TraceKind::Evidence
                && trace.topic.as_ref() == Some(topic)
                && trace.agent_id() == Some(agent_id)
        })
    }

    /// Distinct members who have deposited an `!evidence` trace naming
    /// `topic` **that argues against it**.
    ///
    /// A deposit is read as a refutation when its sentence says so, and as a
    /// reading of the option otherwise — see [`RULES_OUT`] for why a
    /// participant can draw that line and the library's fold cannot. Without
    /// the distinction the evidence-first opening would be self-defeating:
    /// four lay members each stating what they read the planted decoy at
    /// would each be counted as having refuted it, and the decoy would
    /// collapse under the weight of its own supporters' agreement.
    ///
    /// Under `Expertise::Uniform` nobody holds a `refutes` topic, so no
    /// participant ever deposits a refutation and this stays zero for every
    /// topic.
    ///
    /// Unlike the peer count above, this deliberately does *not* exclude
    /// `agent` itself. A peer's backing is social evidence and a member
    /// cannot be its own peer; a stated fact is not social, and a member that
    /// applied everybody's refuting facts except the one it holds itself
    /// would be arguing against its own knowledge. The hidden profile's
    /// fact-holder is exactly that member, and without this it is swamped by
    /// the social weight of the four peers backing the decoy it just refuted.
    fn refuters_of(&self, _agent: &SimAgent, topic: &TopicId) -> i32 {
        let mut seen: Vec<&str> = Vec::new();
        for trace in &self.traces {
            if trace.kind != TraceKind::Evidence || trace.topic.as_ref() != Some(topic) {
                continue;
            }
            if !trace.text.contains(RULES_OUT) {
                continue;
            }
            let Some(author) = trace.agent_id() else {
                continue;
            };
            if seen.contains(&author) {
                continue;
            }
            seen.push(author);
        }
        i32::try_from(seen.len()).unwrap_or(0)
    }

    /// The grounds this member would cite for a position on `topic`: its own
    /// deposit about it where it has one, and otherwise the first deposit
    /// anybody made about it.
    ///
    /// Its own first, because a member arguing from what it stated itself is
    /// the ordinary case; anybody's second, because a member persuaded by
    /// somebody else's fact is citing that fact, which is the whole of what a
    /// citation is for.
    pub(crate) fn grounds_for(&self, agent_id: &str, topic: &TopicId) -> Option<Sequence> {
        let deposits = || {
            self.traces.iter().filter(move |trace| {
                trace.kind == TraceKind::Evidence && trace.topic.as_ref() == Some(topic)
            })
        };
        deposits()
            .find(|trace| trace.agent_id() != Some(agent_id))
            .map(|trace| trace.sequence)
    }

    /// The option this member now rates highest over *everything it holds*,
    /// when that option is not on the floor and nothing on the floor scores as
    /// well.
    ///
    /// The comparison is over posteriors on both sides, so a peer's backing
    /// and a deposited refutation both count: a member does not put an option
    /// up merely because it likes it, only because the room has offered it
    /// nothing better. Ties go to the floor, which is what keeps this from
    /// re-opening a question the room has already converged on.
    pub(crate) fn better_than_floor<'a>(&self, agent: &'a SimAgent) -> Option<&'a TopicId> {
        let best = agent
            .evals
            .iter()
            .map(|(topic, _)| topic)
            .max_by_key(|topic| self.posterior(agent, topic))?;
        if self.proposal(best).is_some() {
            return None;
        }
        let held = self
            .floor()
            .into_iter()
            .map(|(topic, _)| self.posterior(agent, topic))
            .max();
        match held {
            Some(held) if held >= self.posterior(agent, best) => None,
            _ => Some(best),
        }
    }

    /// The proposed option this participant rates highest once the room's own
    /// independent backing is weighed against its private evaluation.
    pub(crate) fn best_proposal(&self, agent: &SimAgent) -> Option<(&TopicId, Sequence)> {
        self.floor()
            .into_iter()
            .max_by_key(|(topic, _)| self.posterior(agent, topic))
    }

    /// The topic carrying the most support, breaking ties by this
    /// participant's own updated evaluation, with a sequence to cite.
    pub(crate) fn leading(&self, agent: &SimAgent) -> Option<(&TopicId, Sequence)> {
        self.floor()
            .into_iter()
            .max_by_key(|(topic, _)| (self.backers(topic), self.posterior(agent, topic)))
    }

    /// The option one supporter short of quorum that this participant has not
    /// backed, when it is not clearly worse than what this participant did
    /// back, with grounds to cite.
    pub(crate) fn closable(&self, agent: &SimAgent) -> Option<(&TopicId, Sequence)> {
        let short = self.threshold.checked_sub(1)?;
        let (topic, grounds) = self
            .floor()
            .into_iter()
            .filter(|(topic, _)| self.backers(topic) == short)
            .filter(|(topic, _)| !self.has_backed(&agent.id, topic))
            .max_by_key(|(topic, _)| self.posterior(agent, topic))?;
        let held = self
            .floor()
            .into_iter()
            .filter(|(held, _)| self.has_backed(&agent.id, held))
            .map(|(held, _)| self.posterior(agent, held))
            .max()
            .unwrap_or(i32::MIN);
        if held != i32::MIN && self.posterior(agent, topic) < held.saturating_sub(CONCESSION) {
            return None;
        }
        Some((topic, grounds))
    }

    /// Two or more options carrying at once, and this participant rates one of
    /// them below another: the message to object to, and the grounds to cite.
    ///
    /// "Carrying" is read the way the library reads it — at or above the quorum
    /// threshold, after cross-inhibition — because that is exactly the
    /// condition that deadlocks an episode. Adding support cannot resolve it:
    /// both options stay above the threshold no matter how much weight one
    /// gains. Silencing an advocate can, and that asymmetry is why the
    /// objection targets a message rather than a topic.
    pub(crate) fn weaker_contender(
        &self,
        agent: &SimAgent,
    ) -> Option<(&TopicId, Sequence, Sequence)> {
        let mut contenders: Vec<&TopicId> = self
            .standings
            .iter()
            .filter(|standing| standing.supporters.len() >= self.threshold)
            .map(|standing| &standing.topic)
            .collect();
        if contenders.len() < 2 {
            return None;
        }
        contenders.sort_by_key(|topic| std::cmp::Reverse(self.posterior(agent, topic)));
        let best = *contenders.first()?;
        let worst = *contenders.last()?;
        if self.posterior(agent, worst) >= self.posterior(agent, best) {
            return None;
        }
        // Silence one advocate of the weaker option: somebody other than this
        // participant, who is still counted as advocating it.
        let target = self.traces.iter().find(|trace| {
            matches!(trace.kind, TraceKind::Propose | TraceKind::Support)
                && trace.topic.as_ref() == Some(worst)
                && trace
                    .agent_id()
                    .is_some_and(|id| id != agent.id && self.has_backed(id, worst))
        })?;
        Some((worst, target.sequence, self.proposal(best)?))
    }

    /// A message advocating a topic this participant rates below its own
    /// favourite, authored by somebody else.
    pub(crate) fn rival_advocacy(&self, agent: &SimAgent) -> Option<(&TopicId, Sequence)> {
        let mine = agent.score(&agent.favourite);
        self.traces
            .iter()
            .filter(|trace| matches!(trace.kind, TraceKind::Propose | TraceKind::Support))
            .filter(|trace| trace.agent_id().is_some_and(|id| id != agent.id))
            .filter_map(|trace| trace.topic.as_ref().map(|topic| (topic, trace.sequence)))
            .filter(|(topic, _)| **topic != agent.favourite && agent.score(topic) < mine)
            .max_by_key(|(topic, _)| self.backers(topic))
    }
}

/// The topic one check names, if it names one.
pub(crate) fn parse_topic(body: &str) -> Option<TopicId> {
    let word = body.split_whitespace().find(|word| word.starts_with('#'))?;
    Some(TopicId::from(word.trim_start_matches('#')))
}

/// The topic and reading one answered check carries, if it is an answer.
///
/// A question carries no number and parses to `None`, which is exactly how the
/// two halves of an exchange are told apart.
pub(crate) fn parse_reading(body: &str) -> Option<(TopicId, i32)> {
    parse_readings(body).into_iter().next()
}

/// The option a row says it rules out, if it says so.
///
/// The refuted option is the last one named before the [`RULES_OUT`] phrase.
/// A single-topic answer names only one option, so this reduces to it; a full
/// exchange carries several readings and one refutation, and the phrase's
/// position is what says which option the refutation is about.
pub(crate) fn parse_ruled_out(body: &str) -> Option<TopicId> {
    let stated = body.find(RULES_OUT)?;
    body.get(..stated)?
        .split_whitespace()
        .filter_map(|word| word.strip_prefix('#'))
        .next_back()
        .map(TopicId::from)
}

/// Every topic and reading one answered check carries, in the order written.
///
/// A pairwise check answers about one option; a full exchange answers about
/// all of them in one row, which is what a contact transfers in the biology
/// and what the alongside arms can afford now that a row costs no turn. Both
/// forms parse here: each `#topic` claims the next `reads N` after it, so the
/// single-topic line is just the one-element case.
pub(crate) fn parse_readings(body: &str) -> Vec<(TopicId, i32)> {
    let mut readings = Vec::new();
    let mut topic: Option<TopicId> = None;
    let mut words = body.split_whitespace().peekable();
    while let Some(word) = words.next() {
        if let Some(name) = word.strip_prefix('#') {
            topic = Some(TopicId::from(name));
        } else if word == ASIDE_READS
            && let Some(held) = topic.take()
            && let Some(value) = words
                .next()
                .and_then(|word| word.trim_end_matches(['.', ',']).parse::<i32>().ok())
        {
            readings.push((held, value));
        }
    }
    readings
}

/// The pairwise-check self-check: assert the properties the aside arms are
/// defined to have, over rooms this file builds itself.
///
/// `sim` is an example module, so `cargo test` never runs a `#[test]` placed
/// in it. This is that coverage's stand-in for the check arms, and it runs in
/// CI behind the same `--stats-check` flag the statistics module uses. Every
/// case is a property the arm is defined to have rather than a fitted number,
/// so a break here is always a real regression.
pub(crate) fn check_selfcheck() -> bool {
    let mut ok = true;
    let room = Room::generate_with(1, 5, 4, 90, Expertise::HiddenProfile, false);

    // `pre_checked(0, ..)` opens no contact, so it is the identity. This is
    // what underwrites `--aside-cap 0` leaving every check arm bit-identical
    // to `hive+`.
    let untouched = room.pre_checked(0, true);
    ok &= untouched
        .agents
        .iter()
        .zip(&room.agents)
        .all(|(after, before)| after.favourite == before.favourite && after.imports.is_empty());

    // The ceiling hands every member every peer's reading of every option, so
    // each member holds exactly one import entry per option it evaluates, and
    // every fact any peer holds.
    let pooled = room.pooled();
    let topics = room.agents.first().map_or(0, |agent| agent.evals.len());
    let facts: Vec<TopicId> = room
        .agents
        .iter()
        .filter_map(|a| a.refutes.clone())
        .collect();
    ok &= pooled.agents.iter().enumerate().all(|(index, agent)| {
        agent.imports.len() == topics
            && facts
                .iter()
                .filter(|topic| {
                    room.agents.get(index).and_then(|a| a.refutes.as_ref()) != Some(*topic)
                })
                .all(|topic| agent.ruled_out.contains(topic))
    });

    // `run_episode_checking` -- the entry point `hive+fact°` and `hive+pooled`
    // both run through -- clones the room and calls `set_aside_cap` on every
    // agent before driving a single turn. That call must not discard what
    // `pre_checked` and `pooled` just preloaded: the regression this guards
    // was landing at exactly that step, silently wiping every ruled-out fact
    // an off-floor control had just injected, so the arm executed the rest of
    // the episode without the fact it was built to carry.
    let checked = room.pre_checked(1, true);
    ok &= checked
        .agents
        .iter()
        .any(|agent| !agent.ruled_out.is_empty());
    let mut after_reset = checked.clone();
    for agent in &mut after_reset.agents {
        agent.set_aside_cap(0, CheckStyle::PLAIN);
    }
    ok &= after_reset
        .agents
        .iter()
        .zip(&checked.agents)
        .all(|(after, before)| after.ruled_out == before.ruled_out);
    let mut pooled_after_reset = pooled.clone();
    for agent in &mut pooled_after_reset.agents {
        agent.set_aside_cap(0, CheckStyle::PLAIN);
    }
    ok &= pooled_after_reset
        .agents
        .iter()
        .zip(&pooled.agents)
        .all(|(after, before)| after.ruled_out == before.ruled_out);

    ok && payload_selfcheck(&room)
}

/// The half of the check self-check that is about an answer's **payload**:
/// what a responder hands over, and what a reader does with it.
///
/// Split from [`check_selfcheck`] only to keep each function inside the line
/// budget clippy holds every function to; the two run together behind
/// `--stats-check` and neither is meaningful alone.
fn payload_selfcheck(room: &Room) -> bool {
    let mut ok = true;
    let Some(sample) = room.agents.first().cloned() else {
        return false;
    };
    let Some((topic, _)) = sample.evals.first().cloned() else {
        return false;
    };

    // A muted check takes nothing in. The matched-turn control has to be
    // exactly that: same turns, same words, no transfer.
    let answer = format!("{ASIDE_MARKER} @a #{topic} My own {ASIDE_READS} 7.");
    let heard = crate::run::one_agent_message("peer", &answer);
    let mut muted = sample.clone();
    muted.set_aside_cap(1, CheckStyle::MUTE);
    muted.absorb(std::slice::from_ref(&heard));
    ok &= muted.imports.is_empty() && muted.score(&topic) == sample.own_reading(&topic);
    let mut listening = sample.clone();
    listening.set_aside_cap(1, CheckStyle::PLAIN);
    listening.absorb(std::slice::from_ref(&heard));
    ok &= listening.imports.len() == 1 && listening.score(&topic) != sample.own_reading(&topic);

    // A fact-carrying answer discounts the option for its reader instead of
    // averaging in the number that came with it -- the fact is taken in
    // place of the reading, not alongside it -- while a reading-only arm
    // ignores the sentence entirely and only ever averages the number.
    let refutation = format!(
        "{ASIDE_MARKER} @a #{topic} My own {ASIDE_READS} 7. The reading I hold {RULES_OUT}."
    );
    let told = crate::run::one_agent_message("peer", &refutation);
    let mut fact_reader = sample.clone();
    fact_reader.set_aside_cap(1, CheckStyle::FACT);
    fact_reader.absorb(std::slice::from_ref(&told));
    ok &= fact_reader.imports.is_empty()
        && fact_reader.ruled_out.contains(&topic)
        && fact_reader.score(&topic)
            == fact_reader
                .own_reading(&topic)
                .saturating_sub(GROUNDS_WEIGHT);
    let mut number_reader = sample.clone();
    number_reader.set_aside_cap(1, CheckStyle::AIMED);
    number_reader.absorb(std::slice::from_ref(&told));
    ok &= number_reader.imports.len() == 1
        && number_reader.ruled_out.is_empty()
        && number_reader.score(&topic) != number_reader.own_reading(&topic);

    // The informed check aims at a depositor who argues *against* the topic,
    // even when a plain deposit on the same topic came first. Aiming at the
    // first depositor is the defect this replaced.
    let plain = crate::run::one_agent_message(
        "early",
        &format!("!evidence #{topic} My own read of it is 9."),
    );
    let against = crate::run::one_agent_message(
        "holder",
        &format!("!evidence #{topic} My own read of it is 1. The reading I hold {RULES_OUT}."),
    );
    let view = View::fold(&[plain, against], QuorumPolicy::DEFAULT);
    ok &= view.grounded_by(&topic, "asker").as_deref() == Some("holder");

    // A full exchange answers about every option in one row, and every reading
    // in it is the responder's own rather than a pooled one. The reader takes
    // all of them.
    let mut donor = sample.clone();
    donor.set_aside_cap(1, CheckStyle::EXCHANGE);
    donor.import(&topic, 999);
    let asked =
        crate::run::one_agent_message("peer", &format!("{ASIDE_MARKER} @{} Well?", donor.id));
    let Some(offered) = donor.answer_check(std::slice::from_ref(&asked)) else {
        return false;
    };
    let carried = parse_readings(&offered);
    ok &= carried.len() == sample.evals.len()
        && carried
            .iter()
            .all(|(held, reading)| *reading == sample.own_reading(held));
    let mut taker = sample.clone();
    taker.set_aside_cap(1, CheckStyle::EXCHANGE);
    taker.absorb(std::slice::from_ref(&crate::run::one_agent_message(
        "peer", &offered,
    )));
    ok &= taker.imports.len() == sample.evals.len();

    // A refutation inside a multi-option row names the option it rules out,
    // and the reader discounts that one rather than whichever was written
    // first.
    let Some((other, _)) = sample.evals.get(1).cloned() else {
        return false;
    };
    let mixed = format!(
        "{ASIDE_MARKER} @a #{topic} {ASIDE_READS} 7. #{other} {ASIDE_READS} 9. \
         #{other} The reading I hold {RULES_OUT}."
    );
    ok &= parse_ruled_out(&mixed).as_ref() == Some(&other);
    // And the single-option form still reads as being about its one option.
    ok &= parse_ruled_out(&format!(
        "{ASIDE_MARKER} @a #{topic} My own {ASIDE_READS} 7. The reading I hold {RULES_OUT}."
    ))
    .as_ref()
        == Some(&topic);

    ok
}
