//! Web-assisted OpenRouter GPT-OSS OpenHuman hive experiment for Project Euler 1006.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use openhuman_embed::{
    Access, Agent, AgentDefinitionSpec, AgentSpec, McpServer, Provider, Runtime, RuntimeConfig,
    ServiceSet, ToolScopeSpec, Workspace,
};
use serde_json::json;
use tinyhivemind_embed::{AgentRegistry, RoutingSource, route_broadcast, route_message};
use tinyhivemind_hive::{
    CompletionEpisodeState, CompletionStep, ParticipantCompletion, apply_assignment,
    apply_completion, completion_status,
};
use tinyhivemind_typesafe::JevRouter;
use tokio::time::timeout;
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[path = "pe1006_hive/episode.rs"]
mod episode_support;
#[path = "pe1006_hive/tools.rs"]
mod hive_tools;
#[path = "pe1006_hive/typesafe.rs"]
mod typesafe_support;
#[path = "pe1006_hive/workspace.rs"]
mod workspace_support;

use episode_support::{
    completion_assignment, deterministic_broadcast_fallback, enqueue, recover_tool_call,
    role_prompt, route_candidates, routed_ids,
};
use workspace_support::{TurnSnapshots, hive_workspace, initialize_workspace};

const MODEL: &str = "openai/gpt-oss-120b:nitro";
const PROVIDER_BASE: &str = "https://openrouter.ai/api/v1";
const TURN_TIMEOUT: Duration = Duration::from_secs(600);
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
    let agents = AgentRegistry::new([
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
    ])?;
    let router = JevRouter::new(typesafe_support::Transport::new(typesafe_api_key)?);
    let mut roster_version = 1_u64;
    let initial_request = typesafe_support::request(
        task,
        RoutingSource::DeskMessage,
        route_candidates(&problem, None),
        roster_version,
        &problem,
    );
    let route = route_message(Some(&router), None, &initial_request, None, "lead").await;
    let mut selected = routed_ids(&route);
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
    let team = ["theory", "solver", "checker", "lead", "researcher"];
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
    let mut queue: VecDeque<String> = selected.into();
    let mut route_trace = vec![route];
    let mut missed_tools: BTreeMap<String, u8> = BTreeMap::new();
    let mut turns = 0_u32;

    while !matches!(completion_status(&episode), CompletionStep::Complete { .. }) && turns < 25 {
        let Some(id) = queue.pop_front() else {
            anyhow::bail!("completion episode has pending work but no scheduled agent")
        };
        turns += 1;
        let agent = agents
            .get(&id)
            .ok_or_else(|| anyhow::anyhow!("missing {id}"))?;
        let outbox = outbox_dir.join(format!("{id}.jsonl"));
        let turn = seat_turn(
            agent,
            &id,
            TurnContext {
                transcript: &transcript,
                visibility: &visibility,
                snapshots: &mut snapshots,
                outbox: &outbox,
                assignment: &completion_assignment(&problem, &id),
                task,
                prior_failure,
                problem: &problem,
            },
        )
        .await?;
        visibility.mark_delivered(&id, transcript.len());
        let mut utterances = turn.utterances;
        if utterances.is_empty()
            && let Some(recovered) = recover_tool_call(&turn.reply)
        {
            println!("[compatibility-recovered-tool-call] @{id}");
            utterances.push(recovered);
        }
        if utterances.is_empty() {
            let misses = missed_tools.entry(id.clone()).or_default();
            *misses = misses.saturating_add(1);
            if *misses >= 4 {
                anyhow::bail!("@{id} four times failed to call a TinyHiveMind tool")
            }
            println!("[no-hive-tool] @{id}; rescheduling once");
            enqueue(&mut queue, &id);
            continue;
        }
        missed_tools.remove(&id);
        for utterance in utterances {
            sequence = sequence.saturating_add(1);
            match utterance {
                tinyhivemind::speech::Utterance::Broadcast { message } => {
                    let index = transcript.len();
                    transcript.push(DeskMessage {
                        author: id.clone(),
                        body: format!("BROADCAST: {message}"),
                    });
                    visibility.mark_own(&id, index);
                    roster_version = roster_version.saturating_add(1);
                    let request = typesafe_support::request(
                        &message,
                        RoutingSource::AgentBroadcast {
                            author_id: id.clone(),
                        },
                        route_candidates(&problem, Some(&id)),
                        roster_version,
                        &problem,
                    );
                    let plan = route_broadcast(Some(&router), None, &request, "lead").await;
                    let mut recipients = routed_ids(&plan);
                    if recipients.is_empty() {
                        recipients.push(deterministic_broadcast_fallback(&id).into());
                    }
                    println!("[broadcast] @{id} -> {}", recipients.join(","));
                    route_trace.push(plan);
                    if !recipients.is_empty() {
                        episode = apply_assignment(
                            &episode,
                            recipients.iter().map(String::as_str),
                            tinyhivemind::Sequence(sequence),
                        )?;
                        for recipient in recipients {
                            enqueue(&mut queue, &recipient);
                        }
                    }
                    enqueue(&mut queue, &id);
                }
                tinyhivemind::speech::Utterance::CompleteEpisode { message } => {
                    let index = transcript.len();
                    transcript.push(DeskMessage {
                        author: id.clone(),
                        body: format!("COMPLETE: {message}"),
                    });
                    visibility.mark_own(&id, index);
                    episode = apply_completion(&episode, &id, tinyhivemind::Sequence(sequence))?;
                    println!("[complete_episode] @{id}");
                }
                tinyhivemind::speech::Utterance::Post { .. }
                | tinyhivemind::speech::Utterance::Dm { .. } => {
                    anyhow::bail!("MCP completion surface emitted an unsupported utterance")
                }
            }
        }
    }
    std::fs::write(
        run_dir.join("routing-trace.json"),
        serde_json::to_vec_pretty(&route_trace)?,
    )?;
    std::fs::write(
        run_dir.join("completion-state.json"),
        serde_json::to_vec_pretty(&episode)?,
    )?;
    println!("completion_status: {:?}", completion_status(&episode));

    let trace = transcript
        .iter()
        .map(|row| format!("## @{}\n\n{}", row.author, row.body))
        .collect::<Vec<_>>()
        .join("\n\n");
    std::fs::write(run_dir.join("HIVE_TRANSCRIPT.md"), &trace)?;
    println!("\n{trace}");
    Ok(())
}

fn instantiated(
    runtime: &Runtime,
    workspace: &Path,
    outbox_dir: &Path,
    problem: &str,
    id: &'static str,
    role: String,
) -> anyhow::Result<(&'static str, Agent)> {
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
        .map(|agent| (id, agent))
        .map_err(Into::into)
}

async fn seat_turn(agent: &Agent, id: &str, context: TurnContext<'_>) -> anyhow::Result<SeatTurn> {
    hive_tools::clear(context.outbox)?;
    let first = context
        .visibility
        .seen
        .get(id)
        .is_none_or(BTreeSet::is_empty);
    let delta = context.visibility.delta(id, context.transcript);
    let policy = if id == "researcher" {
        RESEARCH_POLICY
    } else {
        SEALED
    };
    let prompt = format!(
        "{}{}\n\n## New desk messages\n{}\n\n## This assignment\n{}\n\nThe durable shared workspace is `{}`. Read `AGENTS.md` and `MEMORY.md` before working. Write role-prefixed artifacts there and update `MEMORY.md` only with reproduced, evidence-linked learnings. Do not write or read `/tmp/openhuman` or any other directory.\n\nYou MUST end this turn with exactly one TinyHiveMind action through `mcp_call_tool` on server `tinyhive`: call remote tool `broadcast` with a self-contained message when another teammate should take work, or `complete_episode` with your evidence-dense final result when your assignment is done. First use `mcp_list_tools` if needed. Text outside that MCP call is private thinking and is not delivered to the team.",
        if first {
            format!(
                "## Official statement\n{}\n\n## Prior experiment status\n{}\n\n",
                context.task, context.prior_failure
            )
        } else {
            String::new()
        },
        policy,
        if delta.is_empty() { "(none)" } else { &delta },
        context.assignment,
        agent.action_dir().display(),
    );
    let session_id = format!(
        "tinyhivemind-pe{}-run-{}:{id}",
        context.problem,
        std::process::id()
    );
    let snapshot = context
        .snapshots
        .begin(id, agent.id(), &session_id, &prompt)?;
    let outcome = timeout(TURN_TIMEOUT, agent.turn(prompt).session(&session_id).send())
        .await
        .map_err(|_| anyhow::anyhow!("@{id} timed out"))??;
    context.snapshots.complete(snapshot, &outcome.reply)?;
    println!(
        "[completed] @{id}: {}",
        outcome.reply.chars().take(500).collect::<String>()
    );
    Ok(SeatTurn {
        reply: outcome.reply,
        utterances: hive_tools::drain(context.outbox)?,
    })
}

struct TurnContext<'a> {
    transcript: &'a [DeskMessage],
    visibility: &'a Visibility,
    snapshots: &'a mut TurnSnapshots,
    outbox: &'a Path,
    assignment: &'a str,
    task: &'a str,
    prior_failure: &'a str,
    problem: &'a str,
}

struct SeatTurn {
    reply: String,
    utterances: Vec<tinyhivemind::speech::Utterance>,
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
