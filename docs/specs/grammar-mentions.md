# Grammar reference: mentions

**Status:** Implemented
**Owner:** tinyhivemind maintainers

The normative reference for the `@` grammar. Every rule here is derived from
`crates/tinyhivemind-core/src/mention/` and the modules it folds against
(`roster/`, `desk/`, `chat/`), and each non-obvious rule cites the test that
pins it. Where prose elsewhere disagreed with the parser, the parser won; the
[index](grammar.md) lists what was corrected.

Behavioural intent lives in [`mentions.md`](mentions.md) and
[`mention-dispatch.md`](mention-dispatch.md). This file is the syntax and the
resolution algorithm.

## 1. Lexical rules

Mention extraction is a **scan**, not a whole-body parse. The body is walked
character by character; every `@` that survives the opener and code-mask tests
is tried as the start of one token, and everything else is ordinary text.

### 1.1 EBNF

```ebnf
(* One mention token, recognised at a position that satisfies `opener`. *)
mention      = "@" , [ "#" ] , alias ;

alias        = alias-start , { alias-cont } ;
alias-start  = ? char::is_alphanumeric ? | "_" ;
alias-cont   = ? any char that is not closer and not "@" ? ;

closer       = ? char::is_whitespace ?
             | "," | ";" | "." | "?" | "!" | ":"
             | ")" | "]" | "}" | "'" | '"' ;

(* Context condition, not a consumed terminal: what may precede the "@". *)
opener       = start-of-body | ascii-whitespace | "(" | "[" | "{" ;
ascii-whitespace = " " | "\t" | "\n" | "\r" | "\x0b" | "\x0c" ;
```

The token ends at the first `closer` or at the end of the body. `alias-cont`
excludes `@` in a specific way: the run is collected first, and if it contains
an `@` anywhere the **whole token is rejected**, rather than truncated
(`mention_token`). `a@b` after a valid opener therefore yields nothing.

### 1.2 The opener rule

`opens_at` inspects the single **byte** before the `@`. It admits offset 0,
ASCII whitespace, and `(`, `[`, `{`. Everything else — including a letter, a
digit, `;`, `-`, and any non-ASCII whitespace such as U+00A0 — refuses the
token.

- `x@alice` is not a mention: `rejects_bad_openers_alias_starts_and_unknown_names`.
- `prefix;@alice` is not a mention: `rejects_a_mention_opened_after_a_semicolon`.
- `(@ALICE)` and `[@#Engineering!]` are:
  `extracts_at_allowed_boundaries_with_punctuation_case_and_utf8_offsets`.

Checking a byte rather than a `char` is safe because every UTF-8 continuation
byte is `>= 0x80` and so can never look like ASCII whitespace or a bracket. It
does mean a non-ASCII space is not an opener, which is a deliberate asymmetry
with the trace grammar — see [`grammar-traces.md`](grammar-traces.md) §1.2.

### 1.3 Case sensitivity

Alias comparison is **case-insensitive**, via full `str::to_lowercase` on both
sides (`same_alias`). Ids stored in a `MentionTarget` are always the canonical
record ids, never the authored casing.

`@ALICE` resolves to the agent whose id is `alice`
(`extracts_at_allowed_boundaries_with_punctuation_case_and_utf8_offsets`).

Because full case folding can change a string's UTF-8 length, the token is cut
on **authored character boundaries first** and only then compared, so an alias
like `İ` matching an authored `i̇` never corrupts `offset` or `text`:
`matches_unicode_aliases_when_case_mapping_changes_utf8_length`.

This is one of two places case is handled differently across the workspace:
`DeskSet::resolve_id` compares desk ids and names **case-sensitively**
(`named_desk_ids_remain_case_sensitive`). So `@#engineering` resolves through
the alias table to the desk whose id is `Engineering`, while
`DeskSet::resolve_id("engineering")` returns `Error::UnknownDesk`. No
inconsistency results, because a resolved `MentionTarget::Desk` always carries
the canonical id, and it is that id every later lookup uses —
`target_is_active`, `mentioned_members` and the referral fold all receive the
canonical id, never the authored casing.

Neither half is a bug to be aligned with the other. Folding case in
`resolve_id` would collide two desks named `Ops` and `ops`, which the roster
allows; refusing to fold it in alias lookup would stop `@#Engineering`
addressing a desk it plainly names. `docs/specs/mentions.md` records the same
rule in prose.

### 1.4 Offsets

`Mention::offset` is a UTF-8 byte offset into the **original** body, and
`Mention::text` is the exact authored span including `@` or `@#`. Masking never
shifts an offset, because masking records ranges rather than rewriting the
body.

## 2. Code masking

An `@` inside code is not a mention. `code_ranges` computes fenced ranges
first, then inline ranges outside them, and any `@` whose offset falls inside a
range is skipped.

**Fenced.** A line with at most 3 leading spaces whose first character is
`` ` `` or `~`, repeated at least 3 times, opens a fence. A backtick fence only
opens if the remainder of the line contains no further backtick (the CommonMark
info-string rule). It closes on a line with the **same** marker, a run at least
as long as the opener, and nothing but whitespace after the run. An unclosed
fence masks to the end of the body.

- `recognizes_tilde_fences_and_masks_an_unclosed_fence_to_eof` pins both the
  tilde fence and the rule that ` ```still code ` does not close a fence.
- `ignores_closed_inline_and_fenced_code_but_not_an_unclosed_inline_tick` pins
  the ordinary fenced case.

**Inline.** A run of *n* backticks opens a span that closes at the next run of
**exactly** *n* backticks outside a fenced range. An unclosed run masks
nothing; the scanner steps past it and continues.

- `` `@alice` `` masks, `` ` text @alice `` does not, and
  ``` `` @alice `` @bob ``` masks only the first —
  `ignores_closed_inline_and_fenced_code_but_not_an_unclosed_inline_tick`.

`code_ranges` and `fenced_ranges` both live in
`tinyhivemind_core::masking`, the one scanner every authored grammar in this
workspace shares — this grammar, the trace grammar of `tinyhivemind_core::hive`, and
the pin directives of `tinyhivemind`. All three read `code_ranges`: a
multi-line inline span quotes whole lines it did not open, so even a
line-leading marker needs inline spans masked, not only a mention that can sit
mid-line. `grammar-traces.md` §2 states it from the trace side.

## 3. The alias table

`aliases()` rebuilds the whole table on every `resolve` call from the borrowed
snapshots. Nothing is cached, and nothing outlives the call.

Entries are added in this order, and the order matters only for person-slug
allocation:

| # | source | alias text | target | desk-only |
| --- | --- | --- | --- | --- |
| 1 | each **active** roster member | `member.id` | `Agent { id }` | no |
| 2 | each active roster member | `member.name`, when `Some` and not blank after trim | `Agent { id }` | no |
| 3 | each person, in roster order | `person.label`, when not blank after trim | `Person { id }` | no |
| 4 | each person, in roster order | `ascii_slug(label)`, when non-empty, with a collision suffix | `Person { id }` | no |
| 5 | each desk, declared then added | `desk.id` | `Desk { id }` | **yes** |
| 6 | each desk, declared then added | `desk.name` | `Desk { id }` | **yes** |
| 7 | fixed | `everyone`, `channel`, `here` | `Everyone` | no |

Retired agents contribute no alias (`Roster::active_members`). People are never
retired; there is no retirement list for the person namespace.

### 3.1 Which alias texts are admissible

`add_alias` silently drops an alias whose first character is not
`alias-start`, or any of whose later characters is a `closer`. That is what
makes the table and the token grammar agree: an alias that could not be
lexed is never registered.

The practical consequence is that a **multi-word display name is not
addressable**. A desk named `Platform Team` contributes only its id as an
alias; a person labelled `Ada Lovelace` is addressable by slug, not by label.

An alias containing `@` is registered but is dead code: `mention_token`
rejects any token whose run contains `@`, so nothing can ever match it.

### 3.2 Person slugs

`ascii_slug` lowercases ASCII alphanumerics and collapses every maximal run of
other characters into a single `_`, emitting no leading or trailing separator
and discarding non-ASCII characters entirely.

- `"  Chief--Researcher  "` → `chief_researcher`, `"A / B / C"` → `a_b_c`:
  `person_slugs_normalize_ascii_runs_without_leading_or_trailing_separators`.
- `"😀 Élodie / Agent 7"` → `lodie_agent_7`, `"東京"` → `""` (no slug alias at
  all): `person_slugs_discard_non_ascii_words_without_leaking_separators`.

Collisions are resolved by appending `_2`, `_3`, … in roster order, and the
allocator skips any suffix already taken, so a later person whose base slug
*is* an already-allocated suffixed form gets its own suffix:
`allocates_person_slugs_without_colliding_with_an_existing_suffixed_base`
(`Ada`, `Ada`, `Ada 2` → `ada`, `ada_2`, `ada_2_2`).

Slug allocation is a pure function of the people snapshot, so the same roster
always produces the same slugs.

### 3.3 Longest-first is not greedy matching

`aliases()` sorts descending by `text.len()`, but matching is **exact
case-insensitive equality of the whole token** against each alias, so the sort
cannot change which alias matches. The span is decided by the closer rule
before any alias is consulted. See the discrepancy list in
[`grammar.md`](grammar.md).

## 4. Resolution

```rust
resolve(body, supplied, author, roster, desks) -> Vec<Mention>
```

1. **Validate.** If `Roster::validate` or `DeskSet::validate` fails, return an
   empty vector. Resolution never returns an error; a malformed snapshot fails
   closed rather than producing a partial routing decision.
2. **Build** the alias table (§3) and the mask ranges (§2).
3. **Extract** (`supplied == None`) or **revalidate** (`supplied == Some`).
4. **Normalize** (§6).

### 4.1 Extraction

For each `@` at an admissible opener and outside a mask:

1. Lex the token (§1). Failure ⇒ no mention at this offset.
2. Take the candidate set: every alias equal to the authored run
   case-insensitively, restricted to `desk == true` when the `#` sigil was
   present.
3. **Empty candidate set ⇒ no mention.** An unknown name is plain text.
4. **Candidates disagreeing on target ⇒ no mention.** Ambiguity fails closed.
5. Otherwise emit `Mention { target, text, offset, quiet: false }`.

Step 4 is why `@Shared` yields nothing when an agent name, a person label and a
desk name all spell `Shared`, while `@#Shared` yields the desk: the sigil
removes the agent and person candidates before the disagreement test runs —
`ambiguous_aliases_fail_closed_but_desk_syntax_bypasses_other_namespaces`.

Candidates that agree — two desks sharing one id string cannot exist, but a
desk whose id and name are the same string contributes two entries — do not
trip the test, because the comparison is on `target`, not on entry count.

### 4.2 Revalidation

When `supplied` is `Some`, the host has already decided *whether* there are
mentions; an empty vector suppresses extraction entirely
(`supplied_empty_is_authoritative_and_malformed_or_code_spans_are_dropped`).
Supplied metadata is authoritative about existence, never a routing bypass.
Each supplied record must survive, in order:

| check | failure |
| --- | --- |
| `offset + text.len()` does not overflow | **dropped** |
| `offset` and `end` are char boundaries of `body` | **dropped** |
| `body[offset..end] == text` | **dropped** |
| `offset` is not inside a mask range | **dropped** |
| `opens_at(offset)` | **dropped** |
| the token lexed at `offset` ends exactly at `end` | **dropped** |
| re-extraction at `offset` yields the same target | **quiet** |
| the target is currently active | **quiet** |

`supplied_mentions_must_match_one_exact_mention_token` pins the span rule: a
record whose `text` is `"@alice blah"` is dropped, because the token ends at
`@alice`. `supplied_stale_or_wrong_alias_targets_survive_as_quiet_context`
pins the two quiet outcomes — a retired agent and a target that no longer
matches the alias the body carries both stay visible and stop routing.

"Currently active" means: `Agent` ⇒ an active roster member; `Person` ⇒ any
person in the snapshot; `Desk` ⇒ `DeskSet::resolve_id` succeeds; `Everyone` ⇒
always.

## 5. Targets

| target | what it names | can start a turn |
| --- | --- | --- |
| `Agent { id }` | one active roster member | **yes** |
| `Person { id }` | one human participant | no |
| `Desk { id }` | the desk's effective members | no (but see §7.3) |
| `Everyone` | the addressed desk, or the full active roster in General | no |

Wire form is a `kind`-tagged, snake_case union; `quiet` is omitted from JSON
when false — `pins_exact_mention_and_author_wire_shapes`.

## 6. Normalization

`normalize` runs on the output of either input mode, in this exact order:

1. **Stable sort by `offset`.** Reading order is the only order the rest of the
   library uses; a host's supplied order is discarded.
2. **Drop a repeated offset.** The first record at an offset survives; later
   ones are removed, not quieted.
3. **Drop a self-mention.** `Agent`/`Agent` and `Person`/`Person` with equal
   ids. `MentionAuthor::Other` is never self, and an agent mentioning a person
   with the same id string is not self — the namespaces are independent.
4. **Quiet a repeated target.** The first mention of a target pings; every
   later mention of the same target is retained as quiet context.
5. **Enforce `MENTION_CAP` (50).** Only non-quiet mentions count against it.
   Once 50 have pinged, every subsequent non-quiet mention is made quiet.
   Nothing is dropped for the cap.

`normalizes_order_offsets_self_repeats_and_ping_cap` pins all five: 52 authored
mentions by `a0` yield 51 records (self dropped), 50 pinging and the last quiet;
`@a1 @a1` pings once; and a supplied list out of order with a duplicate offset
sorts, deduplicates, and keeps the first record at the shared offset.

Because steps 2–5 run after sorting, "first" always means *lowest offset*, and
the stable sort makes the survivor at a shared offset the first one in the
caller's input order.

## 7. Consumers, and which mention wins

Four folds read a normalized mention list. They agree on reading order and
disagree, deliberately, about what to do with a bad first candidate.

### 7.1 `direct_responder` — skips

Sorts by offset, then returns the first mention that is non-quiet, is an
`Agent`, **and** whose id is an active roster member. An inactive agent
mention is skipped and the walk continues.
`direct_responder_uses_only_first_nonquiet_active_agent`: `@everyone`, then a
retired `@alice`, then a quiet `@bob`, then a live `@bob` selects `bob`. An
invalid roster returns `None` —
`direct_responder_fails_closed_for_an_invalid_roster`.

### 7.2 `mention_dispatch` — stops

Takes the lowest-offset non-quiet `Agent` mention **without** checking
liveness, then fails closed if that target is the author (`SelfMention`) or
inactive (`TargetInactive`). It never falls through to a later mention. This is
the one-message-one-turn rule made unbypassable: a message cannot be crafted so
that a dead first mention hands the turn to a second.

### 7.3 `referral::candidate` — stops, and may act on a desk

Same lowest-offset, non-quiet rule, but the admissible kinds depend on
`ReferralPolicy::reach`: `Agent` always, `Desk` when the policy addresses
desks, `Person` and `Everyone` never. A desk candidate is resolved to **one**
agent — the first effective desk member that is neither the author nor
inactive — so `@#platform` still starts at most one turn. An empty or unknown
desk stops the decision rather than falling through.

### 7.4 `mentioned_members` — expands, never dispatches

Sorts by offset and appends active agent ids, keeping the first appearance of
each and excluding `responder`:

- `Agent` ⇒ that id, if active.
- `Desk` ⇒ that desk's effective members in desk order; an unresolvable desk
  contributes nothing.
- `Everyone` ⇒ the addressed desk's members when `addressed_desk` is `Some` and
  is not a General spelling; otherwise the full active roster in roster order.
  An `addressed_desk` that does not resolve contributes nothing at all.
- `Person` ⇒ nothing. People are not given turns and are not context members.

`expands_context_without_fanout_deduplicates_and_excludes_responder` pins all
four, including the empty result for `Some("missing")`.

`Everyone` is a **list**, not a broadcast. No code path in this workspace turns
one mention into more than one turn.

## 8. Worked examples

Roster: agents `alice` (name `Alice`) and `bob`; person `p1` labelled
`Ada Lovelace`; desk `eng` named `Engineering`. Author is `Other` unless said.

| input | parse | resolution |
| --- | --- | --- |
| `@alice` | token `@alice` at 0 | `Agent { alice }`, pinging |
| `@ALICE` | token `@ALICE` | `Agent { alice }` — case-insensitive |
| `x@alice` | no token | nothing — bad opener |
| `prefix;@alice` | no token | nothing — `;` is not an opener |
| `(@ALICE),` | token `@ALICE` at 1 | `Agent { alice }`; `)` closes the alias |
| `@Alice` | token `@Alice` | `Agent { alice }` via the name alias |
| `@_missing` | token `@_missing` | nothing — no such alias |
| `@nobody` | token `@nobody` | nothing — unknown alias is plain text |
| `@ada_lovelace` | token | `Person { p1 }` via the slug |
| `@Ada Lovelace` | token `@Ada` | nothing — the space closes the alias |
| `@#Engineering!` | desk token `@#Engineering` | `Desk { eng } `; `!` closes |
| `@#engineering` | desk token | `Desk { eng } ` — alias lookup folds case |
| `@eng` | token `@eng` | `Desk { eng } ` — a desk id is a plain alias too |
| `@everyone` / `@channel` / `@here` | token | `Everyone` |
| `@a@b` | token rejected | nothing — an `@` inside the run kills the token |
| `` `@alice` `` | masked | nothing |
| `` ` text @alice `` | unclosed tick masks nothing | `Agent { alice }` |
| ```` ```rust\n@alice\n``` ```` | fenced | nothing |
| `@alice @alice` | two tokens | first pings, second quiet (repeated target) |
| `@bob` authored by agent `bob` | token | dropped — self-mention |
| `@Shared` with an agent, a person and a desk all named `Shared` | token | nothing — ambiguous |
| `@#Shared`, same snapshot | desk token | `Desk { desk }` — sigil disambiguates |
| 52 distinct agent mentions | 52 tokens | 50 ping, the rest quiet |

## 9. Deliberately not in the grammar

- **`@this`.** There is no such production. The phrase "who does `@this` mean?"
  in `README.md`, `AGENTS.md`, `wiki/Home.md` and the crate docs is rhetorical
  shorthand for *this `@`-mention*, not a token. Nothing in the parser or the
  tests recognises the literal `this`.
- **Escapes.** The only way to write an inert `@` is to put it in a code span
  or fence, or to leave it without a legal opener. There is no `\@`.
- **Wildcards, prefixes, and globs.** Matching is whole-token equality;
  `@ali` does not reach `alice`, and there is no completion or nearest-match
  behaviour. Fuzzy matching would make routing depend on who else is in the
  room.
- **Fan-out.** No production expands to more than one turn. `@everyone` is a
  context list, and a desk mention narrows to one agent before any turn is
  started.
- **Multi-word aliases and quoting.** There is no `@"Platform Team"` form,
  because the closer set has no exception and no quoting rule. Addressability
  is a property a host controls by choosing ids, not something the parser
  guesses at.
- **Errors.** No production reports a failure. Unknown, ambiguous, retired and
  malformed input all fail closed, so a mistyped name can never turn into a
  routing exception a host has to handle mid-message.
- **Storage.** Nothing here reads or writes anything. The alias table is
  rebuilt per call from borrowed snapshots.
- **Host identity types.** The person namespace is `Person { id, label }` and
  nothing else; a host projects whatever user record it owns into it.
