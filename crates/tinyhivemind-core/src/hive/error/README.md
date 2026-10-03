# Errors

One crate-wide `Error` enum, exactly as the workspace convention requires: a
fallible fold in this crate returns a specific variant naming what was
malformed, never a string built up from context.

## Design

`error/mod.rs` holds the whole surface: `Error`, built with `thiserror`, and
the crate's `Result<T>` alias. Every other module in this crate returns
`crate::error::Result<T>` from its fallible public functions rather than
defining its own error type, so a caller matches on one enum regardless of
which fold failed.

Two families of variant:

- **A wrapped algebra failure.** `Error::Core` carries a
  `tinyhivemind_core::error::Error` verbatim, with `#[source]`, rather than
  flattening it into a string. "Which desk was unknown" is the whole content
  of that report, and flattening would throw it away.
- **A malformed policy or roster.** Everything else — a duplicate threshold, a
  threshold naming a member off the desk, a half-life, window or cap of zero
  that would make its owning fold undefined. Each of these is a configuration
  mistake a caller can act on directly: fix the policy, not the transcript.

`ZeroDeferCap` is worth noting on its own: `Option<u32>` is how a host turns
deferral promotion off, so `None` is a real, supported configuration and
`Some(0)` is not a quieter way of saying the same thing — it is rejected.

## Public surface

| Item | Purpose |
| --- | --- |
| `Error` | The crate-wide, `#[non_exhaustive]` failure enum. |
| `Result<T>` | Alias for `std::result::Result<T, Error>`, returned by every fallible public function in this crate. |

## Operational constraints

- **Messages are lowercase, without trailing punctuation**, matching the
  workspace style; `error::test::every_message_is_lowercase_without_trailing_punctuation`
  enforces it over every variant so a new one cannot slip past review with the
  wrong tone.
- **No IO variant exists, and none should be added.** This crate performs no
  IO; a host's own IO failures are the host's error type, not this one's.
- **`#[non_exhaustive]`.** A host must not exhaustively match this enum, so a
  new variant here is not a breaking change for one that does not.
