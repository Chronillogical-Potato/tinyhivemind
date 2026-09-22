# Graph

`mod.rs` owns one desk, its route candidates, the `BoundAgent` trait a bound
handle implements (`openhuman_embed::Agent` does, and is the default), and bindings from canonical hive
ids to existing OpenHuman agents. Canonical ids need not equal OpenHuman
runtime ids, but the member, candidate, and binding sets must match exactly;
blank ids and `none` are invalid. `test.rs` covers identity validation, route
resolution, cloned runtime handles, and desk-private messaging.

Desk routing accepts the host-neutral `RoutingRequest` shape for compatibility,
but validates its desk id, desk surface, source, purpose, and exact ordered
candidate snapshot against this graph before invoking a router. Explicit and
fallback responders must also be bound hive members.
