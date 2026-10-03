# Grammar reference

**Status:** Implemented
**Owner:** tinyhivemind maintainers

This workspace defines three textual grammars. Two are large enough to need a
reference of their own, and this is the index to them; either one alone runs
past the 500-line limit this repository keeps on a Markdown file.

- [`grammar-mentions.md`](grammar-mentions.md) — the `@` grammar of
  `tinyhivemind-core`: openers and closers, the alias table, `@#` desk
  addressing, code masking, normalization, and which mention wins for each
  consumer.
- [`grammar-traces.md`](grammar-traces.md) — the `!marker` grammar of
  `tinyhivemind_core::hive`: the eight kinds, the `#topic`, `>target` and `^cite`
  qualifiers, fence masking, and the two markers that fail closed.

The third is the `!pin` / `!unpin` directive grammar of `tinyhivemind::pins`,
described in [`recall.md`](recall.md) rather than here: it is eight lines of
grammar, it is line-leading like a trace, and it shares the trace grammar's
masking for the same reason.

Both files are derived from the parsers and their unit tests, not from prose.
Every non-obvious rule cites the test that pins it. Where a rule follows from
the code but no test covers it, the file says so.

## What each file is for

The specifications describe *behaviour*: what mentions and traces are for, and
what a host may rely on. This reference describes *syntax and resolution*: what
exactly parses, what it resolves to, and what happens when it does not.

| question | read |
| --- | --- |
| what does this `@` span parse to | [`grammar-mentions.md`](grammar-mentions.md) |
| what does this `!` line deposit | [`grammar-traces.md`](grammar-traces.md) |
| why mentions work this way | [`mentions.md`](mentions.md) |
| how one mention starts one turn | [`mention-dispatch.md`](mention-dispatch.md) |
| what a deliberation episode does with traces | [`hive-mind.md`](hive-mind.md) |
| why `!refute` needs grounds | [`refutation-and-grounds.md`](refutation-and-grounds.md) |
| why `!defer` needs a topic | [`expert-delegation.md`](expert-delegation.md) |

The narrative versions are [Mentions](../../wiki/Mentions.md) and
[Trace grammar](../../wiki/Trace-grammar.md) on the wiki. They link here rather
than restating the productions.

## Two grammars, deliberately unlike each other

They look similar and are not, and the differences are load bearing rather than
accidental:

| | mentions | traces |
| --- | --- | --- |
| unit | a span inside a sentence | a whole line |
| sigil | `@`, `@#` | `!`, with `#`, `>`, `^` qualifiers |
| what ends a token | whitespace **or** `,;.?!:)]}'"` | whitespace only |
| position rule | must follow start-of-body, ASCII whitespace, `(`, `[`, `{` | must lead its line after any Unicode whitespace |
| case | alias lookup folds case | kind spellings and topic ids are verbatim |
| masking | fenced, indented, **and** inline code | fenced, indented, **and** inline code |
| supplied input | authoritative about existence; revalidated, stale entries kept quiet | selects extracted traces only; cannot assert anything |
| cap | `MENTION_CAP` 50 pinging | `TRACE_CAP` 16 total |

Neither grammar reports an error. Both fail closed, so a mistyped name or a
malformed marker is inert text rather than an exception a host has to handle in
the middle of a message.

## Discrepancies found against the prose, and what was corrected

Each was resolved in favour of the parser.

1. **"`@this`" is not a production.** `README.md`, `AGENTS.md`,
   `wiki/Home.md` and `tinyhivemind-core/src/lib.rs` ask "who does `@this`
   mean?" as rhetorical shorthand for *this `@`-mention*. No parser or test
   recognises the literal `this`. The reference states this explicitly so the
   phrase is not read as a token.
2. **"Aliases are tried longest-first" implies greedy matching, and there is
   none.** `docs/specs/mentions.md` and
   `crates/tinyhivemind-core/src/mention/README.md` both say it. `aliases()`
   does sort descending by length, but matching is whole-token
   case-insensitive equality: the token boundary is fixed by the closer rule
   before any alias is consulted, so the sort cannot change the outcome. The
   reference describes the equality rule and notes the sort is inert.
3. **`hive-mind.md` lists six trace kinds; there are eight.** Its `TraceKind`
   snippet and its inline grammar list omit `Refute` and `Defer`, which were
   added by `refutation-and-grounds.md` and `expert-delegation.md`. The
   reference lists all eight.
4. **`hive-mind.md` said trace parsing masks "inline and fenced code spans";
   an earlier draft of this reference corrected it to fenced-only, and that
   correction was itself later reversed.** The reasoning that inline masking
   buys a line-leading grammar nothing holds only when a marker's backtick is
   on its own line; an inline span opened on one line and closed on a later
   one quotes every whole line between them, so a marker on an interior line
   has no backtick of its own yet still sits inside quoted code. The trace and
   pin grammars now read `code_ranges`, the same level the mention grammar
   reads, closing that hole — see `grammar-traces.md`'s "the asymmetry that
   turned out not to be one". `hive-mind.md`'s original wording was
   substantively right; the reference now agrees with it.
5. **`hive-mind.md` says trace `resolve` "mirrors `mention::resolve` exactly".**
   It does not: a supplied trace can only *select* an extracted trace by
   `(offset, kind)`, while a supplied mention is authoritative about existence
   and is revalidated. `trace/mod.rs` and `wiki/Trace-grammar.md` already
   describe the narrower contract correctly.
6. **`hive-mind.md` describes the qualifier as "an optional trailing `^123`".**
   Citations are neither trailing nor singular — qualifiers are recognised in
   any position after the kind, `^` may repeat, and `>target` is a qualifier
   the same list omits.
7. **Two different fence scanners — since unified.** The survey found
   `mention/mod.rs` implementing CommonMark's indent limit, info-string rule
   and closing-run rules while `trace/mod.rs` toggled on any line beginning
   ` ``` ` or `~~~`, so one body could mask differently in the two crates. It
   also missed a third copy, in `crates/tinyhivemind-core/src/runtime/pins/mod.rs`. There is
   now one scanner — `tinyhivemind_core::masking` — and all three grammars call
   `code_ranges`, on CommonMark's rules throughout. An earlier draft had the
   trace and pin grammars take `fenced_ranges` instead, on the reasoning that a
   line-leading marker preceded by a backtick is by definition not
   line-leading; that reasoning holds only when the backtick and the marker
   share a line. An inline span opened on one line and closed on a later one
   quotes every whole line between them, so a marker on an interior line of
   that span has no backtick of its own and would still fire as live grammar
   under `fenced_ranges` alone — `grammar-traces.md`'s "asymmetry that turned
   out not to be one" has the full account. There is no remaining asymmetry:
   every grammar in this workspace reads the same level, and a same-line
   backtick still needs no special-casing — `parse_line` already rejects a
   line that does not start with its marker character.
   [`grammar-traces.md`](grammar-traces.md) §2 states it from the trace side.
8. **Case handling differs between mention lookup and `DeskSet::resolve_id`.**
   `@#engineering` resolves to the desk whose id is `Engineering`, while
   `DeskSet::resolve_id("engineering")` fails: alias lookup folds case, desk
   resolution does not. This is safe, because a resolved target always carries
   the canonical id, but it was written down nowhere.
9. **Two rules the code has and no test covers.** A `>` whose value does not
   parse does not consume the target slot, and an empty `#` does not consume
   the topic slot — so `!object >x >2` targets 2, and `!propose # #real` names
   `real`. Both are stated as derived-from-code in
   [`grammar-traces.md`](grammar-traces.md) §3.
10. **Trace ordering depends on the input mode.** Extraction returns line
    order; selection returns the caller's supplied order. Only `read` sorts.
    Untested, and now stated.
11. **Punctuation is part of a topic id.** `!propose #stage.` names `stage.`,
    not `stage`, because the marker line is split on whitespace alone. Untested,
    and now stated — it is the likeliest authoring mistake after dropping the
    `#`.

`README.md` was checked and contradicts nothing. Its claim that `@#platform`
"resolves to exactly one agent before the decision leaves the fold" is accurate:
`referral::forward_to_desk` narrows a desk mention to the first effective
member that is neither the author nor inactive.
