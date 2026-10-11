//! What a run leaves behind: the viewer's `structure.json`, each turn's
//! prompt and reply, the closing frames, the memory counts read back from
//! CortexDB, and `report.json` / `REPORT.md`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use serde_json::json;
use tinymemory_api::MemoryEngine;

use super::artifacts::RunDir;
use super::budget::Budget;
use super::floors::Federation;
use super::frames::{Frame, UsageFrame};
use super::memory::count_at;
use super::sandbox::PythonSandbox;
use super::search::{Provider, WebSearch};
use super::turn::{Seat, TurnOutput};

/// Write `structure.json`: exactly what `hive_view <package> --json` prints,
/// produced by running `hive_view` itself from the workspace root.
pub(super) fn structure(package: &Path, run: &RunDir) {
    let root = package
        .ancestors()
        .find(|dir| dir.join("crates").is_dir() && dir.join("Cargo.toml").is_file());
    let Some(root) = root else {
        eprintln!("[structure] no workspace root above {}", package.display());
        return;
    };
    let output = std::process::Command::new("cargo")
        .args([
            "run",
            "--quiet",
            "-p",
            "tinyhivemind-lang",
            "--example",
            "hive_view",
            "--",
        ])
        .arg(package)
        .arg("--json")
        .current_dir(root)
        .output();
    match output {
        Ok(output) if output.status.success() => run.write("structure.json", output.stdout),
        Ok(output) => eprintln!(
            "[structure] hive_view failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ),
        Err(error) => eprintln!("[structure] could not run hive_view: {error}"),
    }
}

/// `--probe`: one call through each live dependency, and no model call.
///
/// # Errors
///
/// Returns the first dependency that refuses.
pub(super) async fn probe(
    search: &WebSearch,
    sandbox: &PythonSandbox,
    engine: &dyn MemoryEngine,
    run: &RunDir,
    root: &str,
    run_id: &str,
) -> anyhow::Result<()> {
    for provider in [
        Provider::Exa,
        Provider::Gemini,
        Provider::Parallel,
        Provider::Tinyfish,
    ] {
        match search
            .search("Steiner system S(5,8,24) Witt design", provider)
            .await
        {
            Ok(found) => {
                println!(
                    "probe search {provider:?}: ok, {} sources, {} chars",
                    found.urls.len(),
                    found.text.len()
                );
                run.json(&format!("probe-{provider:?}.json"), &found);
            }
            Err(error) => println!("probe search {provider:?}: FAILED {error}"),
        }
    }
    let name = format!("euler-probe-{}", run_id.to_lowercase());
    let output = sandbox.run(
        "import socket\ntry:\n    socket.create_connection(('1.1.1.1', 53), 3)\n    print('network: reachable')\nexcept OSError as e:\n    print('network: blocked', type(e).__name__)\nimport os\nprint('mounted:', sorted(os.listdir('/')))\nprint(sum(range(10)))\n",
        &name,
    )?;
    println!(
        "probe sandbox: code={:?} stdout={:?}",
        output.code, output.stdout
    );
    let at = format!("{root}/ws:probe-{}/agent:probe", run_id.to_lowercase());
    let meta = tinymemory_api::MemoryMeta {
        namespace: at.parse()?,
        agent_id: Some("probe".into()),
        ..tinymemory_api::MemoryMeta::from_source(tinymemory_api::SourceKind::Agent, None)
    };
    let receipt = engine
        .store(tinymemory_api::StoreItem::learning(
            "probe: the euler hive can write to CortexDB",
            tinymemory_api::LearningKind::Fact,
            1.0,
            meta,
        ))
        .await?;
    println!("probe memory store: {receipt:?}");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    println!(
        "probe memory count at {at}: {:?}",
        count_at(engine, &at).await?
    );
    for (label, kinds) in [
        ("learnings", vec![tinymemory_api::ItemKind::Learning]),
        ("any kind", vec![]),
    ] {
        let mut request = tinymemory_api::FetchRequest::new(
            "can the euler hive write to CortexDB",
            tinymemory_api::FetchMode::Hybrid,
            5,
        );
        request.filter = tinymemory_api::MetaFilter {
            reach: Some(tinymemory_api::Reach::exact(at.parse()?)),
            kinds,
            ..tinymemory_api::MetaFilter::default()
        };
        match engine.fetch(request).await {
            Ok(page) => println!("probe memory fetch ({label}): {} hits", page.hits.len()),
            Err(error) => println!("probe memory fetch ({label}): FAILED {error}"),
        }
    }
    run.write("probe.txt", "probe complete\n");
    Ok(())
}

/// Save one turn's prompt, reply and tool calls.
pub(super) fn turn_artifacts(run: &RunDir, output: &TurnOutput) {
    run.event(&json!({
        "event": "turn",
        "seat": output.seat,
        "moment": output.moment,
        "action": output.action,
        "recovered": output.recovered,
        "calls": output.calls,
        "recalled": output.recalled,
        "remembered": output.remembered,
        "model_calls": output.model_calls,
        "input_tokens": output.input_tokens,
        "output_tokens": output.output_tokens,
        "cost_usd": output.cost_usd,
        "error": output.error,
    }));
    let number = run.next_turn();
    run.write(
        &format!(
            "turns/{number:04}-{}.md",
            super::artifacts::file_component(&output.seat)
        ),
        format!(
            "# @{} ({})\n\n## Prompt\n\n{}\n\n## Reply\n\n{}\n\n## Action\n\n```json\n{}\n```\n",
            output.seat,
            output.moment,
            output.prompt,
            output.reply,
            serde_json::to_string_pretty(&output.action).unwrap_or_default()
        ),
    );
}

/// The run's spend as a `usage` frame.
pub(super) fn usage(budget: &Budget) -> UsageFrame {
    let input = budget.input_tokens.load(Ordering::Relaxed);
    let output = budget.output_tokens.load(Ordering::Relaxed);
    let calls = budget.model_calls.load(Ordering::Relaxed);
    UsageFrame {
        calls,
        prompt_tokens: input,
        completion_tokens: output,
        total_tokens: input + output,
        cost_usd: budget.spend_usd(),
        wall_ms: u64::try_from(budget.elapsed().as_millis()).unwrap_or(u64::MAX),
        exhausted_calls: calls >= budget.limits.max_model_calls,
    }
}

/// The run's ending: `converged` only for a committed candidate.
pub(super) fn ending_of(decided: bool, undecided: bool, stop: Option<&str>) -> String {
    match (decided, undecided, stop) {
        (true, ..) => "converged".into(),
        (false, true, _) => "no_decision".into(),
        (false, false, Some(stop)) => stop.into(),
        (false, false, None) => "idle".into(),
    }
}

/// What closing a run reads.
pub(super) struct Closing<'a> {
    pub(super) run: &'a RunDir,
    pub(super) federation: &'a Federation,
    pub(super) seats: &'a BTreeMap<String, Arc<Seat>>,
    pub(super) budget: &'a Budget,
    pub(super) engine: &'a dyn MemoryEngine,
    pub(super) pool: Option<&'a str>,
    pub(super) stop: Option<String>,
    pub(super) model: &'a str,
}

/// Close the run: desk and hive endings, the store read back, and the report.
///
/// # Errors
///
/// Returns an engine failure while counting, or the first closing artifact
/// that could not be written.
pub(super) async fn finish(closing: Closing<'_>) -> anyhow::Result<()> {
    let Closing {
        run,
        federation,
        seats,
        budget,
        engine,
        pool,
        stop,
        model,
    } = closing;
    for floor in &federation.floors {
        let ending = if floor.reported {
            "converged"
        } else if floor.opened {
            "exhausted"
        } else {
            "unopened"
        };
        run.frame(&Frame::DeskEnd {
            desk: floor.desk.id.clone(),
            ending: ending.into(),
            topic: floor.last_candidate.clone(),
            correct: None,
            rounds: floor.rounds,
            turns: floor.turns,
        });
    }
    run.frame(&Frame::Usage(usage(budget)));
    let answer = federation.finished.as_ref();
    let ending = ending_of(
        answer.is_some(),
        federation.undecided.is_some(),
        stop.as_deref(),
    );
    run.frame(&Frame::End {
        ending: ending.clone(),
        topic: answer.and_then(|answer| answer.candidate.clone()),
        correct: None,
        rounds: federation.round,
        turns: federation.floors.iter().map(|floor| floor.turns).sum(),
        desk: answer.map(|answer| answer.desk.clone()),
    });
    // Read the store back: what CortexDB actually holds for each seat.
    // Optional store inspection shares the run's deadline; never force a
    // five-second tail after the execution budget has already expired.
    let settled = !budget.turn_timeout().is_zero()
        && super::turn::within(
            budget.turn_timeout(),
            tokio::time::sleep(std::time::Duration::from_secs(5)),
        )
        .await
        .is_some();
    let mut memory = serde_json::Map::new();
    for (id, seat) in seats {
        let namespace = seat.memory.binding().namespace.clone();
        let stored = if settled {
            match super::turn::within(budget.turn_timeout(), count_at(engine, &namespace)).await {
                Some(Ok(counts)) => json!(counts),
                Some(Err(error)) => json!({"error": error.to_string()}),
                None => json!({"skipped": "wall-clock deadline"}),
            }
        } else {
            json!({"skipped": "wall-clock deadline"})
        };
        // Read the binding's reaches once more, after indexing has settled:
        // what a seat joining now would recall.
        let host = seat.memory.ledger.snapshot();
        let recall_after_run = if settled {
            match super::turn::within(
                budget.turn_timeout(),
                seat.memory.search("verified findings and outcomes"),
            )
            .await
            {
                Some(Ok(notes)) => json!(notes.iter().map(|note| note.lines.len()).sum::<usize>()),
                Some(Err(error)) => json!({"error": error.to_string()}),
                None => json!({"skipped": "wall-clock deadline"}),
            }
        } else {
            json!({"skipped": "wall-clock deadline"})
        };
        memory.insert(
            id.clone(),
            json!({"namespace": namespace, "host": host, "stored": stored, "recall_after_run": recall_after_run, "last_recall": seat.memory.seen()}),
        );
    }
    let pool_counts = match pool {
        Some(pool) if settled => {
            match super::turn::within(budget.turn_timeout(), count_at(engine, pool)).await {
                Some(Ok(counts)) => json!({"namespace": pool, "stored": counts}),
                Some(Err(error)) => json!({"namespace": pool, "error": error.to_string()}),
                None => json!({"namespace": pool, "skipped": "wall-clock deadline"}),
            }
        }
        Some(pool) => json!({"namespace": pool, "skipped": "wall-clock deadline"}),
        None => json!(null),
    };
    let report = json!({
        "ending": ending,
        "model": model,
        "answer": answer,
        "budget": budget.snapshot(),
        "spend_usd": budget.spend_usd(),
        "desks": federation.floors.iter().map(|floor| json!({
            "desk": floor.desk.id, "rounds": floor.rounds, "turns": floor.turns,
            "reported": floor.reported, "last_candidate": floor.last_candidate,
        })).collect::<Vec<_>>(),
        "referrals": federation.referrals,
        "memory": memory,
        "verified_pool": pool_counts,
    });
    run.json("report.json", &report);
    let transcript: String = federation
        .floors
        .iter()
        .map(|floor| {
            let rows: String = floor
                .journal
                .iter()
                .map(|row| {
                    format!(
                        "### [{}] @{} ({})\n\n{}\n\n",
                        row.sequence, row.author, row.kind, row.body
                    )
                })
                .collect();
            format!("## #{} ({})\n\n{rows}", floor.desk.id, floor.desk.name)
        })
        .collect();
    run.write("TRANSCRIPT.md", transcript);
    // The closing artifacts are written after the last per-round check; a
    // failed one must not leave a run that exits cleanly with a truncated end.
    run.check()?;
    println!("\n== {ending} ==");
    if let Some(answer) = answer {
        println!("committed by @{} on #{}", answer.seat, answer.desk);
        println!(
            "candidate: {}",
            answer.candidate.as_deref().unwrap_or("(none)")
        );
    }
    println!("{}", serde_json::to_string_pretty(&report["budget"])?);
    Ok(())
}
