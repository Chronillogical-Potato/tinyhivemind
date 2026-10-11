//! Wire types for the package manifest (`hive.json`) and the whole package.
//!
//! Every struct denies unknown fields, so a misspelled key is a decode error
//! rather than a silently ignored setting. Identities are opaque,
//! case-sensitive strings: [`crate::validate`] rejects an empty (or
//! whitespace-only) identity and a duplicate within the same section.
use crate::{Constitution, MemorySpec};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use tinyhivemind_core::{
    aside::AsidePolicy,
    desk::Desk,
    driver::ConductPolicy,
    embed::RoutingPolicy,
    hive::{DirectoryPolicy, DivisionPolicy, EpisodePolicy, ExchangePolicy},
    runtime::{ApprovalPolicy, BrevityPolicy, DigestPolicy, ReferralPolicy},
};
/// A complete hive package: the three canonical JSON files plus its Markdown.
///
/// [`crate::parse`] builds one from a document map keyed by `hive.json`,
/// `constitution.json`, `memory.json` and relative Markdown paths. The struct
/// also serializes as a single JSON object with these four fields, which is
/// the form a host can store or hand across a process boundary in one file.
/// [`Package::default`] is an empty but valid package.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Package {
    /// The decoded `hive.json`: what the hive is made of.
    pub manifest: Manifest,
    /// The decoded `constitution.json`: caps and judging settings an editor
    /// proposal may never change.
    pub constitution: Constitution,
    /// The decoded `memory.json`: memory identities and seat bindings, never
    /// remembered contents.
    pub memory: MemorySpec,
    /// Markdown documents keyed by safe relative path.
    ///
    /// [`crate::parse`] keeps only keys ending in `.md` (any case); every other
    /// non-canonical file is dropped. Documents no manifest entry references,
    /// such as a `README.md`, are accepted and carried along, and so take part
    /// in [`crate::lineage::content_id`].
    pub documents: BTreeMap<String, String>,
}
/// The typed hive definition decoded from `hive.json`.
///
/// Every field is required on the wire.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// The `$schema` key; must be exactly
    /// `https://tinyhumans.ai/schemas/hive/v1` or validation fails with
    /// [`crate::Error::Schema`].
    #[serde(rename = "$schema")]
    pub schema: String,
    /// Wire format version; only `1` is accepted.
    pub version: u32,
    /// The hive's identity; must be non-blank. Lowering carries it as
    /// [`crate::LoweredHive::id`].
    pub id: String,
    /// Human-readable hive name, carried unchanged into lowering.
    pub name: String,
    /// The roles seats and missions refer to, each with its own charter.
    pub roles: Vec<Role>,
    /// The agents the host creates, one per seat.
    pub seats: Vec<Seat>,
    /// Core desk values; every member must name a declared seat, at most once.
    pub desks: Vec<Desk>,
    /// Goals, which may decompose into subgoals; the decomposition must be
    /// acyclic.
    pub goals: Vec<Goal>,
    /// Missions assigning goals to roles.
    pub missions: Vec<Mission>,
    /// Directed parent-to-child desk hierarchy; both ends must name declared
    /// desks, and cycles are permitted.
    pub topology: Vec<Link>,
    /// Directed channels along which one desk may refer work to another; both
    /// ends must name declared desks.
    pub referrals: Vec<Link>,
    /// Paths of shared context documents. Each must start with `context/`,
    /// end in `.md`, be present in [`Package::documents`], and carry
    /// well-formed, unique `<!-- item:id -->` markers.
    pub context: Vec<String>,
    /// Core policy values the host installs on activation, bounded by the
    /// constitution.
    pub policies: Policies,
    /// Scheduling settings for a host coordinator.
    pub coordinator: CoordinatorSettings,
}
impl Default for Manifest {
    fn default() -> Self {
        Self {
            schema: "https://tinyhumans.ai/schemas/hive/v1".into(),
            version: 1,
            id: "hive".into(),
            name: "Hive".into(),
            roles: vec![],
            seats: vec![],
            desks: vec![],
            goals: vec![],
            missions: vec![],
            topology: vec![],
            referrals: vec![],
            context: vec![],
            policies: Policies::default(),
            coordinator: CoordinatorSettings::default(),
        }
    }
}
/// One role: a responsibility seats fill and missions are assigned to.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Role {
    /// The role's identity, unique among roles.
    pub id: String,
    /// Path of the role's charter; must start with `roles/`, end in `.md`, and
    /// be present in [`Package::documents`].
    ///
    /// The charter may open with JSON frontmatter between `---\n` and
    /// `\n---\n`; if it does, the object may hold only `id` and `charter`, with
    /// the same values as here. The charter body, without frontmatter, becomes
    /// the routing candidate description of every seat filling this role.
    pub charter: String,
}
/// One seat: an agent the host creates and places on desks.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Seat {
    /// The seat's identity, unique among seats; desk members, memory bindings
    /// and routing candidates refer to it.
    pub id: String,
    /// Display label, also used as the routing candidate label.
    pub label: String,
    /// Identity of the [`Role`] this seat fills; must name a declared role.
    pub role: String,
    /// Path of the seat's prompt; must start with `seats/`, end in `.md`, and
    /// be present in [`Package::documents`].
    ///
    /// The prompt may open with JSON frontmatter between `---\n` and
    /// `\n---\n`. Every key it holds must name a field of this seat, or one of
    /// the seat's memory binding settings (`reads`, `recall_at`,
    /// `budget_chars`, `remember`, and `memory` for the bound identity), with
    /// exactly the manifest's value; otherwise validation fails with
    /// [`crate::Error::Frontmatter`]. Lowering hands the host the body alone.
    pub prompt: String,
    /// Name of the host factory template that creates the agent; must be
    /// non-blank. Never a credential.
    pub template: String,
    /// Opaque, nonsecret factory configuration passed through to the host.
    pub config: Value,
    /// Model the host should run the seat on; `None` leaves it to the host.
    pub model: Option<String>,
    /// Tool access the host must enforce; the allow list also becomes the
    /// routing candidate's capabilities.
    pub tools: ToolAccess,
    /// Names of host-defined MCP server configurations to attach.
    pub mcp: Vec<String>,
    /// Names of skills the host installs for the seat.
    pub skills: Vec<String>,
}
/// A seat's tool access, enforced by the host; this crate only carries it.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ToolAccess {
    /// Tool names the seat may call.
    pub allow: Vec<String>,
    /// Tool names the seat must not call.
    pub deny: Vec<String>,
}
/// One goal, optionally decomposed into subgoals.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Goal {
    /// The goal's identity, unique among goals.
    pub id: String,
    /// What the goal is for, in prose.
    pub description: String,
    /// Acceptance criteria, opaque to this crate and judged by the host.
    /// Unlike `acceptance` elsewhere in a package, these are editable by a
    /// [`crate::patch::PatchClass::Goal`] proposal.
    pub acceptance: Vec<String>,
    /// Identities of child goals; each must name a declared goal, and the
    /// whole decomposition must be acyclic ([`crate::Error::GoalCycle`]).
    pub subgoals: Vec<String>,
}
/// One mission: an obligation assigning goals to a role.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Mission {
    /// The mission's identity, unique among missions.
    pub id: String,
    /// Identity of the [`Role`] that carries the mission; must name a declared
    /// role (a role, not a seat).
    pub role: String,
    /// Identities of the goals the role is responsible for; each must name a
    /// declared [`Goal`].
    pub goals: Vec<String>,
}
/// A directed edge between two desks, used by topology and referrals.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Link {
    /// Identity of the desk the edge leaves; must name a declared desk.
    pub from: String,
    /// Identity of the desk the edge enters; must name a declared desk.
    pub to: String,
}
/// Policies in the portable hive wire format.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Policies {
    /// Declared episode.
    pub episode: EpisodePolicy,
    /// Declared routing.
    pub routing: RoutingPolicy,
    /// Declared approval.
    pub approval: ApprovalPolicy,
    /// Declared aside.
    pub aside: AsidePolicy,
    /// Declared referral.
    pub referral: ReferralPolicy,
    /// Declared digest.
    pub digest: DigestPolicy,
    /// Declared brevity.
    pub brevity: BrevityPolicy,
    /// Declared directory.
    pub directory: DirectoryPolicy,
    /// Declared exchange.
    pub exchange: ExchangePolicy,
    /// Declared division.
    pub division: DivisionPolicy,
    /// Declared conduct.
    pub conduct: ConductPolicy,
}
impl Default for Policies {
    fn default() -> Self {
        Self {
            episode: EpisodePolicy {
                round_width: 1,
                ..EpisodePolicy::default()
            },
            routing: RoutingPolicy {
                minimum_confidence: tinyhivemind_core::runtime::responder::Probability::ZERO,
                high_impact_minimum_confidence:
                    tinyhivemind_core::runtime::responder::Probability::ZERO,
                clarification_threshold: tinyhivemind_core::runtime::responder::Probability::ZERO,
                high_impact_threshold: tinyhivemind_core::runtime::responder::Probability::ZERO,
                round_width: 1,
                choice_option_limit: 8,
            },
            approval: ApprovalPolicy {
                enabled: false,
                default: tinyhivemind_core::approval::DefaultVerdict::Deny,
                rules: vec![],
                approver: tinyhivemind_core::approval::ApproverRule::Absent,
                allow_grants: false,
                max_grant_ttl: None,
            },
            aside: AsidePolicy::default(),
            referral: ReferralPolicy::default(),
            digest: DigestPolicy::default(),
            brevity: BrevityPolicy::default(),
            directory: DirectoryPolicy::default(),
            exchange: ExchangePolicy::default(),
            division: DivisionPolicy::default(),
            conduct: ConductPolicy::default(),
        }
    }
}
/// `CoordinatorSettings` in the portable hive wire format.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinatorSettings {
    /// Declared round width.
    pub round_width: usize,
    /// Declared conduct policy.
    pub conduct_policy: ConductPolicy,
    /// Declared broadcast budget.
    pub broadcast_budget: Option<u32>,
    /// Declared retention.
    pub retention: RetentionSettings,
}
impl Default for CoordinatorSettings {
    fn default() -> Self {
        Self {
            round_width: 1,
            conduct_policy: ConductPolicy::default(),
            broadcast_budget: None,
            retention: RetentionSettings::default(),
        }
    }
}
/// `RetentionSettings` in the portable hive wire format.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize, Default)]
#[serde(deny_unknown_fields)]
pub struct RetentionSettings {
    /// Declared settled episodes.
    pub settled_episodes: Option<usize>,
    /// Declared delivered.
    pub delivered: Option<usize>,
    /// Declared interrupted.
    pub interrupted: Option<usize>,
    /// Declared pending per agent.
    pub pending_per_agent: Option<usize>,
}
