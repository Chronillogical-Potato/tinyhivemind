# `live_swarm/`

Driving a live federation of desks (`--swarm`) through a real backend: several
desks, each seated over a CLI agent or the HTTP backend, that reach each other
only through a referral. See [`../SWARM.md`](../SWARM.md).

| File | Purpose |
| --- | --- |
| `mod.rs` | The drivers: what runs, from simulated federations to live desks and one traced episode. |
| `totals.rs` | What a federated comparison counts and how it prints: per-arm outcomes, running totals, the arm fan-out, and the table. |
