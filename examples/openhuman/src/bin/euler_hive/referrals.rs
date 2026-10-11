//! Routing between desks: asks, reports and returns, each decided by core's
//! `referral` fold and then checked against the package's declared links.
//! See `floors.rs` for the rules this host adds to the fold.

use tinyhivemind_core::desk::DeskSet;
use tinyhivemind_core::dispatch::{DispatchConversation, DispatchKey};
use tinyhivemind_core::mention::{MentionAuthor, resolve as mentions_in};
use tinyhivemind_core::referral::{
    ReferralDecision, ReferralInput, ReferralKind, ReferralOrigin, referral,
};
use tinyhivemind_core::roster::Roster;

use super::floors::{ASK_CAP, Federation, Final, Incoming, Mode};
use super::frames::ReferralFrame;
use super::tools::Verdict;

impl Federation {
    pub(super) fn ask(
        &mut self,
        desk: usize,
        seat: &str,
        to: &str,
        message: &str,
    ) -> anyhow::Result<()> {
        let from = self.floors[desk].desk.id.clone();
        let content = format!("@#{to} {message}");
        let sequence = self.floors[desk]
            .journal
            .last()
            .map_or(0, |row| row.sequence);
        let mut frame = self.frame("forward", &from, to, seat, &content, 0);
        if self.floors[desk].asks >= ASK_CAP {
            return self.bounce(desk, frame, "width_cap");
        }
        let decision = self.decide(&from, seat, &content, sequence, 0, None)?;
        let one = match decision {
            ReferralDecision::None { reason } => {
                return self.bounce(desk, frame, &snake(&format!("{reason:?}")));
            }
            ReferralDecision::One { referral } => *referral,
        };
        frame.to_desk.clone_from(&one.to.desk_id);
        frame.target = Some(one.target_id.clone());
        frame.hops = one.child_hop;
        if !one.crosses() || one.kind != ReferralKind::Forward {
            return self.bounce(desk, frame, "not_a_desk_referral");
        }
        if !self.linked(&from, &one.to.desk_id) {
            return self.bounce(desk, frame, "unlinked");
        }
        let target = self
            .index(&one.to.desk_id)
            .ok_or_else(|| anyhow::anyhow!("core routed to an unknown desk"))?;
        let origin = one
            .origin
            .clone()
            .ok_or_else(|| anyhow::anyhow!("a crossing forward carries no origin"))?;
        self.floors[desk].asks += 1;
        self.owe(desk, seat);
        let chain = self.next_chain;
        self.next_chain += 1;
        let floor = &mut self.floors[target];
        let row = floor.row(seat, "referral", &format!("from #{from}: {message}"));
        frame.sequence = Some(row.sequence);
        frame.accepted = true;
        floor.queue.push_back(Incoming {
            mode: Mode::Answering {
                origin,
                hop: one.child_hop,
                chain,
            },
            assign: vec![one.target_id],
            reason: "referral",
            row,
        });
        self.referrals.push(frame);
        Ok(())
    }

    pub(super) fn finish_episode(&mut self, desk: usize) -> anyhow::Result<()> {
        let floor = &mut self.floors[desk];
        floor.episode_rounds = 0;
        let Some(mode) = floor.mode.take() else {
            return Ok(());
        };
        let chain = mode.chain();
        // The chain asked questions that have not come back: it continues in
        // the episodes their answers open, and reports from the last of them.
        if floor.outstanding.get(&chain).is_some_and(|owed| *owed > 0) {
            return Ok(());
        }
        floor.outstanding.remove(&chain);
        let completions = floor.completions.remove(&chain).unwrap_or_default();
        let report = completions
            .iter()
            .map(|(seat, report, candidate, verdict)| {
                let tag = match (candidate, verdict) {
                    (Some(candidate), Some(Verdict::Object)) => {
                        format!(" [objects to {candidate}]")
                    }
                    (Some(candidate), _) => format!(" [candidate {candidate}]"),
                    (None, _) => String::new(),
                };
                format!("@{seat}{tag}: {report}")
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        let last = completions
            .last()
            .map(|(seat, ..)| seat.clone())
            .unwrap_or_default();
        let candidate = completions
            .iter()
            .rev()
            .find(|(_, _, candidate, verdict)| {
                candidate.is_some() && *verdict != Some(Verdict::Object)
            })
            .and_then(|(_, _, candidate, _)| candidate.clone());
        let from = floor.desk.id.clone();
        match mode {
            Mode::Answering { origin, hop, .. } => self.answer(desk, &last, &report, origin, hop),
            Mode::Pipeline { hop, .. } => {
                self.floors[desk].reported = true;
                if desk + 1 == self.floors.len() {
                    self.finished = Some(Final {
                        desk: from,
                        seat: last,
                        candidate,
                        report,
                    });
                    return Ok(());
                }
                let outgoing: Vec<String> = self
                    .topology
                    .iter()
                    .filter(|(a, _)| *a == from)
                    .map(|(_, b)| b.clone())
                    .collect();
                for to in outgoing {
                    self.report(desk, &last, &to, &report, hop)?;
                }
                Ok(())
            }
        }
    }

    fn report(
        &mut self,
        desk: usize,
        seat: &str,
        to: &str,
        report: &str,
        hop: u32,
    ) -> anyhow::Result<()> {
        let from = self.floors[desk].desk.id.clone();
        let content = format!("@#{to} {} reports:\n{report}", self.floors[desk].desk.name);
        let sequence = self.floors[desk]
            .journal
            .last()
            .map_or(0, |row| row.sequence);
        let mut frame = self.frame("report", &from, to, seat, &content, hop);
        let decision = self.decide(&from, seat, &content, sequence, hop, None)?;
        let one = match decision {
            ReferralDecision::None { reason } => {
                return self.refuse(desk, frame, &snake(&format!("{reason:?}")));
            }
            ReferralDecision::One { referral } => *referral,
        };
        frame.hops = one.child_hop;
        frame.target = Some(one.target_id.clone());
        let Some(target) = self
            .index(&one.to.desk_id)
            .filter(|_| self.linked(&from, &one.to.desk_id))
        else {
            return self.refuse(desk, frame, "unlinked");
        };
        let chain = self.next_chain;
        self.next_chain += 1;
        let floor = &mut self.floors[target];
        let row = floor.row(seat, "report", &format!("from #{from}:\n{report}"));
        frame.sequence = Some(row.sequence);
        frame.accepted = true;
        floor.queue.push_back(Incoming {
            mode: Mode::Pipeline {
                hop: one.child_hop,
                chain,
            },
            assign: floor.desk.members.clone(),
            reason: "report",
            row,
        });
        self.referrals.push(frame);
        Ok(())
    }

    fn answer(
        &mut self,
        desk: usize,
        seat: &str,
        report: &str,
        origin: ReferralOrigin,
        hop: u32,
    ) -> anyhow::Result<()> {
        let from = self.floors[desk].desk.id.clone();
        let sequence = self.floors[desk]
            .journal
            .last()
            .map_or(0, |row| row.sequence);
        let to = origin.conversation.desk_id.clone();
        let origin_asker = origin.asker_id.clone();
        let mut frame = self.frame("return", &from, &to, seat, report, hop);
        let decision = self.decide(&from, seat, report, sequence, hop, Some(origin))?;
        let one = match decision {
            ReferralDecision::None { reason } => {
                // The asker must not wait forever on an answer core refused:
                // its chain resumes with the refusal instead.
                let reason = snake(&format!("{reason:?}"));
                if let Some(asker) = self.index(&to) {
                    let notice = format!("host: the answer from #{from} was refused ({reason})");
                    self.come_back(asker, &origin_asker, "refused", &notice);
                }
                return self.refuse(desk, frame, &reason);
            }
            ReferralDecision::One { referral } => *referral,
        };
        frame.hops = one.child_hop;
        frame.target = Some(one.target_id.clone());
        let linked = self.linked(&to, &from) || self.linked(&from, &to);
        let Some(target) = self
            .index(&one.to.desk_id)
            .filter(|_| linked && one.kind == ReferralKind::Return)
        else {
            if let Some(asker) = self.index(&to) {
                let notice =
                    format!("host: the answer from #{from} could not travel back (unlinked)");
                self.come_back(asker, &origin_asker, "refused", &notice);
            }
            return self.refuse(desk, frame, "unlinked");
        };
        let sequence = self.come_back(
            target,
            &one.target_id,
            "return",
            &format!("answer from #{from}:\n{report}"),
        );
        frame.sequence = Some(sequence);
        frame.accepted = true;
        self.referrals.push(frame);
        Ok(())
    }

    /// Record that `seat`'s current chain on `desk` is owed one answer.
    fn owe(&mut self, desk: usize, seat: &str) {
        let floor = &mut self.floors[desk];
        let mode = floor
            .mode
            .clone()
            .unwrap_or(Mode::Pipeline { hop: 0, chain: 0 });
        *floor.outstanding.entry(mode.chain()).or_insert(0) += 1;
        floor.waiting.entry(seat.to_owned()).or_default().push(mode);
    }

    /// Hand `seat` back the chain it asked from, with `body` (an answer or a
    /// refusal) as the row that reopens it. Returns that row's sequence.
    fn come_back(&mut self, desk: usize, seat: &str, reason: &'static str, body: &str) -> u64 {
        let floor = &mut self.floors[desk];
        let author = if reason == "return" { seat } else { "host" };
        let row = floor.row(
            author,
            if reason == "return" {
                "return"
            } else {
                "notice"
            },
            body,
        );
        let mode = floor
            .waiting
            .get_mut(seat)
            .and_then(Vec::pop)
            .unwrap_or(Mode::Pipeline { hop: 0, chain: 0 });
        let sequence = row.sequence;
        floor.queue.push_back(Incoming {
            mode,
            assign: vec![seat.to_owned()],
            reason,
            row,
        });
        sequence
    }

    fn decide(
        &self,
        from: &str,
        author: &str,
        content: &str,
        sequence: u64,
        hop: u32,
        origin: Option<ReferralOrigin>,
    ) -> anyhow::Result<ReferralDecision> {
        let roster = Roster::new(&self.everyone, &[], &[]);
        let desks = DeskSet::new(&self.desks, &[], &[], &[], &[]);
        let mentions = mentions_in(
            content,
            None,
            &MentionAuthor::Agent {
                id: author.to_owned(),
            },
            &roster,
            &desks,
        );
        let input = ReferralInput {
            key: DispatchKey {
                trigger_sequence: sequence,
            },
            conversation: DispatchConversation {
                desk_id: from.to_owned(),
                thread_root: None,
            },
            author_id: author.to_owned(),
            content: content.to_owned(),
            mentions,
            hop,
            origin,
        };
        Ok(referral(self.policy, &input, &roster, &desks)?)
    }

    fn frame(
        &self,
        kind: &str,
        from: &str,
        to: &str,
        seat: &str,
        content: &str,
        hops: u32,
    ) -> ReferralFrame {
        ReferralFrame {
            round: self.round,
            kind: kind.into(),
            from_desk: from.into(),
            to_desk: to.into(),
            seat: seat.into(),
            target: None,
            content: content.into(),
            hops,
            accepted: false,
            reason: None,
            sequence: None,
        }
    }

    fn refuse(
        &mut self,
        desk: usize,
        mut frame: ReferralFrame,
        reason: &str,
    ) -> anyhow::Result<()> {
        frame.reason = Some(reason.into());
        let notice = format!(
            "host: the referral to #{} was refused ({reason})",
            frame.to_desk
        );
        self.floors[desk].row("host", "notice", &notice);
        self.referrals.push(frame);
        Ok(())
    }

    /// A refused ask: tell the desk, and give the asking seat its turn back
    /// before the desk may report, as if the refusal were the answer.
    fn bounce(
        &mut self,
        desk: usize,
        mut frame: ReferralFrame,
        reason: &str,
    ) -> anyhow::Result<()> {
        frame.reason = Some(reason.into());
        let notice = format!(
            "host: your referral to #{} was refused ({reason}); carry on with what your desk has",
            frame.to_desk
        );
        let seat = frame.seat.clone();
        self.owe(desk, &seat);
        self.come_back(desk, &seat, "refused", &notice);
        self.referrals.push(frame);
        Ok(())
    }
}

fn snake(name: &str) -> String {
    let mut out = String::new();
    for (index, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if index > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}
