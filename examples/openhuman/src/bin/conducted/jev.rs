//! A live `SystemOneTransport` over `tinyjevclient`.
//!
//! Every `SystemOneTransport` in the workspace is a fixture, so nothing has
//! ever routed for real. This is the one that does.
//!
//! It bridges through the **wire form** rather than hand-mapping each variant.
//! `SystemOneRequest` and `tinyjevclient::EvaluationRequest` are both
//! `{state, model, questions}` with the same `#[serde(tag = "type")]` spelling,
//! and both responses are `{model, answers, usage}` -- so a round trip through
//! `serde_json::Value` is the identity when the shapes agree, and a loud decode
//! error the day one of them changes. Hand-written arms would silently drop a
//! field instead.

use tinyhivemind_typesafe::{
    SystemOneRequest, SystemOneResponse, SystemOneTransport, SystemOneTransportFuture,
    TransportError,
};
use tinyjevclient::{Client, EvaluationRequest};

/// Jev's System One, over the real endpoint.
pub struct LiveJev {
    client: Client,
}

impl LiveJev {
    /// Build a client from `TYPESAFE_API_KEY`.
    ///
    /// # Errors
    ///
    /// Returns the client's own configuration error when the variable is
    /// absent or the defaults do not resolve.
    pub fn from_env() -> anyhow::Result<Self> {
        Ok(Self {
            client: Client::from_env()?,
        })
    }
}

impl SystemOneTransport for LiveJev {
    fn evaluate<'a>(&'a self, request: &'a SystemOneRequest) -> SystemOneTransportFuture<'a> {
        Box::pin(async move {
            let wire = serde_json::to_value(request).map_err(|error| transport(&error))?;
            let evaluation: EvaluationRequest =
                serde_json::from_value(wire).map_err(|error| transport(&error))?;
            let result = self
                .client
                .evaluate(&evaluation)
                .await
                .map_err(|failure| transport(&failure))?;
            let wire = serde_json::to_value(result.response).map_err(|error| transport(&error))?;
            let response: SystemOneResponse =
                serde_json::from_value(wire).map_err(|error| transport(&error))?;
            Ok(response)
        })
    }
}

/// Every failure here is the transport's, and the message is provider-safe:
/// neither the key nor a request body is ever formatted into it.
fn transport(error: &dyn std::fmt::Display) -> TransportError {
    TransportError::Transport {
        status: None,
        message: error.to_string(),
    }
}
