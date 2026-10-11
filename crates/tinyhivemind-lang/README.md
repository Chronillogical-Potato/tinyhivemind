# tinyhivemind-lang

Portable hive definitions and guarded edits for a host that owns agents,
transcripts, evaluation and memory. The crate accepts an in-memory document
map and performs no file, socket or store access. See the
[repository overview](../../README.md) and
[language specification](../../docs/specs/hive-language.md).

A `hive.json` spells out only what has no safe default: `$schema`,
`version`, `id`, `name` and `seats`, and per seat its `id`, `role`, `prompt`
and `template`. Every other section, seat field, policy block and policy
field takes a documented default; see
[optional fields and defaults](../../docs/specs/hive-language.md#optional-fields-and-defaults).

Call `parse` for canonical JSON plus Markdown side documents, `validate` before
activation, and `lower` to obtain existing core desks and routing candidates
with declared roles. Lowered seat configuration preserves memory constraints;
the host must enforce those when registering a runner.

Editors change a hive by editing its files. Call `check` with the original
and edited files: like a compiler, it reports every violation of the language
and the immutable constitution as a located diagnostic, or returns the edit as
one patch per edit class for lineage. The `hive_check` example runs the same
check from the command line. See the
[checking specification](../../docs/specs/hive-language-checking.md).

The immutable constitution pins evaluator settings and hard caps. The host
must check every edit, or guard every patch it applies, before activation.
Accepted and rejected evaluated candidates remain host-owned lineage records.

| Directory | Purpose |
| --- | --- |
| src | Typed wire definitions, validation, lowering, edits and lineage. |
| tests | Checked-in package fixtures and public API integration evidence. |
| examples | Self-edit demonstrations, the `hive_check` command, the `hives/` package gallery, and the `hive_view` viewer. |
