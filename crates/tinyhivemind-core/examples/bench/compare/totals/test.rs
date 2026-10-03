//! Unit tests for [`super::Totals`]: that pricing the whole table reaches
//! every arm, `ladder_directed` included.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::Totals;
use crate::cost::CostModel;

/// A cost model with every constant moved off [`CostModel::DEFAULT`], so a
/// row still carrying the default after `priced_at` is easy to catch.
const MOVED: CostModel = CostModel {
    tokens_per_row: 7,
    tokens_per_turn: 8,
    prompt_base: 9,
    ttft_ms: 10,
    decode_tok_s: 11,
};

/// Every active arm uses the selected cost model.
#[test]
fn priced_at_reaches_every_active_arm() {
    let totals = Totals::priced_at(MOVED);
    for arm in totals.arms() {
        assert_eq!(
            arm.model, MOVED,
            "an arm in Totals::arms() was not priced at the model priced_at was given",
        );
    }
}
