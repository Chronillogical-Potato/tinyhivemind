//! Live TypeSafe transport and routing request construction.

use tinyhivemind::Sequence;
use tinyhivemind::responder::Probability;
use tinyhivemind_embed::{
    ConversationKind, ConversationRef, RouteCandidate, RoutingPolicy, RoutingRequest, RoutingSource,
};
use tinyhivemind_typesafe::{
    Error, SystemOneRequest, SystemOneResponse, SystemOneTransport, SystemOneTransportFuture,
};

#[derive(Clone, Debug)]
pub(super) struct Transport {
    client: reqwest::Client,
    api_key: String,
}

impl Transport {
    pub(super) fn new(api_key: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key,
        }
    }
}

impl SystemOneTransport for Transport {
    fn evaluate<'a>(&'a self, request: &'a SystemOneRequest) -> SystemOneTransportFuture<'a> {
        Box::pin(async move {
            let response = self
                .client
                .post("https://api.typesafe.ai/v1/systemone")
                .bearer_auth(&self.api_key)
                .json(request)
                .send()
                .await
                .map_err(|error| Error::Transport {
                    status: None,
                    message: error.to_string(),
                })?;
            let status = response.status();
            if !status.is_success() {
                let message = response.text().await.unwrap_or_default();
                return Err(Error::Transport {
                    status: Some(status.as_u16()),
                    message,
                });
            }
            response
                .json::<SystemOneResponse>()
                .await
                .map_err(|error| Error::Transport {
                    status: Some(status.as_u16()),
                    message: error.to_string(),
                })
        })
    }
}

pub(super) fn request(
    message: &str,
    source: RoutingSource,
    candidates: Vec<RouteCandidate>,
    roster_version: u64,
) -> RoutingRequest {
    RoutingRequest {
        message: message.into(),
        source,
        conversation: ConversationRef {
            id: "pe1006".into(),
            kind: ConversationKind::Desk,
            thread_root: Some(Sequence(1)),
        },
        desk_purpose: Some("derive and independently verify an exact PE1006 residue".into()),
        thread_context: vec!["Only explicit completion ends an assignment".into()],
        candidates,
        roster_version,
        policy: RoutingPolicy {
            minimum_confidence: Probability::ZERO,
            high_impact_minimum_confidence: Probability::ZERO,
            clarification_threshold: Probability::ONE,
            high_impact_threshold: Probability::ONE,
            round_width: 5,
            choice_option_limit: 8,
        },
    }
}
