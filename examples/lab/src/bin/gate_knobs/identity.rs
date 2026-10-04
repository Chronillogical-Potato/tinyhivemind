//! Chat identity, the desk overlay, and the roster's three states.

use tinyhivemind_core::chat::{GENERAL_DESK, MAIN_THREAD_ID, is_general_chat, same_conversation};
use tinyhivemind_core::desk::{Desk, DeskMember, DeskOrder, DeskSet, ResponderMode};
use tinyhivemind_core::roster::{Person, Roster, RosterMember};
use tinyhivemind_lab::{Res, section};

fn member(id: &str) -> RosterMember {
    RosterMember { id: id.into(), name: Some(id.to_uppercase()) }
}

fn desk(id: &str, name: &str, members: &[&str]) -> Desk {
    Desk {
        id: id.into(),
        name: name.into(),
        description: None,
        members: members.iter().map(|m| (*m).to_owned()).collect(),
        responder_mode: ResponderMode::Lead,
    }
}

fn verdict<T: std::fmt::Debug>(result: Result<T, tinyhivemind_core::error::Error>) -> String {
    match result {
        Ok(value) => format!("ok {value:?}"),
        Err(error) => format!("{error}"),
    }
}

pub fn run() -> Res {
    section("chat identity: the four spellings of the default desk");
    println!("  constants: MAIN_THREAD_ID={MAIN_THREAD_ID:?} GENERAL_DESK={GENERAL_DESK:?}");
    for chat in [None, Some(""), Some("main"), Some("MAIN"), Some("General"), Some("general"), Some("engineering")] {
        println!("  is_general_chat({chat:?}) = {}", is_general_chat(chat));
    }
    for (a, b) in [(Some("main"), Some("General")), (None, Some("")), (Some("eng"), Some("ENG")), (Some("eng"), Some("eng")), (Some("eng"), None)] {
        println!("  same_conversation({a:?}, {b:?}) = {}", same_conversation(a, b));
    }

    section("the desk overlay: declared, added, additions, orders, retired, tombstoned");
    let members = [member("ada"), member("ben"), member("cy"), member("di")];
    let declared = [desk("eng", "Engineering", &["ada", "ben"])];
    let added = [desk("ops", "Operations", &["cy"])];
    let additions = [DeskMember { desk_id: "eng".into(), agent_id: "cy".into() }];
    let orders = [DeskOrder { desk_id: "eng".into(), ordered: vec!["cy".into(), "ada".into(), "ben".into()] }];
    let retired = ["ben".to_owned()];
    let tombstoned = ["di".to_owned()];
    let plain = DeskSet::new(&declared, &[], &[], &[], &[]);
    println!("  declared only          eng members {:?} lead {:?}", verdict(plain.members("eng")), verdict(plain.lead("eng")));
    let extended = DeskSet::new(&declared, &added, &additions, &[], &[]);
    println!("  + added desk + member  eng members {:?}; ops members {:?}", verdict(extended.members("eng")), verdict(extended.members("ops")));
    let ordered = DeskSet::new(&declared, &added, &additions, &orders, &[]);
    println!("  + order [cy,ada,ben]   eng members {:?} lead {:?}", verdict(ordered.members("eng")), verdict(ordered.lead("eng")));
    let gone = DeskSet::new(&declared, &added, &additions, &orders, &retired).with_tombstoned(&tombstoned);
    println!("  + ben retired          eng members {:?}", verdict(gone.members("eng")));
    println!("  resolve_id(\"Engineering\") = {:?}; contains(\"nope\") = {}; iter() yields {} desks", verdict(gone.resolve_id("Engineering")), gone.contains("nope"), gone.iter().count());

    section("every way a desk snapshot is invalid");
    let blank_name = [desk("x", "", &[])];
    let blank_id = [desk("", "X", &[])];
    let dup = [desk("x", "X", &[]), desk("x", "Y", &[])];
    let reserved = [desk("main", "Main", &[])];
    let twins = [desk("a", "Same", &[]), desk("b", "Same", &[])];
    let bad_add = [DeskMember { desk_id: "ghost".into(), agent_id: "ada".into() }];
    let bad_order_desk = [DeskOrder { desk_id: "ghost".into(), ordered: vec![] }];
    let twice = [
        DeskOrder { desk_id: "eng".into(), ordered: vec!["ada".into(), "ben".into()] },
        DeskOrder { desk_id: "eng".into(), ordered: vec!["ben".into(), "ada".into()] },
    ];
    let dup_member = [DeskOrder { desk_id: "eng".into(), ordered: vec!["ada".into(), "ada".into(), "ben".into()] }];
    let stranger = [DeskOrder { desk_id: "eng".into(), ordered: vec!["ada".into(), "ben".into(), "di".into()] }];
    let short = [DeskOrder { desk_id: "eng".into(), ordered: vec!["ada".into()] }];
    let cases: Vec<(&str, DeskSet<'_>)> = vec![
        ("EmptyDeskId", DeskSet::new(&blank_id, &[], &[], &[], &[])),
        ("EmptyDeskName", DeskSet::new(&blank_name, &[], &[], &[], &[])),
        ("DuplicateDeskId", DeskSet::new(&dup, &[], &[], &[], &[])),
        ("ReservedDeskIdentity", DeskSet::new(&reserved, &[], &[], &[], &[])),
        ("UnknownMemberDesk", DeskSet::new(&declared, &[], &bad_add, &[], &[])),
        ("UnknownOrderDesk", DeskSet::new(&declared, &[], &[], &bad_order_desk, &[])),
        ("DuplicateDeskOrder", DeskSet::new(&declared, &[], &[], &twice, &[])),
        ("DuplicateOrderMember", DeskSet::new(&declared, &[], &[], &dup_member, &[])),
        ("UnknownOrderMember", DeskSet::new(&declared, &[], &[], &stranger, &[])),
        ("IncompleteOrder", DeskSet::new(&declared, &[], &[], &short, &[])),
    ];
    for (label, set) in &cases {
        println!("  {label:<22} {}", set.validate().err().map_or("valid".into(), |e| e.to_string()));
    }
    let ambiguous = DeskSet::new(&twins, &[], &[], &[], &[]);
    println!("  {:<22} {}", "AmbiguousDesk", verdict(ambiguous.resolve_id("Same")));
    println!("  {:<22} {}", "UnknownDesk", verdict(plain.resolve_id("nope")));

    section("the roster: active, retired, tombstoned, people");
    let people = [Person { id: "pat".into(), label: "Pat".into() }];
    let retired = ["ben".to_owned()];
    let tombstoned = ["cy".to_owned()];
    let roster = Roster::new(&members, &people, &retired).with_tombstoned(&tombstoned);
    println!("  active {:?}", roster.active_members().map(|m| m.id.as_str()).collect::<Vec<_>>());
    for id in ["ada", "ben", "cy", "zed"] {
        println!(
            "  {id}: active={} registered={} retired_or_gone={}",
            roster.active_member(id).is_some(),
            roster.registered_member(id).is_some(),
            roster.is_retired(id)
        );
    }
    println!("  people {:?}; person(pat) = {:?}", roster.people().map(|p| p.label.as_str()).collect::<Vec<_>>(), roster.person("pat").map(|p| &p.label));
    let blank = [RosterMember { id: " ".into(), name: None }];
    let duplicate = [member("ada"), member("ada")];
    let empty_person = [Person { id: "".into(), label: "x".into() }];
    let dup_person = [people[0].clone(), people[0].clone()];
    println!("  EmptyRosterMemberId     {}", Roster::new(&blank, &[], &[]).validate().err().map_or("valid".into(), |e| e.to_string()));
    println!("  DuplicateRosterMemberId {}", Roster::new(&duplicate, &[], &[]).validate().err().map_or("valid".into(), |e| e.to_string()));
    println!("  EmptyPersonId           {}", Roster::new(&members, &empty_person, &[]).validate().err().map_or("valid".into(), |e| e.to_string()));
    println!("  DuplicatePersonId       {}", Roster::new(&members, &dup_person, &[]).validate().err().map_or("valid".into(), |e| e.to_string()));
    Ok(())
}
