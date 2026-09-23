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

use std::pin::Pin;

use tinyhivemind::aside::Viewer;
use tinyhivemind::{
    Conversation, ElsewhereQuery, SESSION_WINDOW, Sequence, SessionAuthor, SessionLog,
    SessionMessage, SessionQuery, gather_elsewhere, project_session,
};
use tinyhivemind_driver::{
    BoundAgent, BroadcastRouting, Commit, CompletionDriver, ConductPolicy, Conductor,
    ConductorState, Door, ElsewhereView, EpisodeBrief, Event, Note, Step, Turn,
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

    /// Every conversation `seat` is in that this episode does not run: its
    /// other desks, and any thread of them. The newest rows of each are
    /// read as the seat and carried in its brief as context. The default is
    /// none, and an episode is then the only thing a seat is shown.
    ///
    /// This desk may be named here too: the turn's own conversation is
    /// skipped, so a thread turn is shown the desk it hangs off and a desk
    /// turn is not shown itself.
    fn channels(&self, seat: &str) -> Vec<Conversation> {
        let _ = seat;
        Vec::new()
    }

    /// The episode paused where it can be resumed: every row it committed
    /// is in the journal and nothing is in flight. The default keeps
    /// nothing, and such a host loses a running episode to a restart.
    ///
    /// Called once per wave, after the wave settles. A host stores the
    /// snapshot beside its rows and hands it to
    /// [`resume_episode`] on boot.
    ///
    /// # Errors
    ///
    /// The host failing to store it, which ends the episode: an episode
    /// that cannot be checkpointed is one a restart would lose silently.
    fn checkpoint(&self, state: &ConductorState) -> Result<()> {
        let _ = state;
        Ok(())
    }

    /// Nothing is due and these seats are parked: the seats the host has
    /// settled, waiting until it has one.
    ///
    /// The episode cannot go on until one comes back, so a host that parks
    /// blocks here on its own queue -- an approval being answered -- and
    /// returns the seats it released. Returning none ends the episode with
    /// [`driver::Error::Parked`](tinyhivemind_driver::Error::Parked), which
    /// is the default, because a host that never parks is never asked.
    ///
    /// # Errors
    ///
    /// Whatever stops the host waiting.
    fn released<'a>(&'a self, parked: &'a [String]) -> Released<'a> {
        let _ = parked;
        Box::pin(async { Ok(Vec::new()) })
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

/// The seats a host released, once it has any.
pub type Released<'a> = Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>>;

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
    let conductor = Conductor::open(driver, routing, policy, door)?;
    drive(journal, runner, conductor, desk).await
}

/// Carry on an episode from a snapshot the host stored.
///
/// The same loop as [`run_episode`], opened from
/// [`Conductor::resume`](tinyhivemind_driver::Conductor::resume) rather than
/// from a door: the same conversations are open, the same seats are held,
/// and the rows already committed are already in the host's journal.
///
/// # Errors
///
/// Whatever [`run_episode`] errors on, plus the driver refusing the snapshot
/// -- one naming a seat or a desk this hive does not have.
pub async fn resume_episode<A, J, R>(
    journal: &J,
    runner: &R,
    driver: &CompletionDriver<'_, A>,
    routing: BroadcastRouting<'_>,
    policy: ConductPolicy,
    snapshot: ConductorState,
) -> Result<Report>
where
    A: BoundAgent,
    J: Journal,
    R: SeatRunner,
{
    let desk = Conversation {
        desk_id: snapshot.chat.clone(),
        desk_name: snapshot.desk_name.clone(),
        thread_root: None,
    };
    let conductor = Conductor::resume(driver, routing, policy, snapshot)?;
    drive(journal, runner, conductor, desk).await
}

/// The loop itself, however the conductor was opened.
async fn drive<A, J, R>(
    journal: &J,
    runner: &R,
    mut conductor: Conductor<'_, A>,
    desk: Conversation,
) -> Result<Report>
where
    A: BoundAgent,
    J: Journal,
    R: SeatRunner,
{
    loop {
        if conductor.finished() {
            break;
        }
        for step in conductor.begin_wave() {
            settle(journal, &mut conductor, step).await?;
        }
        let mut turns = conductor.turns()?;
        if turns.is_empty() {
            turns = wait_for_release(journal, &mut conductor).await?;
        }
        // One watermark for the wave, and every read bounded by it: the
        // host's log may grow while the turns are prepared, and a row above
        // the watermark shown now would be shown again next turn, since a
        // seat is recorded as shown through the watermark and no further.
        let latest = latest(journal.log()).await?;
        let mut jobs: Vec<TurnJob> = Vec::with_capacity(turns.len());
        for turn in &turns {
            jobs.push(open_turn(journal, runner, &mut conductor, &desk, turn, latest).await?);
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
                let calls = events.into_iter().map(|event| event.call);
                if outcome.parked() {
                    conductor.record_parked(turn, calls);
                } else {
                    conductor.record(turn, calls);
                }
            }
        }
        while let Some(step) = conductor.step()? {
            settle(journal, &mut conductor, step).await?;
        }
        // The wave settled: every row it produced is in the journal and
        // nothing is in flight, which is the one point a snapshot is true.
        if let Some(snapshot) = conductor.snapshot() {
            journal.checkpoint(&snapshot)?;
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
                    TurnResult::Failed(format!("the turn's task failed: {error}")),
                ));
            }
        }
    }
    done
}

/// Nothing is due: if seats are held on the host, wait for it to release
/// one and ask again; otherwise the wave is simply empty.
async fn wait_for_release<A: BoundAgent, J: Journal>(
    journal: &J,
    conductor: &mut Conductor<'_, A>,
) -> Result<Vec<Turn>> {
    let parked = conductor.parked();
    if parked.is_empty() {
        return Ok(Vec::new());
    }
    let released = journal.released(&parked).await?;
    if released.is_empty() {
        return Err(tinyhivemind_driver::Error::Parked { seats: parked }.into());
    }
    for seat in &released {
        conductor.resume_seat(seat);
    }
    Ok(conductor.turns()?)
}

/// One turn opened: the rows it has not seen, the record, the brief, the
/// prompt, and the job started on the runner.
async fn open_turn<A: BoundAgent, J: Journal, R: SeatRunner>(
    journal: &J,
    runner: &R,
    conductor: &mut Conductor<'_, A>,
    desk: &Conversation,
    turn: &Turn,
    latest: Option<Sequence>,
) -> Result<TurnJob> {
    let channel = Conversation {
        thread_root: turn.thread(),
        ..desk.clone()
    };
    let log = journal.log();
    let rows = rows_above(log, &channel, &turn.seat, turn.since, latest).await?;
    let window = match turn.thread() {
        None => rows.clone(),
        Some(_) => rows_above(log, &channel, &turn.seat, None, latest).await?,
    };
    runner.open(
        &turn.seat,
        window,
        Dispatch {
            chat: desk.desk_id.clone(),
            parent: turn.thread().map(|root| root.0.to_string()),
        },
    );
    // Only a desk turn is shown its conversations, so only a desk turn
    // reads them.
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
        transcripts.insert(
            root,
            rows_above(log, &thread, &turn.seat, None, latest).await?,
        );
    }
    let mut brief = conductor.open_turn(turn, latest, rows, |root| {
        transcripts.get(&root).cloned().unwrap_or_default()
    });
    brief.elsewhere = elsewhere(journal, &turn.seat, &channel, latest).await?;
    let prompt = journal.compose(&turn.seat, &brief);
    let lane = turn.thread().map_or(Lane::Desk, Lane::Thread);
    Ok(runner.turn(turn.seat.clone(), lane, turn.since, prompt))
}

/// What the seat's other conversations hold, as the brief carries them:
/// every channel the host names but the turn's own, read as the seat,
/// through the wave's watermark.
async fn elsewhere<J: Journal>(
    journal: &J,
    seat: &str,
    current: &Conversation,
    latest: Option<Sequence>,
) -> Result<Vec<ElsewhereView>> {
    let channels = journal.channels(seat);
    if channels.is_empty() {
        return Ok(Vec::new());
    }
    let Some(latest) = latest else {
        return Ok(Vec::new());
    };
    let gathered = gather_elsewhere(
        journal.log(),
        &ElsewhereQuery {
            seat,
            conversations: &channels,
            current: Some(current),
            // Exclusive, so one above the watermark, as every other read
            // of this wave is bounded.
            before: latest.0.checked_add(1).map(Sequence),
            window: SESSION_WINDOW,
        },
    )
    .await?;
    Ok(gathered
        .into_iter()
        .map(|found| ElsewhereView {
            chat: found.conversation.desk_id,
            name: found.conversation.desk_name,
            thread_root: found.conversation.thread_root,
            rows: found.rows.iter().filter_map(render).collect(),
        })
        .collect())
}

/// The newest sequence in the log, or `None` for a log with no rows.
async fn latest(log: &dyn SessionLog) -> Result<Option<Sequence>> {
    let page = log
        .read_before(None, 1)
        .await
        .map_err(|source| tinyhivemind::Error::Read { source })?;
    Ok(page.messages.first().map(|row| row.sequence))
}

/// The rows of `conversation` above `since` and through `latest` that
/// `seat` may read, rendered, newest [`SESSION_WINDOW`] of them: every row
/// for a `since` of `None`, and none for a `latest` of `None`, the wave's
/// watermark on a log that had no rows.
async fn rows_above(
    log: &dyn SessionLog,
    conversation: &Conversation,
    seat: &str,
    since: Option<Sequence>,
    latest: Option<Sequence>,
) -> Result<Vec<String>> {
    let Some(latest) = latest else {
        return Ok(Vec::new());
    };
    let rows = project_session(
        log,
        &SessionQuery {
            conversation: conversation.clone(),
            viewer: Viewer::Agent { id: seat.into() },
            // Exclusive, so one above the watermark; nothing is above the
            // last sequence, so that reads unbounded rather than one short.
            before: latest.0.checked_add(1).map(Sequence),
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
