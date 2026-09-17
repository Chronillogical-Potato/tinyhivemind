//! Strict JSON Schema matching the benchmark's typed questions.

use serde_json::{Value, json};
use tinyjevclient::{EvaluationRequest, Question};

pub(super) fn response_schema(request: &EvaluationRequest) -> Value {
    let properties = request
        .questions
        .iter()
        .map(|(id, question)| {
            let schema = match question {
                Question::Choice(choice) => {
                    let options: Vec<&str> = choice.criteria.keys().map(String::as_str).collect();
                    let probability_properties = choice
                        .criteria
                        .keys()
                        .map(|option| (option.clone(), probability_schema()))
                        .collect::<serde_json::Map<_, _>>();
                    json!({
                        "type": "object",
                        "properties": {
                            "type": {"type": "string", "const": "choice"},
                            "choice": {"type": "string", "enum": options},
                            "probabilities": {
                                "type": "object",
                                "properties": probability_properties,
                                "required": choice.criteria.keys().collect::<Vec<_>>(),
                                "additionalProperties": false
                            },
                            "confidence": probability_schema()
                        },
                        "required": ["type", "choice", "probabilities", "confidence"],
                        "additionalProperties": false
                    })
                }
                Question::Score(score) => {
                    let keys: Vec<String> =
                        (0..score.criteria.len()).map(|i| i.to_string()).collect();
                    let probabilities = keys
                        .iter()
                        .map(|key| (key.clone(), probability_schema()))
                        .collect::<serde_json::Map<_, _>>();
                    let legend = keys
                        .iter()
                        .enumerate()
                        .map(|(index, key)| (key.clone(), score.criteria[index].clone()))
                        .collect::<serde_json::Map<_, _>>();
                    json!({
                        "type": "object",
                        "properties": {
                            "type": {"type": "string", "const": "score"},
                            "score": {"type": "number", "minimum": 0, "maximum": score.criteria.len() - 1},
                            "legend": {"type": "object", "const": legend},
                            "probabilities": {
                                "type": "object", "properties": probabilities,
                                "required": keys, "additionalProperties": false
                            },
                            "confidence": probability_schema()
                        },
                        "required": ["type", "score", "legend", "probabilities", "confidence"],
                        "additionalProperties": false
                    })
                }
                Question::Noul(_) => json!({
                    "type": "object",
                    "properties": {
                        "type": {"type": "string", "const": "noul"},
                        "noul": probability_schema()
                    },
                    "required": ["type", "noul"],
                    "additionalProperties": false
                }),
            };
            (id.clone(), schema)
        })
        .collect::<serde_json::Map<_, _>>();
    json!({
        "type": "object",
        "properties": {"answers": {
            "type": "object",
            "properties": properties,
            "required": request.questions.keys().collect::<Vec<_>>(),
            "additionalProperties": false
        }},
        "required": ["answers"],
        "additionalProperties": false
    })
}

fn probability_schema() -> Value {
    json!({"type": "number", "minimum": 0, "maximum": 1})
}
