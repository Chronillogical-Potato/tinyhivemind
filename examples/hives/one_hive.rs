//! A planner asks a reviewer before completing one hive assignment.
//!
//! Run with `cargo run --manifest-path examples/hives/Cargo.toml --bin one_hive`.

use std::sync::Arc;
use tinyhivemind_hives::{
    AgentRegistration, AgentRunner, Coordinator, CoordinatorOptions, Destination, EpisodeAction,
    HiveInfo, MemoryStorage, Result, SendMessage, TurnDisposition, TurnFuture, TurnOutcome,
    TurnRequest,
};

struct ScriptedAgent {
    coordinator: Coordinator,
}

impl AgentRunner for ScriptedAgent {
    fn run(&self, request: TurnRequest) -> TurnFuture {
        let coordinator = self.coordinator.clone();
        Box::pin(async move {
            if let Some(episode) = &request.episode {
                let action = match (request.agent_id.as_str(), request.session_id.is_none()) {
                    ("planner", true) => EpisodeAction::Ask {
                        agents: vec!["reviewer".into()],
                        body: "Check the rollout plan before I finish.".into(),
                    },
                    ("reviewer", _) => EpisodeAction::Complete {
                        body: "The staged rollout has a rollback path.".into(),
                    },
                    ("planner", false) if episode.brief.contains("rollback path") => {
                        EpisodeAction::Complete {
                            body: "Ship the staged rollout with the reviewed rollback path.".into(),
                        }
                    }
                    _ => {
                        println!("{}: waiting for the review", request.agent_id);
                        return Ok(TurnOutcome {
                            session_id: request
                                .session_id
                                .unwrap_or_else(|| format!("session:{}", request.agent_id)),
                            reply: None,
                            disposition: TurnDisposition::Completed,
                        });
                    }
                };
                println!("{}: {action:?}", request.agent_id);
                coordinator.submit_action(&request.agent_id, &episode.episode_id, action)?;
            }
            Ok(TurnOutcome {
                session_id: request
                    .session_id
                    .unwrap_or_else(|| format!("session:{}", request.agent_id)),
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
    )?;
    for agent_id in ["planner", "reviewer"] {
        coordinator.register_agent(AgentRegistration {
            agent_id: agent_id.into(),
            runtime_id: "example-runtime".into(),
            runner: Arc::new(ScriptedAgent {
                coordinator: coordinator.clone(),
            }),
        })?;
    }
    coordinator.create_hive(HiveInfo {
        hive_id: "release".into(),
        name: "Release".into(),
        description: Some("Review a rollout plan".into()),
        members: vec!["planner".into(), "reviewer".into()],
    })?;
    coordinator.send_as_host(SendMessage {
        message_id: "rollout-request".into(),
        sender: String::new(),
        destination: Destination::Hive("release".into()),
        body: "Choose a safe rollout plan.".into(),
        thread: None,
        only_for: vec!["planner".into()],
    })?;

    let report = coordinator.run_until_idle().await?;
    assert_eq!(report.completed, 4);
    assert_eq!(report.failed, 0);
    let transcript = coordinator.read_hive("planner", "release", None, None)?;
    assert!(
        transcript
            .iter()
            .any(|message| message.body.contains("rollback path"))
    );
    println!(
        "completed {} turns; {} visible hive messages",
        report.completed,
        transcript.len()
    );
    Ok(())
}
