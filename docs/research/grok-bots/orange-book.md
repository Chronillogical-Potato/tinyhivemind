# Grok Bot Orange Book — pattern digest

Source: `KinGao294/grok-bot-orange-book`, commit `1f689f8c7c9143dcdbdbd6ae631f9bdcd8572b23`
(cloned shallow 2026-09-06). Primary artifact is `Grok-Bot-橙皮书.md` (~1.6万字,
Chinese, v260823/v260824). Licence: **CC BY-NC-SA 4.0** (attribution, non-commercial,
share-alike) — this digest is a translated, reorganized summary for internal
engineering reference, not a redistribution of the original text.

Author is a Cursor China-region ambassador; the book is a first-two-weeks field
report on Grok Bot (xAI/SpaceXAI × Cursor, launched 2026-08-11), not an official
xAI/Cursor document. Numbers, prices and unconfirmed mechanics are explicitly
flagged by the author as provisional (see the book's own "unresolved questions"
appendix, reproduced below).

Everything below is organized **by pattern**, not by chapter. Each pattern says
what it prescribes, what problem it solves, and is tagged:

- **[mechanism]** — a rule or algorithm that could be encoded as a fold/algebra
  (a thing `tinyhivemind` could implement or test against).
- **[operational advice]** — a human habit, business practice, or UX
  recommendation with no direct code shape (useful as design intent, not as a
  spec).

---

## 1. Chief of Staff / "总监" (dispatch hub) pattern — **[mechanism + advice]**

**Prescribes:** when a user has three or more Bots, the first Bot is designated
a "Chief of Staff" (总监/幕僚长): pinned, given a human name, and made the
single point of contact. The user always talks to one thread; the CoS does
task decomposition and delegation internally ("research → Barry, email →
Cindy") and tracks progress. Complexity is hidden below the surface the user
sees.

**Problem it solves:** with N bots, "who do I ask" becomes the dominant
cognitive cost, worse than not having delegation at all. A single addressable
head amortizes that cost.

**Mechanism vs. advice split:** the *routing decision* ("this kind of task
goes to this specialist") is currently made by an LLM call inside the CoS
persona, not a deterministic algorithm — so as shipped this is operational
advice + prompting technique, not a pure fold. But the *shape* — one
default-addressable entity that fans a message out to zero or more delegates
and is the only thing that reports back to the human — maps directly onto
`tinyhivemind`'s desk/roster and mention-dispatch concepts: a "desk" for the
front door, expert-delegation for the fan-out, and the *bound on how many
turns one inbound message may start* is exactly the invariant `tinyhivemind`
already encodes as "one message, one turn."

## 2. Group chat / spontaneous inter-agent messaging — **[mechanism]**

**Prescribes:** when several Bots share one chat, they message each other
without being told to — a coding Bot asked to build a feature will
proactively message a content Bot: "give me the context the boss posted on X
a few days ago." Official framing: Bots "autonomously message each other and
share context within the same thread"; when projects overlap, they
self-align on the same account/project "without the user having to copy
information between conversations."

**Problem it solves:** context transfer between specialists without user
relay.

**Mechanism read:** this is precisely the shared-transcript-read pattern
`tinyhivemind-core`'s projection fold already generalizes — an agent's "ask
a teammate for context" reduces to reading further back in a transcript both
already have access to, rather than a private message needing its own
channel. The book does not describe a turn-budget or loop-prevention rule for
this messaging (an omission worth flagging against `tinyhivemind`'s
one-message-one-turn and hop-bounded dispatch guarantees, which this pattern
lacks in the wild).

## 3. Official bug-relay case — the four-stage handoff pipeline — **[mechanism]**

Quoting the official launch material (via FoneArena/Techstrong): an
engineering Bot reproduces a bug in the product UI, files a ticket, then
hands it to a dedicated fix/debug Bot.

The book extracts four generalizable stages from this and other official
deployments (sales-lead research overnight, Gmail invoice processing, CRM
auto-update after a call):

1. **分工 (division of labor)** — each specialist owns one lane.
2. **通信 (communication)** — agents message each other and share context
   in one thread.
3. **交接 (handoff)** — the thing passed between agents is a **ticket/task
   card**, not a chat message — traceable, not lossy.
4. **上报 (escalation)** — only judgment calls come back to the human.

**Mechanism read:** stage 3 — "the handoff artifact is a durable card, not a
transient chat line" — is the one concrete, encodable claim here. It matches
`tinyhivemind`'s `cross-desk-referral.md` design (one bounded child turn, one
answer returns) more than it matches free-form chat. Stages 1/2/4 are the
same shape as pattern 1 and pattern 2 above; the book's real contribution is
naming this the "complete puzzle" of multi-agent collaboration and insisting
all four pieces must be present for it to count as collaboration rather than
just parallel solo work (see pattern 4 below, which shows a case that
satisfies only 1 and 4).

## 4. The five-role fleet — **[mechanism scaffold + operational advice]**

Sourced from AI creator Alex Finn's public setup (as relayed by 新智元/other
media), reproduced and localized by the author. This is presented as a
*template*, and the author is explicit that as configured it demonstrates
**division of labor only**, not the full four-stage collaboration of pattern
3 (each bot reports individually to the "boss," none message each other).

| Bot | Title | Daily job |
|---|---|---|
| Build | CTO | Controls the owner's high-performance home machine over a reverse tunnel, deploys local models on it, hand-builds projects, fixes code errors immediately |
| Barry | PR / content director | Scans top industry accounts every 30 min, 07:00–23:30; pushes on big news; auto-collects material for a newsletter |
| Dusty | Community manager | Runs a forum under an independent mailbox identity 24h; answers questions; proactively DMs lurkers (3+ days silent) with a relevant, researched suggestion |
| Cindy | Business-development front desk | Owns the biz-dev inbox; background-checks every sender; screens scams; delivers one end-of-day spreadsheet of real leads |
| Reed | Growth hacker | Roams forums/social media for unmet needs; on finding one, writes a minimal-validation plan, ships a small tool to test the market |

**Mechanism read:** none of the individual role bodies are mechanisms — they
are prompt content (operational advice, role-card text). What *is*
structurally reusable is the four-field template the book calls out
explicitly as the reason these cards work (see pattern 5): **responsibility +
information sources + judgment criteria + report format**. A role card
missing any of the four produces "directionally correct but unusable" output.
This four-field shape is a good schema for whatever `tinyhivemind` calls a
desk's role/registration metadata, if it ever carries persona content instead
of just membership.

**Upgrade path noted by the author, from five separate desks to one fleet:**
wire Barry's daily brief to auto-CC the CoS and archive to a material
library; have Cindy's screened real-leads sheet `@`-mention Build for
feasibility review; have Reed's validated pain points auto-generate a task
card that enters next week's schedule. This is, again, the pattern-3 shape
(ticket-as-handoff, `@mention` as directed dispatch) applied on top of
pattern-4's parallel desks.

**Ramp-up cadence (operational advice):** don't hire all five on day one.
Hire the CoS first, run it a week; hire the second specialist for whatever
you personally do most; week three starts giving employees routines; week
four prune — a Bot unused for two weeks gets deleted. Weekly 15-minute
retro asking three questions: which Bot saved the most time, which Bot's
output you had to redo the most, does the division of labor need to change.
Feedback should be behaviorally specific ("summarize in two sentences before
forwarding"), not vague ("this isn't good").

## 5. The role-card schema (verbatim, translated) — **[mechanism scaffold]**

The source book reproduces all five original role-card prompt bodies as
directly copyable text. The source is CC BY-NC-SA 4.0 (non-commercial,
share-alike) and this repository is GPL-3.0-only — the two are not
compatible, so this digest paraphrases each card's content and role rather
than quoting it, which is enough to carry the schema the next section draws
on:

- **Build · CTO** — the team's technical lead: owns code and infra upkeep and
  technology choices, is expected to state its plan before acting and list
  what it touched afterward, and files a weekly status report.
- **Barry · Content radar** — a scheduled intelligence-gathering role: polls a
  fixed list of sources on a fixed cadence, applies a stated bar for what
  counts as significant, escalates immediately on a hit with a short
  what/why/so-what, and files a periodic digest otherwise.
- **Dusty · Community manager** — a support and moderation role: answers
  questions with an empathetic opener before substance, proactively reaches
  out to members who have gone quiet, and is barred from unsolicited sales
  pitches.
- **Cindy · Business front desk** — a lead-qualification role: backgrounds
  every inbound contact, buckets it into a small fixed set of categories, and
  reports only the short list worth a human's time, with a one-line rationale
  each.
- **Reed · Growth experimenter** — a discovery role: scans forums and social
  discussion for recurring unmet needs, and gates any resulting experiment
  behind a written validation plan and human approval before building or
  shipping it.

**Common structure the author calls out:** every card has all four of
**responsibility + information sources + judgment criteria + report
format**. This is the schema; the individual card bodies are content, not
mechanism.

## 6. Localization pattern for non-English deployments — **[operational advice]**

Three concrete changes made porting the fleet to a Chinese-language context:
swap Barry's source list from X influencers to WeChat public accounts, Jike,
Zhihu hot list, and podcasts, and change output to short forwardable
messages; teach Cindy to recognize Chinese business-email scam patterns
(fake quotes, barter-resource pitches, mass blasts); add a role the original
fleet lacked — a "materials librarian" managing the content pipeline's
topic/asset library. One-line summary: the fleet's *structure* is portable,
its *information diet* is not.

## 7. Routine (skill-by-demonstration) — **[mechanism, partially]**

**Prescribes:** instead of typing instructions, the user performs a task
once in front of the Bot (clicks and inputs, full run-through); the Bot
stores the process as a replayable routine. Saying "do it the usual way"
re-executes it. Official quote: "I showed it a workflow once and now I
trust it to run on its own," claimed 2-3x efficiency gain from no longer
needing step-by-step approval.

**Fit test — what to teach as a routine:** fixed steps, minimal judgment
(daily competitor scan, weekly report rollup, fixed-format data transfer).
Not a good fit: negotiation requiring live judgment, creative work that
differs every time.

**Three open questions the author flags before use (unconfirmed even by
officials — this is exactly the kind of "operational advice because the
mechanism is opaque" case):**

1. Does a routine record *click trajectory* or *task intent*? Testing
   suggests a blend — simple flows skew trajectory, complex flows skew
   intent — meaning a target site's UI redesign can silently break a
   trajectory-recorded routine. Recommendation: periodically spot-check
   routine output.
2. A routine evolves from correction, not manual override — if it executes
   wrong, tell it what was wrong in words rather than fixing the output
   yourself; it will apply the correction next time.
3. The more generic the routine, the more valuable it is — teach "how to
   organize any new data folder," not "organize this one folder." The
   former is a transferable capability; the latter is a one-shot macro.

**Pre-teaching checklist (four questions, operational advice):**
have I run this manually and reliably at least twice? can every judgment
step be phrased as "if X then Y"? is there tacit knowledge (a hidden
backend URL, an unwritten convention) that needs to be told to it first?
and — after teaching, manually verify the first three executions, since the
teaching window is both the highest-risk period and the cheapest to correct.

## 8. Auto-review / approval gating — **[mechanism]**

**Prescribes:** the Bot pauses and surfaces an approval card in exactly
three situations: **payment involved**, **sensitive action**, or **the Bot's
own confidence is low**. The card shows what it intends to do and why;
approve to continue, decline to redirect. In practice most routine tasks run
silently end to end; interruptions cluster around money and high-risk
actions.

**Anti-patterns named (operational advice):** reviewing every single step
(defeats the purpose — you're back to babysitting); reviewing nothing
(dangerous for payment cards specifically). Author's stated habit: **batch
pre-authorize low-risk work; read every payment card individually.** Trust
is graduated, not a binary switch.

**Mechanism read:** the trigger taxonomy (payment / sensitive / low
confidence) is a decidable-in-principle predicate over an action — closer to
a policy/rule engine than free text. This is the same shape as an
approval-gate port a host could implement; the three-bucket classification
itself could be a pure function over an action descriptor if the action
carries typed metadata (amount, target, confidence score).

## 9. Toolbox additions treated as load-bearing infrastructure — **[operational advice]**

- **Agent Mail** (strongly recommended): give every Bot its own independent
  mailbox identity — never hand a personal Gmail password to a Bot. Identity
  isolation is called "lesson one" of running a Bot army.
- **Vercel / Cursor Origin**: code-writing Bots deploy straight from a push,
  no manual git.
- **Tailscale**: reverse tunnel so a cloud Bot can reach an authorized local
  device (the book's example: a CTO Bot remotely driving a home GPU machine).
- **Last 30 Days**: social-data mining plugin, described as the standard
  ammunition for a Barry-style content-radar Bot.
- **MCP**: Grok Bot's default mode is "click the interface like a human"
  (which is why it can operate arbitrary legacy systems), but wherever a
  clean API exists via MCP, prefer it — faster and more stable. Priority
  order: API where available, UI-clicking as fallback.

Identity isolation (Agent Mail) and network isolation (Tailscale) are the two
items here with a real security-boundary shape; the rest are integration
conveniences.

## 10. Six "invisible quota sinks" — **[mechanism-adjacent diagnostic]**

The book's central cost-control chapter frames quota burn not as "how many
tasks you dispatched" but as six specific waste patterns:

1. **Re-explaining background every session** — motivates pattern 11 (brain
   dump) below: front-load context once so it doesn't get re-paid per call.
2. **Using a senior/expensive Bot for menial copy-paste work** — "like
   sending your CFO to file receipts."
3. **Failed attempts from ambiguous instructions** — one clear task
   description beats three retries.
4. **Meaningless polling** — a Bot re-checking a page every few minutes is
   the single worst offender; convert to event-driven triggers (react to a
   change, don't poll for one).
5. **Unbounded single-session chat** — context snowballs; every turn re-pays
   for the entire history. Practice: "one topic, one session" — start a new
   conversation on topic change and let conclusions settle into memory.
6. **Using a big fleet for a small job** — coordination and inter-agent
   messaging themselves cost tokens; task complexity should set team size,
   not the other way around.

**Mechanism read:** #4 (poll → event-driven) and #5 (session boundary =
topic boundary, with memory as the compaction point) are the two items here
with a clean algorithmic restatement: a poll loop is dominated by an
event/subscription model, and context growth is bounded by projecting a
session's tail into a persisted summary at a topic boundary — which is
structurally close to what `tinyhivemind`'s continuous-sharing watermarks and
paging model already do for cost reasons of their own.

## 11. Four cost-minimization heuristics + a worked calculation — **[operational advice + one diagnostic]**

1. **Task tiering**: mechanical/repetitive → routine (near-zero marginal
   cost after teaching); tasks needing comprehension → an ordinary Bot; only
   tasks needing real reasoning/creativity → the flagship Bot. Each tier
   should only burn the compute it actually needs.
2. **Routine-ify aggressively**: anything done twice should be taught as a
   routine before the third time — this converts "re-reasoned every call"
   into "taught once, reused indefinitely," which the author calls the
   single highest-ROI move in the whole product.
3. **Front-load acceptance criteria**: state the deliverable shape up front
   ("only these three fields," "five sentences max") to avoid a
   full-redo round trip — a redo is double-billed.
4. **Track the weekly reset cadence**: quota refreshes weekly; schedule
   heavy work early in the cycle, light work late; near the cap, switch to
   light tasks rather than running exploratory (inherently high-failure-rate,
   hence high-waste) work in the overage-billed zone.

**Worked example (the book's own illustrative numbers, not a real bill):**
against a nominal 100-unit weekly quota, an unplanned usage pattern (dispatch
on impulse, re-explain background each time, route menial work through the
flagship Bot) lands around 120 units — into overage by the weekend. A tiered
usage pattern (over half of routine daily work converted to routines,
menial work on a light Bot, flagship reserved for comprehension-requiring
tasks) lands around 55–70 units with headroom to spare. Same workload, ~2x
cost spread — illustrating why routine-ification is framed as the top lever.

## 12. Three unattended ("无人值守") operating modes — **[mechanism scaffold]**

1. **Sentinel type**: scheduled scan + anomaly escalation (competitor
   watch, price watch, sentiment watch). Silent by default, speaks only on
   an event. Called the highest ROI, easiest-to-adopt first automation.
2. **Pipeline type**: several Bots chained, each an independent specialist,
   handing off via chat messages — book's example: Reed finds a pain point
   → Build builds a small tool → Dusty posts it to the community for
   feedback → Cindy rolls the feedback into a one-page report.
3. **Night-shift type**: heavy work that doesn't need daytime attention run
   overnight (scoring 500 sales leads, overnight data cleaning) — framed as
   "the cloud VM doesn't cost you electricity, so use it."

**Mechanism read:** type 2 (pipeline) is the same shape as pattern 3's
four-stage handoff, generalized to N stages with the chat message (rather
than a formal ticket) as the connective tissue — a looser, less traceable
handoff than pattern 3's ticket-based one. Type 1 (sentinel) is a
trigger/condition/escalate loop, matching the `hive` module's "silent
unless salience crosses a threshold" shape more than a scheduled cron.

**Worked pipeline example, step by step (operational, but concrete):** (1)
have a content-radar Bot (Barry) with sources configured; (2) instruct the
CoS to double as editor: nightly at 21:00, dedupe the day's pushes, bucket
into product/funding/industry, pick the top ≤2 per bucket, produce a
one-pager, and — quality-of-life detail worth encoding — "if three
consecutive days have nothing notable, send just one line: 'nothing today'"
rather than an empty or padded report; (3) run for three days, spend two
minutes/day checking quality, give verbal correction; (4) once stable, hand
the full chain over and only read the daily output. Named as the "minimum
viable sample" of a pipeline: one collector + one editor + one fixed
trigger time; further pipelines (lead scoring, price monitoring, sentiment
rollup) are the same mold copied sideways.

**Three quality-control safeguards for unattended pipelines — [mechanism-adjacent]:**

- **Front-loaded acceptance criteria**: specify delivery format at dispatch
  time (count, length, structure) so the Bot self-retries on
  non-conformance rather than the human discovering the defect later.
- **Sampled audit**: not every output needs checking, but spot-check
  against source material weekly — its value is partly confidence, partly
  a deterrent (the Bot "knows you check").
- **An explicit escalation channel**: instruct explicitly, "don't guess on
  anything uncertain — turn it into a question and ask me." The stated
  failure mode an unattended system fears most is not erroring, but
  erroring silently and continuing.

## 13. Emergent proactivity — **[operational observation, no mechanism given]**

The author reports a qualitative phase change after memory + routines + CoS
"stack up" sufficiently: the system begins doing things before being asked
(official framing: it "keeps evolving as the collaboration deepens, even
completing work in advance of the user's request"). No mechanism is given
for what triggers this beyond accumulated memory and routine density; this
is a claim, not a specifiable behavior, and is flagged here only because a
reader might otherwise look for a rule to encode — there isn't one described.

---

## Appendix material retained for reference

**Terminology mapped 1:1 to concepts already in `tinyhivemind`'s vocabulary:**

| Term (book) | Plain meaning |
|---|---|
| Bot | one persistent cloud worker = one cloud computer + one AI that drives it |
| Chief of Staff / 总监 | the meta-Bot managing other Bots; single task entry point |
| Lane / 专线 | a functional domain: inbox, expenses, recruiting, bug-fixing, ops |
| Routine | a demonstrated, replayable procedure |
| Auto-review | the approval card triggered by payment/sensitivity/uncertainty |
| Brain dump (大脑倾倒) | community slang: front-loading your own and your company's background once |
| Agent Mail | third-party standard kit: an independent mailbox identity per Bot |
| Tailscale | reverse-tunnel tool: lets a cloud Bot reach an authorized local device |

**Unresolved questions the author explicitly flags (appendix table 3, kept
because they bound how much of this to trust as mechanism):**

1. Whether a routine records click-trajectory or task-intent — evidence for
   both.
2. When memory management (export/delete/cross-device sync) ships.
3. What task volume a week's quota actually buys — no official conversion
   table yet.
4. Whether the underlying model selection is itself dynamically routed per
   task type.
5. Android release timing (later confirmed shipped, per the companion
   awesome-list's dated entries — outside this book's version).

---

## Mechanism → tinyhivemind has → does not have

| Mechanism from the orange book | tinyhivemind has | tinyhivemind does not have |
|---|---|---|
| Chief-of-Staff single entry point that fans out to specialists | Desks + roster (`docs/specs/desks.md`, `mentions.md`) give "who is on a desk" and mention resolution; `expert-delegation.md` gives a directory folded from grounded deposits | No LLM-driven task-decomposition/routing itself — by design, routing decisions live in the host or the model, not the pure algebra |
| Group-chat spontaneous inter-agent messaging, sharing one thread's context | The projection fold (`sessions.md`) already generalizes "read further back in a transcript both parties can see" | No notion of an agent *initiating* a request for context from another agent — that is a host/prompting concern, not a core primitive |
| Bug-relay four-stage handoff with a durable ticket, not a chat line | `cross-desk-referral.md`: one bounded child turn, one answer returns | No generic "ticket" object with its own lifecycle (open/claimed/done) — referral is turn-shaped, not artifact-shaped |
| Five-role fleet's four-field role schema (responsibility + sources + criteria + report format) | Roster records carry identity/membership | No structured persona/role-card schema — persona content is explicitly a host/consumer concern here |
| Routine (skill-by-demonstration, replay on cue) | Nothing analogous — no "teach once, replay" concept in scope | Entirely out of scope: `tinyhivemind` holds no state and does no learning; a routine is host-owned automation, not hive-mind mechanics |
| Auto-review three-bucket approval trigger (payment / sensitive / low-confidence) | Nothing analogous in the pure crates | No approval-gate port; this is squarely a host concern per the "host owns storage" rule, but the *predicate shape* (a decidable classification over an action) is a plausible future port |
| Six quota sinks, esp. poll→event-driven and session-boundary-as-topic-boundary | `continuous-sharing.md` (caller-owned watermarks, stateless deltas) already avoids re-paying for full history per read | No automatic "topic changed, start fresh" heuristic — session boundaries are caller-decided, not inferred |
| Unattended sentinel type (silent-until-threshold escalation) | The `hive` module's decaying salience + response thresholds is structurally the closest analog (a fold that stays silent below a threshold and escalates above it) | No wall-clock scheduling — hive is a pure fold over caller-supplied state, deliberately holds no timer |
| Unattended pipeline type (N-stage chain via chat handoff) | Sequential episode structure in `hive-mind.md`/ADR-0002 (one turn at a time, no concurrent variants) | No named "pipeline" abstraction chaining desks; would be host-composed from repeated referrals/dispatches |
| One-message-can-start-many-turns risk (group chat "no one taught it to do this") | Explicit invariant: "@everyone is a list, not a broadcast" — one message starts at most one turn | The orange book's own examples (spontaneous group messaging) show this invariant being *violated* in the wild by Grok Bot — useful negative evidence for why the invariant matters |
