// Turns a bundled hive-floor frame fixture into the stream `hive_view` emits
// for a multi-desk package: one floor per desk, so every round names its
// `desk` and `desk_round`; a `referral` frame whenever the conversation moves
// to another desk (accepted along a declared link, refused otherwise, plus one
// deliberately refused hop); a `desk_end` per desk; and, live, a `usage`
// frame. Pure (no IO), so the page's tests and fake-hive-view.mjs share it.

/** True when `from`→`to` (or its reverse) is a topology or referral link. */
function linkOf(structure, from, to) {
  const links = [...(structure.topology ?? []), ...(structure.referrals ?? [])];
  if (links.some((e) => e.from === from && e.to === to)) return "forward";
  if (links.some((e) => e.from === to && e.to === from)) return "reverse";
  return null;
}

/** The first declared link that is not `from`→`to`, for the refused hop. */
const someLink = (structure) => [...(structure.topology ?? []), ...(structure.referrals ?? [])][0] ?? null;

/**
 * Rewrite `frames` (a fixture stream) for `structure`. `opts = { live, model }`.
 * A single-desk package only gains `round.desk`/`desk_round`.
 */
export function multiDeskFrames(frames, structure, opts = {}) {
  const desks = (structure.desks ?? []).map((d) => d.id);
  const multi = desks.length > 1;
  const out = [];
  const perDesk = new Map(desks.map((d) => [d, { rounds: 0, turns: 0 }]));
  let global = 0;
  let prev = null;
  let refusedHop = false;
  let end = null;
  const referral = (kind, from, to, accepted, reason, seat) => {
    const f = { type: "referral", round: global, kind, from_desk: from, to_desk: to, seat, target: accepted ? `${to}-01` : null,
      content: `!propose ${from} reports to ${to}`, hops: accepted ? 1 : 4, accepted, sequence: accepted ? out.length : null };
    if (!accepted) f.reason = reason;
    out.push(f);
  };
  for (const frame of frames) {
    if (frame.type === "start") {
      out.push(opts.live ? { ...frame, live: true, model: opts.model ?? "openai/gpt-oss-120b:nitro" } : frame);
    } else if (frame.type === "round") {
      const byDesk = new Map();
      for (const t of frame.turns ?? []) {
        const d = desks.includes(t.desk) ? t.desk : desks[0];
        if (!byDesk.has(d)) byDesk.set(d, []);
        byDesk.get(d).push(t);
      }
      for (const [desk, turns] of byDesk) {
        let incoming = false;
        if (multi && prev && prev.desk !== desk) {
          const seat = prev.turns.at(-1)?.seat ?? null;
          const link = linkOf(structure, prev.desk, desk);
          referral(link === "reverse" ? "return" : "report", prev.desk, desk, link != null, "unlinked", seat);
          incoming = link != null;
          const attempt = someLink(structure);
          if (!refusedHop && attempt && global >= 2) {
            refusedHop = true;
            referral("forward", attempt.from, attempt.to, false, "hop_limit_reached", seat);
            // and one attempt along an edge nobody declared
            const stray = desks.find((d) => d !== desk && d !== prev.desk && !linkOf(structure, desk, d));
            if (stray) referral("forward", desk, stray, false, "unlinked", seat);
          }
        }
        const stats = perDesk.get(desk);
        stats.rounds += 1;
        stats.turns += turns.length;
        global += 1;
        const marked = incoming ? turns.map((t, i) => (i === 0 ? { ...t, reason: "referral" } : t)) : turns;
        const round = { ...frame, round: global, desk, desk_round: stats.rounds, turns: marked };
        if (incoming && frame.phase !== "commit") round.phase = "answer";
        out.push(round);
        prev = { desk, turns };
      }
    } else if (frame.type === "end") {
      end = frame;
    } else out.push(frame);
  }
  if (!end) return out;
  const answerDesk = prev?.desk ?? desks[0] ?? null;
  if (multi) {
    for (const desk of desks) {
      const { rounds, turns } = perDesk.get(desk);
      const opened = rounds > 0;
      out.push({
        type: "desk_end", desk,
        ending: !opened ? "unopened" : desk === answerDesk ? end.ending : turns > 8 ? "converged" : "idle",
        topic: opened && (desk === answerDesk || turns > 8) ? end.topic : null,
        correct: opened && (desk === answerDesk || turns > 8) ? end.correct : null,
        rounds, turns,
      });
    }
  }
  if (opts.live) {
    out.push({ type: "usage", calls: end.turns, prompt_tokens: end.turns * 900, completion_tokens: end.turns * 120,
      total_tokens: end.turns * 1020, cost_usd: Number((end.turns * 0.00021).toFixed(5)), wall_ms: end.turns * 1400, exhausted_calls: false });
  }
  out.push({ ...end, rounds: global, ...(multi ? { desk: answerDesk } : {}) });
  return out;
}
