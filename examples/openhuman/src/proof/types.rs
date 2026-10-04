//! Host fixture handles and provisioning configuration.
use openhuman_embed::Runtime;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, atomic::AtomicUsize};
use wiremock::MockServer;

/// Native calls queued by a host scenario and unique call identities.
#[derive(Clone, Default)]
pub(super) struct Script {
    pub calls: Arc<Mutex<VecDeque<(&'static str, Value)>>>,
    pub sequence: Arc<AtomicUsize>,
}

/// One host runtime and the loopback services/configuration it owns.
pub(super) struct Fixture {
    pub runtime: Arc<Runtime>,
    pub provider: MockServer,
    pub script: Script,
    pub _backend: MockServer,
    pub files: tempfile::TempDir,
}

/// Provisioning through the same runtime as the initially supplied agent.
pub(super) struct Factory {
    pub runtime: Arc<Runtime>,
    pub root: std::path::PathBuf,
    pub calls: Arc<AtomicUsize>,
}

/// Allows management only to the configured host manager.
pub(super) struct Authorizer;
