//! `web_search`: the TinyHumans backend's agent integrations, through the
//! TinyHumans SDK.
//!
//! Two providers sit behind one tool, both through the SDK's typed clients:
//! `parallel` is Parallel's web search (`parallel_search`: ranked pages with
//! excerpts) and `gemini` is Gemini with Google Search grounding
//! (`gemini_generate_content`). Both authenticate with `TINYHUMANS_API_KEY`,
//! sent by the SDK as `x-api-key`; inference never goes this way.
//!
//! Exa (`/agent-integrations/exa/search`, posted with the SDK's raw client
//! as it has no typed method) and TinyFish are wired too, but the backend
//! refuses an API-key caller on both (Exa: 403 "API keys cannot be used on
//! this endpoint"; TinyFish: 404), so `--probe` exercises them and seats are
//! not offered them.

use serde_json::{Value, json};
use tinyhumans_sdk::TinyHumansClient;
use tinyhumans_sdk::api::agent_integrations::{
    GeminiContent, GeminiGenerateContentRequest, GeminiTool, ParallelSearchRequest,
    TinyFishSearchRequest,
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
    /// Parallel's web search: ranked pages with excerpts.
    Parallel,
    /// TinyFish search: ranked pages with snippets.
    Tinyfish,
}

impl Provider {
    /// The providers a seat may pick: those the backend serves an API key.
    /// Exa and TinyFish refuse API-key callers (`--probe` records why).
    pub(super) const SEAT_CHOICES: [&'static str; 2] = ["parallel", "gemini"];

    /// Read the tool's `provider` argument; absent means Parallel.
    pub(super) fn parse(value: Option<&str>) -> anyhow::Result<Self> {
        let value = value.unwrap_or("parallel");
        if !Self::SEAT_CHOICES.contains(&value) {
            anyhow::bail!("unknown provider {value:?}; use one of {:?}", Self::SEAT_CHOICES);
        }
        match value {
            "exa" => Ok(Self::Exa),
            "gemini" => Ok(Self::Gemini),
            "parallel" => Ok(Self::Parallel),
            "tinyfish" => Ok(Self::Tinyfish),
            other => anyhow::bail!("unknown provider {other:?}"),
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
    /// Gemini's grounding metadata, kept whole for the run record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) grounding: Option<Value>,
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
            Provider::Parallel => {
                let response = self
                    .client
                    .agent_integrations()
                    .parallel_search(&ParallelSearchRequest {
                        objective: query.into(),
                        search_queries: vec![query.into()],
                        mode: None,
                        excerpts: None,
                    })
                    .await?;
                let pages = response
                    .results
                    .iter()
                    .map(|page| (page.title.clone(), page.url.clone(), page.excerpts.join(" … ")))
                    .collect::<Vec<_>>();
                Ok(pages_found(provider, &pages, Some(response.cost_usd)))
            }
            Provider::Tinyfish => {
                let response = self
                    .client
                    .agent_integrations()
                    .tinyfish_search(&TinyFishSearchRequest {
                        query: query.into(),
                        location: None,
                        language: None,
                        page: None,
                        include_thumbnail: None,
                    })
                    .await?;
                let pages = response
                    .results
                    .iter()
                    .map(|page| (page.title.clone(), page.url.clone(), page.snippet.clone().unwrap_or_default()))
                    .collect::<Vec<_>>();
                Ok(pages_found(provider, &pages, Some(response.cost_usd)))
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
                    grounding: response
                        .candidates
                        .first()
                        .and_then(|candidate| candidate.grounding_metadata.clone()),
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
        grounding: None,
    }
}

/// Ranked `(title, url, text)` pages as a seat reads them.
pub(super) fn pages_found(provider: Provider, pages: &[(String, String, String)], cost_usd: Option<f64>) -> Found {
    let mut text = String::new();
    let mut urls = Vec::new();
    for (index, (title, url, body)) in pages.iter().take(EXA_RESULTS).enumerate() {
        let snippet: String = body.chars().take(SNIPPET_CHARS).collect();
        text.push_str(&format!("[{}] {title}\n{url}\n{snippet}\n\n", index + 1));
        if !url.is_empty() {
            urls.push(url.clone());
        }
    }
    if text.is_empty() {
        text = "no results".into();
    }
    Found {
        provider,
        text,
        urls,
        cost_usd,
        grounding: None,
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
