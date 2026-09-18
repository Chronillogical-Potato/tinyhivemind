//! Web-assisted OpenRouter GPT-OSS OpenHuman hive experiment for Project Euler 1006.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use futures::future::join_all;
use openhuman_embed::{
    Access, AgentDefinitionSpec, AgentSpec, McpServer, Provider, Runtime, RuntimeConfig,
    ServiceSet, ToolScopeSpec, Workspace,
};
use serde_json::json;
use tinyhivemind::desk::{Desk, ResponderMode};
use tinyhivemind_hive::{
    CompletionEpisodeState, CompletionStep, ParticipantCompletion, apply_assignment,
    completion_status,
};
use tinyhivemind_openhuman::{
    AgentBinding, BroadcastRouting, CommittedUtterance, CompletionDriver, HiveGraph, HostAction,
    OpenHumanHive,
};
use tinyhivemind_typesafe::JevRouter;
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[path = "pe1006_hive/episode.rs"]
mod episode_support;
#[path = "pe1006_hive/tools.rs"]
mod hive_tools;
#[path = "pe1006_hive/round.rs"]
mod round_support;
#[path = "pe1006_hive/typesafe.rs"]
mod typesafe_support;
#[path = "pe1006_hive/workspace.rs"]
mod workspace_support;

use episode_support::{completion_assignment, recover_tool_call, role_prompt, route_candidates};
use round_support::{TurnContext, prepare_seat_turn, seat_turn};
use workspace_support::{TurnSnapshots, hive_workspace, initialize_workspace};

const MODEL: &str = "openai/gpt-oss-120b:nitro";
const PROVIDER_BASE: &str = "https://openrouter.ai/api/v1";
const TURN_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_TURNS: u32 = 25;
const TASK_1006: &str = r#"Starting with two strings S_0 = 0 and S_1 = 01, define S_n as the
concatenation S_(n-1)S_(n-2) for n >= 2.

For example, S_2 = 010, S_3 = 01001, and S_4 = 01001010.

A string is called a Fibonacci subword if it is a contiguous substring of
some S_n. For every positive integer k there are exactly k+1 different
Fibonacci subwords of length k. Interpret each as a decimal number, ignoring
leading zeroes, and let Psi(k) be the sum of their squares.

For k = 3 the four subwords are 001, 010, 100, and 101, so
Psi(3) = 20302. You are also given
Psi(10) = 10699667 (mod 101001001).

Find Psi(10^18) mod 101001001."#;
const TASK_1008: &str = r#"Define the (N,M)-functional inverse of x^2 to be the monic
polynomial Q(x) of degree N+1 such that Q(n^2) is congruent to n modulo M for
all integers 0 <= n <= N and all coefficients are non-negative and smaller
than M.

For example, the (2,7)-functional inverse of x^2 is
x^3 + 3x^2 + 4x.

Find the coefficient of x^10 in the (10^7, 10^9+7)-functional inverse of x^2.

Source: https://projecteuler.net/problem=1008"#;
const TASK: &str = TASK_1006;

const SEALED: &str = "Use only the statement, this desk transcript, and computations in the shared workspace. Do not search the web, inspect this repository, use inherited solution memory, or read outside the workspace. Never invent a residue. Keep the desk message below 1800 characters and name concrete files or checks.";
const PRIOR_FAILURE: &str = "Prior hive runs were rejected. Candidate residues 58302041 and 14193671 came from invalid methods and must not be reused. A later run fabricated 123456789, which is not even a canonical residue modulo 101001001; its claimed verifier actually failed at k=1 and its solver printed a different value. One run fitted an order-60 Berlekamp-Massey recurrence from only 120 terms and tested it on no held-out suffix; that is interpolation, not proof. Another used a finite-state factor language that already overcounts at k=5, and its claimed code failed the supplied k=10 sample when actually executed. Do not use Berlekamp-Massey, guessed recurrences, fitted scaling factors, or a finite forbidden-pattern DFA. Derive an exact identity from Fibonacci/Sturmian/Ostrowski structure, and validate any implementation well beyond the cases used to derive it.";
const RESEARCH_POLICY: &str = "You are the only seat allowed to access the public web. Use shell commands such as curl to search and fetch public sources. Return direct source URLs, distinguish a claimed answer from a derivation, and never treat one copied number as verification. Do not inspect this repository, inherited solution files, or any filesystem path outside the named workspace. Keep the desk message below 1800 characters.";
const RESEARCH_START: &str = "Public code search located these potentially relevant sources. Fetch and assess them; do not merely quote a residue:\n- https://github.com/senamakel/math-agent/blob/be919bc1bdc6b77a075413192654931b80cae602/workspace/euler1006/code/lean/code/python/euler1006.py\n- https://github.com/senamakel/math-agent/blob/be919bc1bdc6b77a075413192654931b80cae602/workspace/euler1006/refs/context.md\n- https://github.com/dawei7/code_n/tree/012e178619373894a06afb8db07953df0202a071/dsa/euler/1006_fibonacci-subwords\n- https://github.com/senamakel/math-superagent/blob/f0b35053424007d21d71363ce4ed73e0c8baca9e/workspace/project-euler/1006/derived/APPROACHES.md\n- https://github.com/senamakel/math-superagent/blob/f0b35053424007d21d71363ce4ed73e0c8baca9e/workspace/project-euler/1006/code/out/PE1006-verification.md\n- https://eulersolve.org/problem/1006/\n- https://eulersolve.org/solutionsPython/Euler1006.py\n- https://github.com/cirosantilli/project-euler-solutions/blob/master/solvers/1006.md";

#[derive(Clone, Debug)]
struct DeskMessage {
    author: String,
    body: String,
}

#[derive(Default)]
struct Visibility {
    seen: BTreeMap<String, BTreeSet<usize>>,
}

impl Visibility {
    fn delta(&self, agent: &str, transcript: &[DeskMessage]) -> String {
        let seen = self.seen.get(agent);
        transcript
            .iter()
            .enumerate()
            .filter(|(index, _)| !seen.is_some_and(|set| set.contains(index)))
            .map(|(_, row)| format!("@{}: {}", row.author, row.body))
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn mark_delivered(&mut self, agent: &str, transcript_len: usize) {
        self.seen
            .entry(agent.to_string())
            .or_default()
            .extend(0..transcript_len);
    }

    fn mark_own(&mut self, agent: &str, index: usize) {
        self.seen
            .entry(agent.to_string())
            .or_default()
            .insert(index);
    }
}

fn main() -> anyhow::Result<()> {
    if let Some(server) = hive_tools::requested()? {
        return hive_tools::serve(&server);
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()?;
    runtime.block_on(run())
}

async fn run() -> anyhow::Result<()> {
    let problem = std::env::var("OPENHUMAN_HIVE_PROBLEM").unwrap_or_else(|_| "1006".into());
    let (task, prior_failure) = match problem.as_str() {
        "1006" => (TASK_1006, PRIOR_FAILURE),
        "1008" => (
            TASK_1008,
            "No prior attempts are supplied. Derive the result independently and verify it on small N before scaling.",
        ),
        other => anyhow::bail!("unsupported hive problem {other}"),
    };
    let api_key = std::env::var("OPENROUTER_API_KEY")?;
    let typesafe_api_key = std::env::var("TYPESAFE_API_KEY")?;
    let backend = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "success": true,
            "data": {"id": "pe1006-hive", "email": "local@openhuman.local"}
        })))
        .mount(&backend)
        .await;

    let workspace = hive_workspace();
    let run_id = format!("run-{}", std::process::id());
    let run_dir = workspace.join("runs").join(&run_id);
    let outbox_dir = run_dir.join("hive-tool-outboxes");
    initialize_workspace(&workspace)?;
    if problem == "1008" {
        std::fs::write(
            workspace.join("TASK.md"),
            format!("# Project Euler 1008\n\n{task}\n"),
        )?;
    }
    std::fs::create_dir_all(&run_dir)?;
    if problem == "1006" {
        stage_research_sources(&workspace).await?;
    }

    let config = inherited_config().await?;
    if config.subsystems.memory.driver != "tinycortex" {
        anyhow::bail!(
            "machine OpenHuman memory driver is {:?}, expected tinycortex",
            config.subsystems.memory.driver
        );
    }
    let runtime = Arc::new(
        Runtime::builder()
            .config(config)
            .workspace(Workspace::dir(run_dir.join("openhuman-runtime")))
            .services(memory_services())
            .backend_url(backend.uri())
            .provider(Provider::openai_compatible(PROVIDER_BASE, api_key).model(MODEL))
            .access(Access::full())
            .build()
            .await?,
    );
    let bindings = vec![
        instantiated(
            &runtime,
            &workspace,
            &outbox_dir,
            &problem,
            "theory",
            role_prompt(&problem, "theory"),
        )?,
        instantiated(
            &runtime,
            &workspace,
            &outbox_dir,
            &problem,
            "solver",
            role_prompt(&problem, "solver"),
        )?,
        instantiated(
            &runtime,
            &workspace,
            &outbox_dir,
            &problem,
            "checker",
            role_prompt(&problem, "checker"),
        )?,
        instantiated(
            &runtime,
            &workspace,
            &outbox_dir,
            &problem,
            "lead",
            role_prompt(&problem, "lead"),
        )?,
        instantiated(
            &runtime,
            &workspace,
            &outbox_dir,
            &problem,
            "researcher",
            role_prompt(&problem, "researcher"),
        )?,
    ];
    let team = ["theory", "solver", "checker", "lead", "researcher"];
    let hive = OpenHumanHive::new(
        HiveGraph::new(
            Desk {
                id: format!("pe{problem}"),
                name: format!("PE{problem}"),
                description: Some(format!(
                    "derive and independently verify the exact Project Euler {problem} answer"
                )),
                members: team.iter().map(|id| (*id).into()).collect(),
                responder_mode: ResponderMode::Auto,
            },
            route_candidates(&problem, None),
        ),
        bindings,
    )?;
    let router = JevRouter::new(typesafe_support::Transport::new(typesafe_api_key)?);
    let mut roster_version = 1_u64;
    let routing_policy = typesafe_support::routing_policy();
    let thread_context = typesafe_support::thread_context();
    let initial_request = hive.desk_request(
        task,
        thread_context.clone(),
        Some(tinyhivemind::Sequence(1)),
        roster_version,
        routing_policy.clone(),
    );
    let route = hive
        .route_desk(Some(&router), None, &initial_request, None, "lead")
        .await?;
    let mut selected = hive
        .resolve_plan(&route)?
        .iter()
        .map(|binding| binding.hive_agent_id.clone())
        .collect::<Vec<_>>();
    if selected.is_empty() {
        selected.push("lead".into());
    }
    std::fs::write(
        run_dir.join("initial-route.json"),
        serde_json::to_vec_pretty(&route)?,
    )?;
    println!("runtime_agents: {}", runtime.agent_ids().join(","));
    println!("initial_route: {}", selected.join(","));
    println!("model: {MODEL}");
    println!("memory_driver: tinycortex");
    println!("workspace: {}", workspace.display());
    println!("run_dir: {}", run_dir.display());

    let mut transcript = Vec::new();
    let mut visibility = Visibility::default();
    let mut snapshots = TurnSnapshots::new(&run_dir)?;
    let mut episode = CompletionEpisodeState {
        conversation: tinyhivemind::Conversation {
            desk_id: format!("pe{problem}"),
            desk_name: format!("PE{problem}"),
            thread_root: None,
        },
        watermark: tinyhivemind::Sequence(0),
        participants: team
            .iter()
            .map(|id| ParticipantCompletion {
                agent_id: (*id).into(),
                assigned_at: tinyhivemind::Sequence(0),
                completed_at: Some(tinyhivemind::Sequence(0)),
            })
            .collect(),
    };
    let mut sequence = 1_u64;
    episode = apply_assignment(
        &episode,
        selected.iter().map(String::as_str),
        tinyhivemind::Sequence(sequence),
    )?;
    let driver = CompletionDriver::new(&hive, routing_policy.round_width)?;
    let mut driver_state = driver.start(episode)?;
    let mut route_trace = vec![route];
    let mut missed_tools: BTreeMap<String, u8> = BTreeMap::new();
    let mut turns = 0_u32;
    while !matches!(
        completion_status(driver_state.episode()),
        CompletionStep::Complete { .. }
    ) {
        let pending = driver.pending_round(&driver_state)?;
        if pending.is_empty() {
            anyhow::bail!("completion episode has pending work but no scheduled agent")
        }
        let transcript_len = transcript.len();
        let round_agents: BTreeMap<_, _> = pending
            .agents()
            .iter()
            .map(|pending_agent| {
                (pending_agent.hive_agent_id.to_owned(), pending_agent.agent.clone())
            })
            .collect();
        let mut remaining: Vec<_> = round_agents.keys().cloned().collect();
        let mut round_utterances = BTreeMap::new();
        while !remaining.is_empty() {
            ensure_round_fits(turns, remaining.len())?;
            let prepared = remaining
                .iter()
                .map(|id| {
                    let agent = round_agents
                        .get(id)
                        .ok_or_else(|| anyhow::anyhow!("pending agent disappeared from round"))?;
                    prepare_seat_turn(
                        agent.clone(),
                        id,
                        TurnContext {
                            transcript: &transcript,
                            visibility: &visibility,
                            outbox: outbox_dir.join(format!("{id}.jsonl")),
                            assignment: completion_assignment(&problem, id),
                            task,
                            prior_failure,
                            problem: &problem,
                        },
                        &mut snapshots,
                    )
                })
                .collect::<anyhow::Result<Vec<_>>>()?;
            let outcomes = join_all(prepared.into_iter().map(seat_turn)).await;
            turns = turns.saturating_add(u32::try_from(outcomes.len()).unwrap_or(u32::MAX));
            let mut retry = Vec::new();
            for outcome in outcomes {
                let mut outcome = outcome?;
                snapshots.complete(outcome.snapshot, &outcome.reply)?;
                visibility.mark_delivered(&outcome.id, transcript_len);
                if outcome.utterances.is_empty()
                    && let Some(recovered) = recover_tool_call(&outcome.reply)
                {
                    println!("[compatibility-recovered-tool-call] @{}", outcome.id);
                    outcome.utterances.push(recovered);
                }
                if outcome.utterances.is_empty() {
                    let misses = missed_tools.entry(outcome.id.clone()).or_default();
                    *misses = misses.saturating_add(1);
                    if *misses >= 4 {
                        anyhow::bail!(
                            "@{} four times failed to call a TinyHiveMind tool",
                            outcome.id
                        )
                    }
                    println!("[no-hive-tool] @{}; retrying only that seat", outcome.id);
                    retry.push(outcome.id);
                    continue;
                }
                if outcome.utterances.len() != 1 {
                    anyhow::bail!(
                        "@{} emitted {} TinyHiveMind actions; exactly one is required",
                        outcome.id,
                        outcome.utterances.len()
                    )
                }
                missed_tools.remove(&outcome.id);
                round_utterances.insert(outcome.id, outcome.utterances.remove(0));
            }
            remaining = retry;
        }

        let mut committed = Vec::with_capacity(round_utterances.len());
        for id in round_agents.keys() {
            let utterance = round_utterances
                .remove(id)
                .ok_or_else(|| anyhow::anyhow!("round did not retain {id}'s action"))?;
            sequence = sequence.saturating_add(1);
            if matches!(&utterance, tinyhivemind::speech::Utterance::Broadcast { .. }) {
                roster_version = roster_version.saturating_add(1);
            }
            match &utterance {
                tinyhivemind::speech::Utterance::Broadcast { message } => {
                    let index = transcript.len();
                    transcript.push(DeskMessage {
                        author: id.clone(),
                        body: format!("BROADCAST: {message}"),
                    });
                    visibility.mark_own(&id, index);
                }
                tinyhivemind::speech::Utterance::CompleteEpisode { message } => {
                    let index = transcript.len();
                    transcript.push(DeskMessage {
                        author: id.clone(),
                        body: format!("COMPLETE: {message}"),
                    });
                    visibility.mark_own(&id, index);
                }
                tinyhivemind::speech::Utterance::Post { .. }
                | tinyhivemind::speech::Utterance::Dm { .. } => {
                    anyhow::bail!("MCP completion surface emitted an unsupported utterance")
                }
            }
            if !matches!(&utterance, tinyhivemind::speech::Utterance::Broadcast { .. }) {
                println!("[complete_episode] @{id}");
            }
            committed.push(CommittedUtterance {
                author_id: id.clone(),
                sequence: tinyhivemind::Sequence(sequence),
                utterance,
            });
        }
        let routing = committed.iter().any(|event| {
            matches!(
                &event.utterance,
                tinyhivemind::speech::Utterance::Broadcast { .. }
            )
        });
        let transition = driver
            .apply_committed_round(
                &driver_state,
                &pending,
                committed,
                routing.then_some(BroadcastRouting {
                    primary: Some(&router),
                    reasoning: None,
                    policy: &routing_policy,
                    roster_version,
                    thread_context: &thread_context,
                }),
            )
            .await?;
        for action in &transition.actions {
            match action {
                HostAction::RunAgents { agent_ids, plan } => {
                    println!("[broadcast] -> {}", agent_ids.join(","));
                    route_trace.push(plan.clone());
                }
                HostAction::DeliverDm { .. } => {
                    anyhow::bail!("MCP completion surface emitted an unsupported DM")
                }
            }
        }
        driver_state = transition.state;
    }
    std::fs::write(
        run_dir.join("routing-trace.json"),
        serde_json::to_vec_pretty(&route_trace)?,
    )?;
    std::fs::write(
        run_dir.join("completion-state.json"),
        serde_json::to_vec_pretty(&driver_state)?,
    )?;
    println!(
        "completion_status: {:?}",
        completion_status(driver_state.episode())
    );

    let trace = transcript
        .iter()
        .map(|row| format!("## @{}\n\n{}", row.author, row.body))
        .collect::<Vec<_>>()
        .join("\n\n");
    std::fs::write(run_dir.join("HIVE_TRANSCRIPT.md"), &trace)?;
    println!("\n{trace}");
    Ok(())
}

fn ensure_round_fits(turns: u32, round_size: usize) -> anyhow::Result<()> {
    let round_size = u32::try_from(round_size)
        .map_err(|_| anyhow::anyhow!("round size exceeds the turn counter"))?;
    if round_size > MAX_TURNS.saturating_sub(turns) {
        anyhow::bail!("next round of {round_size} would exceed {MAX_TURNS} turns");
    }
    Ok(())
}

#[cfg(test)]
#[path = "pe1006_hive/test.rs"]
mod test;

fn instantiated(
    runtime: &Runtime,
    workspace: &Path,
    outbox_dir: &Path,
    problem: &str,
    id: &'static str,
    role: String,
) -> anyhow::Result<AgentBinding> {
    let runtime_id = format!("{id}-pe{problem}-{}", std::process::id());
    let tools = vec![
        "file_read".into(),
        "file_write".into(),
        "mcp_list_tools".into(),
        "mcp_call_tool".into(),
        "shell".into(),
    ];
    let policy = if id == "researcher" {
        RESEARCH_POLICY
    } else {
        SEALED
    };
    let executable = std::env::current_exe()?;
    let mcp = McpServer::stdio(
        "tinyhive",
        executable.to_string_lossy(),
        [
            "--hive-tools".to_string(),
            "--agent".to_string(),
            id.to_string(),
            "--outbox".to_string(),
            outbox_dir.join(format!("{id}.jsonl")).display().to_string(),
        ],
    )
    .allow_tools(["broadcast", "complete_episode"])
    .description("Completion-driven TinyHiveMind episode tools");
    runtime
        .agent(
            AgentSpec::new(runtime_id)
                .system_prompt(format!("{role}\n\n{policy}"))
                .definition(
                    AgentDefinitionSpec::new()
                        .tools(ToolScopeSpec::Named(tools))
                        .disallow_tools(["run_code", "ask_docs"])
                        .max_iterations(12)
                        .temperature(0.0),
                )
                .mcp(mcp)
                .action_dir(workspace),
        )
        .map(|agent| AgentBinding::new(id, agent))
        .map_err(Into::into)
}

async fn inherited_config() -> anyhow::Result<RuntimeConfig> {
    let mut config = RuntimeConfig::load_or_init().await?;
    config.agent.compact_context = true;
    config.agent.max_tool_iterations = 6;
    config.agent.max_history_messages = 64;
    config.default_temperature = 0.0;
    Ok(config)
}

fn memory_services() -> ServiceSet {
    let mut services = ServiceSet::none();
    services.memory_queue = true;
    services.harness_init = true;
    services
}

async fn stage_research_sources(scratch: &Path) -> anyhow::Result<()> {
    let directory = scratch.join("research_sources");
    std::fs::create_dir_all(&directory)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let sources = [
        (
            "rauzy.md",
            "https://raw.githubusercontent.com/senamakel/math-superagent/f0b35053424007d21d71363ce4ed73e0c8baca9e/workspace/project-euler/1006/research/approaches/pe1006-rauzy-block-semidir-product.md",
        ),
        (
            "approaches.md",
            "https://raw.githubusercontent.com/senamakel/math-superagent/f0b35053424007d21d71363ce4ed73e0c8baca9e/workspace/project-euler/1006/derived/APPROACHES.md",
        ),
        (
            "verification.md",
            "https://raw.githubusercontent.com/senamakel/math-superagent/f0b35053424007d21d71363ce4ed73e0c8baca9e/workspace/project-euler/1006/code/out/PE1006-verification.md",
        ),
        (
            "external_approach.md",
            "https://raw.githubusercontent.com/dawei7/code_n/012e178619373894a06afb8db07953df0202a071/dsa/euler/1006_fibonacci-subwords/variants/optimal/approach.md",
        ),
        (
            "external_solution.py",
            "https://raw.githubusercontent.com/dawei7/code_n/012e178619373894a06afb8db07953df0202a071/dsa/euler/1006_fibonacci-subwords/variants/optimal/solutions/solution.py",
        ),
        (
            "external_cases.json",
            "https://raw.githubusercontent.com/dawei7/code_n/012e178619373894a06afb8db07953df0202a071/dsa/euler/1006_fibonacci-subwords/cases.json",
        ),
        (
            "eulersolve_solution.py",
            "https://eulersolve.org/solutionsPython/Euler1006.py",
        ),
        (
            "eulersolve_explanation.html",
            "https://eulersolve.org/problem/1006/",
        ),
        (
            "cirosantilli_1006.md",
            "https://raw.githubusercontent.com/cirosantilli/project-euler-solutions/master/solvers/1006.md",
        ),
    ];
    for (name, url) in sources {
        let target = directory.join(name);
        if target.is_file() {
            println!("using cached public research source: {name}");
            continue;
        }
        let body = match client
            .get(url)
            .send()
            .await
            .and_then(|reply| reply.error_for_status())
        {
            Ok(reply) => reply.text().await?,
            Err(error) if name != "eulersolve_solution.py" => {
                println!("optional research source unavailable: {name} ({error})");
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let source = if name.ends_with(".py") {
            format!("# Source: {url}\n\n{body}")
        } else {
            format!("Source: {url}\n\n{body}")
        };
        std::fs::write(target, source)?;
    }
    stage_authenticated_github_source(
        &directory,
        "candidate_euler1006.py",
        "repos/senamakel/math-agent/contents/workspace/euler1006/code/lean/code/python/euler1006.py?ref=be919bc1bdc6b77a075413192654931b80cae602",
        "https://github.com/senamakel/math-agent/blob/be919bc1bdc6b77a075413192654931b80cae602/workspace/euler1006/code/lean/code/python/euler1006.py",
    )?;
    stage_authenticated_github_source(
        &directory,
        "candidate_context.md",
        "repos/senamakel/math-agent/contents/workspace/euler1006/refs/context.md?ref=be919bc1bdc6b77a075413192654931b80cae602",
        "https://github.com/senamakel/math-agent/blob/be919bc1bdc6b77a075413192654931b80cae602/workspace/euler1006/refs/context.md",
    )?;
    Ok(())
}

fn stage_authenticated_github_source(
    directory: &Path,
    name: &str,
    endpoint: &str,
    source_url: &str,
) -> anyhow::Result<()> {
    let target = directory.join(name);
    if target.is_file() {
        println!("using cached authenticated research source: {name}");
        return Ok(());
    }
    let output = std::process::Command::new("gh")
        .args([
            "api",
            "-H",
            "Accept: application/vnd.github.raw+json",
            endpoint,
        ])
        .output()?;
    if !output.status.success() {
        anyhow::bail!("gh api could not stage {name}");
    }
    let mut body = format!("Source: {source_url}\n\n").into_bytes();
    body.extend(output.stdout);
    std::fs::write(target, body)?;
    Ok(())
}
