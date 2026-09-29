import { expect, test } from "bun:test";
import init from "../pkg/subrass.js";
import { startDemo } from "../demo/shared/app.ts";
import type { RenderBackend } from "../demo/shared/types.ts";

class Element {
  textContent = "";
  innerHTML = "";
  style = { display: "" };
  classList = { toggle() {} };
  disabled = true;
  checked = true;
  value = "0";
  listeners = new Map<string, (event: any) => unknown>();
  addEventListener(name: string, listener: (event: any) => unknown) { this.listeners.set(name, listener); }
  matches() { return false; }
  async emit(name: string, target: unknown = this) { await this.listeners.get(name)?.({ target }); }
}

test("demo file loading keeps valid state on errors and reads raw subtitle bytes", async () => {
  await init();
  const ids = ["video", "subtitleCanvas", "playPauseBtn", "stopBtn", "seekBar", "loopCheck",
    "backendBadge", "eventList", "timeDisplay", "summaryRes", "summaryStyles", "summaryEvents",
    "summaryTitle", "videoInput", "videoName", "assInput", "assName", "fontInput", "fontList", "error",
    "inspectCheck", "sceneSelect", "sceneNote", "previewStage", "debugOverlay", "hoverTooltip", "inspectorDetails", "inspectStatus"];
  const elements = new Map(ids.map((id) => [id, new Element()]));
  const canvas = Object.assign(elements.get("subtitleCanvas")!, {
    width: 1920, height: 1080,
    getContext: () => ({ fillStyle: "", fillRect() {} }),
  });
  Object.assign(elements.get("video")!, { videoWidth: 0, videoHeight: 0 });
  const saved = new Map(["document", "window", "fetch"].map((name) =>
    [name, Object.getOwnPropertyDescriptor(globalThis, name)]));
  const timers: ReturnType<typeof setTimeout>[] = [];
  const script = (word: string) => `[Script Info]\nPlayResX: 640\nPlayResY: 480\n[Events]\nDialogue: 0,0:00:00.00,0:06:00.00,Default,,0,0,0,,${word}`;
  let failBackend = false;
  let receivedBytes = false;
  const rendered: number[] = [];
  const backend: RenderBackend = {
    kind: "test", async init() {}, setFrameTarget() {}, resize() {}, loadFont() {}, dispose() {},
    async loadAss(content) {
      if (failBackend) throw new Error("backend rejected replacement");
      receivedBytes = content instanceof Uint8Array;
      return { resolution: [640, 480], styles: 0, events: 1 };
    },
    renderFrame(time) { rendered.push(time); },
  };
  Object.defineProperty(globalThis, "document", { configurable: true,
    value: { getElementById: (id: string) => elements.get(id) } });
  Object.defineProperty(globalThis, "window", { configurable: true, value: {
    addEventListener() {}, clearTimeout,
    setTimeout(callback: () => void, ms: number) { const timer = setTimeout(callback, ms); timers.push(timer); return timer; },
  } });
  Object.defineProperty(globalThis, "fetch", { configurable: true,
    value: async () => new Response(script("original")) });
  try {
    await startDemo(backend);
    expect(elements.get("eventList")!.innerHTML).toContain("original");
    expect([canvas.width, canvas.height]).toEqual([1920, 1080]);
    // The scene display follows seeks instead of staying at Introduction.
    for (const [time, scene] of [[66243, "67000"], [289852, "287000"]] as const) {
      elements.get("seekBar")!.value = String(time / 363000 * 1000);
      await elements.get("seekBar")!.emit("input");
      expect(elements.get("sceneSelect")!.value).toBe(scene);
    }
    expect(elements.get("sceneNote")!.textContent).toContain("overlap intentionally");
    await elements.get("stopBtn")!.emit("click");
    expect(elements.get("sceneSelect")!.value).toBe("0");
    // A parser error and a backend error both preserve the previous document.
    for (const content of ["[Script Info]\nPlayResX: nope", script("rejected")]) {
      failBackend = content.includes("rejected");
      await elements.get("assInput")!.emit("change", { files: [{ name: "bad.ass",
        arrayBuffer: async () => new TextEncoder().encode(content).buffer }] });
      elements.get("seekBar")!.value = "100";
      await elements.get("seekBar")!.emit("input");
      expect(elements.get("eventList")!.innerHTML).toContain("original");
      expect(elements.get("summaryRes")!.textContent).toBe("640x480");
      expect(elements.get("assName")!.textContent).toBe("sample.ass");
    }
    failBackend = false;
    const bytes = Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from(script("UTF16 café"), "utf16le")]);
    await elements.get("assInput")!.emit("change", { files: [{ name: "unicode.ass",
      arrayBuffer: async () => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) }] });
    expect(receivedBytes).toBe(true);
    expect(elements.get("assName")!.textContent).toBe("unicode.ass");
    expect(elements.get("eventList")!.innerHTML).toContain("UTF16 café");
    expect(elements.get("sceneSelect")!.disabled).toBe(true);
    expect(elements.get("sceneSelect")!.value).toBe("");
    expect(elements.get("sceneNote")!.textContent).not.toContain("overlap intentionally");
    expect(rendered.length).toBeGreaterThan(0);
  } finally {
    for (const timer of timers) clearTimeout(timer);
    for (const [name, descriptor] of saved) {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else Reflect.deleteProperty(globalThis, name);
    }
  }
});
