//! Build a room and a speaker for a package, and print one run.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Mutex;

use tinyhivemind_core::hive::{
    EpisodePolicy, QuorumPolicy, desk::Desk, desk::ResponderMode, roster::RosterMember,
};
use tinyhivemind_core::telemetry::{Clock, Stamped, TraceSink, Tracer};

use crate::Result;
use crate::episode::{Ending, Outcome, Room, RoundRecord, Speaker, run};
use crate::frames::{self, Frame, ProblemFrame};
use crate::load::{Loaded, roles};
use crate::math::{Problem, problems};
use crate::rng::Rng;
use crate::script::{Scripted, Seeded};
use crate::sim::{MathSim, SimConfig};

/// Run knobs from the command line.
#[derive(Clone, Debug, Default)]
pub(crate) struct Knobs {
    pub(crate) seed: u64,
    pub(crate) round_width: Option<u32>,
    pub(crate) turn_budget: Option<u32>,
    pub(crate) problem: Option<String>,
}

/// A room ready to run, its speaker, and the problem it poses, if math.
pub(crate) struct Prepared {
    pub(crate) room: Room,
    pub(crate) speaker: Box<dyn Speaker>,
    pub(crate) problem: Option<(Problem, u64)>,
}

impl std::fmt::Debug for Prepared {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Prepared")
            .field("room", &self.room)
            .finish_non_exhaustive()
    }
}

/// The problem a seed poses: `--problem` if given, else a seeded pick.
///
/// # Errors
///
/// Fails when `--problem` names a problem the bank does not hold.
pub(crate) fn pick(bank: &[Problem], seed: u64, chosen: Option<&str>) -> Result<Problem> {
    if let Some(id) = chosen {
        return bank
            .iter()
            .find(|problem| problem.id == id)
            .cloned()
            .ok_or_else(|| format!("no problem {id} in the bank").into());
    }
    let index = usize::try_from(Rng::stream(seed, "problem").below(bank.len() as u64))?;
    bank.get(index)
        .cloned()
        .ok_or_else(|| "empty problem bank".into())
}

fn apply(policy: &mut EpisodePolicy, knobs: &Knobs) {
    if let Some(width) = knobs.round_width {
        policy.round_width = width;
        policy.revealed_width = width;
    }
    if let Some(budget) = knobs.turn_budget {
        policy.turn_budget = budget;
    }
}

fn opening_for(loaded: &Loaded) -> String {
    let hive = &loaded.lowered;
    let ask = hive
        .goals
        .first()
        .map(|goal| goal.description.clone())
        .or_else(|| hive.desks.first().and_then(|desk| desk.description.clone()))
        .unwrap_or_else(|| hive.name.clone());
    format!("{ask}. Deliberate with !propose, !support and !object, citing with ^N.")
}

fn math_opening(problem: &Problem) -> String {
    format!(
        "Problem {}: {} Propose your answer as #<answer> citing ^1.",
        problem.id, problem.statement
    )
}

/// Prepare a package's run for these knobs.
///
/// # Errors
///
/// Fails on a malformed `sim.json` or an unknown `--problem`.
pub(crate) fn prepare(loaded: &Loaded, knobs: &Knobs) -> Result<Prepared> {
    let Some(sim) = &loaded.sim else {
        let mut room = Room::of(&loaded.lowered, opening_for(loaded));
        apply(&mut room.policy, knobs);
        let speaker: Box<dyn Speaker> = match &loaded.script {
            Some(script) => Box::new(Scripted::new(script)),
            None => Box::new(Seeded::new(knobs.seed, room.members.len())),
        };
        return Ok(Prepared {
            room,
            speaker,
            problem: None,
        });
    };
    let config = SimConfig::from(sim)?;
    let problem = pick(&problems(sim)?, knobs.seed, knobs.problem.as_deref())?;
    let mut room = Room::of(&loaded.lowered, math_opening(&problem));
    apply(&mut room.policy, knobs);
    let role_of = roles(&loaded.lowered);
    let seats: Vec<(String, String)> = room
        .members
        .iter()
        .map(|member| {
            (
                member.id.clone(),
                role_of
                    .get(member.id.as_str())
                    .copied()
                    .unwrap_or("")
                    .to_owned(),
            )
        })
        .collect();
    let simulation = MathSim::new(&config, &problem, knobs.seed, &seats)?;
    let answer = simulation.answer();
    Ok(Prepared {
        room,
        speaker: Box::new(simulation),
        problem: Some((problem, answer)),
    })
}

/// The single-seat baseline: one seat of the baseline role answering alone.
///
/// Its episode still runs through core: a one-member desk, quorum of one,
/// three turns — propose, then record.
///
/// # Errors
///
/// As [`prepare`].
pub(crate) fn baseline(loaded: &Loaded, knobs: &Knobs) -> Result<Prepared> {
    let sim = loaded
        .sim
        .as_ref()
        .ok_or("the baseline needs a math package")?;
    let config = SimConfig::from(sim)?;
    let problem = pick(&problems(sim)?, knobs.seed, knobs.problem.as_deref())?;
    let seat = "solo".to_owned();
    let desk = Desk {
        id: "solo".into(),
        name: "Single seat".into(),
        description: Some("One seat answering alone".into()),
        members: vec![seat.clone()],
        responder_mode: ResponderMode::Auto,
    };
    let policy = EpisodePolicy {
        turn_budget: 3,
        round_width: 1,
        revealed_width: 1,
        quorum: QuorumPolicy {
            threshold: 1,
            window: 3,
            require_grounded: true,
        },
        ..loaded.lowered.policies.episode
    };
    let mut lone = config.clone();
    lone.roles.clear();
    let simulation = MathSim::new(
        &lone,
        &problem,
        knobs.seed,
        &[(seat.clone(), config.baseline.role.clone())],
    )?;
    let answer = simulation.answer();
    let room = Room {
        package: "single-seat".into(),
        members: vec![RosterMember {
            id: seat.clone(),
            name: Some(seat.clone()),
        }],
        home: [(seat.clone(), desk.id.clone())].into(),
        desk,
        policy,
        opening: math_opening(&problem),
    };
    Ok(Prepared {
        room,
        speaker: Box::new(simulation),
        problem: Some((problem, answer)),
    })
}

/// Run a prepared room, calling `observe` per round.
///
/// # Errors
///
/// As [`run`].
pub(crate) fn execute(
    prepared: &mut Prepared,
    tracer: Option<&Tracer<'_>>,
    observe: &mut dyn FnMut(&RoundRecord),
) -> Result<Outcome> {
    run(&prepared.room, prepared.speaker.as_mut(), tracer, observe)
}

/// Whether a math outcome decided the right answer; `None` when not math.
pub(crate) fn correct(outcome: &Outcome, problem: Option<&(Problem, u64)>) -> Option<bool> {
    problem.map(|(_, answer)| outcome.ending.topic() == Some(answer.to_string().as_str()))
}

/// Print one run: human lines, or NDJSON frames.
///
/// # Errors
///
/// Fails on a run error or a trace file that cannot be created.
pub(crate) fn print(
    loaded: &Loaded,
    knobs: &Knobs,
    as_frames: bool,
    trace: Option<&Path>,
) -> Result<()> {
    let mut prepared = prepare(loaded, knobs)?;
    let truth = prepared
        .problem
        .as_ref()
        .map(|(_, answer)| answer.to_string());
    let sink = trace
        .map(|path| File::create(path).map(|file| JsonlSink(Mutex::new(BufWriter::new(file)))))
        .transpose()?;
    let clock = TickClock::default();
    let tracer = sink
        .as_ref()
        .map(|sink| Tracer::new(prepared.room.package.clone(), sink, &clock));
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if as_frames {
        let start = Frame::Start {
            package: prepared.room.package.clone(),
            seed: knobs.seed,
            seats: prepared.room.members.len(),
            problem: prepared
                .problem
                .as_ref()
                .map(|(problem, answer)| ProblemFrame {
                    id: problem.id.clone(),
                    statement: problem.statement.clone(),
                    answer: answer.to_string(),
                }),
        };
        writeln!(out, "{}", serde_json::to_string(&start)?)?;
    } else {
        header(&mut out, &prepared)?;
    }
    let mut failure: Option<std::io::Error> = None;
    let outcome = execute(&mut prepared, tracer.as_ref(), &mut |record| {
        let written = if as_frames {
            serde_json::to_string(&frames::round(record, truth.as_deref()))
                .map_err(std::io::Error::other)
                .and_then(|line| writeln!(out, "{line}"))
        } else {
            human(&mut out, record)
        };
        if let Err(error) = written {
            failure.get_or_insert(error);
        }
    })?;
    if let Some(error) = failure {
        return Err(error.into());
    }
    let correct = correct(&outcome, prepared.problem.as_ref());
    if as_frames {
        let end = Frame::End {
            ending: outcome.ending.name().into(),
            topic: outcome.ending.topic().map(str::to_owned),
            correct,
            rounds: outcome.rounds,
            turns: outcome.turns,
        };
        writeln!(out, "{}", serde_json::to_string(&end)?)?;
    } else {
        ending(&mut out, &outcome, correct)?;
    }
    if let Some(sink) = &sink
        && let Ok(mut writer) = sink.0.lock()
    {
        writer.flush()?;
    }
    Ok(())
}

fn header(out: &mut dyn Write, prepared: &Prepared) -> std::io::Result<()> {
    let room = &prepared.room;
    writeln!(
        out,
        "{} on desk {} — {} seats, budget {} turns, rounds of {}",
        room.package,
        room.desk.id,
        room.members.len(),
        room.policy.turn_budget,
        room.policy.round_width
    )?;
    if let Some((problem, answer)) = &prepared.problem {
        writeln!(
            out,
            "problem {}: {} (answer {answer})",
            problem.id, problem.statement
        )?;
    }
    writeln!(out)
}

fn human(out: &mut dyn Write, record: &RoundRecord) -> std::io::Result<()> {
    writeln!(
        out,
        "round {} ({:?}, {:?})",
        record.round, record.phase, record.visibility
    )?;
    for turn in &record.turns {
        writeln!(
            out,
            "  {:>14}  {:<9} {:<5} saw {}/{}  {}",
            turn.seat,
            turn.reason,
            format!("{:?}", record.visibility).to_lowercase(),
            turn.saw,
            turn.of,
            turn.content.replace('\n', " | "),
        )?;
    }
    Ok(())
}

fn ending(out: &mut dyn Write, outcome: &Outcome, correct: Option<bool>) -> std::io::Result<()> {
    let verdict = match correct {
        Some(true) => " (correct)",
        Some(false) => " (not correct)",
        None => "",
    };
    match &outcome.ending {
        Ending::Converged(topic) => writeln!(out, "\n  converged on #{topic}{verdict}")?,
        Ending::Deadlocked(topics) => writeln!(
            out,
            "\n  deadlocked between #{}{verdict}",
            topics.join(" and #")
        )?,
        Ending::Exhausted => writeln!(out, "\n  budget exhausted, no decision{verdict}")?,
        Ending::Idle => writeln!(out, "\n  nobody had anything to say{verdict}")?,
    }
    writeln!(out, "  {} rounds, {} turns", outcome.rounds, outcome.turns)
}

/// Writes each telemetry event as one JSON line, as `examples/lab` does.
struct JsonlSink<W: Write + Send>(Mutex<W>);

impl<W: Write + Send> TraceSink for JsonlSink<W> {
    fn record(&self, event: &Stamped) {
        if let (Ok(mut out), Ok(line)) = (self.0.lock(), serde_json::to_string(event)) {
            // A failed trace write must not fail the run it describes.
            let _ = writeln!(out, "{line}");
        }
    }
}

/// One millisecond per reading, so traces repeat exactly.
#[derive(Debug, Default)]
struct TickClock(std::sync::atomic::AtomicU64);

impl Clock for TickClock {
    fn now_ms(&self) -> u64 {
        self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }
}
