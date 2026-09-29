import { expect, test } from "bun:test";

test("real render worker preserves its frame size and renderer after a failed reload", async () => {
  const worker = new Worker(new URL("../demo/worker/render-worker.ts", import.meta.url), { type: "module" });
  let id = 0;
  function request(message: Record<string, unknown>): Promise<Record<string, any>> {
    const requestId = ++id;
    return new Promise((resolve, reject) => {
      const timeout = setTimeout(() => reject(new Error("worker response timed out")), 5000);
      worker.onmessage = (event) => {
        if (event.data.kind === "ready" || event.data.requestId === requestId) {
          clearTimeout(timeout);
          resolve(event.data);
        }
      };
      worker.onerror = (event) => {
        clearTimeout(timeout);
        reject(new Error(event.message));
      };
      worker.postMessage({ ...message, requestId });
    });
  }
  try {
    expect((await request({ kind: "init" })).kind).toBe("ready");
    worker.postMessage({ kind: "setVideoSize", w: 256, h: 144 });
    const script = "[Script Info]\nPlayResX: 640\nPlayResY: 480\n[Events]\nDialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,hello";
    expect((await request({ kind: "loadAss", content: script })).kind).toBe("loaded");
    const before = await request({ kind: "render", timeMs: 1000 });
    expect(before.kind).toBe("frame");
    expect([before.w, before.h]).toEqual([256, 144]);
    expect((await request({ kind: "loadAss", content: "[Script Info]\nPlayResX: nope" })).kind).toBe("error");
    const after = await request({ kind: "render", timeMs: 1000 });
    expect(after.kind).toBe("frame");
    expect([after.w, after.h]).toEqual([256, 144]);
    expect(new Uint8Array(after.bytes)).toEqual(new Uint8Array(before.bytes));
    // A failed size change must not poison the size applied to the next load.
    expect((await request({ kind: "setVideoSize", w: 0, h: 144 })).kind).toBe("error");
    expect((await request({ kind: "loadAss", content: script })).kind).toBe("loaded");
    const replacement = await request({ kind: "render", timeMs: 1000 });
    expect(replacement.kind).toBe("frame");
    expect([replacement.w, replacement.h]).toEqual([256, 144]);
    const utf16 = new Uint8Array(Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from(script, "utf16le")]));
    expect((await request({ kind: "loadAss", content: utf16 })).kind).toBe("loaded");
    const encoded = await request({ kind: "render", timeMs: 1000 });
    expect(encoded.kind).toBe("frame");
    expect(new Uint8Array(encoded.bytes)).toEqual(new Uint8Array(before.bytes));
  } finally {
    worker.terminate();
  }
});
