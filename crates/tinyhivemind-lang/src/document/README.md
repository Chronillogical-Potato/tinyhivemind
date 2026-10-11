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

| File | Purpose |
| --- | --- |
| `mod.rs` | `parse`, JSON decoding, and the nested unknown-field check shared with `patch` |
| `types.rs` | `Package`, `Manifest` and its sections, `Policies`, and coordinator settings |

There is no `test.rs` here: parsing is exercised alongside validation in
[`../validation/test.rs`](../validation/test.rs), which builds packages through
`parse` and checks both its decoding and its validation failures.
