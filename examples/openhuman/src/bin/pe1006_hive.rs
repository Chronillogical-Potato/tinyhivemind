//! Web-assisted OpenRouter GPT-OSS OpenHuman hive experiment for Project Euler 1006.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use openhuman_embed::{
    Access, Agent, AgentDefinitionSpec, AgentSpec, Provider, Runtime, RuntimeConfig, ServiceSet,
    ToolScopeSpec, Workspace,
};
use serde_json::json;
use tinyhivemind::responder::Probability;
use tinyhivemind_embed::{
    AgentRegistry, CandidateProbability, ContributionProbability, EvaluationDisposition,
    RoutedAgents, RoutingEvaluation, RoutingPlan,
};
use tokio::time::timeout;
use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[path = "pe1006_hive/workspace.rs"]
mod workspace_support;

use workspace_support::{TurnSnapshots, hive_workspace, initialize_workspace};

const MODEL: &str = "openai/gpt-oss-120b:nitro";
const PROVIDER_BASE: &str = "https://openrouter.ai/api/v1";
const TURN_TIMEOUT: Duration = Duration::from_secs(600);
const TASK: &str = r#"Starting with two strings S_0 = 0 and S_1 = 01, define S_n as the
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
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_stack_size(16 * 1024 * 1024)
        .build()?;
    runtime.block_on(run())
}

async fn run() -> anyhow::Result<()> {
    let api_key = std::env::var("OPENROUTER_API_KEY")?;
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
    initialize_workspace(&workspace)?;
    std::fs::create_dir_all(&run_dir)?;
    stage_research_sources(&workspace).await?;

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
            "theory",
            "You are the Fibonacci-word combinatorics specialist. Derive exact structure and logarithmic formulas; test every claimed identity on small k.",
        )?,
        instantiated(
            &runtime,
            &workspace,
            "solver",
            "You are the implementation specialist. Turn proven formulas into exact modular code, run it, and report reproducible commands and residues.",
        )?,
        instantiated(
            &runtime,
            &workspace,
            "checker",
            "You are the adversarial verifier. Independently reproduce samples, attack extrapolations, and sign only an exact candidate supported by code.",
        )?,
        instantiated(
            &runtime,
            &workspace,
            "lead",
            "You coordinate the desk. Reconcile disagreements, demand missing evidence, and state a final residue only after checker sign-off.",
        )?,
        instantiated(
            &runtime,
            &workspace,
            "researcher",
            "You are the web researcher. Locate public derivations, implementations, or corroborating results and report exact URLs plus the useful mathematical steps.",
        )?,
    ])?;
    let route = hive_plan();
    let RoutedAgents::Hive { primary, invited } = agents.resolve(&route)? else {
        anyhow::bail!("sealed route did not open a hive")
    };
    println!("runtime_agents: {}", runtime.agent_ids().join(","));
    println!(
        "hive_route: primary={} invited={}",
        primary.id,
        invited
            .iter()
            .map(|seat| seat.id)
            .collect::<Vec<_>>()
            .join(",")
    );
    println!("model: {MODEL}");
    println!("memory_driver: tinycortex");
    println!("workspace: {}", workspace.display());
    println!("run_dir: {}", run_dir.display());

    let mut transcript = Vec::new();
    let mut visibility = Visibility::default();
    let mut snapshots = TurnSnapshots::new(&run_dir)?;

    run_and_append(
        &agents,
        "researcher",
        &mut transcript,
        &mut visibility,
        &mut snapshots,
        &format!("Research first: inspect the public documents mirrored under `research_sources/` and cite their original URLs. Then use authenticated `gh search repos 'project euler answers'` and inspect problem-1006 paths or answer indexes without searching for a known residue. Extract an exact method or independently reported result; do not merely quote a number.\n\n{RESEARCH_START}"),
    )
    .await?;

    // Run against one provider connection at a time while keeping the round
    // blind between specialists: each sees the same researcher evidence, but
    // none of their own results enters the transcript until all three return.
    let blind = [
        (
            "theory",
            seat_turn(
            agents
                .get("theory")
                .ok_or_else(|| anyhow::anyhow!("missing theory"))?,
            "theory",
            &transcript,
            &visibility,
            &mut snapshots,
            "Blind round: independently derive the mathematical structure needed for an exact O(polylog k) solution. Write theory_* files only.",
        )
        .await?,
        ),
        (
            "solver",
            seat_turn(
            agents
                .get("solver")
                .ok_or_else(|| anyhow::anyhow!("missing solver"))?,
            "solver",
            &transcript,
            &visibility,
            &mut snapshots,
            "Blind round: independently search for an exact fast algorithm and implement brute-force sample oracles. Write solver_* files only.",
        )
        .await?,
        ),
        (
            "checker",
            seat_turn(
            agents
                .get("checker")
                .ok_or_else(|| anyhow::anyhow!("missing checker"))?,
            "checker",
            &transcript,
            &visibility,
            &mut snapshots,
            "Blind round: independently reproduce both supplied samples and identify proof obligations any huge-k method must meet. Write checker_* files only.",
        )
        .await?,
        ),
    ];
    append_round(&mut transcript, &mut visibility, blind);

    run_and_append(
        &agents,
        "lead",
        &mut transcript,
        &mut visibility,
        &mut snapshots,
        "Read the blind round. Produce a concrete reconciliation: accepted facts, rejected shortcuts, and one sharply scoped next task for each specialist.",
    )
    .await?;

    let revealed_snapshot = transcript.clone();
    let revealed = [
        (
            "theory",
            seat_turn(
            agents
                .get("theory")
                .ok_or_else(|| anyhow::anyhow!("missing theory"))?,
            "theory",
            &revealed_snapshot,
            &visibility,
            &mut snapshots,
            "Revealed round: address the lead's questions and peer evidence. Derive the missing arbitrary-k bridge exactly; reject any empirical recurrence without proof.",
        )
        .await?,
        ),
        (
            "solver",
            seat_turn(
            agents
                .get("solver")
                .ok_or_else(|| anyhow::anyhow!("missing solver"))?,
            "solver",
            &revealed_snapshot,
            &visibility,
            &mut snapshots,
            "Revealed round: inspect the newly staged public PE1006 explanation and Python implementation. Reimplement or audit the compressed-word method, run its checkpoints plus an independent brute-force comparison, and compute a candidate only if the algorithm reaches 10^18 exactly.",
        )
        .await?,
        ),
        (
            "checker",
            seat_turn(
            agents
                .get("checker")
                .ok_or_else(|| anyhow::anyhow!("missing checker"))?,
            "checker",
            &revealed_snapshot,
            &visibility,
            &mut snapshots,
            "Revealed round: run peers' code independently, find counterexamples, and specify what remains before sign-off.",
        )
        .await?,
        ),
    ];
    append_round(&mut transcript, &mut visibility, revealed);

    run_and_append(
        &agents,
        "lead",
        &mut transcript,
        &mut visibility,
        &mut snapshots,
        "Synthesize a candidate solution from the revealed round. If evidence is insufficient, assign exactly one repair task instead of guessing. If sufficient, state the residue and ask checker for final sign-off.",
    )
    .await?;
    run_and_append(
        &agents,
        "checker",
        &mut transcript,
        &mut visibility,
        &mut snapshots,
        "Final audit: independently run the decisive code and inspect the derivation. You must execute `python3 research_sources/eulersolve_solution.py` and compare it with an independently written brute-force oracle. Begin with SIGNED or REFUSED. Matching k=3 and k=10 is necessary but not sufficient. SIGNED requires understanding the compressed-word transitions, exact agreement with brute force for at least every k=1..50, a canonical residue in 0..101001000, and the exact command/file output you observed.",
    )
    .await?;
    run_and_append(
        &agents,
        "lead",
        &mut transcript,
        &mut visibility,
        &mut snapshots,
        "Close the run. If checker signed, state the exact answer and minimal evidence chain. If checker refused, say unsolved and name the precise blocker; do not guess.",
    )
    .await?;

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
    id: &'static str,
    role: &str,
) -> Result<(&'static str, Agent), openhuman_embed::AgentError> {
    let runtime_id = format!("{id}-pe1006-{}", std::process::id());
    let mut tools = vec!["file_read".into(), "file_write".into()];
    if matches!(id, "solver" | "checker" | "researcher") {
        tools.push("shell".into());
    }
    let policy = if id == "researcher" {
        RESEARCH_POLICY
    } else {
        SEALED
    };
    runtime
        .agent(
            AgentSpec::new(runtime_id)
                .system_prompt(format!("{role}\n\n{policy}"))
                .definition(
                    AgentDefinitionSpec::new()
                        .tools(ToolScopeSpec::Named(tools))
                        .max_iterations(if matches!(id, "solver" | "checker" | "researcher") {
                            6
                        } else {
                            4
                        })
                        .temperature(0.0),
                )
                .action_dir(workspace),
        )
        .map(|agent| (id, agent))
}

async fn seat_turn(
    agent: &Agent,
    id: &str,
    transcript: &[DeskMessage],
    visibility: &Visibility,
    snapshots: &mut TurnSnapshots,
    assignment: &str,
) -> anyhow::Result<String> {
    let first = visibility.seen.get(id).is_none_or(BTreeSet::is_empty);
    let delta = visibility.delta(id, transcript);
    let policy = if id == "researcher" {
        RESEARCH_POLICY
    } else {
        SEALED
    };
    let prompt = format!(
        "{}{}\n\n## New desk messages\n{}\n\n## This turn\n{}\n\nThe durable shared workspace is `{}`. Read `AGENTS.md` and `MEMORY.md` before working. Write role-prefixed artifacts there and update `MEMORY.md` only with reproduced, evidence-linked learnings. Do not write or read `/tmp/openhuman` or any other directory. Return one evidence-dense desk message; do not narrate tool use.",
        if first {
            format!(
                "## Official statement\n{TASK}\n\n## Rejected prior experiment\n{PRIOR_FAILURE}\n\n"
            )
        } else {
            String::new()
        },
        policy,
        if delta.is_empty() { "(none)" } else { &delta },
        assignment,
        agent.action_dir().display(),
    );
    let session_id = format!("tinyhivemind-pe1006-run-{}:{id}", std::process::id());
    let snapshot = snapshots.begin(id, agent.id(), &session_id, &prompt)?;
    let outcome = timeout(TURN_TIMEOUT, agent.turn(prompt).session(&session_id).send())
        .await
        .map_err(|_| anyhow::anyhow!("@{id} timed out"))??;
    snapshots.complete(snapshot, &outcome.reply)?;
    println!(
        "[completed] @{id}: {}",
        outcome.reply.chars().take(500).collect::<String>()
    );
    Ok(outcome.reply)
}

fn append_round<const N: usize>(
    transcript: &mut Vec<DeskMessage>,
    visibility: &mut Visibility,
    rows: [(&str, String); N],
) {
    let delivered = transcript.len();
    for (id, _) in &rows {
        visibility.mark_delivered(id, delivered);
    }
    for (id, body) in rows {
        let index = transcript.len();
        transcript.push(DeskMessage {
            author: id.to_string(),
            body,
        });
        visibility.mark_own(id, index);
    }
}

async fn run_and_append(
    agents: &AgentRegistry<Agent>,
    id: &str,
    transcript: &mut Vec<DeskMessage>,
    visibility: &mut Visibility,
    snapshots: &mut TurnSnapshots,
    assignment: &str,
) -> anyhow::Result<()> {
    let agent = agents
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("missing {id}"))?;
    let body = seat_turn(agent, id, transcript, visibility, snapshots, assignment).await?;
    visibility.mark_delivered(id, transcript.len());
    let index = transcript.len();
    transcript.push(DeskMessage {
        author: id.to_string(),
        body,
    });
    visibility.mark_own(id, index);
    Ok(())
}

fn hive_plan() -> RoutingPlan {
    let p = |parts| Probability::new(parts).expect("fixture probability is bounded");
    RoutingPlan::Hive {
        primary_id: "lead".into(),
        invited_ids: vec![
            "theory".into(),
            "solver".into(),
            "checker".into(),
            "researcher".into(),
        ],
        evaluation: RoutingEvaluation {
            primary_responder: "lead".into(),
            primary_probabilities: vec![
                CandidateProbability {
                    candidate_id: "lead".into(),
                    probability: p(600_000),
                },
                CandidateProbability {
                    candidate_id: "theory".into(),
                    probability: p(100_000),
                },
                CandidateProbability {
                    candidate_id: "solver".into(),
                    probability: p(100_000),
                },
                CandidateProbability {
                    candidate_id: "checker".into(),
                    probability: p(100_000),
                },
                CandidateProbability {
                    candidate_id: "researcher".into(),
                    probability: p(100_000),
                },
                CandidateProbability {
                    candidate_id: "none".into(),
                    probability: Probability::ZERO,
                },
            ],
            confidence: p(900_000),
            needs_collaboration: p(950_000),
            needs_clarification: Probability::ZERO,
            contributions: ["lead", "theory", "solver", "checker", "researcher"]
                .into_iter()
                .map(|id| ContributionProbability {
                    candidate_id: id.into(),
                    probability: p(900_000),
                })
                .collect(),
            high_impact: p(500_000),
            model_identity: "sealed-fixture".into(),
            question_schema_version: 1,
            roster_version: 1,
            disposition: EvaluationDisposition::Accepted,
        },
    }
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
        let body = reqwest::get(url).await?.error_for_status()?.text().await?;
        let source = if name.ends_with(".py") {
            format!("# Source: {url}\n\n{body}")
        } else {
            format!("Source: {url}\n\n{body}")
        };
        std::fs::write(directory.join(name), source)?;
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
    std::fs::write(directory.join(name), body)?;
    Ok(())
}
