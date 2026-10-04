//! One seat's activation: a conversation with the model and its tools.
//!
//! An activation is a loop of model calls. Each call may request `bash`
//! (run in the sandbox) or a speaking tool (interpreted and committed through
//! core onto the board). The first accepted utterance ends the activation in
//! hive mode; the single-agent baseline runs the same loop with only
//! `complete_episode` to speak with, so the two modes share every line that
//! touches the model, the sandbox and the meter.
//!
//! An activation runs on the seat's [`SeatSession`]. A new session opens with
//! the system prompt and the activation's opening message; a resumed one gets
//! only that message (the desk delta) appended, so the seat still sees every
//! command it ran before. Only compaction ([`compact`]) removes messages.
//! With memory on ([`recall`]), a pack is recalled when the session starts,
//! when it resumes and after compaction, and what the seat did is stored at
//! the end of every activation.
//!
//! Telemetry: one turn per model call (`turn_started`, `turn_finished` with the
//! provider's real tokens and measured latency), a `tool_call` per tool, a
//! `mark` per executed command, a `session` mark per activation and a
//! `memory` mark per memory call.

mod compact;
mod recall;

pub use recall::finish as finish_memory;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tinyhivemind_core::runtime::speech::{CallArguments, ToolCall, Utterance, interpret};
use tinyhivemind_core::telemetry::{TraceEvent, Tracer};

use super::board::{Board, Committed};
use super::context::Settings;
use super::llm::{Llm, ToolUse, tool_result};
use super::memory::{LedgerEntry, SeatMemory};
use super::meter::Abort;
use super::sandbox::{Exec, refuse, truncate};
use super::session::{SeatSession, Sessions};
use super::tools::parse_arguments;

/// Everything an activation borrows from the run.
pub struct Env<'a> {
    /// The metered model client.
    pub llm: &'a Llm,
    /// Where `bash` runs.
    pub exec: &'a dyn Exec,
    /// Telemetry.
    pub tracer: &'a Tracer<'a>,
    /// The shared transcript.
    pub board: &'a Board,
    /// Turn id source, shared by every seat in the run.
    pub turns: &'a AtomicU64,
    /// Per-command timeout.
    pub cmd_timeout: Duration,
    /// Bytes of command output returned to the model.
    pub output_limit: usize,
    /// Every seat's session, kept between activations when persistent.
    pub sessions: &'a Sessions,
    /// Hive memory, when the run has it.
    pub memory: Option<&'a dyn SeatMemory>,
}

/// What to run for one seat.
pub struct Activation<'a> {
    /// The seat id.
    pub seat: &'a str,
    /// System prompt, used when the session is new.
    pub system: String,
    /// This activation's opening user message: the briefing for a new
    /// session, the desk delta for a resumed one.
    pub user: String,
    /// Desk rows that message shows (for the `session` mark).
    pub shown_rows: usize,
    /// What the seat is about to do, for steering memory recall.
    pub focus: String,
    /// Tool schemas offered.
    pub tools: Vec<Value>,
    /// Names of the speaking tools offered (the rest are refused).
    pub speaking: &'a [&'a str],
    /// Model calls allowed before the activation is cut off.
    pub steps: usize,
    /// Whether text with no tool call becomes a `post` (hive) or is nudged
    /// (single agent).
    pub implicit_post: bool,
    /// What to do when the prompt outgrows its budget.
    pub context: Settings,
}

/// How an activation ended.
#[derive(Debug, Default)]
pub struct Outcome {
    /// The utterance it committed, if any.
    pub spoke: Option<Committed>,
    /// Whether it reported done through `complete_episode`.
    pub completed: bool,
    /// Why it was stopped from outside, if it was.
    pub abort: Option<Abort>,
    /// Model calls made.
    pub steps: usize,
    /// The largest prompt any call of this activation reported.
    pub max_prompt: u64,
    /// Every command this activation ran, for memory.
    pub ledger: Vec<LedgerEntry>,
}

/// Activation-local state the model loop and compaction share.
#[derive(Default)]
struct Work {
    /// Commands already handed to memory (a prefix of the ledger).
    remembered: usize,
    /// The model's latest non-empty text.
    last_text: String,
}

/// Run one activation to its end on `session`.
pub fn run(env: &Env<'_>, act: &Activation<'_>, session: &mut SeatSession) -> Outcome {
    let mut out = Outcome::default();
    let mut work = Work::default();
    recall::open(env, act, session);
    session.activations += 1;
    env.tracer.emit(TraceEvent::Mark {
        label: "session".into(),
        detail: format!(
            "{}: activation {} messages {} delta_rows {} mode {}",
            act.seat,
            session.activations,
            session.messages.len(),
            act.shown_rows,
            env.sessions.mode().name()
        ),
    });
    if session.activations > 1 && session.last_prompt > act.context.budget {
        let prompt = session.last_prompt;
        compact::apply(env, act, session, &mut out, &mut work, prompt);
    }
    drive(env, act, session, &mut out, &mut work);
    recall::remember(env, act, &out, &mut work, "activation");
    out
}

/// The model loop.
fn drive(
    env: &Env<'_>,
    act: &Activation<'_>,
    session: &mut SeatSession,
    out: &mut Outcome,
    work: &mut Work,
) {
    let mut nudges = 0;
    while out.steps < act.steps {
        let turn = env.turns.fetch_add(1, Ordering::SeqCst);
        env.tracer.emit(TraceEvent::TurnStarted {
            turn,
            seat: act.seat.to_owned(),
        });
        let started = Instant::now();
        let reply = env.llm.complete(act.seat, &session.messages, &act.tools);
        let completion = match reply {
            Ok(completion) => completion,
            Err(abort) => {
                finish_turn(env, act.seat, turn, 0, 0, started);
                out.abort = Some(abort);
                return;
            }
        };
        out.steps += 1;
        let prompt = completion.input_tokens;
        finish_turn(
            env,
            act.seat,
            turn,
            completion.input_tokens,
            completion.output_tokens,
            started,
        );
        session.messages.push(completion.message.clone());
        session.last_prompt = prompt;
        out.max_prompt = out.max_prompt.max(prompt);
        if !completion.content.trim().is_empty() {
            work.last_text.clone_from(&completion.content);
        }
        if completion.tool_calls.is_empty() {
            if act.implicit_post {
                let text = work.last_text.clone();
                commit_implicit(env, act, out, &text);
                return;
            }
            nudges += 1;
            if nudges > 2 {
                return;
            }
            session.messages.push(json!({
                "role": "user",
                "content": "Use a tool: bash to work, or complete_episode when the task is done."
            }));
            continue;
        }
        // Only consecutive text-only replies mean the model has stopped; a
        // long task is bound to think aloud now and then between tool calls.
        nudges = 0;
        for call in &completion.tool_calls {
            let content = handle(env, act, turn, call, out);
            session.messages.push(tool_result(&call.id, &content));
        }
        if out.spoke.is_some() {
            return;
        }
        if prompt > act.context.budget {
            compact::apply(env, act, session, out, work, prompt);
        }
    }
    if act.implicit_post && out.spoke.is_none() {
        let note = format!("(stopped after {} steps) {}", act.steps, work.last_text);
        commit_implicit(env, act, out, &note);
    }
}

fn finish_turn(env: &Env<'_>, seat: &str, turn: u64, input: u64, output: u64, started: Instant) {
    env.tracer.emit(TraceEvent::TurnFinished {
        turn,
        seat: seat.to_owned(),
        input_tokens: input,
        output_tokens: output,
        latency_ms: elapsed_ms(started),
    });
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

fn commit_implicit(env: &Env<'_>, act: &Activation<'_>, out: &mut Outcome, text: &str) {
    let body = if text.trim().is_empty() {
        "(no report)"
    } else {
        text
    };
    if let Ok(done) = env.board.commit(
        act.seat,
        &Utterance::Post {
            message: body.to_owned(),
        },
    ) {
        out.spoke = Some(done);
    }
}

fn handle(
    env: &Env<'_>,
    act: &Activation<'_>,
    turn: u64,
    call: &ToolUse,
    out: &mut Outcome,
) -> String {
    let started = Instant::now();
    let (content, refusal) = match &call.args {
        Err(why) => (format!("error: {why}"), Some(why.clone())),
        Ok(args) if call.name == "bash" => bash(env, act.seat, args, &mut out.ledger),
        Ok(args) => speak(env, act, call, args, out),
    };
    env.tracer.emit(TraceEvent::ToolCall {
        turn,
        seat: act.seat.to_owned(),
        tool: call.name.clone(),
        latency_ms: elapsed_ms(started),
        refused: refusal.is_some(),
        reason: refusal,
    });
    content
}

fn bash(
    env: &Env<'_>,
    seat: &str,
    args: &Value,
    ledger: &mut Vec<LedgerEntry>,
) -> (String, Option<String>) {
    let cmd = args.get("cmd").and_then(Value::as_str).unwrap_or_default();
    if let Some(why) = refuse(cmd) {
        ledger.push(recall::entry(cmd, None, &format!("refused: {why}")));
        return (format!("refused: {why}"), Some(why.to_owned()));
    }
    env.tracer.emit(TraceEvent::Mark {
        label: "exec".into(),
        detail: format!("{seat}: {}", truncate(cmd, 240)),
    });
    match env.exec.exec(cmd, env.cmd_timeout) {
        Ok(done) => {
            ledger.push(recall::entry(cmd, Some(done.exit), &done.stdout));
            (
                format!(
                    "exit={}\n{}",
                    done.exit,
                    truncate(&done.stdout, env.output_limit)
                ),
                None,
            )
        }
        Err(why) => {
            ledger.push(recall::entry(cmd, None, &why));
            (format!("error: {why}"), Some(why))
        }
    }
}

fn speak(
    env: &Env<'_>,
    act: &Activation<'_>,
    call: &ToolUse,
    args: &Value,
    out: &mut Outcome,
) -> (String, Option<String>) {
    let refuse = |why: String| (format!("error: {why}"), Some(why));
    if !act.speaking.contains(&call.name.as_str()) {
        return refuse(format!("tool {} is not available", call.name));
    }
    let given = parse_arguments(args);
    let arguments = CallArguments {
        message: given.message.as_deref(),
        to: &given.to,
        limit: given.limit,
    };
    match interpret(&call.name, &arguments) {
        Err(rejection) => refuse(rejection.to_string()),
        Ok(ToolCall::Read { limit }) => (env.board.read(act.seat, limit), None),
        Ok(ToolCall::Speak(_)) if out.spoke.is_some() => {
            refuse("you already spoke this turn".into())
        }
        Ok(ToolCall::Speak(utterance)) => match env.board.commit(act.seat, &utterance) {
            Ok(done) => {
                out.completed |= done.completes;
                let note = format!("recorded as ^{}", done.sequence);
                out.spoke = Some(done);
                (note, None)
            }
            Err(why) => refuse(why),
        },
    }
}

#[cfg(test)]
mod test;
