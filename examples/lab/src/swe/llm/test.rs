//! Wire parsing, the retry-once rule, and metering through a fake transport.

use std::sync::Mutex;

use serde_json::{Value, json};

use super::*;

struct Script(Mutex<Vec<Result<Value, String>>>);

impl Chat for Script {
    fn send(&self, _body: &Value) -> Result<Value, String> {
        self.0.lock().expect("script lock").remove(0)
    }
}

fn reply(content: &str, tool: Option<(&str, &str)>) -> Value {
    let mut message = json!({ "role": "assistant", "content": content });
    if let Some((name, args)) = tool {
        message["tool_calls"] = json!([{ "id": "c1", "type": "function", "function": { "name": name, "arguments": args } }]);
    }
    json!({ "choices": [{ "message": message }], "usage": { "prompt_tokens": 11, "completion_tokens": 4 } })
}

fn llm(script: Vec<Result<Value, String>>, cap: Option<u64>) -> Llm {
    Llm::new(
        Box::new(Script(Mutex::new(script))),
        "m",
        Meter::new(cap, None),
    )
    .without_retry_pause()
}

#[test]
fn parses_text_tool_calls_and_usage() {
    let done = parse_response(&reply("hi", Some(("bash", r#"{"cmd":"ls"}"#)))).expect("parses");
    assert_eq!(done.content, "hi");
    assert_eq!((done.input_tokens, done.output_tokens), (11, 4));
    assert_eq!(done.tool_calls[0].name, "bash");
    assert_eq!(done.tool_calls[0].args, Ok(json!({ "cmd": "ls" })));
    assert_eq!(done.message["tool_calls"][0]["id"], "c1");
}

#[test]
fn bad_argument_json_is_a_value_not_a_failure() {
    let done = parse_response(&reply("", Some(("bash", "{oops")))).expect("parses");
    assert!(done.tool_calls[0].args.is_err());
}

#[test]
fn empty_arguments_mean_an_empty_object() {
    let done = parse_response(&reply("", Some(("read", "")))).expect("parses");
    assert_eq!(done.tool_calls[0].args, Ok(json!({})));
}

#[test]
fn provider_error_body_is_an_error() {
    let body = json!({ "error": { "message": "rate limited" } });
    assert!(
        parse_response(&body)
            .expect_err("error")
            .contains("rate limited")
    );
    assert!(parse_response(&json!({})).is_err());
}

#[test]
fn missing_usage_falls_back_to_an_estimate() {
    let body = json!({ "choices": [{ "message": { "content": "abcdefgh" } }] });
    let done = parse_response(&body).expect("parses");
    assert_eq!((done.input_tokens, done.output_tokens), (0, 2));
}

#[test]
fn request_body_omits_tools_when_there_are_none() {
    let body = request_body("m", &[json!({ "role": "user", "content": "x" })], &[]);
    assert!(body.get("tools").is_none());
    let with = request_body("m", &[], &[json!({ "type": "function" })]);
    assert_eq!(with["tool_choice"], "auto");
}

#[test]
fn config_escape_protects_quotes_and_newlines() {
    assert_eq!(config_escape("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
}

#[test]
fn retries_once_then_meters_the_success() {
    let client = llm(vec![Err("boom".into()), Ok(reply("ok", None))], None);
    let done = client.complete("lead", &[], &[]).expect("retry succeeds");
    assert_eq!(done.content, "ok");
    let snap = client.meter().snapshot();
    assert_eq!((snap.input, snap.output, snap.calls), (11, 4, 1));
}

#[test]
fn two_failures_abort_without_charging_tokens() {
    let client = llm(vec![Err("a".into()), Err("b".into())], None);
    assert!(matches!(client.complete("x", &[], &[]), Err(Abort::Llm(_))));
    assert_eq!(client.meter().snapshot().input, 0);
}

#[test]
fn a_context_overflow_is_recognised_and_not_retried() {
    let client = llm(
        vec![Err("http 400: maximum context length is 131072 tokens".into())],
        None,
    );
    assert!(matches!(
        client.complete("x", &[], &[]),
        Err(Abort::ContextOverflow(_))
    ));
    assert!(is_context_overflow("provider error: context_length_exceeded"));
    assert!(!is_context_overflow("http 429: rate limited"));
}

#[test]
fn a_reached_cap_refuses_the_call_before_sending() {
    let client = llm(vec![Ok(reply("ok", None))], Some(10));
    client.complete("x", &[], &[]).expect("first call fits");
    assert!(matches!(
        client.complete("x", &[], &[]),
        Err(Abort::TokenCap { .. })
    ));
}

#[test]
fn the_curl_script_carries_the_key_only_when_given() {
    let with = CurlChat::new("http://h/v1/", "sk-test".into(), 5).script(&json!({}));
    assert!(with.contains("Bearer sk-test"));
    assert!(with.contains("url = \"http://h/v1/chat/completions\""));
    let without = CurlChat::new("http://h/v1", String::new(), 5).script(&json!({}));
    assert!(!without.contains("Authorization"));
}
