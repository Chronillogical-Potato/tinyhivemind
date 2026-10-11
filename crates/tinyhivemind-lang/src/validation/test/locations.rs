//! Where each validation finding is located, and agreement between
//! `diagnose` and fail-fast `validate`.
#![allow(clippy::expect_used)] // Tests fail loudly on unexpected results.
use crate::*;
use serde_json::json;

/// A valid package exercising roles, seats, desks, goals, missions, links,
/// context and memory.
fn fixture() -> Package {
    let mut p = Package::default();
    p.manifest.roles.push(Role {
        id: "builder".into(),
        charter: "roles/builder.md".into(),
    });
    p.manifest.seats.push(Seat {
        id: "alice".into(),
        label: "Alice".into(),
        role: "builder".into(),
        prompt: "seats/alice.md".into(),
        template: "agent".into(),
        config: json!({}),
        ..Seat::default()
    });
    p.manifest.desks.push(
        serde_json::from_value(json!({"id":"main","name":"Main","description":null,
            "members":["alice"],"responder_mode":"auto"}))
        .expect("a desk literal decodes"),
    );
    p.manifest.goals.push(Goal {
        id: "g".into(),
        ..Goal::default()
    });
    p.manifest.missions.push(Mission {
        id: "m".into(),
        role: "builder".into(),
        goals: vec!["g".into()],
    });
    p.manifest.topology.push(Link {
        from: "main".into(),
        to: "main".into(),
    });
    p.manifest.referrals.push(Link {
        from: "main".into(),
        to: "main".into(),
    });
    p.manifest.context.push("context/notes.md".into());
    p.documents
        .insert("roles/builder.md".into(), "Build.\n".into());
    p.documents
        .insert("seats/alice.md".into(), "Solve.\n".into());
    p.documents
        .insert("context/notes.md".into(), "<!-- item:a -->\nA\n".into());
    p.memory.identities.push(MemoryIdentity {
        id: "notes".into(),
        namespace: "team:hive/agent:alice".into(),
        kind: MemoryKind::Agent,
        lifecycle: MemoryLifecycle::Run,
        read_only: false,
        fork_parent: None,
    });
    p.memory.bindings.push(SeatMemory {
        seat: "alice".into(),
        identity: "notes".into(),
        reads: vec![MemoryReach {
            at: "team:hive/agent:alice".into(),
            inherit: false,
            descendants: false,
        }],
        recall_at: vec![],
        budget_chars: 10,
        remember: vec![],
    });
    p
}

/// Diagnose `fixture` after `edit`; check `validate` returns the first.
fn located(edit: impl FnOnce(&mut Package)) -> Vec<(String, Option<String>, Rule)> {
    let mut p = fixture();
    edit(&mut p);
    let diagnostics = diagnose(&p);
    let first = validate(&p).err().map(|error| error.to_string());
    assert_eq!(first, diagnostics.first().map(|d| d.message.clone()));
    diagnostics
        .into_iter()
        .map(|d| (d.file, d.pointer, d.rule))
        .collect()
}

fn at(file: &str, pointer: &str, rule: Rule) -> (String, Option<String>, Rule) {
    (file.into(), Some(pointer.into()), rule)
}

fn whole(file: &str, rule: Rule) -> (String, Option<String>, Rule) {
    (file.into(), None, rule)
}

#[test]
fn the_fixture_is_valid() {
    assert_eq!(located(|_| {}), []);
}

#[test]
fn manifest_identity_and_schema_findings_name_their_fields() {
    assert_eq!(
        located(|p| {
            p.manifest.version = 2;
            p.manifest.schema = "other".into();
            p.manifest.id = " ".into();
            p.manifest.roles.push(p.manifest.roles[0].clone());
            p.manifest.missions.push(p.manifest.missions[0].clone());
            p.manifest.goals.push(p.manifest.goals[0].clone());
            p.manifest.desks[0].members.push("alice".into());
        }),
        [
            at("hive.json", "/version", Rule::Schema),
            at("hive.json", "/$schema", Rule::Schema),
            at("hive.json", "/id", Rule::Identity),
            at("hive.json", "/roles/1/id", Rule::Identity),
            at("hive.json", "/goals/1/id", Rule::Identity),
            at("hive.json", "/missions/1/id", Rule::Identity),
            at("hive.json", "/desks/0/members/1", Rule::Identity),
        ]
    );
}

#[test]
fn declared_documents_are_reported_at_the_manifest_field_naming_them() {
    assert_eq!(
        located(|p| {
            p.manifest.roles[0].charter = "seats/builder.md".into();
            p.manifest.seats[0].prompt = "seats/missing.md".into();
            p.manifest.seats[0].template = String::new();
            p.manifest.context[0] = "context/gone.md".into();
            p.documents.insert("a/../b.md".into(), String::new());
        }),
        [
            whole("a/../b.md", Rule::Path),
            at("hive.json", "/roles/0/charter", Rule::Path),
            at("hive.json", "/seats/0/prompt", Rule::MissingDocument),
            at("hive.json", "/seats/0/template", Rule::Identity),
            at("hive.json", "/context/0", Rule::MissingDocument),
        ]
    );
}

#[test]
fn frontmatter_and_markers_are_reported_on_their_markdown_files() {
    assert_eq!(
        located(|p| {
            p.documents
                .insert("roles/builder.md".into(), "---\n{\"id\":\"x\"}\n---\nB\n".into());
            p.documents
                .insert("seats/alice.md".into(), "---\n[]\n---\nS\n".into());
            p.documents
                .insert("context/notes.md".into(), "<!-- item:a -->\n<!-- item:a -->\n".into());
        }),
        [
            whole("roles/builder.md", Rule::Frontmatter),
            whole("seats/alice.md", Rule::Frontmatter),
            whole("context/notes.md", Rule::Context),
        ]
    );
    assert_eq!(
        located(|p| {
            p.documents
                .insert("roles/builder.md".into(), "---\nunclosed".into());
        }),
        [whole("roles/builder.md", Rule::Frontmatter)]
    );
}

#[test]
fn references_name_the_field_that_fails_to_resolve() {
    assert_eq!(
        located(|p| {
            p.manifest.seats[0].role = "ghost".into();
            p.manifest.desks[0].members[0] = "ghost".into();
            p.manifest.goals[0].subgoals.push("ghost".into());
            p.manifest.missions[0].role = "ghost".into();
            p.manifest.missions[0].goals[0] = "ghost".into();
            p.manifest.topology[0].to = "ghost".into();
            p.manifest.referrals[0].from = "ghost".into();
        }),
        [
            at("hive.json", "/seats/0/role", Rule::Reference),
            at("hive.json", "/desks/0/members/0", Rule::Reference),
            at("hive.json", "/goals/0/subgoals/0", Rule::Reference),
            at("hive.json", "/missions/0/role", Rule::Reference),
            at("hive.json", "/missions/0/goals/0", Rule::Reference),
            at("hive.json", "/topology/0/to", Rule::Reference),
            at("hive.json", "/referrals/0/from", Rule::Reference),
        ]
    );
}

#[test]
fn a_goal_cycle_names_a_goal_on_it() {
    assert_eq!(
        located(|p| p.manifest.goals[0].subgoals.push("g".into())),
        [at("hive.json", "/goals/0", Rule::GoalCycle)]
    );
}

#[test]
fn bounds_name_the_policy_field_or_the_constitution_cap() {
    assert_eq!(
        located(|p| p.constitution.max_turns = 0),
        [
            at("constitution.json", "/max_turns", Rule::Bound),
            at("hive.json", "/policies/episode/turn_budget", Rule::Bound),
            at("hive.json", "/policies/conduct", Rule::Bound),
            at("hive.json", "/coordinator/conduct_policy", Rule::Bound),
        ]
    );
    assert_eq!(
        located(|p| p.constitution.max_width = 0)[0],
        at("constitution.json", "/max_width", Rule::Bound)
    );
    assert_eq!(
        located(|p| {
            let e = &mut p.manifest.policies.episode;
            e.round_width = 0;
            e.dominance_cap = 101;
            e.quorum.window = 0;
            e.repetition_cap = 0;
            e.weights.half_life = 0;
            let q = &mut p.manifest.policies;
            q.division.round_width = 0;
            q.routing.choice_option_limit = 0;
            q.digest.input_limit = 0;
            q.aside.enabled = true;
            q.referral.enabled = true;
            q.brevity.window = 0;
            q.exchange.enabled = true;
            p.manifest.coordinator.round_width = 0;
            p.manifest.coordinator.broadcast_budget = Some(0);
        }),
        [
            at("hive.json", "/policies/episode/round_width", Rule::Bound),
            at("hive.json", "/policies/episode/dominance_cap", Rule::Bound),
            at("hive.json", "/policies/division/round_width", Rule::Bound),
            at("hive.json", "/policies/routing", Rule::Bound),
            at("hive.json", "/coordinator/round_width", Rule::Bound),
            at("hive.json", "/coordinator/broadcast_budget", Rule::Bound),
            at("hive.json", "/policies/digest", Rule::Bound),
            at("hive.json", "/policies/aside", Rule::Bound),
            at("hive.json", "/policies/episode/quorum", Rule::Bound),
            at("hive.json", "/policies/episode/repetition_cap", Rule::Bound),
            at("hive.json", "/policies/episode/weights/half_life", Rule::Bound),
            at("hive.json", "/policies/referral", Rule::Bound),
            at("hive.json", "/policies/brevity", Rule::Bound),
            at("hive.json", "/policies/exchange", Rule::Bound),
        ]
    );
    assert_eq!(
        located(|p| {
            p.manifest.policies.episode.revealed_width = 9;
            p.manifest.policies.episode.round_width = 9;
            p.manifest.policies.directory.half_life = 0;
        }),
        [
            at("hive.json", "/policies/episode/revealed_width", Rule::Bound),
            at("hive.json", "/policies/directory/half_life", Rule::Bound),
        ]
    );
}

#[test]
fn memory_findings_name_memory_json_or_the_constitution() {
    assert_eq!(
        located(|p| {
            p.memory.root = "bad".into();
            p.memory.identities.push(p.memory.identities[0].clone());
            p.memory.bindings.push(p.memory.bindings[0].clone());
            p.constitution.protected_namespaces = vec!["nope".into()];
            p.constitution.read_only_memories = vec!["ghost".into()];
        }),
        [
            at("memory.json", "/root", Rule::Namespace),
            at("memory.json", "/identities/1/id", Rule::Identity),
            at("memory.json", "/identities/1/namespace", Rule::Identity),
            at("memory.json", "/bindings/1/seat", Rule::Identity),
            at("constitution.json", "/protected_namespaces/0", Rule::Namespace),
            at("constitution.json", "/read_only_memories/0", Rule::Reference),
            at("memory.json", "/identities/0/namespace", Rule::MemoryAccess),
            at("memory.json", "/identities/1/namespace", Rule::MemoryAccess),
            at("memory.json", "/bindings/0/reads/0/at", Rule::MemoryAccess),
            at("memory.json", "/bindings/1/reads/0/at", Rule::MemoryAccess),
        ]
    );
}

#[test]
fn memory_identity_and_reach_findings_name_the_offending_field() {
    assert_eq!(
        located(|p| {
            let notes = p.memory.identities[0].clone();
            p.memory.identities[0].namespace = "team:hive/project:p".into();
            p.memory.identities.push(MemoryIdentity {
                id: "fork".into(),
                namespace: "team:hive/agent:fork".into(),
                fork_parent: Some("notes".into()),
                ..notes.clone()
            });
            p.memory.identities.push(MemoryIdentity {
                id: "orphan".into(),
                namespace: "team:hive/agent:orphan".into(),
                fork_parent: Some("ghost".into()),
                ..notes.clone()
            });
            p.memory.identities.push(MemoryIdentity {
                id: "outside".into(),
                namespace: "team:else/agent:x".into(),
                ..notes.clone()
            });
            p.memory.identities.push(MemoryIdentity {
                id: "held".into(),
                namespace: "team:hive/project:held".into(),
                kind: MemoryKind::Pool,
                ..notes
            });
            p.constitution.read_only_memories = vec!["held".into()];
            p.constitution.protected_namespaces = vec!["team:hive/project:secret".into()];
            let binding = &mut p.memory.bindings[0];
            binding.identity = "ghost".into();
            binding.budget_chars = 0;
            binding.reads = vec![
                MemoryReach {
                    at: "team:hive".into(),
                    inherit: true,
                    descendants: false,
                },
                MemoryReach {
                    at: "team:hive".into(),
                    inherit: false,
                    descendants: true,
                },
                MemoryReach {
                    at: "team:else".into(),
                    inherit: false,
                    descendants: false,
                },
            ];
        }),
        [
            at("memory.json", "/identities/0/namespace", Rule::MemoryAccess),
            at("memory.json", "/identities/1/fork_parent", Rule::MemoryAccess),
            at("memory.json", "/identities/2/fork_parent", Rule::Reference),
            at("memory.json", "/identities/3/namespace", Rule::MemoryAccess),
            at("memory.json", "/identities/4/read_only", Rule::MemoryAccess),
            at("memory.json", "/bindings/0/identity", Rule::Reference),
            at("memory.json", "/bindings/0/budget_chars", Rule::Bound),
            at("memory.json", "/bindings/0/reads/0/inherit", Rule::MemoryAccess),
            at("memory.json", "/bindings/0/reads/1/at", Rule::MemoryAccess),
            at("memory.json", "/bindings/0/reads/2/at", Rule::MemoryAccess),
        ]
    );
}
