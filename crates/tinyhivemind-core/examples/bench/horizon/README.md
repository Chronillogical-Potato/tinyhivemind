# `horizon/`

The long-horizon sweep (`--stages N`): a chain of sub-decisions decided by the
same members, where context accumulates and a wrong stage poisons the next.
See [`../HORIZON.md`](../HORIZON.md) for the method.

| File | Purpose |
| --- | --- |
| `mod.rs` | The staged rooms, the `solo`, `solo+fold`, `hive+` and `hive+pooled` arms, and the end-to-end report. |
| `test.rs` | Distinct option names per stage, inherited windows, poisoning, the soloist's whole brief, and that a chain is never likelier than one of its stages. |
