# Approval module

Approval is a total pure fold over a request, policy, grants, refusals, roster,
desks, and a host-supplied monotonic time. It returns allow, deny, or one human
question and never performs the action. `types.rs` holds stable wire payloads,
`mod.rs` implements deny-before-allow evaluation, and `test/` pins failure,
grant, epoch, rendering, and wire behavior.
