//! Hive memory: the lab's reference host for core's memory ports.
//!
//! [`HiveMemory`] implements core's
//! [`Recall`](tinyhivemind_core::runtime::Recall) and
//! [`Remember`](tinyhivemind_core::runtime::Remember) over any tinymemory
//! engine: CortexDB in a run (`--memory cortex`), the in-memory reference
//! engine in tests. Every run writes below its own namespace root,
//! `team:<run-id>` ([`run_root`]), which is also the `conversation` every
//! request must name; each seat is one tinymemory agent at
//! `team:<run-id>/agent:<seat>` whose single thread is named after the seat.
//!
//! | `RecallMoment` | tinymemory call | Notes |
//! | --- | --- | --- |
//! | `SessionStart` | `AgentMemory::start_session` (the seat's own thread first) | everything relevant to the focus |
//! | `Rejoin` | `holistic_recall` over learnings and one section per teammate | only items this seat has not been shown |
//! | `Compaction { dropped }` | `AgentMemory::recall_for_compaction` | a summary of the seat's thread plus related memory |
//!
//! `Remember` stores the activation's entries as one turn, waiting until the
//! engine has indexed it (`WriteOptions::visible`, which CortexDB serves as
//! `POST /v1/experience?wait=indexed`). `AgentMemory::post_turn` only waits
//! for acceptance, which let a teammate's recall a moment later miss the
//! turn; the wait is bounded by the same timeout as before.
//!
//! Seats are threads and tinymemory is async, so `HiveMemory` owns a small
//! multi-threaded tokio runtime. Each port call spawns its work there (with
//! its timeout) and returns a future that only awaits the task, so any
//! executor can drive it. Belief builds the policy asks for run there too and
//! are collected, bounded, by [`SeatMemory::finish`].

mod notes;
mod types;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use tinymemory_api::{ItemId, MemoryEngine, Namespace, Role, Turn, WriteOptions};
use tinymemory_integrations::cortex::{CortexCredential, CortexEngine};
use tinymemory_tools::{
    AgentMemory, Compaction, ContextPack, HolisticRecall, MemoryLayout, RecallPolicy,
    ScopeSection, SessionStart, holistic_recall,
};
use tinyhivemind_core::runtime::{
    BriefingNote, Error as CoreError, Recall, RecallFuture, RecallMoment, RecallRequest, Remember,
    RememberFuture, RememberRequest,
};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

pub use notes::{entries_text, pack_notes};
pub use types::{LedgerEntry, SeatMemory, Timeouts, kind_label};

/// Characters per token: `--memory-budget` is in tokens, requests in chars.
pub const CHARS_PER_TOKEN: usize = 4;
/// Longest a run id segment may be once sanitized.
const RUN_ID_CHARS: usize = 64;
/// Teammate turns one rejoin section may show.
const REJOIN_PER_TEAMMATE: usize = 3;
/// Ask for a belief build of a seat's turns after every this many.
const BUILD_EVERY: u32 = 5;
/// Heading of the shared learnings section of a rejoin pack.
const LEARNINGS_HEADING: &str = "Learnings";

/// Per-seat bookkeeping.
#[derive(Debug, Default)]
struct SeatState {
    /// The next turn index of the seat's thread.
    turn: u32,
    /// Every item this seat was shown, so a rejoin shows only new ones.
    seen: Vec<ItemId>,
}

/// What the spawned tasks share.
struct Inner {
    engine: Arc<dyn MemoryEngine>,
    layout: MemoryLayout,
    seats: Vec<String>,
    timeouts: Timeouts,
    state: Mutex<HashMap<String, SeatState>>,
    background: Mutex<Vec<JoinHandle<String>>>,
}

/// One run's memory: a namespace root on an engine, one agent per seat.
pub struct HiveMemory {
    runtime: Runtime,
    inner: Arc<Inner>,
    budget_chars: usize,
}

impl std::fmt::Debug for HiveMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HiveMemory")
            .field("engine", &self.inner.engine.descriptor().id)
            .field("root", &self.inner.layout.root().to_string())
            .field("seats", &self.inner.seats)
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

fn recall_error(message: impl Into<String>) -> CoreError {
    CoreError::Recall {
        source: message.into().into(),
    }
}

fn remember_error(message: impl Into<String>) -> CoreError {
    CoreError::Remember {
        source: message.into().into(),
    }
}

impl HiveMemory {
    /// Memory for `seats` of run `run_id` on `engine`, framing recalls within
    /// `budget_tokens`.
    ///
    /// # Errors
    ///
    /// Returns a message for an unusable run id or a blank seat id, or when
    /// the runtime cannot start.
    pub fn new(
        engine: Arc<dyn MemoryEngine>,
        run_id: &str,
        seats: &[&str],
        budget_tokens: usize,
    ) -> Result<Self, String> {
        let layout = MemoryLayout::new(run_root(run_id)?).map_err(|error| error.to_string())?;
        if let Some(seat) = seats.iter().find(|seat| seat.trim().is_empty()) {
            return Err(format!("bad seat id {seat:?}: a seat needs an id"));
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("hive-memory")
            .enable_all()
            .build()
            .map_err(|error| format!("cannot start the memory runtime: {error}"))?;
        Ok(Self {
            runtime,
            inner: Arc::new(Inner {
                engine,
                layout,
                seats: seats.iter().map(|seat| (*seat).to_owned()).collect(),
                timeouts: Timeouts::DEFAULT,
                state: Mutex::new(HashMap::new()),
                background: Mutex::new(Vec::new()),
            }),
            budget_chars: budget_tokens.saturating_mul(CHARS_PER_TOKEN),
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

    /// The same memory under other timeouts. Call it before the first port
    /// call: once a task holds the shared state, the old timeouts stay.
    #[must_use]
    pub fn with_timeouts(mut self, timeouts: Timeouts) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.timeouts = timeouts;
        }
        self
    }

    /// The run's namespace root.
    #[must_use]
    pub fn root(&self) -> &Namespace {
        self.inner.layout.root()
    }

    /// One line for the trace: engine, root and health.
    #[must_use]
    pub fn describe(&self) -> String {
        let inner = &self.inner;
        let health = self.runtime.block_on(async {
            tokio::time::timeout(inner.timeouts.recall, inner.engine.health()).await
        });
        format!(
            "{} root {} health {}",
            inner.engine.descriptor().id,
            inner.layout.root(),
            health.map_or_else(|_| "timed out".to_owned(), |h| format!("{h:?}"))
        )
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, HashMap<String, SeatState>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn agent(&self, seat: &str, budget_chars: usize) -> Result<AgentMemory, String> {
        let policy = RecallPolicy {
            budget_tokens: (budget_chars / CHARS_PER_TOKEN).max(1),
            build_beliefs_every: Some(BUILD_EVERY),
            ..RecallPolicy::default()
        };
        AgentMemory::new(self.engine.clone(), self.layout.clone(), seat)
            .map(|memory| memory.with_policy(policy))
            .map_err(|error| error.to_string())
    }

    /// Refuse a request for another run's namespace.
    fn check(&self, conversation: &str) -> Result<(), String> {
        let root = self.layout.root().to_string();
        if conversation == root {
            Ok(())
        } else {
            Err(format!(
                "conversation {conversation:?} is not this run's memory {root:?}"
            ))
        }
    }

    /// The rejoin read: learnings, then each teammate's turns, never an item
    /// this seat was already shown.
    fn rejoin_request(&self, request: &RecallRequest) -> HolisticRecall {
        let seat = request.seat.as_str();
        let mut sections = vec![ScopeSection::fetch(
            LEARNINGS_HEADING,
            self.layout.learnings_filter(),
            RecallPolicy::default().learnings_limit,
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
        let query = request.focus.clone().filter(|f| !f.trim().is_empty());
        HolisticRecall {
            budget_tokens: (request.budget_chars / CHARS_PER_TOKEN).max(1),
            exclude_ids: self.lock().get(seat).map(|s| s.seen.clone()).unwrap_or_default(),
            ..HolisticRecall::new(query, sections)
        }
    }

    async fn read(&self, request: &RecallRequest) -> Result<ContextPack, String> {
        self.check(&request.conversation)?;
        let seat = request.seat.as_str();
        let agent = self.agent(seat, request.budget_chars)?;
        let pack = match &request.moment {
            RecallMoment::SessionStart => {
                agent
                    .start_session(SessionStart {
                        thread_id: Some(seat.to_owned()),
                        focus: request.focus.clone(),
                    })
                    .await
            }
            RecallMoment::Rejoin => {
                holistic_recall(self.engine.as_ref(), &self.rejoin_request(request)).await
            }
            RecallMoment::Compaction { dropped } => {
                agent
                    .recall_for_compaction(Compaction {
                        thread_id: seat.to_owned(),
                        dropped: dropped
                            .iter()
                            .map(|text| Turn::new(Role::Assistant, text.clone()))
                            .collect(),
                        focus: request.focus.clone(),
                    })
                    .await
            }
        };
        pack.map_err(|error| error.to_string())
    }

    async fn recall_notes(&self, request: RecallRequest) -> Result<Vec<BriefingNote>, CoreError> {
        let read = tokio::time::timeout(self.timeouts.recall, self.read(&request)).await;
        let pack = match read {
            Err(_) => {
                return Err(recall_error(format!(
                    "timed out after {:?}",
                    self.timeouts.recall
                )));
            }
            Ok(Err(error)) => return Err(recall_error(error)),
            Ok(Ok(pack)) => pack,
        };
        let (notes, seen) = pack_notes(&pack);
        self.lock()
            .entry(request.seat.clone())
            .or_default()
            .seen
            .extend(seen);
        Ok(notes)
    }

    async fn store(&self, request: RememberRequest, budget_chars: usize) -> Result<(), CoreError> {
        self.check(&request.conversation)
            .map_err(remember_error)?;
        let text = entries_text(&request);
        if text.trim().is_empty() {
            return Ok(());
        }
        let seat = request.seat.as_str();
        let agent = self.agent(seat, budget_chars).map_err(remember_error)?;
        let node = self
            .layout
            .conversations(seat)
            .map_err(|error| remember_error(error.to_string()))?;
        let turn = {
            let mut state = self.lock();
            let entry = state.entry(seat.to_owned()).or_default();
            let turn = entry.turn;
            entry.turn += 1;
            turn
        };
        let item = notes::turn_item(node, seat, turn, text);
        let write = self.engine.store_with(item, WriteOptions::visible());
        match tokio::time::timeout(self.timeouts.remember, write).await {
            Err(_) => {
                return Err(remember_error(format!(
                    "timed out after {:?}",
                    self.timeouts.remember
                )));
            }
            Ok(Err(error)) => return Err(remember_error(error.to_string())),
            Ok(Ok(_)) => {}
        }
        if (turn + 1).is_multiple_of(BUILD_EVERY) {
            let job = agent.history_build();
            let handle = tokio::spawn(async move {
                let started = Instant::now();
                let outcome = agent.run_background(job).await;
                let ms = started.elapsed().as_millis();
                match outcome {
                    Ok(_) => format!("belief_build latency_ms={ms}"),
                    Err(error) => format!("belief_build latency_ms={ms} error={error}"),
                }
            });
            self.background
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(handle);
        }
        Ok(())
    }
}

impl Recall for HiveMemory {
    fn recall<'a>(&'a self, request: &'a RecallRequest) -> RecallFuture<'a> {
        let inner = Arc::clone(&self.inner);
        let request = request.clone();
        let task = self
            .runtime
            .spawn(async move { inner.recall_notes(request).await });
        Box::pin(async move {
            task.await
                .map_err(|error| recall_error(format!("recall task failed: {error}")))?
        })
    }
}

impl Remember for HiveMemory {
    fn remember<'a>(&'a self, request: &'a RememberRequest) -> RememberFuture<'a> {
        let inner = Arc::clone(&self.inner);
        let request = request.clone();
        let budget = self.budget_chars;
        let task = self
            .runtime
            .spawn(async move { inner.store(request, budget).await });
        Box::pin(async move {
            task.await
                .map_err(|error| remember_error(format!("remember task failed: {error}")))?
        })
    }
}

impl SeatMemory for HiveMemory {
    fn conversation(&self) -> String {
        self.inner.layout.root().to_string()
    }

    fn budget_chars(&self) -> usize {
        self.budget_chars
    }

    fn finish(&self) -> Vec<String> {
        let handles: Vec<JoinHandle<String>> = std::mem::take(
            &mut *self
                .inner
                .background
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        let deadline = Instant::now() + self.inner.timeouts.finish;
        handles
            .into_iter()
            .map(|handle| {
                let left = deadline.saturating_duration_since(Instant::now());
                let waited = self
                    .runtime
                    .block_on(async { tokio::time::timeout(left, handle).await });
                match waited {
                    Ok(Ok(line)) => line,
                    Ok(Err(error)) => format!("belief_build error={error}"),
                    Err(_) => "belief_build error=still running at the end of the run".into(),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod test;
