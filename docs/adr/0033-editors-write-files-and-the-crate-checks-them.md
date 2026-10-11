# 33. Editors write files; the crate checks them

- **Status:** Accepted — supersedes the operation-authoring part of the
  editing model in [ADR 0032](0032-hive-language-owns-the-wire-format.md) and
  [the hive language spec](../specs/hive-language.md#edits-and-archive)
- **Date:** 2026-10-11
- **Specification:** [checking direct edits](../specs/hive-language-checking.md)

## Context

The hive language let an editor change a package only by authoring a
`Patch`: one class, and operations carrying exact before/after
preconditions — a JSON Pointer with the whole old value, a prompt body
reproduced byte for byte, a context item's exact previous text. `apply` built
the candidate and `guard` enforced the constitution, stopping at the first
violation.

That is the wrong interface for an orchestrator or master agent. Agents are
good at editing files and poor at reproducing preconditions exactly; a
one-character drift in `before` turns a sound edit into a `Stale` refusal.
An edit that spans two classes, such as adding a seat and its memory binding,
needs two hand-built patches in the right order. And a refusal named one
problem without saying where it was, so an agent fixed problems one round
trip at a time.

## Decision

Editors write files directly, as they want. The crate checks them the way a
compiler checks source.

- `check(original, edited)` over two document maps, and `check_packages` over
  two packages, run every language and constitution rule and return
  `Result<Review, Vec<Diagnostic>>`.
- A `Diagnostic` is located — file, JSON Pointer, rule family, message — and
  every finding is collected rather than the first. `diagnose` and
  `diagnose_documents` do the same for one package. `validate` and `parse`
  keep their signatures and return the first finding as the error they always
  returned.
- The checker infers the edit classes from the files that changed. A change
  of a class the constitution does not permit, or one no class owns, is a
  finding, never silently accepted. Every guard rule — an unchanged
  constitution, protected keys, frontmatter, canaries, read-only memories,
  full validation — applies, located.
- On success the `Review` carries one derived `Patch` per touched class, in an
  order valid at every step. The `Patch` wire, `apply`, `guard` and `diff` are
  unchanged: a patch is now the *recorded* form of an edit that lineage
  stores, derived by the checker. Memory learning operations are not files
  and are still proposed as patches.

## Consequences

- An editor's interface is the package's own files, which it can read,
  rewrite whole and re-check until clean; the `hive_check` example is that
  loop as a command.
- One call reports every problem, each where it lives, so a refused edit can
  be fixed in one round.
- A multi-class edit becomes several lineage records, one per class, each with
  its own content identity; the last is the edited package's. A host that
  evaluates only the whole edit gives each step the whole's verdict.
- An edit whose per-class steps cannot each be valid on their own (a new seat
  whose prompt frontmatter restates a binding that a memory change adds) is
  refused with a `sequence` finding and must be split. This is the cost of
  keeping lineage per class.
- Frontmatter stays immutable through an edit, as `diff` already required:
  it restates the manifest, which is the field to change.
