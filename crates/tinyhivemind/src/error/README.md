# `error`

The crate-wide `Error` and `Result<T>`.

## Why it exists

Every fallible public function in `tinyhivemind` returns the same `Result<T>`,
so a host writes one match arm, not one per module. The enum names the ways a
*host* implementation of a port can violate its contract, including bad
pages and stale cursors. It also reports an overflowing present set, a digest
that cannot be assembled, and invalid pure-algebra snapshots from
`tinyhivemind-core`.

## Public surface

| Item | What it is |
| --- | --- |
| `Error` | the crate-wide failure enum, one variant per contract violation |
| `Result<T>` | alias for `std::result::Result<T, Error>` |
| `From<tinyhivemind_core::error::Error> for Error` | wraps a pure snapshot failure as `Error::Core` |

The variants split into two groups:

- **Page-contract violations** — `PageOutOfRange`, `PageNotDescending`,
  `DuplicateSequence`, `EmptyPageCursor`, `CursorDidNotAdvance`,
  `CursorAfterOldest`, `PageTooLarge` — every way a `SessionLog`
  page can disagree with the ordering, uniqueness, size, or cursor contract the
  paging walk relies on. These are checked by
  [`crate::session::validate_page`] on every page read anywhere in the crate.
- **Everything else** — `Read` (a host log read failed), `WatermarkRegression`
  and `PresentSetOverflow`/`PresentSetTooLarge` (continuous-sharing state
  invariants), digest failures, and `Core` (a wrapped pure-algebra error).

## Constraints worth knowing

- **One enum, not one per module.** Adding a new failure mode means adding a
  variant here, not opening a second `Error` type — the charter's rule that
  public surface is centralized applies to errors as much as to exports.
- **A page-contract violation always means the host, not the caller.** Every
  paging walk in this crate validates the page it just read before trusting
  it; a host that returns rows out of order or reuses a sequence surfaces as
  one of these variants rather than corrupting the fold silently.
- **`Core` carries `#[source]`.** It preserves the original pure-algebra
  error for `std::error::Error::source()`.
- **Messages are lowercase, without trailing punctuation**, per the repo's
  error-message convention, so a host can embed one in a larger sentence.

## Where the result goes

Every public fallible function elsewhere in this crate returns `Result<T>`
from this module. Nothing here is displayed to an end user directly; a host
decides how much of an `Error`'s `Display` text, if any, reaches an operator
or an acting agent.
