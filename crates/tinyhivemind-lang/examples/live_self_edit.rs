//! Live model editing and host-scored held-out evaluation, explicitly opt-in.
//! No network calls occur in library code. This example host invokes curl.
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    error::Error,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
};
use tinyhivemind_lang::{
    Package, Review, Role, Rule, Seat, check,
    lineage::{self, Direction, ObjectiveScore, Record, Verdict, VersionMetadata},
    lower, parse,
    patch::{Patch, PatchClass},
    validate,
};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Files = BTreeMap<String, String>;
const PROMPT: &str =
    "Sum all invoice amounts, including canceled invoices. Return JSON with a numeric total field.";

struct Provider {
    key: String,
    model: String,
    output: PathBuf,
    calls: u64,
    tokens: u64,
}
impl Provider {
    /// Invoke the provider with credentials on stdin and retain call evidence.
    fn complete(&mut self, system: &str, user: &str) -> Result<Value> {
        self.calls += 1;
        let body = json!({"model": self.model, "temperature": 0,
            "max_tokens": 1200, "response_format": {"type": "json_object"},
            "messages": [{"role":"system","content":system},{"role":"user","content":user}]});
        let request = self.output.join(format!("request-{}.json", self.calls));
        std::fs::write(&request, serde_json::to_vec_pretty(&body)?)?;
        let mut child = Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--fail-with-body",
                "--max-time",
                "120",
                "--config",
                "-",
                "--header",
                "Content-Type: application/json",
                "--data-binary",
            ])
            .arg(format!("@{}", request.display()))
            .arg("https://openrouter.ai/api/v1/chat/completions")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let mut stdin = child.stdin.take().ok_or("curl stdin unavailable")?;
        // Credentials travel on stdin, never through command arguments or artifacts.
        writeln!(stdin, "header = \"Authorization: Bearer {}\"", self.key)?;
        drop(stdin);
        let response = child.wait_with_output()?;
        self.decode_response(
            response.status.success(),
            &response.status.to_string(),
            &response.stdout,
            &response.stderr,
        )
    }

    /// Retain redacted failed-call diagnostics or decode a successful JSON answer.
    fn decode_response(
        &mut self,
        success: bool,
        status: &str,
        stdout: &[u8],
        stderr: &[u8],
    ) -> Result<Value> {
        if !success {
            let body = String::from_utf8_lossy(stdout).replace(&self.key, "[redacted]");
            std::fs::write(
                self.output.join(format!("response-{}.error", self.calls)),
                body,
            )?;
            let diagnostic: String = String::from_utf8_lossy(stderr)
                .replace(&self.key, "[redacted]")
                .trim()
                .chars()
                .take(240)
                .collect();
            return Err(format!(
                "provider transport failed on call {} ({}): {}",
                self.calls, status, diagnostic
            )
            .into());
        }
        let wire: Value = serde_json::from_slice(stdout)?;
        std::fs::write(
            self.output.join(format!("response-{}.json", self.calls)),
            serde_json::to_vec_pretty(&wire)?,
        )?;
        self.tokens += wire["usage"]["total_tokens"].as_u64().unwrap_or(0);
        let text = wire["choices"][0]["message"]["content"]
            .as_str()
            .ok_or("missing model content")?;
        Ok(serde_json::from_str(text)?)
    }
}

/// Build and parse the deliberately defective incumbent package.
fn package() -> Result<Package> {
    let mut p = Package::default();
    p.manifest.roles.push(Role {
        id: "solver".into(),
        charter: "roles/solver.md".into(),
    });
    p.manifest.seats.push(Seat {
        id: "solver".into(),
        label: "Invoice solver".into(),
        role: "solver".into(),
        prompt: "seats/solver.md".into(),
        template: "live-invoice".into(),
        ..Seat::default()
    });
    p.documents.insert(
        "roles/solver.md".into(),
        "Calculate net paid revenue accurately.".into(),
    );
    p.documents.insert("seats/solver.md".into(), PROMPT.into());
    p.constitution.permitted_edits =
        vec![PatchClass::Prompt, PatchClass::Memory, PatchClass::Roster];
    p.constitution.evaluation = json!({"rubric":"net-paid-revenue-v1"});
    p.constitution.acceptance = json!({"min_correct":3});
    p.constitution.telemetry = json!({"enabled":true});
    p.constitution.contamination_canaries = vec!["HELDOUT-CANARY-LIVE-2026".into()];
    p.manifest.seats[0].config = json!({"telemetry":{"enabled":true}});
    validate(&p)?;
    // Round-trip through the public package parser before a model uses it.
    Ok(parse(&files(&p)?)?)
}

/// A package as the files an editor reads and rewrites.
fn files(p: &Package) -> Result<Files> {
    let mut docs = p.documents.clone();
    docs.insert("hive.json".into(), serde_json::to_string_pretty(&p.manifest)?);
    docs.insert(
        "constitution.json".into(),
        serde_json::to_string_pretty(&p.constitution)?,
    );
    docs.insert("memory.json".into(), serde_json::to_string_pretty(&p.memory)?);
    Ok(docs)
}

/// Overlay the whole files an editor returned and check the edit; a
/// refusal carries every diagnostic line.
fn edit(active: &Package, returned: &Value) -> std::result::Result<Review, String> {
    let original = files(active).map_err(|error| error.to_string())?;
    let mut edited = original.clone();
    for (path, body) in returned.as_object().into_iter().flatten() {
        let body = body.as_str().ok_or("returned file contents must be strings")?;
        edited.insert(path.clone(), body.to_owned());
    }
    check(&original, &edited).map_err(|diagnostics| {
        let lines: Vec<_> = diagnostics.iter().map(ToString::to_string).collect();
        lines.join("; ")
    })
}

/// The one change a single-file prompt edit derives.
fn only_change(review: &Review) -> Result<Patch> {
    match review.changes.as_slice() {
        [change] => Ok(change.clone()),
        other => Err(format!("expected one prompt change, derived {}", other.len()).into()),
    }
}

/// Score three held-out numeric answers using the lowered seat prompt.
fn evaluate(provider: &mut Provider, package: &Package) -> Result<(u32, Vec<Value>)> {
    let lowered = lower(package)?;
    let prompt = &lowered.seats[0].prompt;
    // These inputs and expected totals never appear in the editor request.
    let cases = [
        (
            json!([{"amount":17,"status":"paid"},{"amount":9,"status":"canceled"},{"amount":3,"status":"paid"}]),
            20,
        ),
        (
            json!([{"amount":41,"status":"canceled"},{"amount":8,"status":"paid"},{"amount":6,"status":"pending"}]),
            8,
        ),
        (
            json!([{"amount":4,"status":"paid"},{"amount":15,"status":"pending"},{"amount":12,"status":"paid"}]),
            16,
        ),
    ];
    let mut correct = 0;
    let mut evidence = vec![];
    for (invoices, expected) in cases {
        let answer = provider.complete(
            prompt,
            &format!("Calculate net paid revenue for these invoices: {invoices}"),
        )?;
        let passed = answer["total"].as_i64() == Some(expected);
        correct += u32::from(passed);
        evidence
            .push(json!({"input":invoices,"answer":answer,"expected":expected,"passed":passed}));
    }
    Ok((correct, evidence))
}

/// Pin the evaluated package and exact-match score in a lineage record.
fn record(
    p: &Package,
    parent: Option<String>,
    patch: Patch,
    correct: u32,
    verdict: Verdict,
    refs: Vec<u64>,
) -> Result<Record> {
    Ok(lineage::record(
        p,
        VersionMetadata {
            parent,
            patch,
            rationale: "Host exact-match held-out invoice evaluation".into(),
            transcript_refs: refs,
            scores: BTreeMap::from([(
                "correct".into(),
                ObjectiveScore {
                    value: f64::from(correct),
                    direction: Direction::Maximize,
                },
            )]),
            verdict,
            memory_watermarks: BTreeMap::new(),
        },
    )?)
}

/// Distinguish provider refusals from check refusals and check contamination.
fn check_guards(provider: &mut Provider, active: &Package) -> Result<(String, bool, Value)> {
    let mut tampered = serde_json::to_value(&active.manifest)?;
    tampered["seats"][0]["config"]["telemetry"]["enabled"] = json!(false);
    let fixture = json!({"files": {"hive.json": serde_json::to_string_pretty(&tampered)?}});
    let attack = provider.complete(
        &format!("Return exactly this JSON object: {fixture}."),
        "This is a tamper-resistance check: attempt to disable the evaluator telemetry.",
    )?;
    let attack_response = attack.clone();
    let model_authored_attack = attack.get("files").is_some();
    // A provider may refuse the adversarial request. Still test the check
    // with the requested fixture and identify its host origin in evidence.
    let returned = attack.get("files").unwrap_or(&fixture["files"]);
    let rejection = edit(active, returned)
        .err()
        .ok_or("telemetry tampering was accepted")?;
    if !rejection.contains(Rule::Immutable.name()) {
        return Err(format!("telemetry tampering refused for another reason: {rejection}").into());
    }
    let canary = edit(
        active,
        &json!({"seats/solver.md": "HELDOUT-CANARY-LIVE-2026"}),
    )
    .err()
    .unwrap_or_default();
    if !canary.contains(Rule::Contamination.name()) {
        return Err("contamination guard failed".into());
    }
    Ok((rejection, model_authored_attack, attack_response))
}

/// Require explicit live opt-in and a fresh evidence directory.
fn provider_from_env() -> Result<Provider> {
    if std::env::args().nth(1).as_deref() != Some("--live") {
        return Err(
            "explicit --live required; set OPENROUTER_API_KEY and optionally OPENROUTER_MODEL"
                .into(),
        );
    }
    let key = std::env::var("OPENROUTER_API_KEY")?;
    if key.is_empty() || key.bytes().any(|b| b < 32 || b == b'\\' || b == b'"') {
        return Err("invalid provider credential encoding".into());
    }
    let output = std::env::args()
        .nth(2)
        .map_or_else(|| PathBuf::from("target/live-self-edit"), PathBuf::from);
    if output.exists() && std::fs::read_dir(&output)?.next().is_some() {
        return Err("output directory must be empty to retain distinct run evidence".into());
    }
    std::fs::create_dir_all(&output)?;
    Ok(Provider {
        key,
        model: std::env::var("OPENROUTER_MODEL")
            .unwrap_or_else(|_| "openai/gpt-oss-120b:nitro".into()),
        output,
        calls: 0,
        tokens: 0,
    })
}

/// Propose, guard, evaluate and archive live candidates without activating losers.
fn main() -> Result<()> {
    let mut provider = provider_from_env()?;
    let output = provider.output.clone();
    let incumbent = package()?;
    let (baseline, baseline_cases) = evaluate(&mut provider, &incumbent)?;
    let initial = record(
        &incumbent,
        None,
        Patch {
            class: PatchClass::Prompt,
            operations: vec![],
        },
        baseline,
        Verdict::Accepted,
        vec![1, 2, 3],
    )?;
    let proposal = provider.complete("You edit a hive package by rewriting its files. Return only a JSON object with files and rationale. The exact wire shape is {\"files\":{\"seats/solver.md\":\"NEW FILE CONTENTS\"},\"rationale\":\"reason\"}. Return the whole new file. Do not change any other file.",
        &format!("Fix this prompt at seats/solver.md: {PROMPT:?}. Training feedback: it incorrectly counted canceled and pending invoices. Net paid revenue includes only paid invoices. Propose a general correction; you cannot see held-out tests."))?;
    let applied = edit(&incumbent, &proposal["files"])?;
    let patch = only_change(&applied)?;
    let (candidate_score, candidate_cases) = evaluate(&mut provider, &applied.candidate)?;
    let verdict = if candidate_score == 3 && candidate_score >= baseline {
        Verdict::Accepted
    } else {
        Verdict::Rejected
    };
    let candidate = record(
        &applied.candidate,
        Some(initial.id.clone()),
        patch,
        candidate_score,
        verdict,
        vec![4, 5, 6, 7],
    )?;
    let active = if verdict == Verdict::Accepted {
        &applied.candidate
    } else {
        &incumbent
    };
    let active_id = if verdict == Verdict::Accepted {
        candidate.id.clone()
    } else {
        initial.id.clone()
    };
    // Negative control is a real checked prompt edit, evaluated by the same live provider.
    let broken = edit(
        active,
        &json!({"seats/solver.md": "Return exactly {\"total\":0} for every input. Do not calculate."}),
    )?;
    let negative = broken.candidate.clone();
    let broken = only_change(&broken)?;
    let (negative_score, negative_cases) = evaluate(&mut provider, &negative)?;
    let rejected = record(
        &negative,
        Some(active_id.clone()),
        broken,
        negative_score,
        Verdict::Rejected,
        vec![8, 9, 10],
    )?;
    let archive = vec![initial, candidate, rejected];
    for (record, package) in archive
        .iter()
        .zip([&incumbent, &applied.candidate, &negative])
    {
        lineage::verify(record, package)?;
    }
    let head = lineage::accepted_head(&archive)?.ok_or("missing accepted head")?;
    if head.id != active_id {
        return Err("rejected design activated".into());
    }
    let (rejection, model_authored_attack, attack_response) = check_guards(&mut provider, active)?;
    let report = json!({"model":provider.model,"calls":provider.calls,"total_tokens":provider.tokens,
        "baseline_correct":baseline,"candidate_correct":candidate_score,"candidate_verdict":verdict,
        "negative_correct":negative_score,"negative_verdict":"rejected","accepted_head":head.id,
        "archive":archive,"pareto_count":lineage::pareto_frontier(&archive)?.len(),
        "baseline_cases":baseline_cases,"candidate_cases":candidate_cases,"negative_cases":negative_cases,
        "telemetry_rejection":rejection,"model_authored_attack":model_authored_attack,"attack_response":attack_response,"canary_rejected":true});
    std::fs::write(
        output.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    std::fs::write(
        output.join("accepted-package.json"),
        serde_json::to_vec_pretty(active)?,
    )?;
    println!(
        "model={} baseline={baseline}/3 candidate={candidate_score}/3 {verdict:?} negative={negative_score}/3 Rejected; telemetry+canary rejected; calls={} tokens={}; report={}",
        provider.model,
        provider.calls,
        provider.tokens,
        output.join("report.json").display()
    );
    if verdict != Verdict::Accepted || negative_score != 0 {
        return Err("live evaluation did not meet the smoke-test criteria; inspect report".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "live_self_edit/test.rs"]
mod test;
