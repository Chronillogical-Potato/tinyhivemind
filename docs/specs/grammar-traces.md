# Grammar reference: traces

> **Current behavior note.** The parser still recognizes `refute` and `defer`
> for wire compatibility. `refute` is recorded in `TopicStanding::refuted_by`
> for audit but no longer caps a topic. `defer` is parsed but no longer promotes
> a contested topic or routes a bid. [ADR 0027](../adr/0027-retire-unused-adapters-and-optional-mechanisms.md)
> retired those policy effects.

**Status:** Implemented
**Owner:** tinyhivemind maintainers

The normative reference for the marker grammar of `tinyhivemind_core::hive`. Every
rule here is derived from `crates/tinyhivemind-core/src/hive/trace/` and each
non-obvious rule cites the test that pins it. Where prose elsewhere disagreed
with the parser, the parser won; the [index](grammar.md) lists what was
corrected.

Behavioural intent lives in [`hive-mind.md`](hive-mind.md),
[`refutation-and-grounds.md`](refutation-and-grounds.md) and
[`expert-delegation.md`](expert-delegation.md). This file is the syntax and the
extraction algorithm.

## 1. Lexical rules

A body is split into lines with `split_inclusive('\n')`. Each unmasked line is
tried as **one** marker line; a line yields at most one trace, and a body
yields at most `TRACE_CAP` (16) of them.

Extraction short-circuits on a body containing no `!` at all, which is what
keeps the fold cheap over a transcript of ordinary conversation.

### 1.1 EBNF

```ebnf
body         = { line } ;
line         = trace-line | other-line ;
trace-line   = indent , marker ;
indent       = { ? char::is_whitespace ? } ;

marker       = "!" , kind , { WSP , word } ;
kind         = "propose" | "support" | "object" | "refute"
             | "evidence" | "question" | "commit" | "defer" ;

word         = topic | target | cite | free-word ;
topic        = "#" , { ? non-whitespace ? } ;
target       = ">" , { ? non-whitespace ? } ;
cite         = "^" , { ? non-whitespace ? } ;
free-word    = ? any run of non-whitespace not starting with "#", ">" or "^" ? ;

WSP          = ? one or more char::is_whitespace ? ;
```

The conventional way to write a marker line is

```text
!<kind> [#topic] [>target] [^cite ...] [free text]
```

but that ordering is a **convention, not a rule**: qualifiers are recognised
wherever they appear after the kind, including after free text. `!support #a
because ^3` cites sequence 3.

### 1.2 What terminates a token

Only whitespace. The line is split with `split_whitespace`, so a qualifier's
value runs to the next whitespace character and **punctuation is part of the
value**: `!propose #stage.` names the topic `stage.`, which is a different
topic from `stage`. No test covers this; it follows directly from
`split_whitespace` and is the single most likely authoring mistake after
dropping the `#` entirely.

This is the deliberate opposite of the mention grammar, where `,;.?!:)]}'"`
close an alias ([`grammar-mentions.md`](grammar-mentions.md) §1.1). A mention
is embedded in a sentence; a marker line is a line of its own.

Leading indentation is stripped with `str::trim_start`, which honours **any**
Unicode whitespace — unlike the mention opener rule, which only admits ASCII
whitespace before an `@`.

### 1.3 Case sensitivity

The kind spelling is matched **case-sensitively** against the eight lowercase
literals. `!Propose` is not a marker and yields no trace. Topic ids are
preserved verbatim; `#Stage` and `#stage` are two topics, and the quorum fold
compares them as strings.

### 1.4 Line leading

A marker is recognised only at the start of a line after stripping
indentation. `I would !propose #a` yields nothing, `   !propose #a` yields a
trace — `a_marker_must_lead_its_line`.

Inline backticks need no masking: a marker preceded by a backtick is by
definition not line leading — `a_backticked_marker_is_not_line_leading_and_needs_no_masking`.

### 1.5 Offsets and text

`Trace::offset` is the UTF-8 byte offset of the `!` itself: the line's start
offset plus the **byte length** of the stripped indentation.

- `  !question` reports offset 2 — `an_indented_marker_reports_the_offset_of_its_bang`.
- A marker on a second line after multi-byte content reports a byte offset —
  `a_marker_on_a_later_line_reports_a_utf8_byte_offset`.
- A multi-byte indent (U+00A0) after multi-byte content still points at the `!`
  — `a_multi_byte_indent_after_multi_byte_content_still_reports_the_bang`.

`Trace::text` is the line with leading whitespace stripped and trailing `\n`
and `\r` removed. Trailing spaces and tabs are **not** stripped, so `text` can
end in whitespace.

## 2. Fence masking

Code is found by `tinyhivemind_core::masking::code_ranges`, and that is the
**one scanner every authored grammar in this workspace shares**: the mention
grammar of `tinyhivemind-core`, this one, and the `!pin` / `!unpin` directives
of `tinyhivemind::pins` all call the same code. A span one grammar reads as
code is therefore the same span the others read as code, and a marker one
grammar treats as quoted documentation cannot be a live instruction to
another.

Its rules are CommonMark's, so an author who formats a message for a Markdown
renderer gets the masking they can see:

- an opening fence is indented at most three **spaces** and is a run of at
  least three backticks or at least three tildes;
- a backtick opener's info string may not itself contain a backtick, because
  that is an inline span in a paragraph rather than a block;
- a closing fence uses the same character as its opener, runs at least as long
  as it, and carries nothing but whitespace after the run — so a ` ```rust `
  line inside an open block is content, and a three-backtick line inside a
  four-backtick block does not close it;
- an unclosed fence masks to the end of the body, so a body cannot end
  mid-block with its tail read as grammar;
- a line indented four or more columns — a tab expands to the next multiple of
  four — opens `CommonMark`'s other code block — the indented one — instead of
  a fence, and `code_ranges` masks that too: the run continues across a
  blank line and ends at the first non-blank, under-indented line or the end
  of the body. A quoted example that itself contains a bare `` ``` `` fence,
  written at four columns so it renders as code rather than as a live block,
  stays masked in full for this reason;
- an indented block cannot interrupt a paragraph, so the indented opener must
  be a line with no paragraph open above it: the body's first line, a line
  after a blank one, or a line after one that closes the block it belongs to
  — a fenced block, an ATX heading, a thematic break, or a setext underline. A
  wrapped, indented continuation line right after live text is lazy
  continuation of that paragraph, not a new block, and stays live;
- an inline code span — one opened on a run of backticks and closed on the
  next run of exactly the same length — is masked too, even though this
  grammar's markers are line-leading. A span opened on one line and closed on
  a later one quotes every whole line between them, so a marker with no
  backtick ahead of it on its own line can still sit inside quoted code (see
  "the asymmetry that turned out not to be one" below);
- the four columns of an indented block are measured from the message's left
  margin, not from inside a list or blockquote container the way a full
  `CommonMark` parser measures them. A marker quoted at exactly four raw
  columns inside a list item can therefore be masked differently than a
  renderer would show it — a known, accepted limitation of a lexical scanner
  rather than a block-level parser, not a bug to chase further.

A line is masked when its **start offset** falls inside a range, which also
means the fence lines themselves are inside the range.

- `a_marker_inside_a_fenced_block_is_masked`
- `a_marker_inside_a_tilde_fenced_block_is_masked`
- `a_fence_only_closes_on_the_same_character_that_opened_it`
- `an_unclosed_fence_masks_to_the_end_of_the_body`
- `an_indented_code_block_masks_all_its_lines`,
  `an_indented_code_block_continues_across_a_blank_line`,
  `a_tab_indented_code_block_masks_all_its_lines`,
  `an_indented_line_does_not_open_a_block_mid_paragraph`, and
  `an_indented_line_opens_a_block_after_a_blank_line`
- the fence rules themselves are pinned in
  `crates/tinyhivemind-core/src/masking/test.rs`, one test per rule.

Note that the three-space indent limit counts **spaces only**, and applies to
the fence line rather than to the marker. It is not §1.4's whitespace rule,
where a marker's own leading indentation is stripped with `str::trim_start` and
any Unicode whitespace counts.

### The asymmetry that turned out not to be one

Both a mention and a marker resolve against `code_ranges`, which masks fenced
and indented blocks *and* inline code spans — every authored grammar in this
workspace reads the same, single level. That was not the original design: a
line-leading grammar was expected to need `fenced_ranges` alone, on the
reasoning that a marker preceded by a backtick *on its own line* is by
definition not line-leading —
`a_backticked_marker_is_not_line_leading_and_needs_no_masking` still covers
exactly that case, and still needs no masking to reject it, because
`parse_line` never sees a line starting with `!` in the first place.

The reasoning held for a same-line backtick and broke for a multi-line one. An
inline span opened on one line and closed on a later one quotes every whole
line in between, `CommonMark`-visible newlines and all, so a marker on an
*interior* line of that span has no backtick on its own line yet is still
inside quoted code — `run` `` ` `` on one line, `!pin ^1` on the next, `` ` ``
`carefully` on the one after that renders as one inline span with `!pin ^1` as
its (space-joined) content, not as a line-leading directive. Masking on
fences alone let such a marker fire as though it were unquoted; taking
`code_ranges` instead closes that hole for every grammar the same way, with no
per-grammar special case for "unless the span spans more than one line."

## 3. Parsing one marker line

1. Strip the leading `!`. Absent ⇒ no trace.
2. Split the remainder on whitespace. The first word must be one of the eight
   kind spellings; anything else ⇒ no trace
   (`a_body_without_a_marker_deposits_nothing` covers `!shout at everyone` and
   a bare `!`).
3. Walk the remaining words:
   - `#value` — sets `topic` if `topic` is still `None` **and** `value` is
     non-empty. A later `#` after a set topic is ignored.
   - `>value` — sets `target` if `target` is still `None` and `value` parses as
     a `u64`.
   - `^value` — appends `Sequence(value)` to `cites` if it parses as a `u64`
     and is not already present.
   - anything else is free text and is ignored.
4. Apply the two required-qualifier rules (§4).

`only_the_first_topic_and_target_are_taken`: `!object #first #second >1 >2` →
topic `first`, target 1. `a_repeated_citation_is_recorded_once`:
`^3 ^3 ^4` → `[3, 4]`, order preserved.
`an_empty_or_unparsable_qualifier_is_ignored`: `!propose # >x ^y` → no topic, no
target, no cites.

Two consequences of step 3 have no test and are worth stating explicitly,
because they follow from the code rather than from intent:

- An **unparsable** `>` does not consume the target slot. `!object >x >2`
  yields target 2, because the failed parse leaves `target` at `None`.
- An **empty** `#` does not consume the topic slot. `!propose # #real` yields
  topic `real`.

Both are the fail-open half of a fail-closed grammar: a malformed qualifier is
treated as if it had not been written, not as a claim that consumes the slot.

## 4. Markers that fail closed

Six kinds parse on their own. Two require a qualifier, and yield **no trace at
all** without it rather than a weakened one.

| kind | requires | why |
| --- | --- | --- |
| `refute` | a `#topic` **and** at least one `^cite` | a refutation caps a topic for the whole room, so it must name the hypothesis and point at the fact |
| `defer` | a `#topic` | a deferral zeroes a directory weight and promotes a contested topic; naming nothing, it would do both to nothing |

- `a_refute_marker_is_the_one_that_requires_both_qualifiers` pins all three
  failing forms (`#topic` alone, `^cite` alone, neither).
- `a_refutation_is_grounded_by_construction` pins that a parsed refutation
  always has `grounded() == true`.
- `a_deferral_without_a_topic_yields_no_trace` pins the three failing forms and
  that a citation stays optional on a deferral.

`!object` requires nothing syntactically, but an objection with no `>target`
silences nobody: the quorum fold skips a trace whose `target` is `None`. The
grammar admits it; the fold ignores it.

## 5. The eight kinds

| kind | wire spelling | what it does |
| --- | --- | --- |
| `Propose` | `propose` | puts a new option on the floor |
| `Support` | `support` | adds the author to a topic's supporter set |
| `Object` | `object` | silences the advocate of the targeted message |
| `Refute` | `refute` | records a cited topic objection in `refuted_by` for audit; does not cap a topic |
| `Evidence` | `evidence` | supplies grounds without taking a position |
| `Question` | `question` | asks for something the room has not established |
| `Commit` | `commit` | records the decision after quorum |
| `Defer` | `defer` | parses an abstention; does not promote a topic or route a bid |

`every_marker_spelling_is_recognized` pins the parse of each;
`every_trace_kind_pins_its_wire_spelling` pins each JSON spelling;
`a_trace_pins_its_wire_form` pins the whole record. Every `Option` field is
**required** in JSON and must be present as `null` when empty —
`an_optional_trace_field_is_required_in_json`.

`Trace::grounded()` is `!cites.is_empty()` —
`a_trace_without_citations_is_not_grounded`. `Trace::agent_id()` is `Some` only
for an agent author; an operator, a person and a system author all yield `None`
— `a_non_agent_author_has_no_agent_id`.

## 6. `resolve` and `read`

```rust
resolve(body, supplied, author, sequence) -> Vec<Trace>
read(messages) -> Vec<Trace>
```

`supplied == None` extracts from the body. `supplied == Some` **selects** from
what extraction finds — a deliberately narrower contract than
`mention::resolve` has, because parsing is fully determined by the body and
there is no host-side resolution step a supplied trace could legitimately
carry. A supplied entry names an extracted trace by `(offset, kind)`, and
everything else it claims is discarded in favour of what the body says. A
repeated offset is rejected rather than selecting the same trace twice, and the
offset is consumed before the kind is matched, so a second entry at that offset
is dropped even if it names a different kind.

- `a_supplied_list_only_selects_which_extracted_traces_survive`
- `a_duplicate_supplied_offset_is_rejected`
- `an_empty_supplied_list_deposits_nothing`

Both paths then truncate to `TRACE_CAP` (16) —
`a_body_deposits_at_most_the_trace_cap`. The cap exists for the same reason
`MENTION_CAP` does: one message must not be able to grow the fold without
limit.

Ordering differs between the two paths, which is not covered by a test:
extraction returns traces in line order, while selection returns them in the
**supplied** order. Callers that need a canonical order use `read`, which
sorts by `(sequence, offset)` —
`reading_a_transcript_orders_traces_by_sequence_then_offset`.

A message carrying no marker contributes nothing, so a transcript of ordinary
conversation folds to an empty medium —
`a_transcript_of_ordinary_conversation_folds_to_an_empty_medium`. Nothing is
coerced into a vote for having been typed in a deliberating room.

## 7. Worked examples

Author is agent `planner` at sequence 7 unless said otherwise.

| input | parse | resolution |
| --- | --- | --- |
| `!propose #stage Stage the rollout.` | kind `Propose`, topic `stage` | one trace, ungrounded |
| `!support #stage ^1 Staging bounds it.` | kind `Support`, topic `stage`, cites `[1]` | one grounded trace |
| `!object >12 ^3 ^4 the control is missing` | target 12, cites `[3, 4]` | silences sequence 12's author |
| `!object #stage` | kind `Object`, no target | a trace that silences nobody |
| `!refute #stage ^4 It was retired.` | topic + cite | one grounded refutation |
| `!refute #stage No citation.` | missing `^` | **no trace** |
| `!refute ^4 No topic.` | missing `#` | **no trace** |
| `!defer #pool ^3 Not my area.` | topic `pool`, cites `[3]` | one deferral |
| `!defer not mine` | missing `#` | **no trace** |
| `!commit #stage` | kind `Commit` | one trace |
| `!question` | kind `Question` | one trace; no qualifier needed |
| `I would !propose #a` | not line leading | **no trace** |
| `   !propose #a` | indent stripped | one trace, offset 3 |
| `` `!propose #a` `` | not line leading | **no trace** |
| `!Propose #a` | kind case mismatch | **no trace** |
| `!shout at everyone` | unknown kind | **no trace** |
| `!propose #stage.` | topic `stage.` | a trace on a *different* topic than `stage` |
| `!object #first #second >1 >2` | first of each wins | topic `first`, target 1 |
| `!support #a ^3 ^3 ^4` | duplicate cite dropped | cites `[3, 4]` |
| `!propose # >x ^y` | all qualifiers unparsable | a bare `Propose` trace |
| ```` ```\n!propose #hidden\n``` ```` | fenced | **no trace** |
| `!question` × 26 lines | 26 candidates | first 16 kept |

## 8. Deliberately not in the grammar

- **Addressing.** No production names another participant. Coordination is
  stigmergic: a marker is deposited in the transcript and read back by whoever
  takes the next turn. There is no `!ask @alice`, and nothing in `trace/`
  dispatches anything.
- **Fan-out.** `HiveStep::Speak` carries exactly one turn and no variant
  carries two. See
  [`../adr/0002-hive-episodes-are-sequential.md`](../adr/0002-hive-episodes-are-sequential.md).
- **A topic namespace.** `TopicId` is an opaque string with no registry,
  normalisation, or aliasing. Two spellings of one idea are two topics, and
  support does not add up across them. This is a real observed failure of live
  models, and the fix is a host prompt naming the options already on the floor
  — not a fuzzy match in the parser, which would let one member's typo silently
  join another member's proposal.
- **Multi-line markers.** One line, one trace. There is no continuation
  character.
- **Inline masking.** Inline backticks are not scanned, because a backticked
  marker cannot be line leading.
- **Errors.** As with mentions, nothing reports a failure. An unknown kind, a
  malformed qualifier, and a `!refute` missing its grounds all yield no trace.
- **Escapes.** The only way to write an inert marker is a fenced block, an
  indent-free backtick, or any character before the `!`.
- **Trust in supplied data.** A supplied trace can select, never assert. A
  caller cannot manufacture quorum by attaching an invented topic to a real
  line.
