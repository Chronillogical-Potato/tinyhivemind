# Implement Jev-first routing

**Status:** Implemented locally
**Specification:** [`../specs/jev-first-routing.md`](../specs/jev-first-routing.md)

## Goal

Expose a reusable host-neutral routing API that asks Jev before an unaddressed
desk turn, validates every result in deterministic Rust, and can open a bounded
hive without changing core/hive purity or adopting host types.

## Tasks

1. Add conversation wire tests, then define `ConversationKind`,
   `ConversationRef`, and distinct outbound message routes in
   `crates/tinyhivemind-embed/src/conversation.rs`.
2. Add eligibility, fallback, DM-bypass, escalation, and invitation tests in
   `crates/tinyhivemind-embed/src/routing/test.rs`; implement the stable routing
   payloads and pure acceptance/composition beside them.
3. Pin current System One request/response JSON in
   `crates/tinyhivemind-typesafe/src/test.rs`; implement exact wire types and
   the executor-neutral `SystemOneTransport` in `wire.rs`.
4. Test one-call ordinary routing and bounded large-desk hierarchy; implement
   `JevRouter` question construction and fixed-point conversion in `router.rs`.
5. Add the 1,000-agent/100-desk mixed-surface proof in
   `crates/tinyhivemind-embed/tests/routing_scale.rs`.
6. Add the conversation, routing, and OpenCompany compatibility specs plus
   ADRs 0017 and 0018. Update crate/source indexes and the purity guard.
7. Run formatting, strict Clippy, build, tests, purity, rustdoc, doctests, and
   the deterministic release benchmark before publication.

## Non-goals

Live provider spending, OpenHuman changes, and OpenCompany adapter changes are
dependency-ordered follow-up work after this repository PR merges.

## Completion checklist

- [x] Exact public wire types and documentation.
- [x] Deterministic eligibility and fixed-point acceptance.
- [x] One ordinary request and bounded hierarchical routing.
- [x] DM/mention/surface bypass and one reasoning escalation.
- [x] 1,000-agent/100-desk local scale proof.
- [x] Workspace contract, purity, docs, doctests, and benchmark green.
