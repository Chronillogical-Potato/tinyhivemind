// The replay clock: buffers frames as they stream in and walks a step index
// through them at the chosen speed. DOM-free; `onChange` redraws.
import { atEnd, clampStep, stateAt, timeline } from "./simulation.js";

const ROUND_MS = 900;

export function createPlayer(onChange) {
  let frames = [];
  let tl = timeline([]);
  let step = 0;
  let playing = false;
  let speed = 1;
  let timer = null;

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
    /** Drop everything and start following a fresh stream. */
    reset(autoplay = true) {
      frames = [];
      tl = timeline(frames);
      step = 0;
      playing = autoplay;
      schedule();
      emit();
    },
    push(frame) {
      frames.push(frame);
      tl = timeline(frames);
      if (!playing) emit();
    },
    play() {
      if (atEnd(tl, step)) step = 0;
      playing = true;
      emit();
      schedule();
    },
    pause() {
      playing = false;
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
      return { ...stateAt(tl, step), playing, speed, received: tl.rounds.length, ended: tl.end != null };
    },
  };
  return api;
}
