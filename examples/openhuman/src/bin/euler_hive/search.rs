//! `web_search`: the TinyHumans backend's agent integrations, through the
//! TinyHumans SDK.
//!
//! Two providers sit behind one tool. `exa` is Exa's neural web search
//! (`/agent-integrations/exa/search`; the SDK has no typed method for it, so
//! the SDK's raw client posts it and unwraps the backend envelope). `gemini`
//! is Gemini with Google Search grounding through the SDK's typed
//! `gemini_generate_content`. Both authenticate with `TINYHUMANS_API_KEY`,
//! sent by the SDK as `x-api-key`; inference never goes this way.

use serde_json::{Value, json};
use tinyhumans_sdk::TinyHumansClient;
use tinyhumans_sdk::api::agent_integrations::{
    GeminiContent, GeminiGenerateContentRequest, GeminiTool,
};

/// The TinyHumans backend origin; `TINYHUMANS_BACKEND_URL` overrides it.
pub(super) const DEFAULT_BACKEND: &str = "https://api.tinyhumans.ai";
/// The grounded Gemini model; `EULER_HIVE_GEMINI_MODEL` overrides it.
pub(super) const DEFAULT_GEMINI_MODEL: &str = "gemini-3.8-flash";
/// Results kept from one Exa search.
const EXA_RESULTS: usize = 6;
/// Characters kept from one result's text.
const SNIPPET_CHARS: usize = 700;

/// Which provider answers a query.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Provider {
    /// Exa neural search: ranked pages with text.
    Exa,
    /// Gemini with Google Search grounding: a synthesized answer and sources.
    Gemini,
}

impl Provider {
    /// Read the tool's `provider` argument; absent means Exa.
    pub(super) fn parse(value: Option<&str>) -> anyhow::Result<Self> {
        match value.unwrap_or("exa") {
            "exa" => Ok(Self::Exa),
            "gemini" => Ok(Self::Gemini),
            other => anyhow::bail!("unknown provider {other:?}; use \"exa\" or \"gemini\""),
        }
    }
}

/// A connected search client.
pub(super) struct WebSearch {
    client: TinyHumansClient,
    gemini_model: String,
}

/// One search's condensed result.
#[derive(Clone, Debug, serde::Serialize)]
pub(super) struct Found {
    /// Who answered.
    pub(super) provider: Provider,
    /// What the seat reads.
    pub(super) text: String,
    /// The source URLs it cites.
    pub(super) urls: Vec<String>,
    /// What the backend charged, when it said.
    pub(super) cost_usd: Option<f64>,
}

impl WebSearch {
    /// Build a client for `backend` with the TinyHumans API key.
    pub(super) fn new(backend: &str, api_key: String, gemini_model: String) -> Self {
        Self {
            client: TinyHumansClient::new(backend).with_api_key(Some(api_key)),
            gemini_model,
        }
    }

    /// Run one query.
    ///
    /// # Errors
    ///
    /// Returns the backend's refusal or a transport failure.
    pub(super) async fn search(&self, query: &str, provider: Provider) -> anyhow::Result<Found> {
        match provider {
            Provider::Exa => {
                let data = self
                    .client
                    .raw()
                    .post(
                        "/agent-integrations/exa/search",
                        &json!({"objective": query, "searchQueries": [query]}),
                    )
                    .await?;
                Ok(condense_exa(&data))
            }
            Provider::Gemini => {
                let request = GeminiGenerateContentRequest {
                    contents: vec![GeminiContent {
                        role: Some("user".into()),
                        parts: vec![json!({"text": query})],
                    }],
                    tools: Some(vec![GeminiTool::google_search()]),
                    ..Default::default()
                };
                let response = self
                    .client
                    .agent_integrations()
                    .gemini_generate_content(&self.gemini_model, &request)
                    .await?;
                let urls = response
                    .candidates
                    .iter()
                    .filter_map(|candidate| candidate.grounding_metadata.as_ref())
                    .flat_map(grounding_urls)
                    .collect();
                Ok(Found {
                    provider,
                    text: response.text(),
                    urls,
                    cost_usd: Some(response.cost_usd),
                })
            }
        }
    }
}

/// Keep the ranked pages of an Exa answer, whatever envelope it arrived in.
pub(super) fn condense_exa(data: &Value) -> Found {
    let results = data
        .get("results")
        .or_else(|| data.pointer("/data/results"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut text = String::new();
    let mut urls = Vec::new();
    for (index, result) in results.iter().take(EXA_RESULTS).enumerate() {
        let title = result.get("title").and_then(Value::as_str).unwrap_or("");
        let url = result.get("url").and_then(Value::as_str).unwrap_or("");
        let body = ["text", "summary", "highlights", "excerpts"]
            .iter()
            .find_map(|field| match result.get(*field)? {
                Value::String(text) => Some(text.clone()),
                Value::Array(parts) => Some(
                    parts
                        .iter()
                        .filter_map(Value::as_str)
                        .collect::<Vec<_>>()
                        .join(" … "),
                ),
                _ => None,
            })
            .unwrap_or_default();
        let snippet: String = body.chars().take(SNIPPET_CHARS).collect();
        text.push_str(&format!("[{}] {title}\n{url}\n{snippet}\n\n", index + 1));
        if !url.is_empty() {
            urls.push(url.to_owned());
        }
    }
    if text.is_empty() {
        text = "no results".into();
    }
    let cost_usd = data
        .pointer("/costDollars/total")
        .or_else(|| data.get("cost_usd"))
        .and_then(Value::as_f64);
    Found {
        provider: Provider::Exa,
        text,
        urls,
        cost_usd,
    }
}

fn grounding_urls(metadata: &Value) -> Vec<String> {
    metadata
        .get("groundingChunks")
        .or_else(|| metadata.get("grounding_chunks"))
        .and_then(Value::as_array)
        .map(|chunks| {
            chunks
                .iter()
                .filter_map(|chunk| chunk.pointer("/web/uri").and_then(Value::as_str))
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}
