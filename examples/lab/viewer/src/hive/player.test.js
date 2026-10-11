import { test } from "node:test";
import assert from "node:assert/strict";
import { createPlayer } from "./player.js";

test("finish stops playback when the stream ends without an end frame", () => {
  const player = createPlayer(() => {});
  player.reset(true, false);
  assert.equal(player.state().playing, true);
  player.finish();
  assert.equal(player.state().playing, false);
});
