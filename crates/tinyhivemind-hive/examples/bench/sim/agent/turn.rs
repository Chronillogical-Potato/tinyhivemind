//! What one turn says.
//!
//! Everything here decides the body of a turn -- the pairwise check, the
//! evidence-first opening, and the ladder of floor moves `compose` works down
//! -- and the [`Participant`](crate::run::Participant) impl that lets the
//! host drive it. It reads a member's holdings through the accessors
//! [`super::state`] builds; nothing here mutates them beyond what taking in a
//! reading or spending a check requires.

use std::fmt::Write as _;

use tinyhivemind_hive::trace::TopicId;
use tinyhivemind_hive::{HiveTurn, Phase, SessionAuthor, SessionMessage, Visibility};

use super::SimAgent;
use crate::run::ASIDE_MARKER;
use crate::sim::view::{View, parse_reading, parse_readings, parse_ruled_out, parse_topic};
use crate::sim::{ASIDE_READS, ASIDE_UNCERTAINTY, NONCOMPLIANCE, RULES_OUT, Role};

impl SimAgent {
    /// Spend this turn on a pairwise check, if this member wants one.
    ///
    /// Three parts, in order: fold in whatever readings this member can now
    /// read, answer anybody who asked, and otherwise ask. Each of them reads
    /// the transcript the library authorized this turn to see, so under a
    /// private exchange a member outside it parses a stub and takes nothing,
    /// and under a public one every member parses the same line and takes the
    /// same reading. That difference is the whole of what the aside arms
    /// measure, and the projection rather than anything here produces it.
    fn check(&mut self, visible: &[SessionMessage], view: &View) -> Option<String> {
        if self.aside_cap == 0 {
            return None;
        }
        // A reading is not a position: it changes what this member believes,
        // and it still has to spend a turn saying so before the room counts
        // anything.
        self.absorb(visible);

        // Answering costs this turn, which is what makes one exchange cost
        // two turns rather than one.
        if let Some(line) = self.answer_check(visible) {
            return Some(line);
        }

        // Two options this member cannot separate. A member that already
        // knows its own mind spends the turn saying so instead.
        let line = self.open_check(visible, view)?;
        self.asides_spent = self.asides_spent.saturating_add(1);
        Some(line)
    }

    /// Take in every second reading this member can read and has not yet.
    ///
    /// Marked by sequence rather than by content, so a line is folded once
    /// however many turns it stays in the window.
    ///
    /// `pub(crate)` so the self-check in [`crate::sim::view`] can drive it
    /// directly against a hand-built message, the same way a real turn does.
    pub(crate) fn absorb(&mut self, visible: &[SessionMessage]) {
        for message in visible {
            let Some(body) = message.readable() else {
                // An exchange this member can see happened and cannot read.
                // It costs a row and carries nothing, which is the whole
                // trade an aside makes for everybody outside it: the
                // projection collapses the exchange to one stub instead of
                // showing its messages, so the non-member pays one row rather
                // than however many were said.
                //
                // Charged once, by sequence, for the same reason a reading is:
                // a stub that stayed in the window for four turns is one row,
                // not four.
                if !self.budget.is_unbounded() && !self.handled.contains(&message.sequence) {
                    self.handled.push(message.sequence);
                    let topic = self.favourite.clone();
                    self.note_stub(&topic);
                }
                continue;
            };
            if self.handled.contains(&message.sequence) || !body.starts_with(ASIDE_MARKER) {
                continue;
            }
            let author = match &message.author {
                SessionAuthor::Agent { id, .. } => id.as_str(),
                _ => continue,
            };
            if author == self.id {
                continue;
            }
            let readings = parse_readings(body);
            if readings.is_empty() {
                continue;
            }
            self.handled.push(message.sequence);
            // A refutation is not a reading to be averaged. A member told
            // privately that its best option is ruled out revises its own
            // belief instead of pooling one more number into it — which is
            // what a contact transfers in every biological analogue this
            // workspace cites, and what the room's public grammar has always
            // carried in `!evidence`. It stays a belief: no trace resolves
            // out of an aside row, so the room still counts nothing.
            if self.style.mute() {
                continue;
            }
            // A refutation names the option it rules out explicitly, because
            // a full exchange carries several readings in one row and "the
            // topic" would otherwise be ambiguous.
            let refuted = self
                .style
                .evidence()
                .then(|| parse_ruled_out(body))
                .flatten();
            if let Some(topic) = refuted.clone() {
                self.note_fact(&topic);
            }
            // The fact is carried *instead of* the number for the option it
            // rules out, never in addition to it: `CheckStyle::FACT` exists to
            // ask whether that substitution beats an averaged reading, so
            // importing the refuted option's reading too would answer a
            // different question than the arm is defined to ask. Every *other*
            // reading in the row is still taken.
            for (held, reading) in readings {
                if refuted.as_ref() == Some(&held) {
                    continue;
                }
                self.import(&held, reading);
            }
            // `import` recomputes `favourite` on every reading it takes, but a
            // row carrying only a refutation takes none — so the discount has
            // to be reflected here or the member keeps a favourite its own
            // `ruled_out` has since discounted.
            self.recompute_favourite();
        }
    }

    /// Answer a check addressed to this member, with its own reading.
    ///
    /// `pub(crate)` so the self-check in [`crate::sim::view`] can drive it
    /// directly against a hand-built request, the same way a real turn does.
    pub(crate) fn answer_check(&mut self, visible: &[SessionMessage]) -> Option<String> {
        let request = visible.iter().rev().find(|message| {
            message.readable().is_some_and(|body| {
                body.starts_with(ASIDE_MARKER)
                    && body.contains(&format!("@{}", self.id))
                    && parse_reading(body).is_none()
            }) && !self.handled.contains(&message.sequence)
        })?;
        let from = match &request.author {
            SessionAuthor::Agent { id, .. } => id.clone(),
            _ => return None,
        };
        // A full exchange hands over everything this member holds rather than
        // an answer to the one question asked, so its question names no option
        // and this runs before the topic is read out of one. Same move as the
        // pairwise answer below — this member's own untouched readings, never
        // its pooled ones — over every option instead of one.
        if self.style.exchange() {
            self.handled.push(request.sequence);
            let mut parts: Vec<String> = vec![format!("{ASIDE_MARKER} @{from}")];
            parts.extend(self.evals.iter().map(|(held, _)| {
                let reading = self.own_reading(held);
                format!("#{held} {ASIDE_READS} {reading}.")
            }));
            if let Some(refuted) = self.refutes.clone() {
                parts.push(format!("#{refuted} The reading I hold {RULES_OUT}."));
            }
            return Some(parts.join(" "));
        }
        let topic = parse_topic(request.readable()?)?;
        self.handled.push(request.sequence);
        // The member's *own* reading, not `score()`. `score` averages in every
        // reading this member has already absorbed, so answering with it would
        // echo an already-pooled value back into the room: a later asker would
        // count one peer's signal several times over, and the arm would stop
        // modelling the experiment it documents — one private evaluation
        // averaged with one independent peer's.
        let reading = self.own_reading(&topic);
        // A member holding the one fact that rules this option out says so,
        // rather than handing over a number for the asker to average into an
        // error they already share. Off unless the arm asked for it, so the
        // reading-only arms are unchanged.
        if self.style.evidence() && self.refutes.as_ref() == Some(&topic) {
            return Some(format!(
                "{ASIDE_MARKER} @{from} #{topic} My own {ASIDE_READS} {reading}. \
                 The reading I hold {RULES_OUT}."
            ));
        }
        Some(format!(
            "{ASIDE_MARKER} @{from} #{topic} My own {ASIDE_READS} {reading}."
        ))
    }

    /// The option this member cannot separate from its runner-up, if there is
    /// one. The trigger for a pairwise check, wherever the check happens.
    pub(crate) fn uncertain_about(&self) -> Option<TopicId> {
        let mut ranked: Vec<(&TopicId, i32)> = self
            .evals
            .iter()
            .map(|(topic, _)| (topic, self.score(topic)))
            .collect();
        ranked.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
        let [(best, top), (_, second), ..] = ranked.as_slice() else {
            return None;
        };
        (top.saturating_sub(*second) <= ASIDE_UNCERTAINTY).then(|| (*best).clone())
    }

    /// Ask one peer what they read, when this member cannot separate its own
    /// two best options.
    fn open_check(&mut self, visible: &[SessionMessage], view: &View) -> Option<String> {
        if self.asides_spent >= self.aside_cap {
            return None;
        }
        // A continuous exchange does not wait to be uncertain, and does not
        // pick a peer for a question: it contacts whoever it has not reached
        // yet. `aside_cap` bounds how many that is.
        if self.style.exchange() {
            let peer = self
                .peers
                .iter()
                .find(|id| !self.contacted.contains(id))
                .cloned()?;
            self.contacted.push(peer.clone());
            return Some(format!("{ASIDE_MARKER} @{peer} What do you make of these?"));
        }
        let mut ranked: Vec<(&TopicId, i32)> = self
            .evals
            .iter()
            .map(|(topic, _)| (topic, self.score(topic)))
            .collect();
        ranked.sort_by_key(|(_, score)| std::cmp::Reverse(*score));
        let [(best, top), (_, second), ..] = ranked.as_slice() else {
            return None;
        };
        if top.saturating_sub(*second) > ASIDE_UNCERTAINTY {
            return None;
        }
        let topic = (*best).clone();

        // Whoever the room has already heard ground this option, and
        // otherwise the first peer that has spoken. Asking the member the
        // transcript shows knows something about the question is the informed
        // version of the move, and it is what closes the obvious objection to
        // a negative result — that the check went to the wrong peer.
        //
        // Deterministic either way, and drawn from the transcript rather than
        // the roster, so a member asks somebody the room has actually heard
        // from rather than a name it was handed.
        let peer = if self.style.informed {
            view.grounded_by(&topic, &self.id)
        } else {
            None
        };
        let peer = match peer {
            Some(peer) => peer,
            None => visible.iter().find_map(|message| match &message.author {
                SessionAuthor::Agent { id, .. } if *id != self.id => Some(id.clone()),
                _ => None,
            })?,
        };
        Some(format!(
            "{ASIDE_MARKER} @{peer} #{topic} What do you make of this one?"
        ))
    }

    /// Produce the body of one turn, seeing exactly what the turn authorized.
    fn compose(&mut self, turn: &HiveTurn, visible: &[SessionMessage]) -> String {
        let view = View::fold(visible, self.quorum);

        // Real participants do not speak the grammar on every turn. Modelling
        // that is the difference between benchmarking the protocol and
        // benchmarking a formatter.
        if self.rng.chance(NONCOMPLIANCE) {
            self.complied = false;
            return format!(
                "Thinking about this; {} still looks strongest to me.",
                self.favourite
            );
        }
        self.complied = true;

        // A pairwise check, and the three parts of it. Every one of them reads
        // the transcript the library authorized this turn to see, so under a
        // private exchange a member outside it parses a stub and takes
        // nothing, and under a public one every member parses the same line
        // and takes the same reading. That difference is the whole of what the
        // aside arms measure, and it is produced by the projection rather than
        // by anything here.
        // Under `CheckStyle::alongside` the check does not compete with this
        // turn: the member takes in whatever it can read — which costs
        // nothing, and is the whole point — and then makes its ordinary floor
        // move. The private half is asked for separately, in
        // `Participant::aside`, and appended as a second row on this same
        // turn.
        if self.style.alongside {
            self.absorb(visible);
        } else if let Some(line) = self.check(visible, &view) {
            return line;
        }

        // The evidence-first opening: while nobody can read anybody, say what
        // you know rather than what you want. This is the whole of
        // `--blind-evidence` on the writing side, and the module docs say why
        // it is a participant policy rather than something the library could
        // impose.
        if self.blind_evidence
            && turn.visibility == Visibility::Blind
            && let Some(line) = self.opening_deposit(&view)
        {
            return line;
        }

        // A member holding the fact that rules out a hidden-profile decoy
        // deposits it once. Readers apply it to their own posterior.
        if let Some(topic) = self.refutes.clone()
            && let Some(proposal) = view.proposal(&topic)
            && !view.has_deposited(&self.id, &topic)
        {
            return format!("!evidence #{topic} ^{proposal} The reading I hold {RULES_OUT}.");
        }

        // Two options are both carrying, which is the one state no amount of
        // further support can resolve: a room does not settle by adding weight
        // to one side, because both sides stay above the threshold. Cross-
        // inhibition is the mechanism the library provides for exactly this —
        // object to a *message*, which silences its author as an advocate of
        // whatever it advocated there. That is why the objection is checked
        // before the commit: a member that records a decision into a tie
        // spends the budget without ever reaching one.
        if let Some((topic, target, grounds)) = view.weaker_contender(self) {
            return format!(
                "!object >{target} ^{grounds} I rate {topic} below the other option carrying here."
            );
        }

        // A commit turn records what the room actually carried, not what this
        // member would have preferred.
        if turn.phase == Phase::Commit
            && let Some((topic, grounds)) = view.leading(self)
        {
            return format!("!commit #{topic} ^{grounds} Recording the decision the room reached.");
        }

        // Nothing on the floor is worth as much as something this member is
        // still holding, so put that up instead of backing a worse option.
        //
        // Only under the evidence-first opening, and only because of it: with
        // the ordinary opening every member's favourite reaches the floor in
        // the blind round, so there is never anything better left in a hand.
        // With the floor empty for a whole round, a member that could only
        // ever back what was already there would hand the decision to whoever
        // happened to propose first, and the reading it deposited would
        // decide nothing.
        if self.blind_evidence
            && let Some(topic) = view.better_than_floor(self)
        {
            // The proposal states a conclusion. Its grounds belong on a
            // later support, where a member answers a claim already made.
            return format!(
                "!propose #{topic} It rates highest once I weigh what the room has stated \
                 against my own read."
            );
        }

        // Back the best option currently on the floor, weighing this member's
        // own signal against how many peers independently backed it. This is
        // the step that pools information across the room.
        if let Some((topic, grounds)) = view.best_proposal(self)
            && !view.has_backed(&self.id, topic)
        {
            // Under the evidence-first opening the grounds are the deposit
            // the room actually stated about this option, where there is one.
            let grounds = if self.blind_evidence {
                view.grounds_for(&self.id, topic).unwrap_or(grounds)
            } else {
                grounds
            };
            let mut line = String::new();
            let _ = write!(
                line,
                "!support #{topic} ^{grounds} It scores highest once I weigh the room against my own read."
            );
            return line;
        }

        // The room is one supporter short of settling and this member has not
        // backed the option in front. Closing that out is what ends a
        // deliberation; holding out for a preference the room does not share
        // is what spends the budget without deciding anything.
        if let Some((topic, grounds)) = view.closable(self) {
            return format!(
                "!support #{topic} ^{grounds} Close enough to my own read to settle it here."
            );
        }

        // Nothing worth backing is on the floor, so put an option there.
        if view.proposal(&self.favourite).is_none() {
            return format!(
                "!propose #{} It is the option I rate highest.",
                self.favourite
            );
        }

        match self.role {
            // A critic keeps pressing on an option it privately rates poorly
            // even when the room is not yet tied.
            Role::Critic => {
                if let Some((topic, target)) = view.rival_advocacy(self)
                    && let Some(grounds) = view.proposal(&self.favourite)
                {
                    return format!(
                        "!object >{target} ^{grounds} I rate {topic} below the alternative on the floor."
                    );
                }
                self.evidence(&view)
            }
            Role::Proposer | Role::Archivist => self.evidence(&view),
        }
    }

    /// The deposit this member opens the room with under `--blind-evidence`:
    /// its own reading of the one topic it knows best, stated before anybody
    /// has taken a position.
    ///
    /// The topic is the fact this member holds against a planted decoy where
    /// it holds one, its specialty where it has one, and otherwise its own
    /// favourite — "what I know best" in each of the three shapes the
    /// benchmark generates. There is no citation, because nothing is visible
    /// to cite: an uncited `!evidence` is still a full-weight deposit in the
    /// directory, which also makes the deposit available to task division.
    ///
    /// `None` once this member has already deposited on that topic, which
    /// cannot happen inside a blind round that ends when every member has
    /// spoken once, and is checked anyway so the move stays idempotent.
    fn opening_deposit(&self, view: &View) -> Option<String> {
        let topic = self
            .refutes
            .clone()
            .or_else(|| self.specialty.clone())
            .unwrap_or_else(|| self.favourite.clone());
        if view.has_deposited(&self.id, &topic) {
            return None;
        }
        let reading = self.score(&topic);
        // A reading and a refutation are the same marker to the library's
        // fold; the sentence is what separates them for a reader. See
        // `RULES_OUT`.
        if self.refutes.as_ref() == Some(&topic) {
            Some(format!(
                "!evidence #{topic} My own read of it is {reading}. The reading I hold {RULES_OUT}."
            ))
        } else {
            Some(format!(
                "!evidence #{topic} My own read of it is {reading}."
            ))
        }
    }

    /// Add grounds without taking a side.
    fn evidence(&self, view: &View) -> String {
        view.proposal(&self.favourite).map_or_else(
            || "!question What would make one of these options clearly safer?".to_owned(),
            |grounds| {
                format!("!evidence ^{grounds} Prior rollouts of this shape behaved the same way.")
            },
        )
    }
}

impl crate::run::Participant for SimAgent {
    fn context_rows(&self) -> usize {
        self.context.len()
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn speak(&mut self, turn: &HiveTurn, visible: &[SessionMessage]) -> Result<String, String> {
        Ok(self.compose(turn, visible))
    }

    fn exchange(&mut self, visible: &[SessionMessage]) -> Option<String> {
        // No turn is being taken, so there is no `compose` to have absorbed
        // first and no compliance draw to respect: a member asked in a round
        // it is not otherwise part of reads what it can, then answers or asks.
        if !self.style.alongside || self.aside_cap == 0 {
            return None;
        }
        // A round during the blind phase shows this member no peer at all, so
        // anything it wrote would sit unread until blindness lifted and every
        // contact it spent doing so would be gone by then. Declining is what
        // keeps a bounded budget for the turns that can actually use it —
        // measured, not assumed: without this the arm spends its whole cap
        // before the room can read a word of it.
        if !visible.iter().any(
            |message| matches!(&message.author, SessionAuthor::Agent { id, .. } if *id != self.id),
        ) {
            return None;
        }
        self.absorb(visible);
        if let Some(line) = self.answer_check(visible) {
            return Some(line);
        }
        let view = View::fold(visible, self.quorum);
        let line = self.open_check(visible, &view)?;
        self.asides_spent = self.asides_spent.saturating_add(1);
        Some(line)
    }

    fn aside(&mut self, _turn: &HiveTurn, visible: &[SessionMessage]) -> Option<String> {
        // The same three parts `check` runs, minus the absorb `compose` has
        // already done on this turn: answer whoever asked, and otherwise ask.
        // The difference is only where the line goes — a second row on this
        // turn rather than this turn's only row.
        if !self.style.alongside || !self.complied || self.aside_cap == 0 {
            return None;
        }
        if let Some(line) = self.answer_check(visible) {
            return Some(line);
        }
        let view = View::fold(visible, self.quorum);
        let line = self.open_check(visible, &view)?;
        self.asides_spent = self.asides_spent.saturating_add(1);
        Some(line)
    }

    fn cost_unit(&self) -> u32 {
        self.cost_unit
    }
}
