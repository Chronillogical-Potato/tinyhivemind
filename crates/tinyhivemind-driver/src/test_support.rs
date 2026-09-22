//! Shared seat fixtures for unit tests.
//!
//! The driver stores a bound handle and hands it back; it never runs one. So
//! a test seat is a name and nothing else, and the fixture's only other job
//! is an executor for the driver's async folds.

#![allow(clippy::expect_used)]

use std::sync::{Mutex, OnceLock};

use crate::BoundAgent;

/// A seat with nothing behind it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Seat(pub(crate) String);

impl BoundAgent for Seat {
    fn runtime_id(&self) -> &str {
        &self.0
    }
}

pub(crate) struct Fixture {
    executor: Mutex<tokio::runtime::Runtime>,
    agents: Vec<Seat>,
}

impl Fixture {
    pub(crate) fn agent(&self, index: usize) -> Seat {
        self.agents[index].clone()
    }

    pub(crate) fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.executor
            .lock()
            .expect("test executor lock is not poisoned")
            .block_on(future)
    }
}

pub(crate) fn fixture() -> &'static Fixture {
    static FIXTURE: OnceLock<Fixture> = OnceLock::new();
    FIXTURE.get_or_init(|| {
        let executor = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test executor builds");
        let agents = [
            "runtime-one",
            "runtime-two",
            "runtime-three",
            "runtime-four",
        ]
        .into_iter()
        .map(|id| Seat(id.to_owned()))
        .collect();
        Fixture {
            executor: Mutex::new(executor),
            agents,
        }
    })
}
