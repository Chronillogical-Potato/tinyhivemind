//! Stable native tools with bound attribution and weak service references.
mod types;
use crate::host::{Activation, Inner};
use crate::{Error, ManagementRequest, OpenHumanHost, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Weak;
use tinyhivemind_hives::{Destination, EpisodeAction, HiveInfo, SendMessage};
use tinytools::{PermissionLevel, Tool, ToolResult};
use types::Kind;
pub(crate) fn belt(
    actor: &str,
    host: &Weak<Inner>,
    activation: &std::sync::Arc<Activation>,
    managed: bool,
) -> Vec<Box<dyn Tool>> {
    Kind::all(managed)
        .into_iter()
        .map(|kind| {
            Box::new(HiveTool {
                actor: actor.into(),
                host: host.clone(),
                kind,
                activation: activation.clone(),
            }) as Box<dyn Tool>
        })
        .collect()
}
struct HiveTool {
    actor: String,
    host: Weak<Inner>,
    kind: Kind,
    activation: std::sync::Arc<Activation>,
}
#[async_trait]
impl Tool for HiveTool {
    fn name(&self) -> &str {
        self.kind.name()
    }
    fn description(&self) -> &str {
        self.kind.description()
    }
    fn parameters_schema(&self) -> Value {
        self.kind.schema()
    }
    fn permission_level(&self) -> PermissionLevel {
        match self.kind {
            Kind::ListHives | Kind::ListAgents | Kind::Read => PermissionLevel::ReadOnly,
            _ => PermissionLevel::Write,
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let result = async {
            self.kind.validate(&args).map_err(Error::Harness)?;
            let host = OpenHumanHost {
                inner: self.host.upgrade().ok_or(Error::Unavailable)?,
            };
            if !self.activation.is_ready() {
                return Err(Error::RegistrationPending);
            }
            self.perform(&host, args).await
        }
        .await;
        Ok(match result {
            Ok(value) => ToolResult::success(serde_json::to_string(&value)?),
            Err(error) => ToolResult::error(error.to_string()),
        })
    }
}
impl HiveTool {
    async fn perform(&self, host: &OpenHumanHost, args: Value) -> Result<Value> {
        let coordinator = host.coordinator();
        let text = |name: &str| args[name].as_str().unwrap_or_default().to_owned();
        let actor = &self.actor;
        match self.kind {
            Kind::ListHives => Ok(serde_json::to_value(
                coordinator
                    .list_hives()?
                    .into_iter()
                    .filter(|hive| hive.members.contains(actor))
                    .collect::<Vec<_>>(),
            )?),
            Kind::ListAgents => Ok(serde_json::to_value(coordinator.list_agents()?)?),
            Kind::Read => Ok(serde_json::to_value(coordinator.read_hive(
                actor,
                &text("hive_id"),
                args["after"].as_u64(),
                args["thread"].as_u64(),
            )?)?),
            Kind::SendHive | Kind::SendAgent => {
                let destination = match self.kind {
                    Kind::SendHive => Destination::Hive(text("hive_id")),
                    _ => Destination::Agent(text("agent_id")),
                };
                Ok(serde_json::to_value(coordinator.send(SendMessage {
                    message_id: text("message_id"),
                    sender: actor.clone(),
                    destination,
                    body: text("body"),
                    thread: args["thread"].as_u64(),
                    only_for: strings(&args, "only_for"),
                })?)?)
            }
            Kind::Post | Kind::Ask | Kind::Broadcast | Kind::Complete => {
                let body = text("body");
                let action = match self.kind {
                    Kind::Post => EpisodeAction::Post { body },
                    Kind::Ask => EpisodeAction::Ask {
                        body,
                        agents: strings(&args, "agents"),
                    },
                    Kind::Broadcast => EpisodeAction::Broadcast { body },
                    _ => EpisodeAction::Complete { body },
                };
                submit(coordinator, actor, &text("episode_id"), action)
            }
            Kind::CreateHive => {
                host.manage(
                    actor,
                    ManagementRequest::CreateHive(HiveInfo {
                        hive_id: text("hive_id"),
                        name: text("name"),
                        description: args["description"].as_str().map(str::to_owned),
                        members: strings(&args, "members"),
                    }),
                )
                .await
            }
            Kind::CreateAgent => {
                host.manage(
                    actor,
                    ManagementRequest::CreateAgent {
                        template: text("template"),
                        config: args["config"].clone(),
                    },
                )
                .await
            }
            Kind::JoinHive => {
                host.manage(
                    actor,
                    ManagementRequest::JoinHive {
                        hive_id: text("hive_id"),
                        agent_id: text("agent_id"),
                    },
                )
                .await
            }
            Kind::LeaveHive => {
                host.manage(
                    actor,
                    ManagementRequest::LeaveHive {
                        hive_id: text("hive_id"),
                        agent_id: text("agent_id"),
                    },
                )
                .await
            }
        }
    }
}
#[cfg(test)]
mod test;

fn submit(
    coordinator: &tinyhivemind_hives::Coordinator,
    actor: &str,
    episode: &str,
    action: EpisodeAction,
) -> Result<Value> {
    coordinator.submit_action(actor, episode, action)?;
    Ok(serde_json::json!({"accepted":true}))
}

fn strings(args: &Value, name: &str) -> Vec<String> {
    args[name]
        .as_array()
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
