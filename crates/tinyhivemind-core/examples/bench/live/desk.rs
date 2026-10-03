//! Drive one seat as a member of a federation of desks.
//!
//! A [`LiveDeskAgent`] is a [`LiveAgent`](super::agent::LiveAgent) that also
//! knows which channel it is on and who the other channels are, so it can
//! offer the one extra move a federated member has: asking another desk a
//! question by mentioning its channel. Everything else — the prompt, the
//! parsing, the one process per turn — is unchanged from the plain,
//! non-federated seat in [`super::agent`].

use tinyhivemind_core::hive::SessionMessage;
use tinyhivemind_core::hive::referral::{Referral, ReferralKind};

use super::AgentPrompt;
use super::agent::LiveAgent;
use crate::run::Participant;
use crate::swarm::SwarmMember;

/// The one extra move a member of a *federation* has.
///
/// It is written as an ordinary mention because that is what it is: the host
/// reads the line with the same mention grammar it reads every other line
/// with, and `referral` decides where the turn lands. Nothing about this move
/// is special-cased, which is the point — an agent that writes `@#platform`
/// into a sentence has asked another channel a question whether or not it
/// meant to invoke a protocol.
pub(crate) const CROSS_PROTOCOL: &str = "\
@#deskid  then your question, asking another desk — this is a marker like the \
others and a legal line on its own";

/// What a member needs to know about that move to use it well.
pub(crate) const CROSS_RULES: &str = "\
You are not alone. The other desks below are working the same problem in their \
own channels and you cannot see their transcripts, so a fact one of them holds \
is a fact this desk does not have and cannot deduce. `@#deskid` runs one turn \
on that desk and their answer comes back here as one line. It costs you this \
turn, so ask only for what you cannot settle here — but do ask. Deciding on \
this desk's evidence alone, when the evidence that would change your mind is \
one question away on a desk you can address, is the exact failure this \
arrangement exists to prevent. Put your own reading in the question; a desk \
that has to guess what you already know answers a worse question.";

/// A member of a federation backed by an external agent command.
///
/// It is a [`LiveAgent`] that also knows which channel it is on and who the
/// other channels are. Everything else — the prompt, the parsing, the one
/// process per turn — is unchanged.
pub(crate) struct LiveDeskAgent {
    agent: LiveAgent,
    /// This member's own desk, by display name.
    here: String,
    /// Every other channel, id and display name.
    peers: Vec<(String, String)>,
}

impl LiveDeskAgent {
    /// Seat one live agent on a channel.
    pub(crate) fn new(agent: LiveAgent, here: String, peers: Vec<(String, String)>) -> Self {
        Self { agent, here, peers }
    }

    /// Run one prompt through the agent process and take its one line.
    fn one_line(&self, prompt: &str) -> Result<String, String> {
        self.agent.line(prompt)
    }

    /// The sentence naming the channels this member may reach.
    fn directory(&self) -> String {
        let named = self
            .peers
            .iter()
            .map(|(id, name)| format!("@#{id} — the {name} desk"))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "{CROSS_PROTOCOL}\n\n{CROSS_RULES}\nYou are on the {} desk. The desks you can \
             address, and how:\n{named}\n",
            self.here,
        )
    }
}

impl SwarmMember for LiveDeskAgent {
    fn id(&self) -> &str {
        self.agent.id()
    }

    fn speak(
        &mut self,
        turn: &tinyhivemind_core::hive::HiveTurn,
        visible: &[SessionMessage],
    ) -> Result<String, String> {
        // The federation's own move is offered alongside the ordinary ones,
        // and the agent decides. The harness never writes a mention on an
        // agent's behalf in this arm; if no cross-channel message happens,
        // that is a finding about the agents rather than about the protocol.
        let prompt = self.agent.prompt_with(turn, visible, &self.directory());
        self.one_line(&prompt)
    }

    fn answer(
        &mut self,
        incoming: &Referral,
        visible: &[SessionMessage],
    ) -> Result<String, String> {
        let transcript = AgentPrompt::render(visible);
        let asked = match incoming.kind {
            ReferralKind::Forward => format!(
                "@{} on another desk has asked your desk this, and your answer will be posted \
                 here and carried back to them:\n{}\n\nAnswer it in ONE line, beginning with \
                 !evidence. They cannot see anything on this desk, so state the facts they need \
                 — including the ones above that only you hold — rather than your conclusion \
                 from them. A desk that asks for a number and receives an argument has learned \
                 nothing it can check. Do not ask a question back and do not tell them which \
                 option to pick.",
                incoming.source_id, incoming.content,
            ),
            ReferralKind::Return => format!(
                "You asked another desk a question and this is what came back:\n{}\n\nRelay it \
                 to your own desk in ONE line, beginning with !evidence, stating what they told \
                 you. Do not add anything they did not say.",
                incoming.content,
            ),
        };
        let prompt = format!(
            "You are @{}, on the {} desk.\n\n{}\n{asked}\n\nYour desk's transcript so far:\n\
             {transcript}\n\nYour one line:",
            self.agent.id(),
            self.here,
            self.agent.private(),
        );
        self.one_line(&prompt)
    }
}
