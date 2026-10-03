# The context budget

Companion to [`README.md`](README.md): what a bounded prompt window does to
the arms above, and what it does not change.

Every arm above moves information for free. `hive+pooled` is the extreme case:
it copies every peer's reading straight into every member's state, and it wins
by a distance because of it. That is a fair *information-theoretic* ceiling and
a misleading *engineering* one — a participant is a language model with a
window, "hand it everything" means putting everything in that window, and
[`../../../../docs/research/long-context.md`](../../../../docs/research/long-context.md)
is unkind about what that costs.

`--context-sweep` charges for it. Each arm runs across a ladder of window
capacities under [`context.rs`](context.rs), which models the two published
effects and nothing else: rows past capacity are **evicted from the middle**,
and surviving rows are discounted by a **U-curve** with the edges intact.
`--context N` and `--rot F` set the two directly; `--context 0` is the default
and is bit-identical to a build without any of this.

### What it found

On the hidden profile, 2000 rooms, `hive+pooled` — the arm that looked
unbeatable:

| window | rot 0.0 | rot 0.5 | rot 1.0 |
| --- | --- | --- | --- |
| unbounded | 91.7 | — | — |
| 64 | 92.0 | 65.0 | 43.7 |
| 32 | 92.0 | 65.0 | 43.7 |
| 16 | 60.0 | 51.9 | 42.3 |
| 8 | 28.6 | 28.2 | 28.9 |
| 4 | 29.9 | 29.4 | 28.6 |

It carries **16.8 rows per member**, and it needs a window about twice that to
deliver what it promises. Give it a window the size of its own payload and it
loses a third of its lead; halve that again and it loses nearly all of it. At
64 rows there is *no eviction at all* — every row fits — so the whole fall from
92.0 to 43.7 across that row is the U-curve alone: pooling buries its own
decisive fact in the middle of the window it filled.

`hive+` and `hive+along` are flat at 16.2 in every cell, because they carry
0.0 and 0.1 extra rows. They are insensitive to the window because they barely
use it.

### What it did **not** find

**A crossover.** `hive+pooled` at its worst (28.6) still beats `hive+` (16.2)
everywhere, and at eight agents and eight topics — where pooling carries 56.9
rows per member — it is 18.4 against 7.3. Squeezing the window never makes
the deliberating room the better choice on this task.

So the honest reading is narrower than "context economy vindicates
deliberation":

- The pooled ceiling is **soft and window-dependent**, not the fixed 89–92%
  the other tables imply. Quoting it without a window is quoting telepathy.
- Deliberation's **insensitivity** to the window is real and is a property
  worth having.
- Neither of those makes deliberation *good here*. `hive+` scores 7.3% on an
  eight-seat hidden profile. The binding constraint on this task is not context
  at all — it is that the protocol cannot surface a lone dissenting fact, which
  is what `hive+fact°` (+27.6 over `hive+`) addresses and what a context budget
  cannot.

Fix the protocol first. Context economy is a second-order argument until the
first-order one is answered.

### Read the ordering, not the numbers

`context.rs` is a model, not a measurement of any real model's retrieval. That
is why `--context-sweep` sweeps `rot` rather than picking a value: an ordering
that holds across the whole block is a claim about the protocols, and one that
changes hands between blocks is a claim about the model. Both are reported.

