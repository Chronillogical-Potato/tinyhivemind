//! Schema rendering and argument decoding.

use serde_json::json;

use super::*;

fn names(tools: &[Value]) -> Vec<String> {
    tools
        .iter()
        .map(|t| {
            t["function"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

#[test]
fn hive_list_is_bash_then_core_order() {
    assert_eq!(
        names(&tool_list(HIVE_TOOLS)),
        [
            "bash",
            "post",
            "broadcast",
            "ask",
            "complete_episode",
            "read"
        ]
    );
}

#[test]
fn single_list_has_bash_and_complete_only() {
    assert_eq!(
        names(&tool_list(SINGLE_TOOLS)),
        ["bash", "complete_episode"]
    );
}

#[test]
fn renders_required_text_and_counts() {
    let tools = tool_list(&["ask", "read"]);
    let ask = &tools[1]["function"]["parameters"];
    assert_eq!(ask["required"], json!(["to", "message"]));
    assert_eq!(ask["properties"]["to"]["type"], "string");
    let read = &tools[2]["function"]["parameters"];
    assert_eq!(read["properties"]["limit"]["maximum"], 100);
    assert_eq!(read["required"], json!([]));
}

#[test]
fn arguments_accept_a_string_or_a_list_for_to() {
    let one = parse_arguments(&json!({ "to": "tester", "message": "m" }));
    assert_eq!(one.to, ["tester"]);
    let many = parse_arguments(&json!({ "to": ["a", "b"], "limit": 5 }));
    assert_eq!((many.to.len(), many.limit), (2, Some(5)));
    assert_eq!(parse_arguments(&json!({})), Arguments::default());
}
