//! The session store keeps or drops a seat's conversation by mode.

use serde_json::json;

use super::*;

fn spoken() -> SeatSession {
    SeatSession {
        messages: vec![json!({ "role": "system", "content": "s" })],
        activations: 1,
        ..SeatSession::default()
    }
}

#[test]
fn a_persistent_store_hands_back_what_was_put() {
    let store = Sessions::new(SessionMode::Persistent);
    assert!(store.take("lead").is_new());
    store.put("lead", spoken());
    assert_eq!(store.len_of("lead"), 1);
    let again = store.take("lead");
    assert_eq!(again.activations, 1);
    assert!(!again.is_new());
    assert_eq!(store.len_of("lead"), 0, "taken out while in use");
}

#[test]
fn a_fresh_store_forgets_every_session() {
    let store = Sessions::new(SessionMode::Fresh);
    store.put("lead", spoken());
    assert_eq!(store.len_of("lead"), 0);
    assert!(store.take("lead").is_new());
}

#[test]
fn modes_parse_and_name_themselves() {
    for mode in [SessionMode::Fresh, SessionMode::Persistent] {
        assert_eq!(SessionMode::parse(mode.name()), Some(mode));
    }
    assert_eq!(SessionMode::parse("sticky"), None);
}
