//! `--live`: every seat turn is a real model call through `OpenRouter`.
//!
//! [`LiveSpeaker`] is a [`Speaker`] like the simulation, so core still
//! decides every round and every ending, and `desks.rs` still routes every
//! referral; only the lines change author. A round's calls run concurrently
//! on scoped threads (the round is already bounded by `round_width`), and the
//! replies are appended in the order core authorized them, so a run is
//! reproducible up to what the model says. `--max-calls` is a hard cap: a
//! round the remaining calls cannot fill ends the run exhausted.
//!
//! Every call is logged to `hive-runs/hive-view/<run>/calls.ndjson` (seat,
//! desk, the request file, the raw reply, the recorded line), next to each
//! request body and response. The key is never written.

pub(crate) mod prompt;
pub(crate) mod provider;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::sync::Mutex;

use serde_json::json;
use tinyhivemind_core::hive::{EpisodePolicy, HiveTurn, Phase, SessionMessage};
use tinyhivemind_lang::LoweredHive;

use crate::Result;
use crate::episode::Speaker;
use crate::load::{Loaded, body, roles};
use crate::script::commit_line;
use prompt::{Brief, parse};
use provider::{Provider, Reply};

/// What `--live` was asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LiveConfig {
    /// `OpenRouter` model id.
    pub(crate) model: String,
    /// Hard cap on calls for the whole run.
    pub(crate) max_calls: u32,
    /// `max_tokens` per call.
    pub(crate) max_tokens: u32,
}

/// Seats played by a model.
#[derive(Debug)]
pub(crate) struct LiveSpeaker {
    provider: Provider,
    briefs: BTreeMap<String, Brief>,
    home: BTreeMap<String, String>,
    max_calls: u32,
    seed: u64,
    log: Mutex<File>,
}

impl LiveSpeaker {
    /// A speaker for every seat of `loaded`, writing under `run`.
    ///
    /// # Errors
    ///
    /// As [`Provider::from_env`], or when the call log cannot be created.
    pub(crate) fn new(loaded: &Loaded, config: &LiveConfig, seed: u64, run: &str) -> Result<Self> {
        let provider = Provider::from_env(&config.model, config.max_tokens, run)?;
        let log = Mutex::new(File::create(provider.dir.join("calls.ndjson"))?);
        Ok(Self {
            briefs: briefs(loaded),
            home: home(&loaded.lowered),
            provider,
            max_calls: config.max_calls,
            seed,
            log,
        })
    }

    /// The run's artifact directory.
    pub(crate) fn dir(&self) -> &std::path::Path {
        &self.provider.dir
    }

    /// The provider, for its usage totals.
    pub(crate) fn provider(&self) -> &Provider {
        &self.provider
    }

    /// Whether the call cap has been reached.
    pub(crate) fn capped(&self) -> bool {
        self.provider.issued() >= u64::from(self.max_calls)
    }

    fn brief(&self, seat: &str) -> Brief {
        self.briefs.get(seat).cloned().unwrap_or_else(|| Brief {
            seat: seat.to_owned(),
            ..Brief::default()
        })
    }

    /// One call, logged, reduced to the line the transcript records.
    fn call(&self, seat: &str, kind: &str, system: &str, user: &str) -> Result<String> {
        let salt = seat.bytes().fold(self.seed, |acc, byte| {
            acc.wrapping_mul(31).wrapping_add(u64::from(byte))
        });
        let (number, reply) = self.provider.complete(system, user, salt)?;
        let (raw, parsed, failure) = match reply {
            Reply::Text(text) => {
                let parsed = parse(&text);
                (text, parsed, None)
            }
            Reply::Failed(message) => (
                String::new(),
                prompt::Parsed {
                    content: "note: (no reply: the call failed)".into(),
                    valid: false,
                },
                Some(message),
            ),
        };
        let entry = json!({
            "call": number,
            "seat": seat,
            "desk": self.home.get(seat),
            "kind": kind,
            "request": format!("call-{number:04}.request.json"),
            "reply": raw,
            "recorded": parsed.content,
            "valid": parsed.valid,
            "failure": failure,
        });
        if let Ok(mut log) = self.log.lock() {
            writeln!(log, "{entry}")?;
        }
        Ok(parsed.content)
    }

    fn turn_line(
        &self,
        turn: &HiveTurn,
        visible: &[SessionMessage],
        policy: &EpisodePolicy,
    ) -> Result<String> {
        let brief = self.brief(&turn.agent_id);
        let carried = if turn.phase == Phase::Commit {
            commit_line(visible, policy).and_then(|line| {
                line.split_whitespace()
                    .find_map(|word| word.strip_prefix('#').map(str::to_owned))
            })
        } else {
            None
        };
        let user = prompt::turn(&brief, turn, visible, &self.home, carried.as_deref());
        self.call(&turn.agent_id, "turn", &prompt::system(&brief), &user)
    }
}

impl Speaker for LiveSpeaker {
    fn speak(
        &mut self,
        turn: &HiveTurn,
        visible: &[SessionMessage],
        policy: &EpisodePolicy,
    ) -> String {
        self.turn_line(turn, visible, policy)
            .unwrap_or_else(|error| format!("note: (no reply: {error})"))
    }

    fn speak_round(
        &mut self,
        round: &[(HiveTurn, Vec<SessionMessage>)],
        policy: &EpisodePolicy,
    ) -> Result<Vec<String>> {
        let this = &*self;
        let results: Vec<std::result::Result<String, String>> = std::thread::scope(|scope| {
            let handles: Vec<_> = round
                .iter()
                .map(|(turn, visible)| {
                    scope.spawn(move || {
                        this.turn_line(turn, visible, policy)
                            .map_err(|error| error.to_string())
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|_| Err("a call thread panicked".into()))
                })
                .collect()
        });
        results
            .into_iter()
            .map(|result| result.map_err(Into::into))
            .collect()
    }

    fn answer(&mut self, seat: &str, incoming: &str, visible: &[SessionMessage]) -> Result<String> {
        let brief = self.brief(seat);
        let user = prompt::answer(&brief, incoming, visible, &self.home);
        let line = self.call(seat, "answer", &prompt::system(&brief), &user)?;
        // An answer goes home by core's return edge, never onward.
        Ok(line
            .lines()
            .filter(|line| !line.contains("@#"))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }

    fn remaining(&self) -> Option<u32> {
        let issued = u32::try_from(self.provider.issued()).unwrap_or(u32::MAX);
        Some(self.max_calls.saturating_sub(issued))
    }
}

/// Each seat's home desk.
fn home(lowered: &LoweredHive) -> BTreeMap<String, String> {
    let mut home = BTreeMap::new();
    for desk in &lowered.desks {
        for member in &desk.members {
            home.entry(member.clone())
                .or_insert_with(|| desk.id.clone());
        }
    }
    home
}

/// Persona, charter and desk for every seat.
fn briefs(loaded: &Loaded) -> BTreeMap<String, Brief> {
    let hive = &loaded.lowered;
    let role_of = roles(hive);
    let charter_of: BTreeMap<&str, String> = loaded
        .package
        .manifest
        .roles
        .iter()
        .map(|role| {
            let text = loaded
                .package
                .documents
                .get(&role.charter)
                .map(|text| body(text).trim().to_owned())
                .unwrap_or_default();
            (role.id.as_str(), text)
        })
        .collect();
    let home = home(hive);
    hive.seats
        .iter()
        .map(|seat| {
            let role = role_of.get(seat.id.as_str()).copied().unwrap_or("");
            let desk = home.get(&seat.id).cloned().unwrap_or_default();
            let declared = hive.desks.iter().find(|candidate| candidate.id == desk);
            let mut links: Vec<String> = Vec::new();
            for link in hive.topology.iter().chain(&hive.referrals) {
                if link.from == desk && !links.contains(&link.to) {
                    links.push(link.to.clone());
                }
            }
            let brief = Brief {
                seat: seat.id.clone(),
                persona: seat.prompt.clone(),
                role: role.to_owned(),
                charter: charter_of.get(role).cloned().unwrap_or_default(),
                desk: desk.clone(),
                desk_name: declared.map(|desk| desk.name.clone()).unwrap_or_default(),
                desk_description: declared
                    .and_then(|desk| desk.description.clone())
                    .unwrap_or_default(),
                links: if hive.desks.len() > 1 {
                    links
                } else {
                    Vec::new()
                },
            };
            (seat.id.clone(), brief)
        })
        .collect()
}
