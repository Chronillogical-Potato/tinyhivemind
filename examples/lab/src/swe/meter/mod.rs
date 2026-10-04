//! The token meter both modes share.
//!
//! Every model call, from every seat, is recorded here and nowhere else, so a
//! hive run and a single-agent run are priced by the same counter. The meter
//! also owns the two run caps (`--token-cap`, `--max-turns`) so they mean the
//! same thing in both modes: a call is refused once the cap is reached.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// Why a run stopped before the model finished.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Abort {
    /// Input plus output tokens reached `--token-cap`.
    TokenCap {
        /// Tokens spent.
        used: u64,
        /// The cap.
        cap: u64,
    },
    /// Model calls reached `--max-turns`.
    MaxTurns {
        /// Calls made.
        calls: u64,
        /// The cap.
        cap: u64,
    },
    /// The model endpoint failed twice in a row.
    Llm(String),
}

impl fmt::Display for Abort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TokenCap { used, cap } => write!(f, "token cap reached ({used}/{cap})"),
            Self::MaxTurns { calls, cap } => write!(f, "max turns reached ({calls}/{cap})"),
            Self::Llm(why) => write!(f, "model call failed: {why}"),
        }
    }
}

impl std::error::Error for Abort {}

/// Tokens and calls attributed to one seat.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SeatUsage {
    /// Prompt tokens.
    pub input: u64,
    /// Completion tokens.
    pub output: u64,
    /// Model calls.
    pub calls: u64,
}

/// A point-in-time copy of the meter.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Snapshot {
    /// Prompt tokens, all seats.
    pub input: u64,
    /// Completion tokens, all seats.
    pub output: u64,
    /// Model calls, all seats.
    pub calls: u64,
    /// The same, per seat.
    pub seats: BTreeMap<String, SeatUsage>,
}

/// Thread-safe usage counters with optional caps.
#[derive(Debug, Default)]
pub struct Meter {
    input: AtomicU64,
    output: AtomicU64,
    calls: AtomicU64,
    seats: Mutex<BTreeMap<String, SeatUsage>>,
    token_cap: Option<u64>,
    max_calls: Option<u64>,
}

impl Meter {
    /// A meter with the given caps; `None` means unbounded.
    #[must_use]
    pub fn new(token_cap: Option<u64>, max_calls: Option<u64>) -> Self {
        Self {
            token_cap,
            max_calls,
            ..Self::default()
        }
    }

    /// Refuse a further call when a cap is already reached.
    ///
    /// # Errors
    ///
    /// Returns the [`Abort`] naming the cap.
    pub fn check(&self) -> Result<(), Abort> {
        let used = self.input.load(Ordering::SeqCst) + self.output.load(Ordering::SeqCst);
        if let Some(cap) = self.token_cap
            && used >= cap
        {
            return Err(Abort::TokenCap { used, cap });
        }
        let calls = self.calls.load(Ordering::SeqCst);
        if let Some(cap) = self.max_calls
            && calls >= cap
        {
            return Err(Abort::MaxTurns { calls, cap });
        }
        Ok(())
    }

    /// Reserve one call slot, so concurrent seats cannot overshoot
    /// `--max-turns`; the call is counted whether or not it succeeds.
    ///
    /// # Errors
    ///
    /// Returns the [`Abort`] naming the cap.
    pub fn begin_call(&self) -> Result<(), Abort> {
        self.check()?;
        let before = self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(cap) = self.max_calls
            && before >= cap
        {
            self.calls.fetch_sub(1, Ordering::SeqCst);
            return Err(Abort::MaxTurns { calls: before, cap });
        }
        Ok(())
    }

    /// Record the usage one finished call reported.
    pub fn record(&self, seat: &str, input: u64, output: u64) {
        self.input.fetch_add(input, Ordering::SeqCst);
        self.output.fetch_add(output, Ordering::SeqCst);
        if let Ok(mut seats) = self.seats.lock() {
            let usage = seats.entry(seat.to_owned()).or_default();
            usage.input += input;
            usage.output += output;
            usage.calls += 1;
        }
    }

    /// Copy the counters.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            input: self.input.load(Ordering::SeqCst),
            output: self.output.load(Ordering::SeqCst),
            calls: self.calls.load(Ordering::SeqCst),
            seats: self.seats.lock().map(|s| s.clone()).unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod test;
