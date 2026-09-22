//! One episode, from its door to quiescence, over a journal the host owns.
//!
//! The [`Conductor`] holds every rule of a completion episode and the
//! [`SeatRunner`] runs a turn; between them sits the loop that moves rows
//! from the host's journal to a seat and back, one wave at a time:
//!
//! 1. `begin_wave`: the nudges due, appended.
//! 2. `turns`: who runs, where, above which row.
//! 3. For each turn: the rows the seat has not seen, read from the log as
//!    the seat; the record opened for the turn; the brief; the prompt the
//!    host composes; the turn started on the runner.
//! 4. Every turn awaited together, closed, and what it called recorded.
//! 5. `step` until settled: a commit appended and its sequence reported, a
//!    note appended, an event shown.
//!
//! [`run_episode`] is that loop. It reads through the host's [`SessionLog`]
//! and writes through [`Journal`], so a host implements the journal and
//! calls one function.

#[cfg(test)]
mod test;

use tinyhivemind::aside::Viewer;
use tinyhivemind::{
    Conversation, SESSION_WINDOW, Sequence, SessionAuthor, SessionLog, SessionMessage,
    SessionQuery, project_session,
};
use tinyhivemind_driver::{
    BoundAgent, BroadcastRouting, Commit, CompletionDriver, ConductPolicy, Conductor, Door,
    EpisodeBrief, Event, Note, Step,
};
use tinyhivemind_tools::{Dispatch, Refusal};

use crate::Result;
use crate::runner::{Lane, SeatRunner, TurnJob, TurnResult};

/// The journal an episode runs over: what the host reads for a seat and
/// appends on the conductor's behalf, and what it wants to see of a turn.
///
/// The host renders its own rows. A [`Commit`] carries the utterance and the
/// host appends it as a row of its own shape, returning the sequence the
/// journal gave it; a [`Note`] carries the desk's words, attributed to the
/// desk. Everything else has a default.
pub trait Journal: Send + Sync {
    /// The host's log, read as a seat to seed and to brief a turn.
    fn log(&self) -> &dyn SessionLog;

    /// Append what a seat said, or what the episode says on its behalf, and
    /// return the sequence it was given.
    ///
    /// # Errors
    ///
    /// The journal refusing the row.
    fn commit(&self, commit: &Commit) -> Result<Sequence>;

    /// Append what the desk says to a seat.
    ///
    /// # Errors
    ///
    /// The journal refusing the row.
    fn note(&self, note: &Note) -> Result<()>;

    /// Something the episode did, to show or not. The default shows nothing.
    fn event(&self, event: &Event) {
        let _ = event;
    }

    /// The message a turn is sent. The default is the brief as the episode
    /// words it; a host prepends what it owns.
    fn compose(&self, seat: &str, brief: &EpisodeBrief) -> String {
        let _ = seat;
        brief.render()
    }

    /// A turn came back: its reply or failure, what it was refused, and how
    /// many calls it made that the record accepted. The default does nothing.
    fn turn_done(
        &self,
        seat: &str,
        lane: Lane,
        outcome: &TurnResult,
        refused: &[Refusal],
        recorded: usize,
    ) {
        let _ = (seat, lane, outcome, refused, recorded);
    }
}

/// What one episode came to.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Report {
    /// Seat turns run.
    pub turns: u64,
    /// Waves proposed.
    pub waves: u64,
    /// Seats completed with their work for a spent broadcast budget.
    pub discharged: u64,
    /// Conversations concluded.
    pub conversations: usize,
    /// Seats settled at the end.
    pub settled: usize,
}

/// Run one episode from `door` to quiescence.
///
/// # Errors
///
/// The journal refusing a row, the runner failing to open a turn, or the
/// conductor stopping the episode: a stalled desk, a wall, or a fold error
/// it could not explain to the seat.
pub async fn run_episode<A, J, R>(
    journal: &J,
    runner: &R,
    driver: &CompletionDriver<'_, A>,
    routing: BroadcastRouting<'_>,
    policy: ConductPolicy,
    door: Door,
) -> Result<Report>
where
    A: BoundAgent,
    J: Journal,
    R: SeatRunner,
{
    let desk = Conversation {
        desk_id: door.chat.clone(),
        desk_name: door.desk_name.clone(),
        thread_root: None,
    };
    let mut conductor = Conductor::open(driver, routing, policy, door)?;
    loop {
        if conductor.finished() {
            break;
        }
        for step in conductor.begin_wave() {
            settle(journal, &mut conductor, step).await?;
        }
        let turns = conductor.turns()?;
        // One watermark for the wave: nothing is appended while its turns
        // are prepared, so every seat is shown through the same row.
        let latest = latest(journal.log()).await?;
        let mut jobs: Vec<TurnJob> = Vec::with_capacity(turns.len());
        for turn in &turns {
            let channel = Conversation {
                thread_root: turn.thread(),
                ..desk.clone()
            };
            let rows = rows_above(journal.log(), &channel, &turn.seat, turn.since).await?;
            let window = match turn.thread() {
                None => rows.clone(),
                Some(_) => rows_above(journal.log(), &channel, &turn.seat, None).await?,
            };
            runner.open(
                &turn.seat,
                window,
                Dispatch {
                    chat: desk.desk_id.clone(),
                    parent: turn.thread().map(|root| root.0.to_string()),
                },
            );
            // Only a desk turn is shown its conversations, so only a desk
            // turn reads them.
            let mut transcripts = std::collections::BTreeMap::new();
            let shown = match turn.thread() {
                None => conductor.shown_conversations(&turn.seat),
                Some(_) => Vec::new(),
            };
            for root in shown {
                let thread = Conversation {
                    thread_root: Some(root),
                    ..desk.clone()
                };
                let whole = rows_above(journal.log(), &thread, &turn.seat, None).await?;
                transcripts.insert(root, whole);
            }
            let brief = conductor.open_turn(turn, latest, rows, |root| {
                transcripts.get(&root).cloned().unwrap_or_default()
            });
            let prompt = journal.compose(&turn.seat, &brief);
            let lane = turn.thread().map_or(Lane::Desk, Lane::Thread);
            jobs.push(runner.turn(turn.seat.clone(), lane, turn.since, prompt));
        }
        let named: Vec<(String, Lane)> = turns
            .iter()
            .map(|turn| {
                (
                    turn.seat.clone(),
                    turn.thread().map_or(Lane::Desk, Lane::Thread),
                )
            })
            .collect();
        for (seat, lane, outcome) in join_turns(jobs, named).await {
            // Close the turn first: the record refuses a call on a closed
            // turn, so nothing can land after this point is read.
            let events = runner.close(&seat);
            let refused = runner.tools().drain_refusals(&seat);
            journal.turn_done(&seat, lane, &outcome, &refused, events.len());
            if let Some(turn) = turns.iter().find(|turn| turn.seat == seat) {
                conductor.record(turn, events.into_iter().map(|event| event.call));
            }
        }
        while let Some(step) = conductor.step()? {
            settle(journal, &mut conductor, step).await?;
        }
    }
    Ok(Report {
        turns: conductor.turns_run(),
        waves: conductor.waves(),
        discharged: conductor.discharged(),
        conversations: conductor.conversations(),
        settled: conductor.state().episode().settled(),
    })
}

/// One step taken: a commit appended and its sequence reported, a note
/// appended, an event shown.
async fn settle<A: BoundAgent, J: Journal>(
    journal: &J,
    conductor: &mut Conductor<'_, A>,
    step: Step,
) -> Result<()> {
    match step {
        Step::Commit(commit) => {
            let sequence = journal.commit(&commit)?;
            conductor.committed(sequence).await?;
        }
        Step::Note(note) => journal.note(&note)?,
        Step::Event(event) => journal.event(&event),
    }
    Ok(())
}

/// Every turn of a wave, run together, in the order they finish. A turn
/// whose task panicked is a failed turn, not a failed wave: `named` says
/// which seat and lane each job was, in the order the jobs were made.
async fn join_turns(
    jobs: Vec<TurnJob>,
    named: Vec<(String, Lane)>,
) -> Vec<(String, Lane, TurnResult)> {
    let mut tasks = tokio::task::JoinSet::new();
    let mut who = std::collections::HashMap::new();
    for (job, name) in jobs.into_iter().zip(named) {
        who.insert(tasks.spawn(job).id(), name);
    }
    let mut done = Vec::new();
    while let Some(joined) = tasks.join_next_with_id().await {
        match joined {
            Ok((_, outcome)) => done.push(outcome),
            Err(error) => {
                let (seat, lane) = who
                    .remove(&error.id())
                    .unwrap_or_else(|| (String::new(), Lane::Desk));
                done.push((
                    seat,
                    lane,
                    Some(Err(format!("the turn's task failed: {error}"))),
                ));
            }
        }
    }
    done
}

/// The newest sequence in the log, or `None` for a log with no rows.
async fn latest(log: &dyn SessionLog) -> Result<Option<Sequence>> {
    let page = log
        .read_before(None, 1)
        .await
        .map_err(|source| tinyhivemind::Error::Read { source })?;
    Ok(page.messages.first().map(|row| row.sequence))
}

/// The rows of `conversation` above `since` that `seat` may read, rendered,
/// newest [`SESSION_WINDOW`] of them; every row for `None`.
async fn rows_above(
    log: &dyn SessionLog,
    conversation: &Conversation,
    seat: &str,
    since: Option<Sequence>,
) -> Result<Vec<String>> {
    let rows = project_session(
        log,
        &SessionQuery {
            conversation: conversation.clone(),
            viewer: Viewer::Agent { id: seat.into() },
            before: None,
            window: SESSION_WINDOW,
        },
    )
    .await?;
    Ok(rows
        .iter()
        .filter(|row| since.is_none_or(|since| row.sequence > since))
        .filter_map(render)
        .collect())
}

/// `@author: content`, or nothing for a row the seat may not read.
fn render(row: &SessionMessage) -> Option<String> {
    let content = row.readable()?;
    let author = match &row.author {
        SessionAuthor::Operator => "operator",
        SessionAuthor::Agent { id, .. } => id,
        SessionAuthor::Person { label, .. } | SessionAuthor::System { label, .. } => label,
    };
    Some(format!("@{author}: {content}"))
}
