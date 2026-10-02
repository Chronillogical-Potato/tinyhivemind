//! Unit tests for max-min fair character allocation.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::*;

fn tight(total: usize, min_useful: usize) -> BudgetPolicy {
    BudgetPolicy {
        total_chars: total,
        min_useful_chars: min_useful,
    }
}

fn granted(shares: &[BudgetShare]) -> Vec<(&str, usize)> {
    shares
        .iter()
        .map(|share| (share.source_id.as_str(), share.granted))
        .collect()
}

#[test]
fn grants_every_source_in_full_when_the_budget_covers_them() {
    let requests = [
        BudgetRequest::new("pins", 300),
        BudgetRequest::new("threads", 200),
    ];
    let shares = allocate_chars(&requests, &tight(1_000, 100));
    assert_eq!(granted(&shares), [("pins", 300), ("threads", 200)]);
    assert!(shares.iter().all(|share| share.omitted == 0));
    assert!(
        shares
            .iter()
            .all(|share| share.verdict == BudgetVerdict::Whole)
    );
}

#[test]
fn splits_an_oversubscribed_budget_equally_between_equal_claims() {
    let requests = [
        BudgetRequest::new("pins", 1_000),
        BudgetRequest::new("threads", 1_000),
    ];
    let shares = allocate_chars(&requests, &tight(500, 200));
    assert_eq!(granted(&shares), [("pins", 250), ("threads", 250)]);
    assert!(shares.iter().all(|share| share.omitted == 750));
    assert!(
        shares
            .iter()
            .all(|share| share.verdict == BudgetVerdict::Truncated)
    );
}

#[test]
fn redistributes_what_a_small_source_does_not_need() {
    let requests = [
        BudgetRequest::new("pins", 50),
        BudgetRequest::new("threads", 1_000),
        BudgetRequest::new("board", 1_000),
    ];
    let shares = allocate_chars(&requests, &tight(1_000, 100));
    assert_eq!(
        granted(&shares),
        [("pins", 50), ("threads", 475), ("board", 475)]
    );
    assert_eq!(shares[0].verdict, BudgetVerdict::Whole);
    assert_eq!(shares[1].omitted, 525);
}

#[test]
fn drops_a_source_cut_below_the_useful_floor_and_marks_what_it_omitted() {
    let requests = [
        BudgetRequest::new("threads", 1_000),
        BudgetRequest::new("pins", 100),
    ];
    let shares = allocate_chars(&requests, &tight(250, 200));
    assert_eq!(granted(&shares), [("threads", 0), ("pins", 100)]);
    assert_eq!(shares[0].verdict, BudgetVerdict::Dropped);
    assert_eq!(shares[0].omitted, 1_000);
    assert_eq!(shares[1].verdict, BudgetVerdict::Whole);
}

#[test]
fn the_greediest_claim_yields_first_and_frees_its_share_for_the_rest() {
    let requests = [
        BudgetRequest::new("digest", 1_000),
        BudgetRequest::new("threads", 600),
        BudgetRequest::new("pins", 300),
    ];
    let shares = allocate_chars(&requests, &tight(500, 200));
    assert_eq!(
        granted(&shares),
        [("digest", 0), ("threads", 250), ("pins", 250)]
    );
    assert_eq!(shares[0].verdict, BudgetVerdict::Dropped);
    assert_eq!(shares[1].verdict, BudgetVerdict::Truncated);
    assert_eq!(shares[2].verdict, BudgetVerdict::Truncated);
}

#[test]
fn serves_as_many_equal_claims_as_the_budget_usefully_can() {
    let requests = [
        BudgetRequest::new("board", 1_000),
        BudgetRequest::new("digest", 1_000),
        BudgetRequest::new("threads", 1_000),
    ];
    let shares = allocate_chars(&requests, &tight(500, 200));
    assert_eq!(
        granted(&shares),
        [("board", 250), ("digest", 250), ("threads", 0)]
    );
    assert_eq!(shares[2].verdict, BudgetVerdict::Dropped);
}

#[test]
fn leaves_the_integer_remainder_unspent_rather_than_favouring_a_position() {
    let requests = [
        BudgetRequest::new("a", 7),
        BudgetRequest::new("b", 7),
        BudgetRequest::new("c", 7),
    ];
    let shares = allocate_chars(&requests, &tight(10, 0));
    assert_eq!(granted(&shares), [("a", 3), ("b", 3), ("c", 3)]);
    assert_eq!(shares.iter().map(|share| share.granted).sum::<usize>(), 9);
}

#[test]
fn allocates_the_same_shares_however_the_requests_are_ordered() {
    let requests = [
        BudgetRequest::new("pins", 50),
        BudgetRequest::new("digest", 1_000),
        BudgetRequest::new("threads", 600),
        BudgetRequest::new("board", 300),
    ];
    let policy = tight(700, 200);
    let mut expected: Vec<BudgetShare> = allocate_chars(&requests, &policy);
    expected.sort_by(|held, next| held.source_id.cmp(&next.source_id));

    for rotation in 1..requests.len() {
        let mut rotated = requests.to_vec();
        rotated.rotate_left(rotation);
        let mut shares = allocate_chars(&rotated, &policy);
        shares.sort_by(|held, next| held.source_id.cmp(&next.source_id));
        assert_eq!(shares, expected, "rotation by {rotation}");
    }

    let mut reversed = requests.to_vec();
    reversed.reverse();
    let mut shares = allocate_chars(&reversed, &policy);
    shares.sort_by(|held, next| held.source_id.cmp(&next.source_id));
    assert_eq!(shares, expected, "reversed");
}

#[test]
fn a_zero_budget_drops_every_source_that_wanted_anything() {
    let requests = [
        BudgetRequest::new("pins", 500),
        BudgetRequest::new("threads", 1),
    ];
    let shares = allocate_chars(&requests, &tight(0, 200));
    assert_eq!(granted(&shares), [("pins", 0), ("threads", 0)]);
    assert!(
        shares
            .iter()
            .all(|share| share.verdict == BudgetVerdict::Dropped)
    );
    assert_eq!(shares[0].omitted, 500);
}

#[test]
fn no_requests_allocate_nothing() {
    let found = allocate_chars(&[], &BudgetPolicy::DEFAULT);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_source_wanting_nothing_is_whole_and_omits_nothing() {
    let requests = [BudgetRequest::new("empty", 0)];
    let shares = allocate_chars(&requests, &tight(0, 200));
    assert_eq!(shares[0].granted, 0);
    assert_eq!(shares[0].omitted, 0);
    assert_eq!(shares[0].verdict, BudgetVerdict::Whole);
}

#[test]
fn an_unbounded_claim_does_not_overflow_the_running_sum() {
    let requests = [
        BudgetRequest::new("firehose", usize::MAX),
        BudgetRequest::new("pins", usize::MAX),
    ];
    let shares = allocate_chars(&requests, &tight(1_000, 200));
    assert_eq!(granted(&shares), [("firehose", 500), ("pins", 500)]);
    assert_eq!(shares[0].omitted, usize::MAX - 500);
}

#[test]
fn the_default_policy_is_derived_from_the_stated_message_budget() {
    assert_eq!(BudgetPolicy::DEFAULT.total_chars, 18_000);
    assert_eq!(BudgetPolicy::DEFAULT.min_useful_chars, 200);
    assert_eq!(BudgetPolicy::default(), BudgetPolicy::DEFAULT);
}

fn assert_wire_round_trip<T>(value: &T, expected: serde_json::Value)
where
    T: serde::Serialize + serde::de::DeserializeOwned + Eq + std::fmt::Debug,
{
    assert_eq!(serde_json::to_value(value).expect("serializes"), expected);
    assert_eq!(
        serde_json::from_value::<T>(expected).expect("deserializes"),
        *value
    );
}

#[test]
fn a_request_pins_its_wire_form() {
    assert_wire_round_trip(
        &BudgetRequest::new("pins", 300),
        serde_json::json!({ "source_id": "pins", "wanted": 300 }),
    );
}

#[test]
fn a_policy_pins_its_wire_form() {
    assert_wire_round_trip(
        &tight(500, 200),
        serde_json::json!({ "total_chars": 500, "min_useful_chars": 200 }),
    );
}

#[test]
fn a_share_pins_its_wire_form() {
    assert_wire_round_trip(
        &BudgetShare {
            source_id: "digest".into(),
            granted: 0,
            omitted: 1_000,
            verdict: BudgetVerdict::Dropped,
        },
        serde_json::json!({
            "source_id": "digest",
            "granted": 0,
            "omitted": 1_000,
            "verdict": "dropped",
        }),
    );
}

#[test]
fn every_verdict_pins_its_wire_spelling() {
    for (verdict, spelling) in [
        (BudgetVerdict::Whole, "whole"),
        (BudgetVerdict::Truncated, "truncated"),
        (BudgetVerdict::Dropped, "dropped"),
    ] {
        assert_wire_round_trip(&verdict, serde_json::json!(spelling));
    }
}

#[test]
fn arbitrary_claims_never_overspend_and_never_leave_a_fragment() {
    let mut state = 0x0b0d_6e75_c0de_u64;
    let mut next = move || {
        state ^= state << 7;
        state ^= state >> 9;
        state
    };

    for _ in 0..2_000 {
        let count = usize::try_from(next() % 7).expect("bounded count");
        let requests: Vec<BudgetRequest> = (0..count)
            .map(|index| {
                let wanted = usize::try_from(next() % 2_000).expect("bounded want");
                BudgetRequest::new(format!("source-{index}"), wanted)
            })
            .collect();
        let policy = tight(
            usize::try_from(next() % 3_000).expect("bounded budget"),
            usize::try_from(next() % 400).expect("bounded floor"),
        );

        let shares = allocate_chars(&requests, &policy);
        assert_eq!(shares.len(), requests.len());
        let spent: usize = shares.iter().map(|share| share.granted).sum();
        assert!(
            spent <= policy.total_chars,
            "{shares:?} overspent {policy:?}"
        );

        for (request, share) in requests.iter().zip(&shares) {
            assert_eq!(share.source_id, request.source_id);
            assert!(share.granted <= request.wanted);
            assert_eq!(share.omitted, request.wanted - share.granted);
            match share.verdict {
                BudgetVerdict::Whole => assert_eq!(share.granted, request.wanted),
                BudgetVerdict::Truncated => {
                    assert!(share.granted > 0 && share.granted < request.wanted);
                    assert!(
                        share.granted >= policy.min_useful_chars,
                        "kept a fragment: {share:?} under {policy:?}"
                    );
                }
                BudgetVerdict::Dropped => {
                    assert_eq!(share.granted, 0);
                    assert!(request.wanted > 0);
                }
            }
        }

        let mut reversed = requests.clone();
        reversed.reverse();
        let mut mirrored = allocate_chars(&reversed, &policy);
        mirrored.reverse();
        assert_eq!(mirrored, shares, "order changed the allocation");
    }
}
