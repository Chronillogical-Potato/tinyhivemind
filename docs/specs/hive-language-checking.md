# Checking direct edits to a hive package

Status: Accepted. Extends [the hive language](hive-language.md).
Decision: [ADR 0033](../adr/0033-editors-write-files-and-the-crate-checks-them.md).

## Purpose

An orchestrator or editor agent changes a hive by editing its files, the
way a programmer edits source. It does not author a `Patch` with exact
before/after preconditions. The crate then plays the part of a compiler: it
reads the original files and the edited files, runs every rule the language
and the constitution define, and either reports every problem at once or
returns the classified changes the host needs for lineage.

The `Patch` wire, `apply`, `guard` and `diff` stay as they are. A patch is now
the *recorded* form of an edit, derived by the checker, not the form an
editor writes.

## Diagnostics

A finding is a `Diagnostic` with four fields, all on the wire:

| Field | Meaning |
| --- | --- |
| `file` | Package-relative path of the file the finding concerns, such as `hive.json`, `memory.json`, `constitution.json` or `seats/solver.md` |
| `pointer` | JSON Pointer inside that file, or `null` for a Markdown file or a whole document |
| `rule` | The rule family, a snake_case `Rule` |
| `message` | Lowercase explanation without trailing punctuation |

Rules are one per family: `schema`, `identity`, `reference`, `path`, `bound`,
`goal_cycle`, `namespace`, `memory_access`, `context`, `frontmatter`, `json`
and `missing_document` mirror the package `Error` variants; `forbidden_class`,
`immutable`, `contamination`, `read_only`, `unowned`, `original` and
`sequence` are edit rules. A diagnostic displays compiler style:
`hive.json:/seats/0/role: reference: unresolved role reference ghost`, or
`seats/solver.md: frontmatter: ...` when there is no pointer.

Every validation finding names where it lives. A seat's role and prompt are
`hive.json` at `/seats/<i>/role` and `/seats/<i>/prompt`; a missing prompt
document is reported at the manifest field that names it. Frontmatter and
context-marker findings name the Markdown file. Memory findings name
`memory.json`, except protected namespaces and read-only pins, which live in
`constitution.json`. Policy bounds name the `hive.json` field (`/policies/...`
or `/coordinator/...`) except a zero constitution cap, which names
`constitution.json`.

## Collecting rather than stopping

- `diagnose(&Package) -> Vec<Diagnostic>` runs every validation rule and
  returns every finding. Checks that depend on an earlier one for the same
  entry (a frontmatter check on a document that is missing) are skipped.
- `validate` keeps its signature and result: it returns the first finding
  as the same `Error` it returned before. The rules run in the original
  order, so the first finding is the error the fail-fast code produced.
- `diagnose_documents(&BTreeMap<String, String>)` returns
  `(Option<Package>, Vec<Diagnostic>)`. It reports every unsafe path, a
  missing or undecodable canonical file per file (serde's line and column in
  the message), an unknown nested field at its pointer, and, once the three
  canonical files decode, every validation finding. The package is `Some`
  whenever the canonical files decode; it is valid exactly when the list is
  empty. `parse` returns the first finding as its error.

## The check

`check(original, edited)` takes two document maps;
`check_packages(original, edited)` takes two packages. Both return
`Result<Review, Vec<Diagnostic>>`.

1. The original must be valid. Each finding against it is reported with rule
   `original`, since the editor cannot be blamed for it.
2. The edited files are diagnosed as above.
3. The change is classified. Every manifest or memory top-level field and
   every Markdown document that differs is one change, owned by the edit
   class whose boundary admits it (the same table `patch` enforces). The
   pointer of a field change is its first differing leaf, such as
   `/policies/episode/turn_budget`.
4. Each finding below is collected; nothing stops at the first:
   - `immutable`: any change to `constitution.json`; any protected key
     (`evaluation`, `telemetry`, `logging`, `constitution`, or `acceptance`
     outside goals) in the manifest or a frontmatter; any change to an existing
     document's frontmatter; a prompt body that opens a second frontmatter.
   - `forbidden_class`: a change whose class is not in `permitted_edits`.
   - `unowned`: a change no class owns, such as `hive.json:/name`, a
     top-level `README.md`, or a file that is not part of the package.
   - `contamination`: a constitution canary anywhere in the edited manifest,
     memory or documents.
   - `read_only`: a change to a pinned memory identity or its bindings.
   - `context`: a context document whose edit is not expressible as item
     deltas — text before the first item changed, items reordered, a new item
     not at the end, or a body without its final newline.
   - every validation finding of the edited package.
5. With no findings, the changes are grouped into one `Patch` per touched
   class, in class order (`prompt`, `context`, `memory`, `roster`,
   `topology`, `policy`, `goal`). If applying them in that order would pass
   through an invalid package, the first order that is valid at every step
   is used instead. If no order is, one `sequence` diagnostic says so.

Diagnostics are deduplicated by file, pointer and rule, and sorted by file
then pointer.

## The review and lineage

`Review { changes, candidate }` holds the derived patches and the edited
package. Applying `changes` in order with `patch::apply`, starting from the
original, reproduces `candidate` exactly; every intermediate package is
valid. A host records one lineage `Record` per change, chaining parents, so
each step carries its own class and content id and the last id is the edited
package's. A host that evaluates only the whole edit gives every step the
verdict and scores of the whole.

Memory contents are not files. Learning operations stay on the existing
`Patch` path with `apply`.

## The command-line check

`cargo run -p tinyhivemind-lang --example hive_check -- <original> <edited>
[--json]` loads two package directories, prints one diagnostic per line (or
a JSON array) and exits non-zero on any finding. With one directory it
diagnoses that package alone. Dot-files are skipped.

## Acceptance evidence

Unit tests pin the diagnostic wire, every rule, the first-error equivalence
with `validate`, and multi-finding collection. The `direct_edits`
integration test edits the gallery's `self-improving` package: a prompt
edit is accepted; a policy edit is refused with a pointer; a canary, a
constitution change and a bad reference are reported together; a multi-class
edit's patches reproduce the edited package; unparseable JSON names its file
and line; an unowned change is reported.
