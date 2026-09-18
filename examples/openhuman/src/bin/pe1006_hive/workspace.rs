//! Durable shared-workspace initialization and per-turn prompt snapshots.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::{MODEL, TASK};

const AGENTS_TEMPLATE: &str = r#"# Hive Agents

This directory is the durable shared workspace for every OpenHuman agent in
the hive. Read this file and `MEMORY.md` at the start of every turn.

## Working agreement

- Write code, derivations, evidence, and source notes under this workspace.
- Prefix role-owned working files with `theory_`, `solver_`, `checker_`,
  `researcher_`, or `lead_` to make ownership clear.
- Never overwrite another role's evidence. Create a correction that names the
  contradicted file and explains why.
- Add only reproduced facts to `MEMORY.md`; label hypotheses and rejected
  approaches explicitly.
- A checker may write `SIGNED` only after running the cited command against
  files that exist here. Matching two supplied samples is not sufficient.
- Do not store credentials, API keys, the private answer oracle, or secrets.
- Only the researcher may access the public web. Every web claim needs a URL.

## Roles

- `researcher`: locate and summarize public evidence with exact URLs.
- `theory`: derive the mathematical structure and state proof obligations.
- `solver`: implement justified methods and record reproducible commands.
- `checker`: attack claims, run independent checks, and fail closed.
- `lead`: reconcile the desk and report solved only after checker sign-off.
"#;

const MEMORY_TEMPLATE: &str = r#"# Hive Memory

Durable shared learnings for this workspace. Keep entries concise and
evidence-linked. This file is agent-maintained and survives individual runs.

## Established

- The official statement is in `TASK.md`.
- `Psi(3) = 20302` and `Psi(10) mod 101001001 = 10699667` are supplied checks.

## Rejected

- Do not infer a global recurrence solely from a fitted finite prefix.
- Do not treat a finite forbidden-pattern language as the Fibonacci factor set.

## Open work

- Derive and independently verify an exact scalable method for the target.

## Run notes

Add dated, attributed entries here. Include the command and relative evidence
path for every claimed computation.
"#;

pub(super) struct TurnSnapshots {
    directory: PathBuf,
    index: PathBuf,
    next: usize,
}

pub(super) struct PendingSnapshot {
    directory: PathBuf,
    turn: usize,
    route_id: String,
    agent_id: String,
    session_id: String,
    prompt_chars: usize,
}

impl TurnSnapshots {
    pub(super) fn new(run_dir: &Path) -> anyhow::Result<Self> {
        let directory = run_dir.join("turns");
        std::fs::create_dir_all(&directory)?;
        let index = directory.join("README.md");
        std::fs::write(
            &index,
            "# Agent turn snapshots\n\n| Turn | Route | OpenHuman agent | Session | Status | Files |\n| ---: | --- | --- | --- | --- | --- |\n",
        )?;
        Ok(Self {
            directory,
            index,
            next: 1,
        })
    }

    pub(super) fn begin(
        &mut self,
        route_id: &str,
        agent_id: &str,
        session_id: &str,
        prompt: &str,
    ) -> anyhow::Result<PendingSnapshot> {
        let turn = self.next;
        self.next += 1;
        let directory = self.directory.join(format!("{turn:03}-{route_id}"));
        std::fs::create_dir_all(&directory)?;
        std::fs::write(directory.join("prompt.md"), prompt)?;
        let pending = PendingSnapshot {
            directory,
            turn,
            route_id: route_id.to_string(),
            agent_id: agent_id.to_string(),
            session_id: session_id.to_string(),
            prompt_chars: prompt.chars().count(),
        };
        pending.write_metadata("started", None)?;
        Ok(pending)
    }

    pub(super) fn complete(&self, pending: PendingSnapshot, reply: &str) -> anyhow::Result<()> {
        std::fs::write(pending.directory.join("reply.md"), reply)?;
        pending.write_metadata("completed", Some(reply.chars().count()))?;
        let mut index = std::fs::OpenOptions::new().append(true).open(&self.index)?;
        writeln!(
            index,
            "| {} | @{} | `{}` | `{}` | completed | [prompt](./{:03}-{}/prompt.md), [reply](./{:03}-{}/reply.md), [metadata](./{:03}-{}/metadata.json) |",
            pending.turn,
            pending.route_id,
            pending.agent_id,
            pending.session_id,
            pending.turn,
            pending.route_id,
            pending.turn,
            pending.route_id,
            pending.turn,
            pending.route_id,
        )?;
        Ok(())
    }
}

impl PendingSnapshot {
    fn write_metadata(&self, status: &str, reply_chars: Option<usize>) -> anyhow::Result<()> {
        std::fs::write(
            self.directory.join("metadata.json"),
            serde_json::to_vec_pretty(&json!({
                "turn": self.turn,
                "route_id": self.route_id,
                "openhuman_agent_id": self.agent_id,
                "session_id": self.session_id,
                "model": MODEL,
                "status": status,
                "prompt_chars": self.prompt_chars,
                "reply_chars": reply_chars,
            }))?,
        )?;
        Ok(())
    }
}

pub(super) fn hive_workspace() -> PathBuf {
    std::env::var_os("OPENHUMAN_HIVE_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("workspace/pe1006"))
}

pub(super) fn initialize_workspace(workspace: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(workspace.join("runs"))?;
    write_if_missing(&workspace.join("AGENTS.md"), AGENTS_TEMPLATE)?;
    write_if_missing(&workspace.join("MEMORY.md"), MEMORY_TEMPLATE)?;
    write_if_missing(&workspace.join("TASK.md"), TASK)?;
    Ok(())
}

fn write_if_missing(path: &Path, content: &str) -> anyhow::Result<()> {
    if !path.exists() {
        std::fs::write(path, content)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tinyhivemind-openhuman-{label}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    fn workspace_templates_are_created_once_and_preserve_agent_memory() -> anyhow::Result<()> {
        let workspace = test_directory("workspace");
        initialize_workspace(&workspace)?;
        assert_eq!(
            std::fs::read_to_string(workspace.join("AGENTS.md"))?,
            AGENTS_TEMPLATE
        );
        std::fs::write(workspace.join("MEMORY.md"), "agent-authored memory\n")?;

        initialize_workspace(&workspace)?;

        assert_eq!(
            std::fs::read_to_string(workspace.join("MEMORY.md"))?,
            "agent-authored memory\n"
        );
        assert_eq!(std::fs::read_to_string(workspace.join("TASK.md"))?, TASK);
        std::fs::remove_dir_all(workspace)?;
        Ok(())
    }

    #[test]
    fn turn_snapshots_capture_prompt_reply_session_and_index() -> anyhow::Result<()> {
        let run_dir = test_directory("snapshots");
        let mut snapshots = TurnSnapshots::new(&run_dir)?;
        let pending = snapshots.begin(
            "checker",
            "checker-pe1006-fixture",
            "company:agent:checker",
            "full prompt",
        )?;
        snapshots.complete(pending, "full reply")?;

        let turn = run_dir.join("turns/001-checker");
        assert_eq!(
            std::fs::read_to_string(turn.join("prompt.md"))?,
            "full prompt"
        );
        assert_eq!(
            std::fs::read_to_string(turn.join("reply.md"))?,
            "full reply"
        );
        let metadata: serde_json::Value =
            serde_json::from_slice(&std::fs::read(turn.join("metadata.json"))?)?;
        assert_eq!(metadata["session_id"], "company:agent:checker");
        assert_eq!(metadata["status"], "completed");
        assert!(
            std::fs::read_to_string(run_dir.join("turns/README.md"))?
                .contains("001-checker/prompt.md")
        );
        std::fs::remove_dir_all(run_dir)?;
        Ok(())
    }
}
