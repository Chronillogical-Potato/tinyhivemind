# document

The package wire format and the parser that reads it. A host collects a
package's files into a `BTreeMap<String, String>` and calls `parse`; this
module never opens a file.

`parse` checks that every key is a safe relative path, decodes `hive.json`,
`constitution.json` and `memory.json` (each required), keeps every other key
ending in `.md` as a document and drops the rest, then runs `validate`. Every
wire struct denies unknown fields, and a second pass rejects unknown fields
nested inside core policy values, which serde alone would accept. The one
exception is `"responder_mode": "lead"` on a desk: core omits the lead mode
when it serializes, so an explicit `lead` is accepted.

`diagnose_documents` runs the same steps without stopping: every unsafe
path, every canonical file that is missing or fails to decode (with serde's
line and column), an unknown nested field at its JSON Pointer, and every
validation finding once the three files decode. `parse` returns its first
finding.

| File | Purpose |
| --- | --- |
| `mod.rs` | `parse`, `diagnose_documents`, JSON decoding, and the nested unknown-field check shared with `patch` |
| `types.rs` | `Package`, `Manifest` and its sections, `Policies`, and coordinator settings |
| `test.rs` | Collected parse findings and their agreement with `parse` |

Parsing regressions also run alongside validation in
[`../validation/test/rules.rs`](../validation/test/rules.rs).
