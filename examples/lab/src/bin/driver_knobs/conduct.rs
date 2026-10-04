//! Whole episodes through `Conductor`: scripted seats, walls, snapshots.
//!
//! The host loop is the one `driver::conduct` documents: nudges, turns, brief,
//! record, then steps until none remain. Seats are a pure function of the
//! brief and the host's log, so a run is repeatable and a snapshot is enough
//! to resume it.

use std::collections::BTreeMap;

use tinyhivemind_core::aside::Audience;
use tinyhivemind_core::driver::{
    BroadcastRouting, Channel, CompletionDriver, ConductPolicy, Conductor, ConductorState, Door,
    EpisodeBrief, Event, Step, Turn, starters,
};
use tinyhivemind_core::runtime::speech::{ToolCall, Utterance};
use tinyhivemind_core::runtime::{LogMessage, Sequence, SessionAuthor};
use tinyhivemind_core::telemetry::{TraceEvent, Tracer};
use tinyhivemind_lab::{KeywordRouter, MemoryLog, Res, TraceRig, agent, block_on, section};

use crate::fixture::{Runtime, SEATS, hive, routing_policy};

/// One run's configuration: the driver's knobs, the walls, and the script.
#[derive(Clone, Copy)]
pub struct Scenario {
    pub driver_width: usize,
    pub queue_depth: usize,
    pub budget: Option<u32>,
    pub policy: ConductPolicy,
    pub router_width: usize,
    /// The tester keeps posting in its thread instead of answering.
    pub chatty_tester: bool,
    /// The planner broadcasts three times in one turn.
    pub spam: bool,
    /// The task also needs tests, so the router invites the tester.
    pub wide_task: bool,
    /// The host holds the coder on an approval for one wave.
    pub park: bool,
    /// The coder starts alongside the planner, so work for it must queue.
    pub queue_work: bool,
    /// The router thinks nobody fits a broadcast.
    pub lost_router: bool,
    /// The coder asks and tries to complete in the same turn.
    pub eager_coder: bool,
}

impl Default for Scenario {
    fn default() -> Self {
        Self {
            driver_width: 2,
            queue_depth: 2,
            budget: None,
            policy: ConductPolicy::default(),
            router_width: 1,
            chatty_tester: false,
            spam: false,
            wide_task: false,
            park: false,
            queue_work: false,
            lost_router: false,
            eager_coder: false,
        }
    }
}

/// What a run came to.
#[derive(Debug, Default)]
pub struct Report {
    pub waves: u64,
    pub turns: u64,
    pub conversations: usize,
    pub discharged: u64,
    pub events: BTreeMap<&'static str, usize>,
    pub error: Option<String>,
    pub finished: bool,
    pub rows: Vec<(u64, String, Option<u64>, String)>,
    pub log: Vec<LogMessage>,
    pub snapshots: Vec<(String, usize)>,
    pub widest: usize,
}

fn kind(event: &Event) -> &'static str {
    match event {
        Event::Nudged { .. } => "nudged",
        Event::Parked { .. } => "parked",
        Event::Resumed { .. } => "resumed",
        Event::Broadcast { .. } => "broadcast",
        Event::Unplaced { .. } => "unplaced",
        Event::CompletedByBroadcast { .. } => "completed_by_broadcast",
        Event::Asked { .. } => "asked",
        Event::Handoff { .. } => "handoff",
        Event::Refused { .. } => "refused",
        Event::Discharged { .. } => "discharged",
        Event::Concluded { forced: true, .. } => "concluded_forced",
        Event::Concluded { .. } => "concluded",
    }
}

fn authored(row: &LogMessage, seat: &str, thread: Option<Sequence>) -> bool {
    matches!(&row.author, SessionAuthor::Agent { id, .. } if id == seat) && row.parent == thread
}

/// The seats' lines. Pure in the brief and the log.
fn script(scn: &Scenario, turn: &Turn, brief: &EpisodeBrief, log: &MemoryLog) -> Vec<ToolCall> {
    let say = |u| vec![ToolCall::Speak(u)];
    let done = |m: &str| say(Utterance::CompleteEpisode { message: m.into() });
    let spoke = |seat: &str, thread| {
        log.rows()
            .iter()
            .filter(|r| authored(r, seat, thread))
            .count()
    };
    match (turn.seat.as_str(), &turn.channel) {
        ("planner", Channel::Desk) if scn.spam => (1..=3)
            .map(|n| {
                ToolCall::Speak(Utterance::Broadcast {
                    message: format!("plan the roadmap part {n}"),
                })
            })
            .collect(),
        ("planner", Channel::Desk) if spoke("planner", None) == 0 => say(Utterance::Broadcast {
            message: if scn.wide_task {
                "implement the rust parser and its tests"
            } else {
                "implement the rust parser"
            }
            .into(),
        }),
        ("coder", Channel::Desk) if brief.conversations.iter().any(|v| v.concluded) => {
            done("parser done, edge cases covered")
        }
        ("coder", Channel::Desk) if scn.eager_coder && spoke("coder", None) == 0 => vec![
            ToolCall::Speak(Utterance::Ask {
                to: vec!["tester".into()],
                message: "which edge cases must the parser handle?".into(),
            }),
            ToolCall::Speak(Utterance::CompleteEpisode {
                message: "done before the answer".into(),
            }),
        ],
        ("coder", Channel::Desk) if spoke("coder", None) == 0 => say(Utterance::Ask {
            to: vec!["tester".into()],
            message: "which edge cases must the parser handle?".into(),
        }),
        ("coder", Channel::Desk) => Vec::new(),
        ("tester", Channel::Thread { root, .. })
            if scn.chatty_tester && spoke("tester", Some(*root)) < 8 =>
        {
            say(Utterance::Post {
                message: "still thinking".into(),
            })
        }
        ("tester", Channel::Thread { .. }) => done("empty input; nested quotes"),
        ("tester", Channel::Desk) => done("tests reviewed"),
        (_, Channel::Desk) => done("nothing for me"),
        (_, Channel::Thread { .. }) => done("agreed"),
    }
}

pub struct Host<'t> {
    log: MemoryLog,
    tracer: &'t Tracer<'t>,
    report: Report,
    turn_ids: u64,
}

impl Host<'_> {
    fn audience(only_for: &[String]) -> Audience {
        if only_for.is_empty() {
            Audience::Desk
        } else {
            Audience::Aside {
                members: only_for.to_vec(),
            }
        }
    }

    async fn handle(
        &mut self,
        conductor: &mut Conductor<'_, Runtime>,
        step: Step,
        snapshots: bool,
    ) -> Res {
        match step {
            Step::Event(event) => {
                *self.report.events.entry(kind(&event)).or_default() += 1;
                self.tracer.conducted(&event);
            }
            Step::Note(note) => {
                let only: Vec<String> = note.only_for.into_iter().collect();
                let system = SessionAuthor::System {
                    kind: "conductor".into(),
                    label: "conductor".into(),
                };
                self.log.append(
                    "eng",
                    note.thread,
                    system,
                    &note.body,
                    Self::audience(&only),
                );
            }
            Step::Commit(commit) => {
                let at = self.log.append(
                    "eng",
                    commit.thread,
                    agent(&commit.author),
                    commit.utterance.message(),
                    Self::audience(&commit.only_for),
                );
                conductor.committed(at).await?;
                if let (true, Some(state)) = (snapshots, conductor.snapshot()) {
                    self.report
                        .snapshots
                        .push((serde_json::to_string(&state)?, self.log.rows().len()));
                }
            }
        }
        Ok(())
    }

    async fn drain(&mut self, conductor: &mut Conductor<'_, Runtime>, snapshots: bool) -> Res {
        while let Some(step) = conductor.step()? {
            self.handle(conductor, step, snapshots).await?;
        }
        Ok(())
    }
}

/// The host's own record that the coder was already held once.
fn asked_approval(log: &MemoryLog) -> bool {
    log.rows()
        .iter()
        .any(|row| row.content == "coder is waiting on an approval")
}

fn rows_since(log: &MemoryLog, seat: &str, since: Option<Sequence>) -> Vec<String> {
    log.rows()
        .iter()
        .filter(|row| since.is_none_or(|s| row.sequence > s) && row.parent.is_none())
        .filter(|row| {
            row.audience.admits(
                &tinyhivemind_core::aside::Viewer::Agent { id: seat.into() },
                None,
            ) || matches!(&row.author, SessionAuthor::Agent { id, .. } if id == seat)
        })
        .filter(|row| !matches!(&row.author, SessionAuthor::Agent { id, .. } if id == seat))
        .map(|row| format!("[{}] {}", row.sequence, row.content))
        .collect()
}

/// Play one scenario; `resume` continues from a snapshot over the first
/// `rows` rows of an earlier log instead of opening a new episode.
pub fn play(
    scn: &Scenario,
    tracer: &Tracer<'_>,
    snapshots: bool,
    resume: Option<(&str, &[LogMessage])>,
) -> Report {
    let mut host = Host {
        log: MemoryLog::default(),
        tracer,
        report: Report::default(),
        turn_ids: 0,
    };
    let outcome = block_on(play_inner(scn, &mut host, snapshots, resume));
    let mut report = host.report;
    if let Err(error) = outcome {
        report.error = Some(error.to_string());
    }
    report.log = host.log.rows().to_vec();
    report.rows = host
        .log
        .rows()
        .iter()
        .map(|r| {
            (
                r.sequence.0,
                format!("{:?}", r.author).chars().take(30).collect(),
                r.parent.map(|p| p.0),
                r.content.clone(),
            )
        })
        .collect();
    report
}

async fn play_inner(
    scn: &Scenario,
    host: &mut Host<'_>,
    snapshots: bool,
    resume: Option<(&str, &[LogMessage])>,
) -> Res {
    let hive = hive();
    let driver = CompletionDriver::new(&hive, scn.driver_width)?
        .with_queue_depth(scn.queue_depth)?
        .with_broadcast_budget(scn.budget);
    let router = if scn.lost_router {
        KeywordRouter::new("scripted").none_weight(100_000)
    } else {
        KeywordRouter::new("scripted")
    };
    let policy = routing_policy(scn.router_width);
    let routing = BroadcastRouting {
        primary: Some(&router),
        reasoning: None,
        policy: &policy,
        roster_version: 1,
        thread_context: &[],
    };
    let mut conductor = match resume {
        Some((json, rows)) => {
            for row in rows {
                host.log.append(
                    "eng",
                    row.parent,
                    row.author.clone(),
                    &row.content,
                    row.audience.clone(),
                );
            }
            Conductor::resume(
                &driver,
                routing,
                scn.policy,
                serde_json::from_str::<ConductorState>(json)?,
            )?
        }
        None => {
            let task = host
                .log
                .say("eng", SessionAuthor::Operator, "Build the parser.");
            let request = hive.desk_request(
                if scn.wide_task {
                    "build the rust parser and its tests"
                } else {
                    "build the rust parser"
                },
                Vec::new(),
                None,
                1,
                policy.clone(),
            );
            let plan = hive
                .route_desk(Some(&router), None, &request, None, "planner")
                .await?;
            let door = Door {
                chat: "eng".into(),
                desk_name: "Engineering".into(),
                members: SEATS.iter().map(|s| (*s).to_owned()).collect(),
                starters: if scn.queue_work {
                    vec!["planner".into(), "coder".into()]
                } else if scn.wide_task {
                    starters(&plan, "planner")
                } else {
                    vec!["planner".into()]
                },
                opened_at: task,
            };
            Conductor::open(&driver, routing, scn.policy, door)?
        }
    };
    let result = turn_loop(scn, host, &mut conductor, snapshots).await;
    host.report.waves = conductor.waves();
    host.report.turns = conductor.turns_run();
    host.report.conversations = conductor.conversations();
    host.report.discharged = conductor.discharged();
    host.report.finished = conductor.finished();
    result
}

async fn turn_loop(
    scn: &Scenario,
    host: &mut Host<'_>,
    conductor: &mut Conductor<'_, Runtime>,
    snapshots: bool,
) -> Res {
    for _ in 0..200 {
        host.drain(conductor, snapshots).await?;
        if conductor.finished() {
            break;
        }
        for seat in conductor.parked() {
            conductor.resume_seat(&seat);
        }
        for step in conductor.begin_wave() {
            host.handle(conductor, step, snapshots).await?;
        }
        let turns = conductor.turns()?;
        host.report.widest = host.report.widest.max(turns.len());
        for turn in &turns {
            host.turn_ids += 1;
            let n = host.turn_ids;
            let latest = Some(host.log.head());
            let new_rows = rows_since(&host.log, &turn.seat, turn.since);
            let log = &host.log;
            let brief = conductor.open_turn(turn, latest, new_rows, |root| {
                log.rows()
                    .iter()
                    .filter(|r| r.sequence == root || r.parent == Some(root))
                    .map(|r| format!("{}: {}", r.sequence, r.content))
                    .collect()
            });
            let calls = script(scn, turn, &brief, &host.log);
            host.tracer.emit(TraceEvent::TurnStarted {
                turn: n,
                seat: turn.seat.clone(),
            });
            for call in &calls {
                let tool = match call {
                    ToolCall::Speak(Utterance::Post { .. }) => "post",
                    ToolCall::Speak(Utterance::Broadcast { .. }) => "broadcast",
                    ToolCall::Speak(Utterance::Dm { .. }) => "dm",
                    ToolCall::Speak(Utterance::Ask { .. }) => "ask",
                    ToolCall::Speak(Utterance::CompleteEpisode { .. }) => "complete_episode",
                    ToolCall::Read { .. } => "read",
                };
                host.tracer.emit(TraceEvent::ToolCall {
                    turn: n,
                    seat: turn.seat.clone(),
                    tool: tool.into(),
                    latency_ms: 0,
                    refused: false,
                    reason: None,
                });
            }
            host.tracer.emit(TraceEvent::TurnFinished {
                turn: n,
                seat: turn.seat.clone(),
                // The rendered brief is what the seat would be sent; four
                // characters to the token is the lab's cost proxy.
                input_tokens: brief.render().len() as u64 / 4,
                output_tokens: calls
                    .iter()
                    .map(|c| {
                        if let ToolCall::Speak(u) = c {
                            u.message().len() as u64 / 4
                        } else {
                            0
                        }
                    })
                    .sum(),
                latency_ms: 0,
            });
            if scn.park && turn.seat == "coder" && !asked_approval(&host.log) {
                host.log.say(
                    "eng",
                    SessionAuthor::System {
                        kind: "approval".into(),
                        label: "approval".into(),
                    },
                    "coder is waiting on an approval",
                );
                conductor.record_parked(turn, calls);
            } else {
                conductor.record(turn, calls);
            }
        }
    }
    Ok(())
}

fn summary(report: &Report) -> String {
    let events: Vec<String> = report
        .events
        .iter()
        .map(|(k, v)| format!("{k}x{v}"))
        .collect();
    format!(
        "waves={:<2} turns={:<2} widest={} convs={} discharged={} finished={:<5} {} {}",
        report.waves,
        report.turns,
        report.widest,
        report.conversations,
        report.discharged,
        report.finished,
        report
            .error
            .as_deref()
            .map(|e| format!("ERROR: {e}"))
            .unwrap_or_default(),
        events.join(" ")
    )
}

pub fn run(rig: &TraceRig) -> Res {
    section("Conductor: one episode, then each knob");
    let base = Scenario::default();
    let tracer = rig.tracer("driver:baseline");
    let baseline = play(&base, &tracer, false, None);
    for (seq, author, thread, content) in &baseline.rows {
        println!(
            "  ^{seq:<2} {author:<28} {} {content}",
            thread.map_or("      ".to_owned(), |t| format!("(^{t:<2})"))
        );
    }
    println!("  -> {}", summary(&baseline));

    let mut table: Vec<(String, Scenario)> = Vec::new();
    for width in [1, 2, 4] {
        table.push((
            format!("driver round_width={width}"),
            Scenario {
                driver_width: width,
                wide_task: true,
                router_width: 3,
                ..base
            },
        ));
    }
    table.push((
        "router width 3, driver width 1 (wide task)".into(),
        Scenario {
            driver_width: 1,
            wide_task: true,
            router_width: 3,
            ..base
        },
    ));
    for budget in [None, Some(1), Some(2)] {
        table.push((
            format!("broadcast_budget={budget:?}, planner spams"),
            Scenario {
                budget,
                spam: true,
                ..base
            },
        ));
    }
    for depth in [1, 3] {
        table.push((
            format!("queue_depth={depth}, planner spams"),
            Scenario {
                queue_depth: depth,
                spam: true,
                ..base
            },
        ));
    }
    for (child, wall) in [(2, 60), (6, 60), (30, 60), (6, 3), (6, 8)] {
        table.push((
            format!("child_turn_wall={child} turn_wall={wall}, chatty tester"),
            Scenario {
                policy: ConductPolicy {
                    child_turn_wall: child,
                    turn_wall: wall,
                },
                chatty_tester: true,
                ..base
            },
        ));
    }
    for (label, scn) in [
        (
            "coder is a starter: handoff queues",
            Scenario {
                queue_work: true,
                ..base
            },
        ),
        (
            "router finds nobody for the broadcast",
            Scenario {
                lost_router: true,
                ..base
            },
        ),
        (
            "coder completes while its ask is open",
            Scenario {
                eager_coder: true,
                ..base
            },
        ),
    ] {
        table.push((label.into(), scn));
    }
    table.push((
        "coder parked for one wave (approval)".into(),
        Scenario { park: true, ..base },
    ));
    section("Conductor: DriverKnobs and ConductPolicy swept");
    for (label, scn) in &table {
        let tracer = rig.tracer(&format!("driver:{label}"));
        println!(
            "  {label:<52} {}",
            summary(&play(scn, &tracer, false, None))
        );
    }
    println!("  ConductPolicy::default = {:?}", ConductPolicy::default());
    Ok(())
}
