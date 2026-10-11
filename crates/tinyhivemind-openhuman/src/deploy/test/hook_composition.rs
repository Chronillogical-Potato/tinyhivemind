//! Deployment turn hooks retain asynchronous context from host-supplied hooks.
use super::support::*;
use crate::{TurnHooks, TurnScope, deploy::BuildOptions};
use std::{future::Future, pin::Pin, sync::Arc};

struct AsyncContext;

impl TurnHooks for AsyncContext {
    fn context(&self, _: &TurnScope) -> String {
        "sync host context".into()
    }

    fn context_async<'a>(
        &'a self,
        _: &'a TurnScope,
    ) -> Pin<Box<dyn Future<Output = String> + Send + 'a>> {
        Box::pin(async { "async host context".into() })
    }
}

#[tokio::test]
async fn deployment_context_composes_the_host_async_context() -> anyhow::Result<()> {
    let config = manifest()?;
    let options = BuildOptions {
        hooks: Some(Arc::new(AsyncContext)),
        ..Default::default()
    };
    let hooks = super::super::hooks::Hooks::new(permissions(config, &options), &options);
    let sync_context = hooks.context(&scope("a"));
    assert!(sync_context.contains("sync host context"));
    let context = hooks.context_async(&scope("a")).await;
    assert!(context.contains("async host context"));
    Ok(())
}
