// The replay clock: buffers frames as they stream in and walks a step index
// through them at the chosen speed. DOM-free; `onChange` redraws. In follow
// mode (a live run, where rounds arrive minutes apart) the step jumps to the
// newest round as each frame lands, until the user pauses, steps or scrubs;
// `latest()` resumes following. Once the stream finishes it is a normal replay.
import { atEnd, clampStep, stateAt, timeline } from "./simulation.js";

const ROUND_MS = 900;

export function createPlayer(onChange) {
  let frames = [];
  let tl = timeline([]);
  let step = 0;
  let playing = false;
  let speed = 1;
  let timer = null;
  let following = false;
  let streaming = false;

  const emit = () => onChange(api.state());

  function schedule() {
    clearTimeout(timer);
    if (!playing) return;
    timer = setTimeout(tick, ROUND_MS / speed);
  }

  function tick() {
    if (step < tl.rounds.length) step++;
    else if (tl.end) playing = false;
    // else: waiting on the stream; keep polling at the same pace
    emit();
    schedule();
  }

  const api = {
    /**
     * Drop everything. `autoplay` starts a fresh stream (played at the chosen
     * speed); `follow` instead keeps the step on the newest round as frames land.
     */
    reset(autoplay = true, follow = false) {
      frames = [];
      tl = timeline(frames);
      step = 0;
      streaming = autoplay;
      following = autoplay && follow;
      playing = autoplay && !follow;
      schedule();
      emit();
    },
    push(frame) {
      api.pushMany([frame]);
    },
    /** Several frames at once, folded and drawn once: a long recording loads in one go. */
    pushMany(list) {
      if (!list.length) return;
      for (const f of list) frames.push(f);
      tl = timeline(frames);
      if (following) step = tl.rounds.length;
      if (!playing) emit();
    },
    /** The stream closed (done, failed or aborted): stop following. */
    finish() {
      streaming = false;
      following = false;
      if (!tl.end) {
        playing = false;
        clearTimeout(timer);
      }
      emit();
    },
    /** Jump to the newest round and follow the stream again. */
    latest() {
      playing = false;
      clearTimeout(timer);
      following = streaming;
      step = tl.rounds.length;
      emit();
    },
    play() {
      following = false;
      if (atEnd(tl, step)) step = 0;
      playing = true;
      emit();
      schedule();
    },
    pause() {
      playing = false;
      following = false;
      clearTimeout(timer);
      emit();
    },
    toggle() {
      return playing ? api.pause() : api.play();
    },
    seek(n) {
      step = clampStep(tl, n);
      emit();
    },
    step(by) {
      api.pause();
      api.seek(step + by);
    },
    setSpeed(x) {
      speed = x;
      schedule();
    },
    state() {
      return { ...stateAt(tl, step), playing, speed, following, streaming, received: tl.rounds.length, ended: tl.end != null };
    },
  };
  return api;
}
