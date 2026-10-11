//! The run's hard limits and what has been spent against them.
//!
//! Every limit is checked before the spend it guards: a seat round is not
//! launched unless all of it fits, a program is not run once the sandbox
//! budget is gone, a search is refused once the search budget is gone. Spend
//! is metered two ways: the cost OpenHuman reports per turn, and OpenRouter's
//! own account usage for the key, read before the run and between rounds.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

/// The desk whose sandbox runs are held back for it.
pub(super) const VERIFY_DESK: &str = "verify";

/// The run's limits.
#[derive(Clone, Copy, Debug, Serialize)]
pub(super) struct Limits {
    /// Seat turns, across every desk.
    pub(super) max_turns: u64,
    /// Model calls (tool-loop iterations), across every turn.
    pub(super) max_model_calls: u64,
    /// Wall-clock seconds.
    pub(super) max_wall_seconds: u64,
    /// Sandbox programs.
    pub(super) max_sandbox_runs: u64,
    /// Web searches.
    pub(super) max_searches: u64,
    /// US dollars of model spend.
    pub(super) max_spend_usd: f64,
}

impl Limits {
    /// A smoke run: enough to exercise every desk once.
    pub(super) const SMOKE: Self = Self {
        max_turns: 16,
        max_model_calls: 80,
        max_wall_seconds: 1_200,
        max_sandbox_runs: 6,
        max_searches: 3,
        max_spend_usd: 1.0,
    };
    /// A full attempt.
    pub(super) const FULL: Self = Self {
        max_turns: 60,
        max_model_calls: 400,
        max_wall_seconds: 5_400,
        max_sandbox_runs: 60,
        max_searches: 12,
        max_spend_usd: 8.0,
    };
}

/// Spend so far.
#[derive(Debug)]
pub(super) struct Budget {
    pub(super) limits: Limits,
    started: Instant,
    pub(super) turns: AtomicU64,
    pub(super) model_calls: AtomicU64,
    pub(super) sandbox_runs: AtomicU64,
    /// Sandbox runs claimed by seats off the verify desk.
    others_sandbox_runs: AtomicU64,
    pub(super) searches: AtomicU64,
    pub(super) input_tokens: AtomicU64,
    pub(super) output_tokens: AtomicU64,
    pub(super) reasoning_tokens: AtomicU64,
    /// Model dollars OpenHuman reported, summed over turns.
    reported_usd: Mutex<f64>,
    /// Dollars the search backend reported.
    search_usd: Mutex<f64>,
    /// OpenRouter's account usage when the run started, and the latest read.
    account: Mutex<(Option<f64>, Option<f64>)>,
}

/// Why the run stopped short.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Exhausted {
    Turns,
    ModelCalls,
    WallClock,
    Spend,
}

impl Budget {
    pub(super) fn new(limits: Limits) -> Self {
        Self {
            limits,
            started: Instant::now(),
            turns: AtomicU64::new(0),
            model_calls: AtomicU64::new(0),
            sandbox_runs: AtomicU64::new(0),
            others_sandbox_runs: AtomicU64::new(0),
            searches: AtomicU64::new(0),
            input_tokens: AtomicU64::new(0),
            output_tokens: AtomicU64::new(0),
            reasoning_tokens: AtomicU64::new(0),
            reported_usd: Mutex::new(0.0),
            search_usd: Mutex::new(0.0),
            account: Mutex::new((None, None)),
        }
    }

    /// Reserve an entire tool loop before admitting it. Unknown/cancelled
    /// turns retain their reservation so concurrently admitted seats cannot
    /// spend calls that might already have reached the provider.
    ///
    /// # Errors
    ///
    /// Returns an error when the reservation would exceed the model-call limit
    /// or overflow the counter.
    pub(super) fn reserve_model_calls(&self, calls: u64) -> anyhow::Result<ModelCalls<'_>> {
        self.model_calls
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(calls)
                    .filter(|total| *total <= self.limits.max_model_calls)
            })
            .map_err(|_| {
                anyhow::anyhow!("the run's model calls budget cannot fit this tool loop")
            })?;
        Ok(ModelCalls {
            budget: self,
            reserved: calls,
        })
    }

    /// Poll account usage within the remaining wall-clock budget. An expired
    /// budget never starts the request; a slow request is dropped at its deadline.
    ///
    /// # Errors
    ///
    /// Returns the request's error inside `Some`, or `None` at the deadline.
    pub(super) async fn poll_usage(
        &self,
        request: impl std::future::Future<Output = anyhow::Result<f64>>,
    ) -> Option<anyhow::Result<f64>> {
        super::turn::within(self.turn_timeout(), request).await
    }

    pub(super) fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// The larger of the two spend meters: what OpenHuman reported, and the
    /// OpenRouter account's own delta.
    pub(super) fn spend_usd(&self) -> f64 {
        let reported = self.reported_usd.lock().map_or(0.0, |value| *value);
        let account = self
            .account
            .lock()
            .ok()
            .and_then(|pair| Some(pair.1? - pair.0?))
            .unwrap_or(0.0);
        reported.max(account)
    }

    pub(super) fn account_delta(&self) -> Option<f64> {
        self.account
            .lock()
            .ok()
            .and_then(|pair| Some(pair.1? - pair.0?))
    }

    pub(super) fn reported_usd(&self) -> f64 {
        self.reported_usd.lock().map_or(0.0, |value| *value)
    }

    pub(super) fn search_usd(&self) -> f64 {
        self.search_usd.lock().map_or(0.0, |value| *value)
    }

    pub(super) fn record_account(&self, usage: f64) {
        if let Ok(mut pair) = self.account.lock() {
            if pair.0.is_none() {
                pair.0 = Some(usage);
            }
            pair.1 = Some(usage);
        }
    }

    pub(super) fn record_turn(&self, usage: Option<&openhuman_embed::LastTurnUsage>) {
        let Some(usage) = usage else { return };
        self.input_tokens
            .fetch_add(usage.input_tokens, Ordering::Relaxed);
        self.output_tokens
            .fetch_add(usage.output_tokens, Ordering::Relaxed);
        self.reasoning_tokens
            .fetch_add(usage.reasoning_tokens, Ordering::Relaxed);
        if let (Some(cost), Ok(mut total)) = (usage.cost_usd, self.reported_usd.lock()) {
            *total += cost;
        }
    }

    pub(super) fn record_search_cost(&self, cost: Option<f64>) {
        if let (Some(cost), Ok(mut total)) = (cost, self.search_usd.lock()) {
            *total += cost;
        }
    }

    /// The longest one turn may run: [`super::turn::TURN_TIMEOUT`], cut to what is left of
    /// the wall-clock budget so an in-flight turn cannot outlive it.
    pub(super) fn turn_timeout(&self) -> Duration {
        let left = Duration::from_secs(self.limits.max_wall_seconds).saturating_sub(self.elapsed());
        super::turn::TURN_TIMEOUT.min(left)
    }

    /// Whether a round of `seats` turns may start.
    pub(super) fn admit_round(&self, seats: u64) -> Result<(), Exhausted> {
        let limits = self.limits;
        if self.turns.load(Ordering::Relaxed) + seats > limits.max_turns {
            return Err(Exhausted::Turns);
        }
        if self.model_calls.load(Ordering::Relaxed) >= limits.max_model_calls {
            return Err(Exhausted::ModelCalls);
        }
        if self.elapsed() >= Duration::from_secs(limits.max_wall_seconds) {
            return Err(Exhausted::WallClock);
        }
        if self.spend_usd() >= limits.max_spend_usd {
            return Err(Exhausted::Spend);
        }
        Ok(())
    }

    /// Claim one sandbox run for a seat on `desk`, or say why not.
    ///
    /// A third of the runs (rounded up) is held back for the verify desk, so
    /// the desks that compute a candidate cannot spend the runs that must
    /// check it.
    pub(super) fn claim_sandbox(&self, desk: &str) -> anyhow::Result<u64> {
        let total = self.limits.max_sandbox_runs;
        let off_verify = desk != VERIFY_DESK;
        if off_verify {
            // Reserve the off-verify share atomically first, so concurrent
            // seats cannot all see room for the last slot.
            let share = total - total.div_ceil(3);
            self.others_sandbox_runs
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                    (used < share).then_some(used + 1)
                })
                .map_err(|_| {
                    anyhow::anyhow!(
                        "the remaining sandbox runs are held for the verify desk; refer your program's idea to #verify or report"
                    )
                })?;
        }
        claim(&self.sandbox_runs, total, "sandbox runs").inspect_err(|_| {
            if off_verify {
                self.others_sandbox_runs.fetch_sub(1, Ordering::AcqRel);
            }
        })
    }

    /// Claim one web search, or say why not.
    pub(super) fn claim_search(&self) -> anyhow::Result<u64> {
        claim(&self.searches, self.limits.max_searches, "web searches")
    }

    pub(super) fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "limits": self.limits,
            "turns": self.turns.load(Ordering::Relaxed),
            "model_calls": self.model_calls.load(Ordering::Relaxed),
            "sandbox_runs": self.sandbox_runs.load(Ordering::Relaxed),
            "searches": self.searches.load(Ordering::Relaxed),
            "input_tokens": self.input_tokens.load(Ordering::Relaxed),
            "output_tokens": self.output_tokens.load(Ordering::Relaxed),
            "reasoning_tokens": self.reasoning_tokens.load(Ordering::Relaxed),
            "reported_model_usd": self.reported_usd(),
            "openrouter_account_delta_usd": self.account_delta(),
            "search_usd": self.search_usd(),
            "wall_seconds": self.elapsed().as_secs_f64(),
        })
    }
}

/// Capacity held by one admitted tool loop. Dropping without settling keeps
/// all capacity charged, including when its future is cancelled.
pub(super) struct ModelCalls<'a> {
    budget: &'a Budget,
    reserved: u64,
}

impl ModelCalls<'_> {
    /// Refund only the calls known not to have been dispatched.
    ///
    /// # Errors
    ///
    /// Returns an error without refunding capacity if `issued` exceeds the
    /// reserved number of calls.
    pub(super) fn finish(self, issued: u64) -> anyhow::Result<()> {
        let unused = self
            .reserved
            .checked_sub(issued)
            .ok_or_else(|| anyhow::anyhow!("the tool loop exceeded its reserved model calls"))?;
        self.budget.model_calls.fetch_sub(unused, Ordering::AcqRel);
        Ok(())
    }
}

fn claim(counter: &AtomicU64, limit: u64, what: &str) -> anyhow::Result<u64> {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
            (used < limit).then_some(used + 1)
        })
        .map(|used| used + 1)
        .map_err(|_| {
            anyhow::anyhow!("the run's {what} budget ({limit}) is spent; work with what you have")
        })
}

/// OpenRouter's usage for the calling key, in dollars.
///
/// # Errors
///
/// Returns a transport failure or an unexpected body.
pub(super) async fn openrouter_usage(client: &reqwest::Client, key: &str) -> anyhow::Result<f64> {
    let body: serde_json::Value = client
        .get("https://openrouter.ai/api/v1/key")
        .bearer_auth(key)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    body.pointer("/data/usage")
        .and_then(serde_json::Value::as_f64)
        .ok_or_else(|| anyhow::anyhow!("OpenRouter key usage missing from response"))
}
