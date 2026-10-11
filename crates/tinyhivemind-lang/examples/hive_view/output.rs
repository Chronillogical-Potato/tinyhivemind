//! `--run`: print one run as human lines or NDJSON frames, offline or live.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use tinyhivemind_core::telemetry::{Clock, Stamped, TraceSink, Tracer};

use crate::Result;
use crate::desks::{DeskEnd, ReferralRecord};
use crate::episode::{Ending, Event, Outcome, RoundRecord};
use crate::frames::{self, Frame, ProblemFrame, UsageFrame};
use crate::live::{LiveConfig, LiveSpeaker};
use crate::load::Loaded;
use crate::run::{Knobs, Plan, Prepared, correct, execute, prepare, prepare_with};

/// Print one run: human lines, or NDJSON frames. With `live`, every seat is
/// a model call. A live run, or one with `record`, is also kept as a
/// recording in its run folder (`hive-runs/hive-view/<run>/`): the package's
/// `--json` structure as `structure.json`, written before the first frame,
/// and every frame in `frames.ndjson`, flushed as it is written, so a viewer
/// can follow the run while it is still going. Stdout is unchanged by it.
///
/// # Errors
///
/// Fails on a run error, a refused `--live` setup, or a trace file or
/// recording that cannot be created.
pub(crate) fn print(
    loaded: &Loaded,
    knobs: &Knobs,
    as_frames: bool,
    trace: Option<&Path>,
    live: Option<&LiveConfig>,
    record: bool,
) -> Result<()> {
    let started = Instant::now();
    let run_label = format!(
        "{}-seed{}-{}",
        loaded.lowered.id,
        knobs.seed,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis())
    );
    let mut prepared = match live {
        Some(config) => prepare_with(loaded, knobs, Some((config, &run_label)))?,
        None => prepare(loaded, knobs)?,
    };
    let artifacts = (live.is_some() || record)
        .then(|| crate::live::provider::artifacts_root().join(&run_label));
    let mut writer = FrameWriter {
        stdout: as_frames,
        kept: artifacts
            .as_ref()
            .map(|dir| recording(dir, loaded))
            .transpose()?,
    };
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
        .map(|sink| Tracer::new(prepared.package().to_owned(), sink, &clock));
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    writer.emit(&mut out, &start_frame(&prepared, knobs, live))?;
    if !as_frames {
        header(&mut out, &prepared, live)?;
    }
    let several = matches!(prepared.plan, Plan::Hive(_));
    let mut failure: Option<std::io::Error> = None;
    let outcome = execute(&mut prepared, tracer.as_ref(), &mut |event| {
        let written = write_event(
            &mut writer,
            &mut out,
            event,
            as_frames,
            several,
            truth.as_deref(),
        );
        if let Err(error) = written {
            failure.get_or_insert(error);
        }
    })?;
    if let Some(error) = failure {
        return Err(error.into());
    }
    let correct = correct(&outcome, prepared.problem.as_ref());
    let usage = live_usage(&prepared, started, outcome.spent);
    if outcome.spent {
        let cap = live.map_or(0, |config| config.max_calls);
        eprintln!(
            "hive_view: stopped exhausted: the calls left under --max-calls {cap} cannot fill the next round"
        );
    }
    if let Some(usage) = &usage {
        writer.emit(&mut out, &Frame::Usage(usage.clone()))?;
    }
    let end = Frame::End {
        ending: outcome.ending.name().into(),
        topic: outcome.ending.topic().map(str::to_owned),
        correct,
        rounds: outcome.rounds,
        turns: outcome.turns,
        desk: prepared.answer_desk(),
    };
    writer.emit(&mut out, &end)?;
    if !as_frames {
        ending(
            &mut out,
            &outcome,
            correct,
            prepared.answer_desk().as_deref(),
        )?;
        if let Some(usage) = &usage {
            human_usage(&mut out, usage)?;
        }
    }
    if let Some(dir) = &artifacts {
        let what = if live.is_some() {
            "live artifacts"
        } else {
            "recording"
        };
        eprintln!("hive_view: {what} in {}", dir.display());
    }
    if let Some(sink) = &sink
        && let Ok(mut writer) = sink.0.lock()
    {
        writer.flush()?;
    }
    Ok(())
}

/// Open a recording in `dir`: write the package's structure as
/// `structure.json` and return the `frames.ndjson` writer.
fn recording(dir: &Path, loaded: &Loaded) -> Result<Box<dyn Write>> {
    std::fs::create_dir_all(dir)?;
    let structure = serde_json::to_vec(&crate::structure::structure(loaded)?)?;
    std::fs::write(dir.join("structure.json"), structure)?;
    Ok(Box::new(BufWriter::new(File::create(
        dir.join("frames.ndjson"),
    )?)))
}

/// Writes frames to stdout (in `--frames` mode) and to the recording, when
/// there is one; both are flushed per line, since a viewer follows a run
/// line by line.
pub(crate) struct FrameWriter {
    pub(crate) stdout: bool,
    pub(crate) kept: Option<Box<dyn Write>>,
}

impl FrameWriter {
    pub(crate) fn emit(&mut self, out: &mut dyn Write, frame: &Frame) -> std::io::Result<()> {
        if !self.stdout && self.kept.is_none() {
            return Ok(());
        }
        let json = serde_json::to_string(frame).map_err(std::io::Error::other)?;
        if self.stdout {
            writeln!(out, "{json}")?;
            out.flush()?;
        }
        if let Some(kept) = self.kept.as_mut() {
            writeln!(kept, "{json}")?;
            kept.flush()?;
        }
        Ok(())
    }
}

/// One event as a frame (to stdout in `--frames` mode, and to a recording)
/// and, otherwise, a human line. A one-desk run reports no desk endings: its
/// `end` is its desk's.
fn write_event(
    writer: &mut FrameWriter,
    out: &mut dyn Write,
    event: Event<'_>,
    as_frames: bool,
    several: bool,
    truth: Option<&str>,
) -> std::io::Result<()> {
    match event {
        Event::Round(record) => writer.emit(out, &frames::round(record, truth))?,
        Event::Referral(record) => writer.emit(out, &frames::referral(record))?,
        Event::DeskEnd(end) if several => writer.emit(out, &frames::desk_end(end, truth))?,
        Event::DeskEnd(_) => {}
    }
    if as_frames {
        return Ok(());
    }
    match event {
        Event::Round(record) => human(out, record),
        Event::Referral(record) => human_referral(out, record),
        Event::DeskEnd(end) if several => human_desk_end(out, end),
        Event::DeskEnd(_) => Ok(()),
    }
}

fn start_frame(prepared: &Prepared, knobs: &Knobs, live: Option<&LiveConfig>) -> Frame {
    Frame::Start {
        package: prepared.package().to_owned(),
        seed: knobs.seed,
        seats: prepared.seats(),
        problem: prepared
            .problem
            .as_ref()
            .map(|(problem, answer)| ProblemFrame {
                id: problem.id.clone(),
                statement: problem.statement.clone(),
                answer: answer.to_string(),
            }),
        live: live.map(|_| true),
        model: live.map(|config| config.model.clone()),
    }
}

fn human_usage(out: &mut dyn Write, usage: &UsageFrame) -> std::io::Result<()> {
    writeln!(
        out,
        "  live: {} calls, {} tokens ({} prompt, {} completion), ${:.4}, {:.1}s{}",
        usage.calls,
        usage.total_tokens,
        usage.prompt_tokens,
        usage.completion_tokens,
        usage.cost_usd,
        std::time::Duration::from_millis(usage.wall_ms).as_secs_f64(),
        if usage.exhausted_calls {
            " — stopped by --max-calls"
        } else {
            ""
        }
    )
}

/// The usage frame of a live run; `None` offline.
fn live_usage(prepared: &Prepared, started: Instant, spent: bool) -> Option<UsageFrame> {
    let speaker = prepared.speaker.as_any()?.downcast_ref::<LiveSpeaker>()?;
    let usage = speaker.provider().usage();
    let summary = UsageFrame {
        calls: speaker.provider().issued(),
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
        cost_usd: (usage.cost_usd * 1e6).round() / 1e6,
        wall_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
        exhausted_calls: spent || speaker.capped(),
    };
    let _ = std::fs::write(
        speaker.dir().join("usage.json"),
        serde_json::to_vec_pretty(&summary).unwrap_or_default(),
    );
    Some(summary)
}

fn header(out: &mut dyn Write, prepared: &Prepared, live: Option<&LiveConfig>) -> Result<()> {
    match &prepared.plan {
        Plan::Room(room) => writeln!(
            out,
            "{} on desk {} — {} seats, budget {} turns, rounds of {}",
            room.package,
            room.desk.id,
            room.members.len(),
            room.policy.turn_budget,
            room.policy.round_width
        )?,
        Plan::Hive(hive) => {
            writeln!(
                out,
                "{} — {} seats on {} desks, one floor each, linked by referral",
                hive.package,
                hive.seats(),
                hive.rooms.len()
            )?;
            for room in &hive.rooms {
                writeln!(
                    out,
                    "  desk {:<12} {} seats, budget {} turns, quorum {}, rounds of {}",
                    room.desk.id,
                    room.members.len(),
                    room.policy.turn_budget,
                    room.policy.quorum.threshold,
                    room.policy.round_width
                )?;
            }
        }
    }
    if let Some(config) = live {
        writeln!(
            out,
            "live: {} (at most {} calls, {} tokens each)",
            config.model, config.max_calls, config.max_tokens
        )?;
    }
    if let Some((problem, answer)) = &prepared.problem {
        writeln!(
            out,
            "problem {}: {} (answer {answer})",
            problem.id, problem.statement
        )?;
    }
    writeln!(out)?;
    Ok(())
}

fn human(out: &mut dyn Write, record: &RoundRecord) -> std::io::Result<()> {
    let phase = record
        .phase
        .map_or_else(|| "Answer".to_owned(), |phase| format!("{phase:?}"));
    writeln!(
        out,
        "round {} ({phase}, {:?}){}",
        record.round,
        record.visibility,
        if record.desk_round == record.round {
            String::new()
        } else {
            format!(" desk {} round {}", record.desk, record.desk_round)
        }
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

fn human_referral(out: &mut dyn Write, record: &ReferralRecord) -> std::io::Result<()> {
    let verdict = if record.accepted {
        format!("landed at ^{}", record.sequence.unwrap_or(0))
    } else {
        format!("refused: {}", record.reason.as_deref().unwrap_or("?"))
    };
    writeln!(
        out,
        "  {} {} -> {} by {} (hop {}): {}  [{}]",
        record.kind,
        record.from_desk,
        record.to_desk,
        record.seat,
        record.hops,
        record.content.replace('\n', " | "),
        verdict
    )
}

fn human_desk_end(out: &mut dyn Write, end: &DeskEnd) -> std::io::Result<()> {
    let ending = match &end.ending {
        Ending::Converged(topic) => format!("converged on #{topic}"),
        other => other.name().to_owned(),
    };
    writeln!(
        out,
        "  desk {} {ending} ({} rounds, {} turns)",
        end.desk, end.rounds, end.turns
    )
}

fn ending(
    out: &mut dyn Write,
    outcome: &Outcome,
    correct: Option<bool>,
    desk: Option<&str>,
) -> std::io::Result<()> {
    let verdict = match correct {
        Some(true) => " (correct)",
        Some(false) => " (not correct)",
        None => "",
    };
    let by = desk
        .map(|desk| format!(" by desk {desk}"))
        .unwrap_or_default();
    match &outcome.ending {
        Ending::Converged(topic) => writeln!(out, "\n  converged{by} on #{topic}{verdict}")?,
        Ending::Deadlocked(topics) => writeln!(
            out,
            "\n  deadlocked{by} between #{}{verdict}",
            topics.join(" and #")
        )?,
        Ending::Exhausted | Ending::Unopened => {
            writeln!(out, "\n  budget exhausted{by}, no decision{verdict}")?;
        }
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
