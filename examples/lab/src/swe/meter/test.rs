//! Meter behaviour: totals, per-seat attribution, and both caps.

use super::{Abort, Meter};

#[test]
fn sums_usage_per_seat_and_in_total() {
    let meter = Meter::new(None, None);
    meter.record("lead", 100, 10);
    meter.record("lead", 50, 5);
    meter.record("tester", 7, 3);
    let snap = meter.snapshot();
    assert_eq!((snap.input, snap.output), (157, 18));
    assert_eq!(snap.seats["lead"].calls, 2);
    assert_eq!(snap.seats["tester"].input, 7);
}

#[test]
fn token_cap_refuses_the_next_call_once_reached() {
    let meter = Meter::new(Some(100), None);
    assert!(meter.check().is_ok());
    meter.record("a", 60, 40);
    assert_eq!(meter.check(), Err(Abort::TokenCap { used: 100, cap: 100 }));
}

#[test]
fn max_turns_reserves_slots_atomically() {
    let meter = Meter::new(None, Some(2));
    assert!(meter.begin_call().is_ok());
    assert!(meter.begin_call().is_ok());
    assert_eq!(
        meter.begin_call(),
        Err(Abort::MaxTurns { calls: 2, cap: 2 })
    );
    assert_eq!(meter.snapshot().calls, 2);
}

#[test]
fn abort_messages_are_readable() {
    assert_eq!(
        Abort::TokenCap { used: 5, cap: 4 }.to_string(),
        "token cap reached (5/4)"
    );
    assert!(Abort::Llm("boom".into()).to_string().contains("boom"));
}
