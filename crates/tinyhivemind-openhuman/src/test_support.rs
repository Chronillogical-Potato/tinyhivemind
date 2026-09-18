//! Shared concrete `OpenHuman` agent fixtures for unit tests.

#![allow(clippy::expect_used)]

use std::sync::{Mutex, OnceLock};

use openhuman_embed::{Agent, AgentSpec, Runtime, Workspace};

pub(crate) struct Fixture {
    executor: Mutex<tokio::runtime::Runtime>,
    agents: Vec<Agent>,
}

impl Fixture {
    pub(crate) fn agent(&self, index: usize) -> Agent {
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
        let runtime = executor
            .block_on(
                Runtime::builder()
                    .workspace(Workspace::Ephemeral)
                    .api_key("th_test_tinyhivemind_openhuman")
                    .build(),
            )
            .expect("fixture OpenHuman runtime builds");
        let agents = [
            "runtime-one",
            "runtime-two",
            "runtime-three",
            "runtime-four",
        ]
        .into_iter()
        .map(|id| runtime.agent(AgentSpec::new(id)).expect("agent builds"))
        .collect();
        Fixture {
            executor: Mutex::new(executor),
            agents,
        }
    })
}
