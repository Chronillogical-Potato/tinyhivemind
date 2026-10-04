//! The tool surface: specs, calls, rejections, committed utterances, asides.

use tinyhivemind_core::aside::{AsideDecision, AsideInput, AsidePolicy, aside};
use tinyhivemind_core::dispatch::DispatchConversation;
use tinyhivemind_core::mention::{Mention, MentionTarget};
use tinyhivemind_core::runtime::speech::fence::extract_post;
use tinyhivemind_core::runtime::speech::{
    CallArguments, CommitRequest, ParameterKind, READ_DEFAULT, READ_MAX, ToolCall, Utterance,
    addressed_peers, check_recipients, commit_utterance, interpret, read_limit, tool_specs,
};
use tinyhivemind_core::telemetry::{TraceEvent, Tracer};
use tinyhivemind_lab::{Res, World, section};

fn world() -> World {
    World::new()
        .agent("alice")
        .agent("bob")
        .agent("carol")
        .agent("dave")
        .agent("erin")
        .agent("gus")
        .desk(
            "eng",
            "Engineering",
            "Build it",
            &["alice", "bob", "carol", "gus"],
        )
        .desk("ops", "Operations", "Run it", &["dave", "erin"])
        .retire("gus")
        .retire("erin")
}

fn call(name: &str, message: Option<&str>, to: &[&str], limit: Option<u64>) -> String {
    let to: Vec<String> = to.iter().map(|id| (*id).to_owned()).collect();
    match interpret(
        name,
        &CallArguments {
            message,
            to: &to,
            limit,
        },
    ) {
        Ok(ToolCall::Speak(utterance)) => format!("Speak({utterance:?})"),
        Ok(ToolCall::Read { limit }) => format!("Read(limit={limit})"),
        Err(rejection) => format!("REFUSED: {rejection}"),
    }
}

fn mention(id: &str, offset: usize) -> Mention {
    Mention {
        target: MentionTarget::Agent { id: id.into() },
        text: format!("@{id}"),
        offset,
        quiet: false,
    }
}

pub fn run(tracer: &Tracer<'_>) -> Res {
    section("tool_specs: the surface a host renders");
    for spec in tool_specs() {
        let params: Vec<String> = spec
            .parameters
            .iter()
            .map(|p| {
                let kind = match p.kind {
                    ParameterKind::Text => "text".to_owned(),
                    ParameterKind::TextList => "text[]".to_owned(),
                    ParameterKind::Count { default, min, max } => {
                        format!("count {min}..={max} (default {default})")
                    }
                };
                format!("{}:{kind}{}", p.name, if p.required { "*" } else { "" })
            })
            .collect();
        println!(
            "  {:<17} {:<44} {} chars of contract text",
            spec.name,
            params.join(" "),
            spec.description.len()
        );
    }
    println!(
        "  READ_DEFAULT={READ_DEFAULT} READ_MAX={READ_MAX}; read_limit(None)={} (Some(0))={} (Some(10_000))={}",
        read_limit(None),
        read_limit(Some(0)),
        read_limit(Some(10_000))
    );

    section("interpret: a seat's call becomes an utterance, or a sentence it can read");
    let cases: [(&str, Option<&str>, &[&str], Option<u64>); 16] = [
        ("post", Some("  B holds at 10^18  "), &[], None),
        ("post", Some("   "), &[], None),
        ("broadcast", Some("someone take the migration"), &[], None),
        ("complete_episode", Some("done: fixed"), &[], None),
        ("close", Some("alias of complete_episode"), &[], None),
        ("dm", Some("agree?"), &["@bob", "bob", " carol "], None),
        ("dm", Some("agree?"), &[], None),
        ("ask", Some("what port?"), &["bob"], None),
        ("ask", Some("what port?"), &["bob", "carol"], None),
        ("ask", Some("what port?"), &[], None),
        (
            "ask_teammates",
            Some("agree on the port"),
            &["bob", "carol"],
            None,
        ),
        ("ask_teammates", Some("agree on the port"), &["bob"], None),
        ("read", None, &[], None),
        ("read", None, &[], Some(500)),
        ("search", Some("x"), &[], None),
        ("post", None, &[], None),
    ];
    for (turn, (name, message, to, limit)) in cases.into_iter().enumerate() {
        let outcome = call(name, message, to, limit);
        println!("  {name:<17} -> {outcome}");
        let reason = outcome.strip_prefix("REFUSED: ").map(str::to_owned);
        tracer.emit(TraceEvent::ToolCall {
            turn: turn as u64,
            seat: "alice".into(),
            tool: name.into(),
            latency_ms: 0,
            refused: reason.is_some(),
            reason,
        });
    }

    let world = world();
    let roster = world.roster();
    let desks = world.desks();
    section("check_recipients and addressed_peers");
    for (label, utterance) in [
        (
            "dm to a retired seat",
            Utterance::Dm {
                to: vec!["gus".into()],
                message: "hi".into(),
            },
        ),
        (
            "dm to oneself",
            Utterance::Dm {
                to: vec!["alice".into()],
                message: "hi".into(),
            },
        ),
        (
            "dm to self and bob",
            Utterance::Dm {
                to: vec!["alice".into(), "bob".into()],
                message: "hi".into(),
            },
        ),
        (
            "ask oneself among others",
            Utterance::Ask {
                to: vec!["alice".into(), "bob".into()],
                message: "hi".into(),
            },
        ),
        (
            "post",
            Utterance::Post {
                message: "hi @bob".into(),
            },
        ),
    ] {
        println!(
            "  {label:<26} -> {:?}",
            check_recipients(&utterance, "alice", &roster)
        );
    }
    let post = Utterance::Post {
        message: "@bob and @carol please look, @bob again".into(),
    };
    println!(
        "  addressed_peers of a post naming bob, carol, bob: {:?}",
        addressed_peers(&post, "alice", &roster, &desks)
    );
    for (label, utterance) in [
        (
            "post",
            Utterance::Post {
                message: "m".into(),
            },
        ),
        (
            "broadcast",
            Utterance::Broadcast {
                message: "m".into(),
            },
        ),
        (
            "complete_episode",
            Utterance::CompleteEpisode {
                message: "m".into(),
            },
        ),
        (
            "ask",
            Utterance::Ask {
                to: vec!["bob".into()],
                message: "m".into(),
            },
        ),
    ] {
        println!(
            "  {label:<17} closing={} completes_episode={} broadcasting={} asks={:?}",
            utterance.closing(),
            utterance.completes_episode(),
            utterance.broadcasting(),
            utterance.asks()
        );
    }
    let wire: Utterance = serde_json::from_str(r#"{"kind":"close","message":"wire alias"}"#)?;
    println!("  wire form {{\"kind\":\"close\"}} decodes to {wire:?}");

    section("commit_utterance: the row a call becomes, under each AsidePolicy");
    let conversation = DispatchConversation {
        desk_id: "eng".into(),
        thread_root: None,
    };
    let policies = [
        ("asides off (default)", AsidePolicy::DEFAULT),
        (
            "asides on",
            AsidePolicy {
                enabled: true,
                max_members: 2,
                max_messages: 3,
                must_surface: false,
                require_thread: false,
            },
        ),
        (
            "asides on, 1 member max",
            AsidePolicy {
                enabled: true,
                max_members: 1,
                max_messages: 3,
                must_surface: false,
                require_thread: false,
            },
        ),
    ];
    let utterances = [
        Utterance::Post {
            message: "plain post".into(),
        },
        Utterance::Dm {
            to: vec!["bob".into()],
            message: "just between us".into(),
        },
        Utterance::Dm {
            to: vec!["bob".into(), "carol".into()],
            message: "the three of us".into(),
        },
        Utterance::Post {
            message: "!aside @bob the marker spelling".into(),
        },
        Utterance::Ask {
            to: vec!["bob".into()],
            message: "what port?".into(),
        },
    ];
    for (label, policy) in policies {
        println!("  -- {label}");
        for utterance in &utterances {
            let committed = commit_utterance(&CommitRequest {
                utterance,
                speaker_id: "alice",
                conversation: &conversation,
                aside: policy,
                spent: 0,
                unsettled: false,
                roster: &roster,
                desks: &desks,
            })?;
            let ids: Vec<String> = committed
                .mentions
                .iter()
                .filter_map(|m| match &m.target {
                    MentionTarget::Agent { id } => Some(id.clone()),
                    _ => None,
                })
                .collect();
            println!(
                "    {:<44} audience={:?} mentions={ids:?} refusal={:?}",
                format!("{:?}", utterance.message()),
                committed.audience,
                committed.refusal
            );
        }
    }

    section("aside(): every reason an aside is refused");
    let on = AsidePolicy {
        enabled: true,
        max_members: 2,
        max_messages: 2,
        must_surface: true,
        require_thread: false,
    };
    let input = |author: &str, to: &[&str], spent: usize, unsettled: bool, thread: Option<u64>| {
        AsideInput {
            conversation: DispatchConversation {
                desk_id: "eng".into(),
                thread_root: thread,
            },
            author_id: author.into(),
            mentions: to
                .iter()
                .enumerate()
                .map(|(i, id)| mention(id, i))
                .collect(),
            spent,
            unsettled,
        }
    };
    let quiet = AsideInput {
        mentions: vec![Mention {
            quiet: true,
            ..mention("bob", 0)
        }],
        ..input("alice", &[], 0, false, None)
    };
    let cases: Vec<(&str, AsidePolicy, AsideInput)> = vec![
        (
            "Disabled",
            AsidePolicy::DEFAULT,
            input("alice", &["bob"], 0, false, None),
        ),
        (
            "SourceInactive",
            on,
            input("erin", &["dave"], 0, false, None),
        ),
        (
            "AuthorNotOnDesk",
            on,
            input("dave", &["alice"], 0, false, None),
        ),
        ("NoAudience", on, quiet),
        ("SelfOnly", on, input("alice", &["alice"], 0, false, None)),
        (
            "AudienceTooLarge",
            AsidePolicy {
                max_members: 1,
                ..on
            },
            input("alice", &["bob", "carol"], 0, false, None),
        ),
        (
            "TargetInactive",
            on,
            input("alice", &["gus"], 0, false, None),
        ),
        (
            "TargetNotOnDesk",
            on,
            input("alice", &["dave"], 0, false, None),
        ),
        (
            "ThreadRequired",
            AsidePolicy {
                require_thread: true,
                ..on
            },
            input("alice", &["bob"], 0, false, None),
        ),
        ("BudgetSpent", on, input("alice", &["bob"], 2, false, None)),
        (
            "UnsettledAside",
            on,
            input("alice", &["bob"], 0, true, None),
        ),
        (
            "(allowed)",
            on,
            input("alice", &["bob", "carol"], 1, false, None),
        ),
    ];
    for (label, policy, input) in cases {
        let decision = aside(policy, &input, &roster, &desks)?;
        let shown = match decision {
            AsideDecision::None { reason } => format!("None({reason:?})"),
            AsideDecision::One { audience } => format!("One({audience:?})"),
        };
        println!("  expect {label:<17} -> {shown}");
    }

    section("fence::extract_post: the delimiter for a seat that cannot call tools");
    for raw in [
        "no fence at all",
        "<<<POST the asymmetric form POST>>>",
        "<<<POST>>> the symmetric form <<<POST>>>",
        "quoted: <<<POST ...>>> then <<<POST real answer POST>>>",
    ] {
        println!("  {raw:?} -> {:?}", extract_post(raw));
    }
    Ok(())
}
