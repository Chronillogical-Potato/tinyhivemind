# gojiberryai-sales-os

**What it is.** A Markdown "agent skill" pack for the GojiberryAI sales
platform, distributed as a plugin for Grok Bot / Claude Code / Cursor / Codex.
Thirteen role prompts plus one orchestrator skill plus a hosted MCP server,
no application code at all — the entire repo is instructions for a host LLM
harness, not an executable multi-agent system.

- URL: https://github.com/romangojiberryAI/gojiberryai-sales-os
- SHA: `0ee5d22d380516bf1c16c7c74f81d4e0faa7cb4e`
- Licence: MIT
- Size: 352K, 1196 lines of Markdown total across `agents/*.md`, `AGENTS.md`,
  `skills/sales-os/SKILL.md`, `skills/sales-os/references/*.md`; one
  `.mcp.json` naming a single hosted HTTP MCP server
  (`skills/sales-os/SKILL.md:1-146`, `AGENTS.md:1-55`, `.mcp.json:310-317` in
  the concatenated read — see raw file for exact line numbers, it is an
  8-line JSON file).

## Architecture

A single skill file (`skills/sales-os/SKILL.md`) is the entry point any host
loads; it names a routing table and fans out to thirteen `agents/*.md` prompt
files, each of which in turn is told to load exactly one `references/*.md`
module (`skills/sales-os/SKILL.md:26-46`). There is no runtime: "spawning a
specialist" means the host's own subagent mechanism (Claude Code Task tool,
Grok Bot's agent launcher, etc.) starts a fresh LLM context seeded with that
one Markdown file. The repo supplies no code that decides who goes next,
scores anything, or enforces a gate — every one of those is prose the model
is asked to follow.

The thirteen roles: `head-of-sales`, `signal-hunter`, `icp-analyst`,
`account-researcher`, `lead-enricher`, `intent-scorer`, `linkedin-copywriter`,
`outreach-operator`, `reply-agent`, `follow-up-agent`, `meeting-qualifier`,
`pipeline-analyst`, `sales-manager` (`agents/*.md`, 13 files, `AGENTS.md:3`).

## How roles are declared

A role is a **prompt file with YAML frontmatter**, not a config record or code
object. Every file under `agents/` has the same shape
(`agents/signal-hunter.md:1-24`):

```yaml
---
name: signal-hunter
description: "Use this agent to find prospects showing real buying intent — ... Typical triggers: ... Do not use this agent to write messages or launch campaigns."
model: inherit
color: green
---
```

Fields present: `name`, `description` (doubles as the router's dispatch key —
it is prose a host's own agent-selection heuristic reads, not a machine
schema), `model` (always the literal string `inherit`, every file,
e.g. `agents/head-of-sales.md:4`, `agents/reply-agent.md:4` — no per-role
model tier), `color` (a UI hint, no functional role). No field carries
expertise as data, no tool allowlist as data (tools are named in prose:
"`Load skills/sales-os/references/enrichment.md` ... Use
`ContactExternalController_enrichEmail`, `findOne`, `update`, `create`" —
`agents/lead-enricher.md:12`), no numeric threshold as a schema field (score
cut lines and caps appear as prose numbers inside the body, e.g. "Cap 25
unless the user set another number" — `agents/signal-hunter.md:23`), and no
budget field at all. The closest thing to a threshold is the intent cut line
stated in text: "default first-touch ≥ 65" (`skills/sales-os/references/scoring.md:254`,
`agents/intent-scorer.md:21`).

Contrast with `tinyhivemind`'s `AgentThreshold { agent_id, threshold, affinity }`
(`docs/specs/hive-mind.md`, Attention section) — a typed record a pure fold
reads and updates. Here the equivalent value is a sentence a model has to
parse and remember to honour on every turn; nothing prevents it from drifting
across a long context or being silently dropped by a different host's prompt
truncation.

## Routing between roles

**Fixed pipeline**, written down twice, once as an ASCII chain and once as a
lookup table, with no market and no ad-hoc mention grammar:

- The canonical chain, `skills/sales-os/SKILL.md:52-63`:
  `Signal Hunter → ICP Analyst → Account Researcher → Lead Enricher → Intent
  Scorer → LinkedIn Copywriter → Outreach Operator (approval gate) → Reply
  Agent → Follow-up Agent → Meeting Qualifier`, with Head of Sales run before
  it and Pipeline Analyst after.
- The same chain restated with named triggers in
  `skills/sales-os/references/routing.md:169-175` as a table of `Intent →
  Chain`, e.g. `"New outbound from a prompt" | head-of-sales → signal-hunter →
  icp-analyst → account-researcher → lead-enricher → intent-scorer →
  linkedin-copywriter → STOP for approval → outreach-operator`.
- A per-role "when to invoke" section repeats the same edges locally in every
  agent file (e.g. `agents/meeting-qualifier.md:14-17`: "Take **interested**
  (and strong question) threads from Reply Agent").

The one process actually named "coordinator" is `sales-manager`
(`agents/sales-manager.md:1-32`), and its job description is explicitly to
walk the fixed table, not to decide dynamically: "Publish the pipeline board.
Pick a chain from `routing.md`. Spawn specialists in order (parallel only
where routing allows)" (`agents/sales-manager.md:22-24`). Parallel fan-out is
allowed only within a stage (`skills/sales-os/SKILL.md:67-79`: "one
`account-researcher` per account (cap 8 in parallel)"), never across stages,
and there is no routing rule that reads role output to pick the *next* role
dynamically — the next role is a function of which stage just finished, fixed
at design time.

## Termination

No quorum, no consensus check, no accumulating support score anywhere in the
repo. An episode ends at one of three fixed points, all textual:

1. **A stage boundary that requires human approval.** `STOP for approval` is
   written directly into the chain table before `outreach-operator`
   (`skills/sales-os/references/routing.md:171`) and restated as "Stop at the
   approval gate before Outreach Operator mutates anything — unless
   autonomous + threshold" (`agents/sales-manager.md:26`).
2. **A named cap.** "Cap 50 new prospects per run" (`agents/sales-manager.md:28`,
   `skills/sales-os/references/routing.md:225`), "Cap 25 unless the user set
   another number" (`agents/signal-hunter.md:23`) — a count limit, not a
   convergence condition.
3. **A terminal classification with no next agent**, e.g. reply label
   `negative` → "log, do not message" with no successor
   (`skills/sales-os/references/replies.md:17`), or `noise` → "ignore"
   (same file, line 18).

There is nothing resembling `HiveStep::Converged` / `Deadlocked` /
`Exhausted` — no state machine object exists; "termination" is just "the
table has no next row" or "the prompt tells the model to stop and wait."

## Adversarial / review roles

There is exactly one role built to say no: `icp-analyst`, explicitly framed
as a brake — "You are a brake. False positives cost more than missed logos"
(`agents/icp-analyst.md:10`) — with a three-way verdict Keep/Maybe/Drop and
the rule "Maybe does not go to Outreach Operator. Drops are never silent"
(`agents/icp-analyst.md:21`). This *can* change the outcome in the weak sense
that a `Drop` verdict removes a row from the pipeline before downstream
agents see it. But it is advisory prose enforced by nothing except the next
agent choosing to respect the label — there is no data structure a later
stage consults to confirm the drop actually happened, and no mechanism
analogous to cross-inhibition where an objection *targets and silences a
specific advocate's future turns*. Once a row is marked Drop it simply is not
carried forward in the handoff packet (`skills/sales-os/references/routing.md:199-221`
defines the packet, with fields like `disqualifier:` that a downstream agent
is trusted to check, not one that is structurally enforced).

`meeting-qualifier` is a second soft gate — "Missing one: ask. Missing three:
do not book" (`skills/sales-os/references/qualification.md:60`) — again
prose criteria, no vote, no accumulation, no way for two disagreeing
qualifiers to be reconciled (there is only ever one qualifier per thread).

No role can silence another role's ongoing "advocacy" the way `Object`
removes an author from `supporters` in `hive-mind.md`'s cross-inhibition — a
dropped lead is discarded data, not a demoted participant, because there is
no persistent participant state to demote.

## Approval gates and propose-vs-execute default

This is the one mechanism with real teeth, and it is enforced redundantly at
three layers, though all three are still prompt instructions rather than
code:

- **Skill-level rule**, stated as one of three global rules: "Propose, don't
  send — until asked. Default mode never fires a connection request,
  message, or campaign mutation. Autonomous mode requires an explicit user
  instruction and a score threshold" (`skills/sales-os/SKILL.md:18`).
- **Role-level restatement** on every agent that can mutate anything, e.g.
  `outreach-operator`: "You are the hands. You are not the brain. The
  approval gate is the job" (`agents/outreach-operator.md:10`), with an
  explicit two-mode table: propose mode shows the pack and waits for
  "send"/"launch"/"add them"; autonomous mode requires rows ≥ a min score,
  not already in a campaign, not disqualified
  (`skills/sales-os/references/outreach.md:277-281`). `lead-enricher`:
  "Mutations require approval unless autonomous mode is on"
  (`agents/lead-enricher.md:12`). `follow-up-agent`: "Send via
  `UniboxExternalController_sendMessage` on an **existing** thread, approval
  gate on" (`agents/follow-up-agent.md:12`).
- **Repo-level guardrail** in `AGENTS.md`: "Never send a LinkedIn message,
  connection request, or campaign update unless the user approved it **or**
  they enabled autonomous mode with an explicit score threshold"
  (`AGENTS.md:54`).

Autonomous mode is toggled by a value in `icp-context.md`
("`icp-context.md` sets `Mode: autonomous` with a numeric min score" —
`skills/sales-os/references/routing.md:158`), an external Markdown file the
user edits by hand; the min score is a plain number in that same file. There
is no enforcement outside model compliance — nothing stops a misbehaving host
or a jailbroken prompt from calling the send tool directly, since the actual
MCP tools (`UniboxExternalController_sendMessage`,
`CampaignExternalController_update`, etc., listed in
`skills/sales-os/references/mcp.md`) are ungated at the transport level; the
gate is entirely in what the model is told to do before calling them.

## Cost / budget accounting

None, per role or in aggregate. The only numeric caps are counts of
prospects per run (50, `agents/sales-manager.md:28`) or per hunt (25,
`agents/signal-hunter.md:23`), not token, time, or dollar budgets, and
nothing tracks spend across the chain. `model: inherit` on every agent
(e.g. `agents/icp-analyst.md:4`) means there is no per-role model tier to
even attach a cost differential to — every specialist runs on whatever model
the host session is already using.

## Pattern vs. prompt engineering

Blunt take: **almost none of this is a pattern for a pure algebra.** It is a
well-organized prompt library — a fixed pipeline described three redundant
times in prose, a brake role whose verdict is trusted rather than enforced,
and an approval gate that is a compliance convention repeated at every layer
because the authors evidently don't trust it to hold once. Nothing here folds
over a transcript; nothing is order-independent, decays, or accumulates a
score across turns the way `standings`, `salience`, or `directory` do in
`crates/tinyhivemind-core/src/hive/`. The "thirteen agents" figure is really thirteen
context windows sharing one static routing table — closer to a shell script
with LLM-shaped steps than to a deliberation protocol.

The two things worth naming as *ideas*, even though neither is implemented as
an algebra here:

1. **A single, explicit propose/execute boundary that every mutating role
   must clear**, restated at global, role, and repo level. `tinyhivemind`
   has no equivalent concept at all — a hive episode's `Commit` phase records
   that a decision was reached, but nothing in the spec distinguishes a
   read-only turn from one a host should gate behind human sign-off before
   acting on it in the world. That's a real gap this project's redundancy is
   pointing at, but the *encoding* here (three copies of an English sentence)
   is exactly what you would not want to borrow.
2. **A brake role with veto framing** (`icp-analyst`) is a plain-language
   version of what `Object` + cross-inhibition already does formally and
   better: it silences a specific advocate rather than discarding an option,
   and a grounded objection breaks ties in a way this pipeline's silent
   "Drop" cannot, because there is no ties to break — the pipeline has no
   concurrent standings in the first place.

## Mechanism → tinyhivemind has → does not have

| Mechanism here | tinyhivemind has | tinyhivemind does not have |
|---|---|---|
| Fixed named pipeline stated in prose (3x) | `docs/adr/0002` sequential episodes; a host can wire a fixed responder ladder | No pipeline-as-data type; routing is a bid/argmax over active members, not stage order |
| Role prompt file (`name/description/model/color`) | `RosterMember {id, name}`, `AgentThreshold {agent_id, threshold, affinity}` as typed, foldable records | No expertise/tool/budget fields on a role type — same gap this repo also has, just encoded as prose instead of an unused struct field |
| Brake role (`icp-analyst` Keep/Maybe/Drop) | `Object` + cross-inhibition: silences a specific advocate, can break a tie between equal options | No persistent-participant demotion here; a Drop just removes a row, nothing to silence |
| Propose vs. execute gate, redundant at 3 layers | `Phase::Deliberate → Commit` distinguishes "decided" from "not yet"; `HiveStep::Converged` requires a recorded `Commit` trace, not just reachability | No concept of a mutation needing external (human) sign-off before the host acts on it — this is a real, unaddressed gap |
| Numeric run/hunt caps (50, 25) | `turn_budget`, `Exhausted` | Not a cost/token/dollar budget either — same absence on both sides |
| Score cut line (heuristic ≥ 65) in prose | Fixed-point weight thresholds (`floor`, `WEIGHT_CEILING`) computed by a pure fold from transcript evidence | This repo's score is asserted by an LLM each run, never derived from an auditable trace history |
| No termination state machine; "stop" is a table row with no successor | `HiveStep::{Converged, Deadlocked, Exhausted, Idle}` — explicit, auditable termination reasons | — |
