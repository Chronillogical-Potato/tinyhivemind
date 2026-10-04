//! One seat's activation: a conversation with the model and its tools.
//!
//! An activation is a loop of model calls. Each call may request `bash`
//! (run in the sandbox) or a speaking tool (interpreted and committed through
//! core onto the board). The first accepted utterance ends the activation in
//! hive mode; the single-agent baseline runs the same loop with only
//! `complete_episode` to speak with, so the two modes share every line that
//! touches the model, the sandbox and the meter.
//!
//! Telemetry: one turn per model call (`turn_started`, `turn_finished` with the
//! provider's real tokens and measured latency), a `tool_call` per tool, and a
//! `mark` per executed command.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tinyhivemind_core::runtime::speech::{CallArguments, ToolCall, Utterance, interpret};
use tinyhivemind_core::telemetry::{TraceEvent, Tracer};

use super::board::{Board, Committed};
use super::llm::{Llm, ToolUse, tool_result};
use super::meter::Abort;
use super::sandbox::{Exec, refuse, truncate};
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
}

/// What to run for one seat.
pub struct Activation<'a> {
    /// The seat id.
    pub seat: &'a str,
    /// System prompt.
    pub system: String,
    /// Opening user message.
    pub user: String,
    /// Tool schemas offered.
    pub tools: Vec<Value>,
    /// Names of the speaking tools offered (the rest are refused).
    pub speaking: &'a [&'a str],
    /// Model calls allowed before the activation is cut off.
    pub steps: usize,
    /// Whether text with no tool call becomes a `post` (hive) or is nudged
    /// (single agent).
    pub implicit_post: bool,
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
}

/// Run one activation to its end.
pub fn run(env: &Env<'_>, act: &Activation<'_>) -> Outcome {
    let mut messages = vec![
        json!({ "role": "system", "content": act.system }),
        json!({ "role": "user", "content": act.user }),
    ];
    let mut out = Outcome::default();
    let mut nudges = 0;
    let mut last_text = String::new();
    while out.steps < act.steps {
        let turn = env.turns.fetch_add(1, Ordering::SeqCst);
        env.tracer.emit(TraceEvent::TurnStarted {
            turn,
            seat: act.seat.to_owned(),
        });
        let started = Instant::now();
        let reply = env.llm.complete(act.seat, &messages, &act.tools);
        let completion = match reply {
            Ok(completion) => completion,
            Err(abort) => {
                finish_turn(env, act.seat, turn, 0, 0, started);
                out.abort = Some(abort);
                return out;
            }
        };
        out.steps += 1;
        finish_turn(
            env,
            act.seat,
            turn,
            completion.input_tokens,
            completion.output_tokens,
            started,
        );
        messages.push(completion.message.clone());
        if !completion.content.trim().is_empty() {
            last_text.clone_from(&completion.content);
        }
        if completion.tool_calls.is_empty() {
            if act.implicit_post {
                commit_implicit(env, act, &mut out, &last_text);
                return out;
            }
            nudges += 1;
            if nudges > 2 {
                return out;
            }
            messages.push(json!({
                "role": "user",
                "content": "Use a tool: bash to work, or complete_episode when the task is done."
            }));
            continue;
        }
        for call in &completion.tool_calls {
            let content = handle(env, act, turn, call, &mut out);
            messages.push(tool_result(&call.id, &content));
        }
        if out.spoke.is_some() {
            return out;
        }
    }
    if act.implicit_post && out.spoke.is_none() {
        let note = format!("(stopped after {} steps) {last_text}", act.steps);
        commit_implicit(env, act, &mut out, &note);
    }
    out
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

fn handle(env: &Env<'_>, act: &Activation<'_>, turn: u64, call: &ToolUse, out: &mut Outcome) -> String {
    let started = Instant::now();
    let (content, refusal) = match &call.args {
        Err(why) => (format!("error: {why}"), Some(why.clone())),
        Ok(args) if call.name == "bash" => bash(env, act.seat, args),
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

fn bash(env: &Env<'_>, seat: &str, args: &Value) -> (String, Option<String>) {
    let cmd = args.get("cmd").and_then(Value::as_str).unwrap_or_default();
    if let Some(why) = refuse(cmd) {
        return (format!("refused: {why}"), Some(why.to_owned()));
    }
    env.tracer.emit(TraceEvent::Mark {
        label: "exec".into(),
        detail: format!("{seat}: {}", truncate(cmd, 240)),
    });
    match env.exec.exec(cmd, env.cmd_timeout) {
        Ok(done) => (
            format!("exit={}\n{}", done.exit, truncate(&done.stdout, env.output_limit)),
            None,
        ),
        Err(why) => (format!("error: {why}"), Some(why)),
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
