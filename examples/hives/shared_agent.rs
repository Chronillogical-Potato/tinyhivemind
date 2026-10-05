//! One supplied agent keeps its session while working in three hives.
//!
//! Run with `cargo run --manifest-path examples/hives/Cargo.toml --bin shared_agent`.

use std::sync::Arc;
use tinyhivemind_hives::{
    AgentRegistration, AgentRunner, Coordinator, CoordinatorOptions, Destination, EpisodeAction,
    HiveInfo, MemoryStorage, Result, SendMessage, TurnDisposition, TurnFuture, TurnOutcome,
    TurnRequest,
};
use tokio::sync::Mutex;

struct SharedAgent {
    coordinator: Coordinator,
    seen: Arc<Mutex<Vec<SeenTurn>>>,
}

struct SeenTurn {
    hive_id: String,
    session_id: Option<String>,
}

impl AgentRunner for SharedAgent {
    fn run(&self, request: TurnRequest) -> TurnFuture {
        let coordinator = self.coordinator.clone();
        let seen = self.seen.clone();
        Box::pin(async move {
            if let Some(episode) = &request.episode {
                seen.lock().await.push(SeenTurn {
                    hive_id: episode.hive_id.clone(),
                    session_id: request.session_id.clone(),
                });
                coordinator
                    .submit_action(
                        &request.agent_id,
                        &episode.episode_id,
                        EpisodeAction::Complete {
                            body: format!("Finished work in {}", episode.hive_id),
                        },
                    )
                    .await?;
            }
            Ok(TurnOutcome {
                session_id: request
                    .session_id
                    .unwrap_or_else(|| "shared-session".into()),
                reply: None,
                disposition: TurnDisposition::Completed,
            })
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let coordinator = Coordinator::new(
        "example-runtime".into(),
        Arc::new(MemoryStorage::new()),
        CoordinatorOptions::default(),
    )
    .await?;
    let seen = Arc::new(Mutex::new(Vec::new()));
    coordinator
        .register_agent(AgentRegistration {
            agent_id: "specialist".into(),
            runtime_id: "example-runtime".into(),
            runner: Arc::new(SharedAgent {
                coordinator: coordinator.clone(),
                seen: seen.clone(),
            }),
        })
        .await?;

    for hive_id in ["research", "engineering", "release"] {
        coordinator
            .create_hive(HiveInfo {
                hive_id: hive_id.into(),
                name: hive_id.into(),
                description: None,
                members: vec!["specialist".into()],
            })
            .await?;
        coordinator
            .send_as_host(SendMessage {
                message_id: format!("task:{hive_id}"),
                sender: String::new(),
                destination: Destination::Hive(hive_id.into()),
                body: format!("Handle the {hive_id} task."),
                thread: None,
                only_for: Vec::new(),
                starters: Vec::new(),
            })
            .await?;
    }

    let report = coordinator.run_until_idle().await?;
    assert_eq!(report.completed, 3);
    assert_eq!(report.failed, 0);
    let history = seen.lock().await;
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].session_id, None);
    assert!(
        history[1..]
            .iter()
            .all(|turn| turn.session_id.as_deref() == Some("shared-session"))
    );
    for turn in history.iter() {
        println!("{}: continued {:?}", turn.hive_id, turn.session_id);
    }
    Ok(())
}
