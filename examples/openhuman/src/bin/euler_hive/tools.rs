//! The native tools a seat's turn is handed, exactly its package allow list.
//!
//! Every seat is built host-only, so these are the whole belt: OpenHuman's
//! own shell, file, fetch and memory tools never reach the model, and its
//! host-only gate refuses any other name. The three speaking tools record the
//! seat's one hive action for the turn; the host, not the tool, routes it.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use tinyhivemind_core::runtime::{EntryKind, Remember, RememberEntry, RememberRequest};
use tinytools::{Tool, ToolPolicy, ToolResult};

use super::artifacts::RunDir;
use super::budget::Budget;
use super::memory::SeatMemory;
use super::package::SeatTool;
use super::sandbox::PythonSandbox;
use super::search::{Provider, WebSearch};

/// Characters of program output a seat reads back.
const OUTPUT_TO_MODEL: usize = 6_000;

/// The verdict a completing seat may attach to a candidate.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Verdict {
    Support,
    Object,
}

/// The one hive action a seat ends its turn with.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(super) enum Action {
    /// Hand work to a colleague on this desk.
    Broadcast { to: String, message: String },
    /// Refer work to another desk.
    Refer { desk: String, message: String },
    /// Finish this seat's assignment.
    Complete {
        report: String,
        candidate: Option<String>,
        verdict: Option<Verdict>,
    },
}

/// One native tool call, as the run records it.
#[derive(Clone, Debug, Serialize)]
pub(super) struct CallRecord {
    pub(super) tool: String,
    pub(super) summary: String,
    pub(super) ok: bool,
}

/// What one turn's tools did.
#[derive(Debug, Default)]
pub(super) struct TurnLedger {
    action: Mutex<Option<Action>>,
    calls: Mutex<Vec<CallRecord>>,
    recalled: Mutex<usize>,
    remembered: Mutex<usize>,
}

impl TurnLedger {
    pub(super) fn action(&self) -> Option<Action> {
        self.action.lock().ok().and_then(|action| action.clone())
    }

    pub(super) fn calls(&self) -> Vec<CallRecord> {
        self.calls.lock().map(|calls| calls.clone()).unwrap_or_default()
    }

    pub(super) fn memory(&self) -> (usize, usize) {
        (
            self.recalled.lock().map_or(0, |n| *n),
            self.remembered.lock().map_or(0, |n| *n),
        )
    }

    pub(super) fn add_recalled(&self, notes: usize) {
        if let Ok(mut n) = self.recalled.lock() {
            *n += notes;
        }
    }

    fn record(&self, tool: &str, summary: String, ok: bool) {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(CallRecord {
                tool: tool.into(),
                summary,
                ok,
            });
        }
    }

    fn act(&self, action: Action) -> Result<(), String> {
        let mut slot = self.action.lock().map_err(|_| "turn state poisoned".to_owned())?;
        if slot.is_some() {
            return Err("you already took this turn's hive action; stop now".into());
        }
        *slot = Some(action);
        Ok(())
    }
}

/// What every tool on every belt shares.
pub(super) struct Services {
    pub(super) search: Option<WebSearch>,
    pub(super) sandbox: PythonSandbox,
    pub(super) budget: Arc<Budget>,
    pub(super) run: Arc<RunDir>,
    pub(super) run_id: String,
}

/// Who a belt belongs to and where its speaking tools may point.
#[derive(Clone, Debug)]
pub(super) struct SeatScope {
    pub(super) seat: String,
    pub(super) desk: String,
    pub(super) colleagues: Vec<String>,
    pub(super) linked_desks: Vec<String>,
    pub(super) conversation: String,
}

/// One native tool bound to one seat's turn.
pub(super) struct HiveTool {
    kind: SeatTool,
    scope: SeatScope,
    turn: Arc<TurnLedger>,
    services: Arc<Services>,
    memory: Arc<SeatMemory>,
}

/// Build the belt for one turn: one tool per allowed name, nothing else.
pub(super) fn belt(
    tools: &BTreeSet<SeatTool>,
    scope: &SeatScope,
    turn: &Arc<TurnLedger>,
    services: &Arc<Services>,
    memory: &Arc<SeatMemory>,
) -> Vec<Box<dyn Tool>> {
    tools
        .iter()
        .map(|kind| {
            Box::new(HiveTool {
                kind: *kind,
                scope: scope.clone(),
                turn: turn.clone(),
                services: services.clone(),
                memory: memory.clone(),
            }) as Box<dyn Tool>
        })
        .collect()
}

/// The JSON schema a tool advertises. Pure, so the belt is pinned by test.
pub(super) fn schema(kind: SeatTool, scope: &SeatScope, pool_writer: bool) -> Value {
    match kind {
        SeatTool::WebSearch => json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "What to look up."},
                "provider": {"type": "string", "enum": ["exa", "gemini"],
                    "description": "exa: ranked pages with text (default). gemini: a grounded summary with sources."}
            },
            "required": ["query"]
        }),
        SeatTool::RunPython => json!({
            "type": "object",
            "properties": {"code": {"type": "string", "description": "A complete Python 3.12 program (standard library only). Print everything you need to see."}},
            "required": ["code"]
        }),
        SeatTool::Broadcast => json!({
            "type": "object",
            "properties": {
                "to": {"type": "string", "enum": scope.colleagues},
                "message": {"type": "string", "description": "Self-contained: what you found and what they should do."}
            },
            "required": ["to", "message"]
        }),
        SeatTool::Refer => json!({
            "type": "object",
            "properties": {
                "desk": {"type": "string", "enum": scope.linked_desks},
                "message": {"type": "string", "description": "Self-contained: the question or work for that desk. Its answer comes back to you."}
            },
            "required": ["desk", "message"]
        }),
        SeatTool::Complete => json!({
            "type": "object",
            "properties": {
                "report": {"type": "string", "description": "Your finished work: evidence, outputs, conclusions."},
                "candidate": {"type": "string", "description": "The exact answer this report supports, in the format the problem asks for, if you have one."},
                "verdict": {"type": "string", "enum": ["support", "object"], "description": "Whether you support or object to the candidate."}
            },
            "required": ["report"]
        }),
        SeatTool::Remember => {
            let mut properties = json!({
                "kind": {"type": "string", "enum": ["observation", "failed_attempt", "outcome", "note"]},
                "text": {"type": "string", "description": "One fact per note."}
            });
            if pool_writer {
                properties["verified"] = json!({"type": "boolean",
                    "description": "Publish to the hive's verified-findings pool (only what you reproduced in the sandbox)."});
            }
            json!({"type": "object", "properties": properties, "required": ["kind", "text"]})
        }
        SeatTool::Recall => json!({
            "type": "object",
            "properties": {"query": {"type": "string"}},
            "required": ["query"]
        }),
    }
}

fn description(kind: SeatTool) -> &'static str {
    match kind {
        SeatTool::WebSearch => "Search the public web (Exa, or Gemini with Google Search grounding). Report sources; never treat a copied answer as verified.",
        SeatTool::RunPython => "Run one Python program in a fresh sandbox: no network, no files kept, 120 s, 1 GB. Returns exit code, stdout and stderr.",
        SeatTool::Broadcast => "End your turn by handing work to a colleague on your desk.",
        SeatTool::Refer => "End your turn by referring work to another desk; its answer returns to your desk.",
        SeatTool::Complete => "End your turn by finishing your assignment with a report for your desk.",
        SeatTool::Remember => "Save one note to your CortexDB memory (it is recalled when you rejoin).",
        SeatTool::Recall => "Search your CortexDB memory and the hive's verified findings.",
    }
}

fn text<'a>(args: &'a Value, field: &str) -> Result<&'a str, String> {
    args.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("`{field}` is required"))
}

fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let kept: String = text.chars().take(limit).collect();
    format!("{kept}\n[output clipped]")
}

impl HiveTool {
    async fn run(&self, args: &Value) -> Result<String, String> {
        match self.kind {
            SeatTool::WebSearch => self.web_search(args).await,
            SeatTool::RunPython => self.run_python(args).await,
            SeatTool::Broadcast => {
                let to = text(args, "to")?;
                if !self.scope.colleagues.iter().any(|colleague| colleague == to) {
                    return Err(format!("{to} is not a colleague on your desk"));
                }
                let message = text(args, "message")?;
                self.turn.act(Action::Broadcast {
                    to: to.into(),
                    message: message.into(),
                })?;
                Ok(format!("handed to @{to}; end your turn now"))
            }
            SeatTool::Refer => {
                let desk = text(args, "desk")?;
                if !self.scope.linked_desks.iter().any(|linked| linked == desk) {
                    return Err(format!("your desk has no referral link to {desk}"));
                }
                self.turn.act(Action::Refer {
                    desk: desk.into(),
                    message: text(args, "message")?.into(),
                })?;
                Ok(format!("referred to the {desk} desk; end your turn now"))
            }
            SeatTool::Complete => {
                let verdict = match args.get("verdict").and_then(Value::as_str) {
                    None => None,
                    Some("support") => Some(Verdict::Support),
                    Some("object") => Some(Verdict::Object),
                    Some(other) => return Err(format!("unknown verdict {other}")),
                };
                let candidate = args
                    .get("candidate")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|candidate| !candidate.is_empty())
                    .map(str::to_owned);
                self.turn.act(Action::Complete {
                    report: text(args, "report")?.into(),
                    candidate,
                    verdict,
                })?;
                Ok("assignment complete; end your turn now".into())
            }
            SeatTool::Remember => self.remember(args).await,
            SeatTool::Recall => {
                let query = text(args, "query")?;
                let notes = self.memory.search(query).await.map_err(|e| e.to_string())?;
                let count: usize = notes.iter().map(|note| note.lines.len()).sum();
                self.turn.add_recalled(count);
                Ok(tinyhivemind_core::runtime::frame_recalled(
                    &notes,
                    self.memory.binding().settings.budget_chars,
                )
                .unwrap_or_else(|| "nothing recalled".into()))
            }
        }
    }

    async fn web_search(&self, args: &Value) -> Result<String, String> {
        let query = text(args, "query")?;
        let provider = Provider::parse(args.get("provider").and_then(Value::as_str))
            .map_err(|e| e.to_string())?;
        let search = self
            .services
            .search
            .as_ref()
            .ok_or("web search is not configured for this run")?;
        self.services.budget.claim_search().map_err(|e| e.to_string())?;
        let number = self.services.run.next_search();
        let result = search.search(query, provider).await;
        let record = match &result {
            Ok(found) => {
                self.services.budget.record_search_cost(found.cost_usd);
                json!({"seat": self.scope.seat, "query": query, "provider": provider, "result": found})
            }
            Err(error) => json!({"seat": self.scope.seat, "query": query, "provider": provider, "error": error.to_string()}),
        };
        self.services
            .run
            .json(&format!("search/{number:03}-{}.json", self.scope.seat), &record);
        self.services.run.event(&json!({"event": "web_search", "seat": self.scope.seat, "query": query, "provider": provider, "ok": result.is_ok()}));
        let found = result.map_err(|e| format!("search failed: {e}"))?;
        Ok(format!("{}\n\nSources: {}", found.text, found.urls.join(" ")))
    }

    async fn run_python(&self, args: &Value) -> Result<String, String> {
        let code = text(args, "code")?.to_owned();
        let index = self.services.budget.claim_sandbox().map_err(|e| e.to_string())?;
        let number = self.services.run.next_program();
        let stem = format!("sandbox/{number:03}-{}", self.scope.seat);
        self.services.run.write(&format!("{stem}.py"), &code);
        let sandbox = self.services.sandbox.clone();
        let name = format!("euler-{}-{number:03}", self.services.run_id.to_lowercase());
        let output = tokio::task::spawn_blocking(move || sandbox.run(&code, &name))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("sandbox failed: {e}"))?;
        self.services.run.json(&format!("{stem}.out.json"), &output);
        self.services.run.event(&json!({"event": "run_python", "seat": self.scope.seat, "program": number, "run": index, "code": output.code, "timed_out": output.timed_out, "elapsed_ms": output.elapsed_ms}));
        Ok(format!(
            "exit code: {}{}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            output.code.map_or_else(|| "killed".into(), |code| code.to_string()),
            if output.timed_out { " (timed out)" } else { "" },
            clip(&output.stdout, OUTPUT_TO_MODEL),
            clip(&output.stderr, OUTPUT_TO_MODEL / 3),
        ))
    }

    async fn remember(&self, args: &Value) -> Result<String, String> {
        let kind = match text(args, "kind")? {
            "observation" => EntryKind::Observation,
            "failed_attempt" => EntryKind::FailedAttempt,
            "outcome" => EntryKind::Outcome,
            "note" => EntryKind::Note,
            other => return Err(format!("unknown kind {other}")),
        };
        let entry = RememberEntry {
            kind,
            text: text(args, "text")?.into(),
        };
        let verified = args.get("verified").and_then(Value::as_bool).unwrap_or(false);
        if verified {
            self.memory
                .publish(&entry, &self.scope.conversation)
                .await
                .map_err(|e| e.to_string())?;
        } else {
            self.memory
                .remember(&RememberRequest {
                    seat: self.scope.seat.clone(),
                    conversation: self.scope.conversation.clone(),
                    through: None,
                    entries: vec![entry],
                })
                .await
                .map_err(|e| e.to_string())?;
        }
        if let Ok(mut n) = self.turn.remembered.lock() {
            *n += 1;
        }
        self.services.run.event(&json!({"event": "remember", "seat": self.scope.seat, "verified": verified}));
        Ok(if verified { "published to the verified pool" } else { "remembered" }.into())
    }
}

#[async_trait]
impl Tool for HiveTool {
    fn name(&self) -> &str {
        self.kind.name()
    }

    fn description(&self) -> &str {
        description(self.kind)
    }

    fn parameters_schema(&self) -> Value {
        schema(self.kind, &self.scope, self.memory.pool_writer())
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let result = self.run(&args).await;
        let summary = summarize(self.kind, &args, &result);
        self.turn.record(self.kind.name(), summary, result.is_ok());
        Ok(match result {
            Ok(text) => ToolResult::success(text),
            Err(error) => ToolResult::error(error),
        })
    }

    fn policy(&self) -> ToolPolicy {
        // Nothing here mutates the host: speaking tools write the host's own
        // ledger, programs run in a discarded container, and memory is the
        // seat's own store. Search reads the network.
        ToolPolicy::read_only()
    }
}

fn summarize(kind: SeatTool, args: &Value, result: &Result<String, String>) -> String {
    let field = |name: &str| {
        args.get(name)
            .and_then(Value::as_str)
            .unwrap_or("")
            .chars()
            .take(160)
            .collect::<String>()
    };
    let outcome = match result {
        Ok(text) => text.lines().next().unwrap_or("").chars().take(120).collect(),
        Err(error) => format!("refused: {}", error.chars().take(120).collect::<String>()),
    };
    match kind {
        SeatTool::WebSearch => format!("{} [{}] -> {outcome}", field("query"), field("provider")),
        SeatTool::RunPython => format!("{} lines -> {outcome}", field("code").lines().count().max(1)),
        SeatTool::Broadcast => format!("@{} -> {outcome}", field("to")),
        SeatTool::Refer => format!("#{} -> {outcome}", field("desk")),
        SeatTool::Complete => format!("complete -> {outcome}"),
        SeatTool::Remember => format!("{}: {}", field("kind"), field("text")),
        SeatTool::Recall => format!("{} -> {outcome}", field("query")),
    }
}
