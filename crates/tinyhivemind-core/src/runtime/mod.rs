//! Runtime-neutral session coordination for a hive of agents.
//!
//! The `runtime` module builds on the pure core algebra and adds
//! the [`crate::runtime::SessionLog`] paging port, attributed transcript projection, ephemeral
//! team initialization, and bounded thread and pin views over that log.
//! A channel that outgrows every window can be folded by [`mod@crate::runtime::digest`] into
//! one bounded account behind a live tail through the [`crate::runtime::Digester`] port.
//! The [`crate::runtime::speech`] module interprets a seat's call and turns an accepted
//! utterance into a committed row. The [`mod@crate::runtime::recall`] module
//! defines the host memory ports that feed a seat's persistent session.
//! Core responder, referral, and approval decisions remain available through
//! the re-exported core modules.
//! The host remains responsible for storage, transports, model clients, and
//! choosing an async executor.
//!
//! # Example
//!
//! ```
//! use tinyhivemind_core::runtime::{BriefedTeammate, TeamBriefing};
//!
//! let briefing = TeamBriefing {
//!     viewer_id: "alice".into(),
//!     desk_id: "engineering".into(),
//!     desk_name: "Engineering".into(),
//!     teammates: vec![BriefedTeammate {
//!         id: "bob".into(),
//!         label: "Bob".into(),
//!         role: Some("reviewer".into()),
//!         description: None,
//!     }],
//!     brevity: Default::default(),
//!     asides: Default::default(),
//! };
//! assert!(briefing.system_text().contains("@bob"));
//! ```
//!
pub mod briefing;
pub mod digest;
pub mod elsewhere;
pub mod error;
pub mod pins;
pub mod recall;
pub mod session;
pub mod sharing;
pub mod speech;
pub mod threads;

pub use crate::approval::{
    Action, ActionTarget, AllowBasis, ApprovalDecision, ApprovalPolicy, ApprovalRequest,
    ApprovalRule, ApproverRule, ConsentEpoch, DefaultVerdict, DenyReason, DeskApprover, Effect,
    GrantScope, Millis, RememberedRefusal, RuleVerdict, ScopeKey, StandingGrant, TargetPattern,
    approve,
};
pub use crate::referral::{
    NoReferralReason, Referral, ReferralDecision, ReferralInput, ReferralKind, ReferralOrigin,
    ReferralPolicy, ReferralReach, referral,
};
pub use crate::{
    approval, aside, chat, desk, dispatch, masking, mention, referral, responder, roster,
};
pub use briefing::{
    BrevityPolicy, BriefedTeammate, BriefingNote, MentionDispatchContext, SessionContext,
    SessionInitialization, TeamBriefing, initialize_session, initialize_session_with_context,
};
pub use digest::{
    BoxError, ChannelDigest, ChannelHead, DigestFuture, DigestOutcome, DigestPlan, DigestPolicy,
    DigestRejection, DigestRequest, DigestedHistory, Digester, accept_digest, apply_digest,
    collect_digest_input, plan_digest, refold,
};
pub use elsewhere::{Elsewhere, ElsewhereQuery, gather_elsewhere, render_row};
pub use error::{Error, Result};
pub use pins::{
    PIN_EXCERPT_CHARS, PIN_LIMIT, PIN_SCAN, Pin, PinAction, PinDirective, fold_pins, pin_note,
    read_directives, read_pinboard,
};
pub use recall::{
    DeskDelta, DeskWatermark, EntryKind, MemoryEntry, RECALL_HEADING, Recall, RecallFuture,
    RecallMoment, RecallRequest, RecalledSession, Remember, RememberFuture, RememberRequest,
    desk_delta, frame_recalled, initialize_session_with_recall,
};
pub use session::{
    Conversation, Elision, LogMessage, PAGE_SIZE, SCAN_LIMIT, SESSION_WINDOW, Sequence,
    SessionAuthor, SessionFuture, SessionLog, SessionMessage, SessionPage, SessionQuery,
    SourceError, project_as, project_session,
};
pub use sharing::{
    PRESENT_SET_LIMIT, ReinitializeReason, SessionDelta, SharingPlan, SharingQuery, SharingState,
    initialized_state, note_present, prepare_delta,
};
pub use speech::{
    CallArguments, CommitRequest, CommittedUtterance, ToolCall, ToolParameter, ToolSpec, Utterance,
    UtteranceRejection, addressed_peers, check_recipients, commit_utterance,
    commit_utterance_to_room, interpret, tool_specs,
};
pub use threads::{
    THREAD_INDEX_LIMIT, THREAD_INDEX_SCAN, THREAD_OPENING_CHARS, ThreadLine, fold_thread_index,
    read_thread_index,
};
