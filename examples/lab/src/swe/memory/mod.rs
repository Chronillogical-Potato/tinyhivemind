//! Hive memory: what seats did, stored and recalled through tinymemory.
//!
//! [`HiveMemory`] implements the seat-facing [`SeatMemory`] port over any
//! tinymemory engine: CortexDB in a run (`--memory cortex`), the in-memory
//! reference engine in tests. Every run writes below its own namespace root,
//! `team:<run-id>` ([`run_root`]), and each seat is one tinymemory agent at
//! `team:<run-id>/agent:<seat>` whose single thread is named after the seat.
//!
//! | Moment | Call | What comes back |
//! | --- | --- | --- |
//! | session start | `AgentMemory::start_session` (the seat's own thread first) | everything relevant to the focus |
//! | rejoin | `holistic_recall` over learnings and one section per teammate | only items this seat has not been shown |
//! | compaction | `AgentMemory::recall_for_compaction` | a summary of the seat's thread plus related memory |
//! | end of activation | `AgentMemory::post_turn` | (stores the seat's words and command ledger) |
//!
//! Seats are threads and tinymemory is async, so `HiveMemory` owns a small
//! multi-threaded tokio runtime: each call blocks its seat's thread on the
//! runtime under a timeout, and belief builds the policy asks for are spawned
//! onto it and collected (bounded) by [`SeatMemory::finish`]. Any error or
//! timeout becomes an empty pack and a [`Report::error`], never a failed
//! seat. Packs are framed under the context module's `MEMORY_HEADER` and
//! clipped to the budget, four characters per token.

mod types;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tinymemory_api::{ItemId, MemoryEngine, Namespace, Role, Turn};
use tinymemory_integrations::cortex::{CortexCredential, CortexEngine};
use tinymemory_tools::{
    AgentMemory, Compaction, ContextPack, HolisticRecall, MemoryLayout,
    PostTurn, RecallPolicy, ScopeSection, SessionStart, holistic_recall,
};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

use super::context::MEMORY_HEADER;

pub use types::{LedgerEntry, Moment, Recalled, Remembered, Report, SeatMemory};

/// Characters per token when clipping a pack.
const CHARS_PER_TOKEN: usize = 4;
/// Longest a run id segment may be once sanitized.
const RUN_ID_CHARS: usize = 64;
/// Teammate turns one rejoin section may show.
const REJOIN_PER_TEAMMATE: usize = 3;
/// Ask for a belief build of a seat's turns after every this many.
const BUILD_EVERY: u32 = 5;
/// Heading of the shared learnings section of a rejoin pack.
const LEARNINGS_HEADING: &str = "Learnings";
/// Title tinymemory gives every pack; dropped in favour of our header.
const PACK_TITLE: &str = "Memory";

/// How long each kind of call may take before it is abandoned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Timeouts {
    /// A recall at any moment.
    pub recall: Duration,
    /// Storing one activation.
    pub remember: Duration,
    /// Waiting for every outstanding belief build at the end of the run.
    pub finish: Duration,
}

impl Timeouts {
    /// Recall and remember 4 s each (a compaction recall measured ~1.4 s
    /// against CortexDB), 20 s for background work at the end.
    pub const DEFAULT: Self = Self {
        recall: Duration::from_secs(4),
        remember: Duration::from_secs(4),
        finish: Duration::from_secs(20),
    };
}

/// Per-seat bookkeeping.
#[derive(Debug, Default)]
struct SeatState {
    /// The next turn index of the seat's thread.
    turn: u32,
    /// Every item this seat was shown, so a rejoin shows only new ones.
    seen: Vec<ItemId>,
}

/// One run's memory: a namespace root on an engine, one agent per seat.
pub struct HiveMemory {
    runtime: Runtime,
    engine: Arc<dyn MemoryEngine>,
    layout: MemoryLayout,
    seats: Vec<String>,
    policy: RecallPolicy,
    timeouts: Timeouts,
    state: Mutex<HashMap<String, SeatState>>,
    background: Mutex<Vec<JoinHandle<Report>>>,
}

impl std::fmt::Debug for HiveMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HiveMemory")
            .field("engine", &self.engine.descriptor().id)
            .field("root", &self.layout.root().to_string())
            .field("seats", &self.seats)
            .finish_non_exhaustive()
    }
}

/// The namespace root of run `run_id`: `team:<id>`, with every character a
/// namespace segment does not allow replaced by `-`.
///
/// # Errors
///
/// Returns a message when nothing usable is left of the id.
pub fn run_root(run_id: &str) -> Result<Namespace, String> {
    let id: String = run_id
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .take(RUN_ID_CHARS)
        .collect();
    let id = id.trim_matches('-');
    if id.is_empty() {
        return Err(format!("run id {run_id:?} has no usable characters"));
    }
    format!("team:{id}")
        .parse()
        .map_err(|error| format!("bad run id {run_id:?}: {error}"))
}

/// A run id unique to this process and moment: `<mode>-<unix secs>-<pid>`.
#[must_use]
pub fn generated_run_id(mode: &str) -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    format!("{mode}-{secs}-{}", std::process::id())
}

impl HiveMemory {
    /// Memory for `seats` of run `run_id` on `engine`, recalling at most
    /// `budget_tokens` per pack.
    ///
    /// # Errors
    ///
    /// Returns a message for an unusable run id or seat id, or when the
    /// runtime cannot start.
    pub fn new(
        engine: Arc<dyn MemoryEngine>,
        run_id: &str,
        seats: &[&str],
        budget_tokens: usize,
    ) -> Result<Self, String> {
        let layout = MemoryLayout::new(run_root(run_id)?).map_err(|error| error.to_string())?;
        for seat in seats {
            layout
                .conversations(seat)
                .map_err(|error| format!("bad seat id {seat:?}: {error}"))?;
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("hive-memory")
            .enable_all()
            .build()
            .map_err(|error| format!("cannot start the memory runtime: {error}"))?;
        Ok(Self {
            runtime,
            engine,
            layout,
            seats: seats.iter().map(|seat| (*seat).to_owned()).collect(),
            policy: RecallPolicy {
                budget_tokens,
                build_beliefs_every: Some(BUILD_EVERY),
                ..RecallPolicy::default()
            },
            timeouts: Timeouts::DEFAULT,
            state: Mutex::new(HashMap::new()),
            background: Mutex::new(Vec::new()),
        })
    }

    /// Memory on the CortexDB server at `url`, authenticated with `key`.
    ///
    /// # Errors
    ///
    /// As [`Self::new`], and for a URL the client refuses.
    pub fn cortex(
        url: &str,
        key: &str,
        run_id: &str,
        seats: &[&str],
        budget_tokens: usize,
    ) -> Result<Self, String> {
        let engine = CortexEngine::direct(url, CortexCredential::api_key(key))
            .map_err(|error| format!("cannot reach memory at {url}: {error}"))?;
        Self::new(Arc::new(engine), run_id, seats, budget_tokens)
    }

    /// The same memory under other timeouts.
    #[must_use]
    pub fn with_timeouts(mut self, timeouts: Timeouts) -> Self {
        self.timeouts = timeouts;
        self
    }

    /// The run's namespace root.
    #[must_use]
    pub fn root(&self) -> &Namespace {
        self.layout.root()
    }

    /// One line for the trace: engine, root and health.
    #[must_use]
    pub fn describe(&self) -> String {
        let health = self.runtime.block_on(async {
            tokio::time::timeout(self.timeouts.recall, self.engine.health()).await
        });
        format!(
            "{} root {} health {}",
            self.engine.descriptor().id,
            self.layout.root(),
            health.map_or_else(|_| "timed out".to_owned(), |h| format!("{h:?}"))
        )
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, SeatState>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn agent(&self, seat: &str) -> Result<AgentMemory, String> {
        AgentMemory::new(self.engine.clone(), self.layout.clone(), seat)
            .map(|memory| memory.with_policy(self.policy.clone()))
            .map_err(|error| error.to_string())
    }

    /// The rejoin read: learnings, then each teammate's turns, never an item
    /// this seat was already shown.
    fn rejoin_request(&self, seat: &str, focus: &str) -> HolisticRecall {
        let mut sections = vec![ScopeSection::fetch(
            LEARNINGS_HEADING,
            self.layout.learnings_filter(),
            self.policy.learnings_limit,
        )];
        sections.extend(
            self.seats
                .iter()
                .filter(|other| other.as_str() != seat)
                .map(|other| {
                    ScopeSection::fetch(
                        format!("@{other}"),
                        self.layout.conversations_filter(Some(other)),
                        REJOIN_PER_TEAMMATE,
                    )
                }),
        );
        let query = (!focus.trim().is_empty()).then(|| focus.to_owned());
        HolisticRecall {
            budget_tokens: self.policy.budget_tokens,
            title: PACK_TITLE.to_owned(),
            exclude_ids: self.lock().get(seat).map(|s| s.seen.clone()).unwrap_or_default(),
            ..HolisticRecall::new(query, sections)
        }
    }

    async fn read(&self, seat: &str, moment: &Moment) -> Result<ContextPack, String> {
        let agent = self.agent(seat)?;
        let pack = match moment {
            Moment::SessionStart { focus } => {
                agent
                    .start_session(SessionStart {
                        thread_id: Some(seat.to_owned()),
                        focus: Some(focus.clone()),
                    })
                    .await
            }
            Moment::Rejoin { focus } => {
                let request = self.rejoin_request(seat, focus);
                holistic_recall(self.engine.as_ref(), &request).await
            }
            Moment::Compaction { dropped, focus } => {
                agent
                    .recall_for_compaction(Compaction {
                        thread_id: seat.to_owned(),
                        dropped: dropped
                            .iter()
                            .map(|text| Turn::new(Role::Assistant, text.clone()))
                            .collect(),
                        focus: Some(focus.clone()),
                    })
                    .await
            }
        };
        pack.map_err(|error| error.to_string())
    }

    fn spawn_jobs(&self, agent: &AgentMemory, jobs: Vec<tinymemory_tools::BackgroundJob>) {
        let mut handles = self
            .background
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for job in jobs {
            let agent = agent.clone();
            handles.push(self.runtime.spawn(async move {
                let started = Instant::now();
                let outcome = agent.run_background(job).await;
                Report {
                    op: "background",
                    moment: "belief_build",
                    latency_ms: elapsed_ms(started),
                    error: outcome.err().map(|error| error.to_string()),
                    ..Report::default()
                }
            }));
        }
    }
}

impl SeatMemory for HiveMemory {
    fn recall(&self, seat: &str, moment: &Moment) -> Recalled {
        let started = Instant::now();
        let outcome = self.runtime.block_on(async {
            tokio::time::timeout(self.timeouts.recall, self.read(seat, moment)).await
        });
        let mut report = Report {
            op: "recall",
            moment: moment.name(),
            ..Report::default()
        };
        let pack = match outcome {
            Err(_) => {
                report.error = Some(format!("timed out after {:?}", self.timeouts.recall));
                None
            }
            Ok(Err(error)) => {
                report.error = Some(error);
                None
            }
            Ok(Ok(pack)) => {
                report.items = pack.refs.len();
                self.lock()
                    .entry(seat.to_owned())
                    .or_default()
                    .seen
                    .extend(pack.refs.iter().cloned());
                frame(&pack.markdown, self.policy.budget_tokens)
            }
        };
        report.chars = pack.as_ref().map_or(0, String::len);
        report.latency_ms = elapsed_ms(started);
        Recalled { pack, report }
    }

    fn remember(&self, seat: &str, what: &Remembered) -> Report {
        let started = Instant::now();
        let text = what.render();
        let mut report = Report {
            op: "remember",
            moment: "activation",
            chars: text.len(),
            items: what.ledger.len(),
            ..Report::default()
        };
        let agent = match self.agent(seat) {
            Ok(agent) => agent,
            Err(error) => {
                report.error = Some(error);
                return report;
            }
        };
        let turn = {
            let mut state = self.lock();
            let entry = state.entry(seat.to_owned()).or_default();
            let turn = entry.turn;
            entry.turn += 1;
            turn
        };
        let outcome = self.runtime.block_on(async {
            tokio::time::timeout(
                self.timeouts.remember,
                agent.post_turn(PostTurn::new(seat, turn, text)),
            )
            .await
        });
        match outcome {
            Err(_) => {
                report.error = Some(format!("timed out after {:?}", self.timeouts.remember));
            }
            Ok(Err(error)) => report.error = Some(error.to_string()),
            Ok(Ok(done)) => self.spawn_jobs(&agent, done.jobs),
        }
        report.latency_ms = elapsed_ms(started);
        report
    }

    fn finish(&self) -> Vec<Report> {
        let handles: Vec<JoinHandle<Report>> = std::mem::take(
            &mut *self
                .background
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        let deadline = Instant::now() + self.timeouts.finish;
        handles
            .into_iter()
            .map(|handle| {
                let left = deadline.saturating_duration_since(Instant::now());
                let waited = self
                    .runtime
                    .block_on(async { tokio::time::timeout(left, handle).await });
                match waited {
                    Ok(Ok(report)) => report,
                    Ok(Err(error)) => failed_job(error.to_string()),
                    Err(_) => failed_job("still running at the end of the run".into()),
                }
            })
            .collect()
    }
}

fn failed_job(error: String) -> Report {
    Report {
        op: "background",
        moment: "belief_build",
        error: Some(error),
        ..Report::default()
    }
}

/// A pack's markdown under [`MEMORY_HEADER`] (tinymemory's own title line
/// dropped), clipped to `budget_tokens`; `None` when it says nothing.
#[must_use]
pub fn frame(markdown: &str, budget_tokens: usize) -> Option<String> {
    let body = markdown
        .strip_prefix(&format!("# {PACK_TITLE}"))
        .unwrap_or(markdown)
        .trim();
    if body.is_empty() {
        return None;
    }
    let framed = format!("{MEMORY_HEADER}\n{body}");
    let limit = budget_tokens.saturating_mul(CHARS_PER_TOKEN);
    if framed.chars().count() <= limit {
        return Some(framed);
    }
    let mut clipped: String = framed.chars().take(limit.saturating_sub(4)).collect();
    clipped.push_str("\n...");
    Some(clipped)
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod test;
