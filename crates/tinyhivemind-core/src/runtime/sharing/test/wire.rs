//! Wire-shape tests: the serde representation of sharing payload types.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::super::*;
use super::support::{engineering, state};
use crate::aside::Audience;
use crate::runtime::SessionAuthor;
use std::collections::BTreeSet;

#[test]
fn sharing_values_pin_deterministic_wire_shapes() {
    let mut present = BTreeSet::new();
    present.insert(Sequence(12));
    present.insert(Sequence(11));
    let state = SharingState {
        conversation: engineering(),
        watermark: Sequence(10),
        present_above_watermark: present,
    };
    assert_eq!(
        serde_json::to_value(&state).expect("serializes"),
        serde_json::json!({
            "conversation":{"desk_id":"engineering","desk_name":"Engineering","thread_root":null},
            "watermark":10,
            "present_above_watermark":[11,12]
        })
    );
    assert_eq!(
        serde_json::from_value::<SharingState>(serde_json::to_value(&state).expect("serializes"))
            .expect("deserializes"),
        state
    );
    assert_eq!(
        serde_json::to_value(ReinitializeReason::GapTooLarge).expect("serializes"),
        serde_json::json!("gap_too_large")
    );
    let delta = SessionDelta {
        messages: vec![crate::runtime::SessionMessage {
            sequence: Sequence(11),
            author: SessionAuthor::Operator,
            content: "new".into(),
            audience: Audience::Desk,
            elided: None,
        }],
        next_state: state.clone(),
    };
    assert_eq!(
        serde_json::to_value(&delta).expect("serializes"),
        serde_json::json!({
            "messages":[{"sequence":11,"author":{"type":"operator"},"content":"new","audience":{"kind":"desk"},"elided":null}],
            "next_state":{
                "conversation":{"desk_id":"engineering","desk_name":"Engineering","thread_root":null},
                "watermark":10,
                "present_above_watermark":[11,12]
            }
        })
    );
    let delta_plan = SharingPlan::Delta(delta);
    assert_eq!(
        serde_json::to_value(&delta_plan).expect("serializes"),
        serde_json::json!({
            "type":"delta",
            "messages":[{"sequence":11,"author":{"type":"operator"},"content":"new","audience":{"kind":"desk"},"elided":null}],
            "next_state":{
                "conversation":{"desk_id":"engineering","desk_name":"Engineering","thread_root":null},
                "watermark":10,
                "present_above_watermark":[11,12]
            }
        })
    );
    let plan = SharingPlan::Reinitialize {
        reason: ReinitializeReason::WatermarkUnavailable,
    };
    assert_eq!(
        serde_json::to_value(&plan).expect("serializes"),
        serde_json::json!({"type":"reinitialize","reason":"watermark_unavailable"})
    );
    assert_eq!(
        serde_json::from_value::<SharingPlan>(serde_json::to_value(&plan).expect("serializes"))
            .expect("deserializes"),
        plan
    );
}

#[test]
fn sharing_state_deserialization_rejects_an_oversized_present_set() {
    let mut value = serde_json::to_value(state(10)).expect("serializes");
    value["present_above_watermark"] =
        serde_json::json!((11..=11 + PRESENT_SET_LIMIT as u64).collect::<Vec<_>>());
    let error = serde_json::from_value::<SharingState>(value).expect_err("rejects 65 entries");
    assert!(error.to_string().contains("present set has 65 entries"));
}
