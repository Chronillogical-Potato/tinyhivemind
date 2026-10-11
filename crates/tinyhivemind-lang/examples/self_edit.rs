//! An editor rewrites a package's files; the host checks the edit like a
//! compiler, evaluates the candidate, and archives accepted and rejected
//! designs with one lineage record per derived change.
use std::collections::BTreeMap;
use tinyhivemind_lang::{
    Package, Review, check,
    lineage::{self, Direction, ObjectiveScore, Record, Verdict, VersionMetadata},
    patch::{Patch, PatchClass},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
type Files = BTreeMap<String, String>;

/// The founding package, as the files an editor would see on disk.
fn founding_files() -> Result<Files> {
    let mut package = Package::default();
    package.constitution.permitted_edits = vec![PatchClass::Prompt];
    package.constitution.contamination_canaries = vec!["private-evaluation-canary".into()];
    package
        .documents
        .insert("roles/editor.md".into(), "Answer the task.\n".into());
    let mut files = package.documents.clone();
    files.insert(
        "hive.json".into(),
        serde_json::to_string_pretty(&package.manifest)?,
    );
    files.insert(
        "constitution.json".into(),
        serde_json::to_string_pretty(&package.constitution)?,
    );
    files.insert(
        "memory.json".into(),
        serde_json::to_string_pretty(&package.memory)?,
    );
    Ok(files)
}

/// The editor: it returns whole files, not operations. A real host would
/// ask a model; this one rewrites the charter, or tries to loosen the
/// constitution when asked to misbehave.
fn editor(files: &Files, charter: &str, tamper: bool) -> Files {
    let mut edited = files.clone();
    edited.insert("roles/editor.md".into(), charter.into());
    if tamper {
        let constitution =
            files["constitution.json"].replace("\"max_turns\": 60", "\"max_turns\": 600");
        edited.insert("constitution.json".into(), constitution);
    }
    edited
}

/// Host-owned evaluation: a deterministic rubric standing in for held-out
/// tests. The language neither runs a model nor chooses acceptance.
fn evaluate(candidate: &Package) -> f64 {
    if candidate.documents["roles/editor.md"].contains("evidence") {
        0.8
    } else {
        0.2
    }
}

fn metadata(parent: Option<String>, patch: Patch, score: f64, verdict: Verdict) -> VersionMetadata {
    VersionMetadata {
        parent,
        patch,
        rationale: "Require explicit evidence before answering".into(),
        transcript_refs: vec![4, 9],
        scores: BTreeMap::from([(
            "quality".into(),
            ObjectiveScore {
                value: score,
                direction: Direction::Maximize,
            },
        )]),
        verdict,
        memory_watermarks: BTreeMap::new(),
    }
}

/// Record one lineage record per derived change, each the parent of the
/// next; every step carries the whole edit's score and verdict.
fn archive(
    records: &mut Vec<Record>,
    original: &Package,
    review: &Review,
    verdict: Verdict,
) -> Result<()> {
    let score = evaluate(&review.candidate);
    let mut current = original.clone();
    for change in &review.changes {
        current = tinyhivemind_lang::patch::apply(&current, change)?.candidate;
        let parent = records.last().map(|record| record.id.clone());
        records.push(lineage::record(
            &current,
            metadata(parent, change.clone(), score, verdict),
        )?);
    }
    Ok(())
}

/// What one run of the example produced.
#[derive(Debug)]
struct Summary {
    refusal: Vec<String>,
    archived: usize,
    frontier: usize,
    head: String,
    accepted_id: String,
}

fn run() -> Result<Summary> {
    let founding = founding_files()?;
    let original = tinyhivemind_lang::parse(&founding)?;
    let mut records = vec![lineage::record(
        &original,
        metadata(
            None,
            Patch {
                class: PatchClass::Prompt,
                operations: vec![],
            },
            evaluate(&original),
            Verdict::Accepted,
        ),
    )?];

    // An edit that also loosens the constitution is refused, with every
    // finding located.
    let tampered = editor(&founding, "Answer and cite evidence.\n", true);
    let refusal = match check(&founding, &tampered) {
        Ok(_) => return Err("a constitution edit was accepted".into()),
        Err(diagnostics) => diagnostics.iter().map(ToString::to_string).collect(),
    };

    // A charter rewrite is accepted, evaluated and archived.
    let improved = editor(
        &founding,
        "Answer the task and cite concrete evidence.\n",
        false,
    );
    let review = check(&founding, &improved).map_err(|d| format!("refused: {d:?}"))?;
    archive(&mut records, &original, &review, Verdict::Accepted)?;
    let accepted_id = records.last().map(|r| r.id.clone()).unwrap_or_default();

    // A worse follow-up passes the check but the host rejects it.
    let worse = editor(&improved, "Guess without checking.\n", false);
    let follow_up = check(&improved, &worse).map_err(|d| format!("refused: {d:?}"))?;
    archive(
        &mut records,
        &review.candidate,
        &follow_up,
        Verdict::Rejected,
    )?;

    let head = lineage::accepted_head(&records)?.ok_or("expected an accepted design")?;
    Ok(Summary {
        refusal,
        archived: records.len(),
        frontier: lineage::pareto_frontier(&records)?.len(),
        head: head.id.clone(),
        accepted_id,
    })
}

fn main() -> Result<()> {
    let summary = run()?;
    println!("Refused edit:");
    for line in &summary.refusal {
        println!("  {line}");
    }
    println!("Accepted head: {}", summary.head);
    println!(
        "Archived designs: {}; Pareto frontier: {}",
        summary.archived, summary.frontier
    );
    Ok(())
}

#[cfg(test)]
#[path = "self_edit/test.rs"]
mod test;
