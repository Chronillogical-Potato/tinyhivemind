//! This example as a host: its journal, and its seats.
//!
//! A real host -- OpenCompany -- answers these with its journal, the agents
//! it already builds, and the task-locals its tools read. This one has no
//! agents of its own, so a seat is a library session carrying only the
//! episode's tools, and the wrapper is the core context such a session runs
//! under. The log is the same in-memory journal the episode appends to.

use std::collections::BTreeMap;
use std::sync::Arc;

use openhuman_embed::{Agent, AgentSpec, HostTurnTools, Runtime};
use tinyhivemind::speech::Utterance;
use tinyhivemind::{Sequence, SessionLog};
use tinyhivemind_driver::{Commit, EpisodeBrief, Event, Note, Refusal};
use tinyhivemind_openhuman::{
    EpisodeBeltSource, EpisodeHost, HostedTurn, Journal, Lane, LibraryHost, MemoryLog,
    TurnResult,
};

/// What the host says about the desk, before the episode's own contract.
pub const DESK_PREAMBLE: &str = "\
You are one seat on a desk. You have no codebase, shell or filesystem -- only
the desk's messages and your own judgement. Never ask for permission and never
wait to be told to continue; nobody will answer.";

/// How much of a reply that recorded nothing is shown in the log.
const REPLY_SHOWN: usize = 600;

/// This example's journal, with the prompt and the log lines a host owns.
pub struct DeskJournal {
    log: Arc<MemoryLog>,
    /// Each seat's brief: who it is, and what it privately knows.
    briefs: BTreeMap<String, String>,
    /// Print nothing per turn: a bench prints one table instead.
    quiet: bool,
}

impl DeskJournal {
    pub fn new(log: Arc<MemoryLog>, briefs: BTreeMap<String, String>, quiet: bool) -> Self {
        Self { log, briefs, quiet }
    }
}

/// How a row reads on the desk.
fn describe(utterance: &Utterance) -> String {
    match utterance {
        Utterance::Post { message } | Utterance::Dm { message, .. } => message.clone(),
        Utterance::Broadcast { message } => format!("BROADCAST: {message}"),
        Utterance::Ask { to, message } => format!("asks @{}: {message}", to.join(", @")),
        Utterance::CompleteEpisode { message } => format!("COMPLETE: {message}"),
    }
}

impl Journal for DeskJournal {
    fn log(&self) -> &dyn SessionLog {
        &*self.log
    }

    fn commit(&self, commit: &Commit) -> tinyhivemind_openhuman::Result<Sequence> {
        Ok(self.log.append(
            &commit.author,
            &describe(&commit.utterance),
            commit.thread,
            &commit.only_for,
        ))
    }

    fn note(&self, note: &Note) -> tinyhivemind_openhuman::Result<()> {
        self.log
            .append("desk", &note.body, note.thread, note.only_for.as_slice());
        Ok(())
    }

    /// What the host owns first; what the episode knows after.
    fn compose(&self, seat: &str, brief: &EpisodeBrief) -> String {
        format!(
            "## The desk\n{DESK_PREAMBLE}\n\n## Who you are\n{}\n\n{}",
            self.briefs[seat],
            brief.render()
        )
    }

    fn event(&self, event: &Event) {
        match event {
            Event::Nudged { seat, thread: None } => {
                eprintln!("[nudged] @{seat} on the desk: stalled with open work");
            }
            Event::Nudged {
                seat,
                thread: Some(root),
            } => eprintln!("[nudged] @{seat} in thread {}", root.0),
            Event::Parked { seat, thread } => {
                eprintln!("[parked] @{seat}{} waits on the operator", place(*thread));
            }
            Event::Resumed { seat, thread } => {
                eprintln!("[resumed] @{seat}{} was released", place(*thread));
            }
            Event::Broadcast { seat, to, .. } => {
                println!("[broadcast] @{seat} -> {}", to.join(", "));
            }
            Event::Unplaced { seat, .. } => {
                println!("[unplaced] @{seat}'s broadcast fits no seat; it keeps the work");
            }
            Event::CompletedByBroadcast { seat, .. } => {
                eprintln!("[completed] @{seat} by its broadcast");
            }
            Event::Asked {
                seat,
                askees,
                root,
            } => println!(
                "[ask] @{seat} opened a conversation with @{} (thread {})",
                askees.join(", @"),
                root.0
            ),
            Event::Handoff { to, from, .. } => {
                println!("[handoff] -> @{to} (queued from @{from})");
            }
            Event::Refused {
                seat, thread, why, ..
            } => {
                let where_ = thread.map_or(String::new(), |root| format!(" in thread {}", root.0));
                let reason = match why {
                    Refusal::AwaitingReply { waiting_on } => {
                        format!("may not complete: in conversation with {waiting_on:?}")
                    }
                    Refusal::Undelivered { assigned_at } => format!(
                        "completed before seeing its assignment at {}",
                        assigned_at.0
                    ),
                    Refusal::NotYetShown => "not yet shown".to_owned(),
                };
                eprintln!("[refused] @{seat}{where_}: {reason}");
            }
            Event::Discharged { seat, .. } => {
                eprintln!("[refused] @{seat} has spent its broadcast budget; it keeps the work");
            }
            Event::Concluded {
                root,
                asker,
                askees,
                forced,
                ..
            } => println!(
                "[concluded] thread {} between @{asker} and @{}{}",
                root.0,
                askees.join(", @"),
                if *forced {
                    " (nothing due, or out of turns)"
                } else {
                    ""
                }
            ),
        }
    }

    fn turn_done(
        &self,
        seat: &str,
        lane: Lane,
        outcome: &TurnResult,
        refused: &[tinyhivemind_tools::Refusal],
        recorded: usize,
    ) {
        if self.quiet {
            return;
        }
        let where_ = match lane {
            Lane::Desk => String::new(),
            Lane::Thread(root) => format!(" in thread {}", root.0),
        };
        match outcome {
            TurnResult::Replied(reply) => eprintln!(
                "[turn] @{seat}{where_} replied ({} chars)",
                reply.chars().count()
            ),
            TurnResult::Failed(error) => eprintln!("[turn] @{seat}{where_} failed: {error}"),
            TurnResult::Parked => {
                eprintln!("[turn] @{seat}{where_} parked: waiting on the desk's operator");
            }
        }
        for refusal in refused {
            eprintln!(
                "[refused] @{seat}{where_} `{}`: {}",
                refusal.tool, refusal.reason
            );
        }
        if recorded == 0 && !outcome.parked() {
            // What the seat wrote instead, marked as what it is: not a desk
            // row, and the only trace of a refusal it read or of a
            // deliverable it typed rather than recorded.
            eprintln!("[no tool call] @{seat}{where_} -- reply discarded, not recorded:");
            if let Some(reply) = outcome.reply() {
                let shown: String = reply.chars().take(REPLY_SHOWN).collect();
                let cut = if reply.chars().count() > REPLY_SHOWN {
                    " [...]"
                } else {
                    ""
                };
                eprintln!("    {}{cut}", shown.replace('\n', "\n    "));
            }
        }
    }
}

/// This example's desk, as a host of its own seats.
pub struct DeskHost {
    journal: DeskJournal,
    library: LibraryHost,
    /// The runtime its seats are registered on. A seat is an `AgentSpec`
    /// agent now, not a session built around the episode's belt.
    runtime: Arc<Runtime>,
    /// Each seat's standing prompt: its brief and the contract.
    prompts: BTreeMap<String, String>,
}

impl DeskHost {
    pub fn new(
        journal: DeskJournal,
        library: LibraryHost,
        runtime: Arc<Runtime>,
        prompts: BTreeMap<String, String>,
    ) -> Self {
        Self {
            journal,
            library,
            runtime,
            prompts,
        }
    }
}

impl Journal for DeskHost {
    fn log(&self) -> &dyn SessionLog {
        self.journal.log()
    }

    fn commit(&self, commit: &Commit) -> tinyhivemind_openhuman::Result<Sequence> {
        self.journal.commit(commit)
    }

    fn note(&self, note: &Note) -> tinyhivemind_openhuman::Result<()> {
        self.journal.note(note)
    }

    fn compose(&self, seat: &str, brief: &EpisodeBrief) -> String {
        self.journal.compose(seat, brief)
    }

    fn event(&self, event: &Event) {
        self.journal.event(event);
    }

    fn turn_done(
        &self,
        seat: &str,
        lane: Lane,
        outcome: &TurnResult,
        refused: &[tinyhivemind_tools::Refusal],
        recorded: usize,
    ) {
        self.journal
            .turn_done(seat, lane, outcome, refused, recorded);
    }
}

impl EpisodeHost for DeskHost {
    fn build_seat(
        &self,
        seat: &str,
        belt: EpisodeBeltSource,
    ) -> tinyhivemind_openhuman::Result<Agent> {
        let prompt = self.prompts[seat].clone();
        Ok(self.runtime.agent(
            AgentSpec::new(seat)
                .system_prompt(prompt)
                // Rebuilt every turn out of the source, because that is what
                // `AgentSpec::tools` asks for -- and what lets one agent serve
                // an episode and its ordinary work without existing twice.
                .tools(move |_turn| {
                    let belt = belt.belt();
                    // No tools of its own, so no gate of its own: the
                    // episode's tools are admitted and everything else denied.
                    let gate = belt.admit(None);
                    HostTurnTools::advertised(belt.tools).with_policy(gate)
                }),
        )?)
    }

    fn seat_session(&self, seat: &str) -> String {
        format!("episode:conducted:{seat}")
    }

    fn wrap_turn<'a>(&'a self, _seat: &'a str, turn: HostedTurn<'a>) -> HostedTurn<'a> {
        Box::pin(self.library.scope(turn))
    }
}

/// ` in thread N`, or nothing on the desk.
fn place(thread: Option<Sequence>) -> String {
    thread.map_or_else(String::new, |root| format!(" in thread {}", root.0))
}
