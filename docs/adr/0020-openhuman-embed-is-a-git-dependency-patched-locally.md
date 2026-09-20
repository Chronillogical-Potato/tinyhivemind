# 20. `openhuman-embed` is a git dependency, patched locally

- **Status:** Accepted
- **Date:** 2026-09-20
- **Amends:** [ADR 0013](0013-a-vendored-crate-is-an-example-dependency.md)

## Context

[ADR 0013](0013-a-vendored-crate-is-an-example-dependency.md) drew the line at
library crates: a vendored, outside-the-workspace crate may back an example and
never a dependency of `tinyhivemind-core`, `tinyhivemind-hive`, or
`tinyhivemind`, because this repository is itself vendored — a consumer takes
`crates/*` as path dependencies — and a vendored dependency of a vendored
dependency becomes a nested submodule in every consumer.

`tinyhivemind-openhuman` is the exception that decision named without solving:
it is a library crate, and it must name `openhuman_embed::Agent` to bind
already-built OpenHuman agents into a hive. `openhuman-embed` cannot be an
example-only dev-dependency the way `tinytools` and `tinyinference` are,
because the type it names has to be the same type a consuming host's own agent
handles are built with — a host binding one of its own `Agent` values into a
`tinyhivemind-openhuman` adapter is exactly the point of the crate.

A consumer of this repository — OpenCompany is the first — vendors this
repository as a submodule *and* independently vendors its own OpenHuman
checkout, because OpenCompany's own runtime is built against it directly, not
only through this adapter. Two checkouts of the same crate compiled into one
binary are two `Agent` types and two copies of OpenHuman's process-wide state,
which is a linker-level bug, not a style objection.

`openhuman-embed = { path = "vendor/openhuman/crates/openhuman-embed" }` — the
form ADR 0013 would otherwise require — cannot be resolved onto a different
tree by a downstream consumer. Cargo's `[patch]` table can redirect a git or
registry source onto a local path; it has no mechanism to redirect a path
source onto a different path. A path dependency here is therefore not just the
form ADR 0013 asks for done slightly wrong: it is the one form that forecloses
the fix a consumer would need.

## Decision

`openhuman-embed` is a **git dependency pinned by revision** in the root
`Cargo.toml`'s `[workspace.dependencies]`, consumed by `tinyhivemind-openhuman`
— a library crate — same as any other dependency there. This repository's own
build patches that source onto the vendored `vendor/openhuman` submodule it
already carries and tests against:

```toml
[workspace.dependencies]
openhuman-embed = { git = "https://github.com/tinyhumansai/openhuman", rev = "…", default-features = false }

[patch."https://github.com/tinyhumansai/openhuman"]
openhuman-embed = { path = "vendor/openhuman/crates/openhuman-embed" }
```

`.github/scripts/assert-openhuman-pin.sh`, run from the `rust` CI job, fails
the build when the pinned `rev` and the `vendor/openhuman` submodule pointer
disagree — the two must name the same tree, or this repository's own tests
would pass against a different OpenHuman than the `rev` promises to a
consumer.

A consumer that vendors its own OpenHuman checkout adds the identical shape of
`[patch]` entry in its own root manifest, pointed at its own vendored path.
That entry is what actually unifies the `Agent` type across the two trees —
the git dependency only names *which* OpenHuman revision both sides agree to
patch onto their own copy of. This repository's own `examples/openhuman`
integration target needed the same entry for the same reason once
`openhuman-embed` stopped being a path dependency: it is a separate Cargo
workspace (its own `[workspace]` table) that reaches `openhuman-embed`
transitively through `tinyhivemind-openhuman`'s `openhuman-embed.workspace =
true`, and `field.workspace = true` copies the dependency *spec* — now a git
source — across a workspace boundary without carrying the root workspace's
`[patch]` table with it. Its own `Cargo.toml` now repeats the same patch entry
this repository's root manifest declares, at the relative path its own
directory needs. Any consumer with the same shape of transitive reach to
`openhuman-embed` — a path dependency on a crate that inherits the dependency
from a *different* workspace's `[workspace.dependencies]` — needs the same
patch in that workspace's own manifest, not only in the workspace that first
declared the dependency spec.

## Consequences

- **This repository's own build is unaffected in substance.** The `[patch]`
  table means every `cargo` invocation here still compiles
  `vendor/openhuman/crates/openhuman-embed`, the same tree it compiled when
  the dependency was a path. `cargo metadata --locked` and the existing
  `Cargo.lock` needed no update for this change, because a patched-to-path
  package resolves and locks identically to a plain path dependency.
- **A consumer gets a real choice instead of a forced fork.** Without this
  change, a host vendoring both this repository and OpenHuman had no supported
  way to make `tinyhivemind-openhuman`'s `openhuman_embed::Agent` the same type
  as its own. With it, the host's `[patch]` table is the whole fix, and no
  change to this repository is required to add one.
- **The dependency-policy line ADR 0013 drew — git dependencies only in
  example dev-dependencies — has a second exception now, and it is the last
  one this reasoning permits.** The exception is not "git dependencies are
  fine in library crates"; it is narrowly this: a library crate that must
  share a type with a consumer's own vendored copy of the same upstream crate
  cannot take that dependency as a path, because `[patch]` cannot retarget a
  path. Any future case must show the same shared-type, patch-only-fixes-it
  shape or it is a plain policy violation, not a second exception.
- **A silent rev/submodule drift becomes a CI failure, not a silent split
  build.** `.github/scripts/assert-openhuman-pin.sh` is the enforcement; it
  runs in the `rust` job on every push.
- **`assert-pure.sh` is unaffected.** `tinyhivemind-openhuman` was never in
  `pure_crates`; it already carries a transport-shaped dependency
  (`openhuman-embed` binds a running agent), and this change does not move it
  across that boundary.
