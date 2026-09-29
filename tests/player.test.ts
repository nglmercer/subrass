import { afterEach, beforeEach, describe, expect, spyOn, test } from "bun:test";
import { Player } from "../demo/shared/player.ts";

let now = 1000;
let frameId = 0;
const frames = new Map<number, FrameRequestCallback>();
const originalRequest = globalThis.requestAnimationFrame;
const originalCancel = globalThis.cancelAnimationFrame;
let clock: ReturnType<typeof spyOn>;

beforeEach(() => {
  now = 1000;
  frameId = 0;
  frames.clear();
  clock = spyOn(performance, "now").mockImplementation(() => now);
  globalThis.requestAnimationFrame = (callback) => {
    frames.set(++frameId, callback);
    return frameId;
  };
  globalThis.cancelAnimationFrame = (id) => { frames.delete(id); };
});

afterEach(() => {
  clock.mockRestore();
  globalThis.requestAnimationFrame = originalRequest;
  globalThis.cancelAnimationFrame = originalCancel;
});

function virtualPlayer() {
  const rendered: number[] = [];
  const player = new Player({} as HTMLVideoElement, {
    onFrame: (time) => { rendered.push(time); },
  });
  player.setVirtualDuration(311000);
  return { player, rendered };
}

describe("demo playback clock", () => {
  test("pause and resume preserve elapsed virtual playback", () => {
    const { player } = virtualPlayer();
    player.seekMs(68000);
    player.play();
    now += 109;
    expect(player.currentTimeMs()).toBe(68109);
    player.pause();
    expect(player.currentTimeMs()).toBe(68109);
    expect(frames.size).toBe(0);
    now += 5000;
    expect(player.currentTimeMs()).toBe(68109);
    player.play();
    now += 250;
    expect(player.currentTimeMs()).toBe(68359);
    player.pause();
  });

  test("seeking while playing rebases the clock and stop cancels frames", () => {
    const { player, rendered } = virtualPlayer();
    player.play();
    now += 1000;
    player.seekMs(289345);
    expect(rendered.at(-1)).toBe(289345);
    now += 100;
    expect(player.currentTimeMs()).toBe(289445);
    player.stop();
    expect(player.currentTimeMs()).toBe(0);
    expect(frames.size).toBe(0);
  });
});
