import { afterAll, beforeAll, describe, expect, test } from "bun:test";
import { DirectBackend } from "../demo/shared/direct-backend.ts";

// Exercise the real WASM backend under Bun. Only the canvas drawing surface
// is replaced; parsing, sizing, rasterization, and renderer lifetime are real.
class TestContext {
  image: { width: number; height: number } | null = null;
  ink = 0;
  clearRect() {}
  putImageData(image: TestImageData) {
    this.image = { width: image.width, height: image.height };
    this.ink = image.data.filter((value, index) => index % 4 === 3 && value > 0).length;
  }
}
class TestImageData {
  constructor(public data: Uint8ClampedArray, public width: number, public height: number) {}
}
const contextDescriptor = Object.getOwnPropertyDescriptor(globalThis, "CanvasRenderingContext2D");
const imageDescriptor = Object.getOwnPropertyDescriptor(globalThis, "ImageData");

beforeAll(() => {
  Object.defineProperty(globalThis, "CanvasRenderingContext2D", { value: TestContext, configurable: true });
  Object.defineProperty(globalThis, "ImageData", { value: TestImageData, configurable: true });
});
afterAll(() => {
  if (contextDescriptor) Object.defineProperty(globalThis, "CanvasRenderingContext2D", contextDescriptor);
  else Reflect.deleteProperty(globalThis, "CanvasRenderingContext2D");
  if (imageDescriptor) Object.defineProperty(globalThis, "ImageData", imageDescriptor);
  else Reflect.deleteProperty(globalThis, "ImageData");
});

const source = (width: number, height: number) => `[Script Info]\nPlayResX: ${width}\nPlayResY: ${height}\n[Events]\nDialogue: 0,0:00:00.00,0:00:05.00,Default,,0,0,0,,hello`;

describe("main-thread demo renderer lifecycle", () => {
  test("subtitle reload keeps the existing output size", async () => {
    const ctx = new TestContext();
    const canvas = { width: 256, height: 144, getContext: () => ctx } as unknown as HTMLCanvasElement;
    const backend = new DirectBackend();
    await backend.init();
    backend.setFrameTarget(canvas);
    await backend.loadAss(source(384, 288));
    backend.resize(256, 144);
    backend.renderFrame(1000);
    expect(ctx.image).toMatchObject({ width: 256, height: 144 });
    await backend.loadAss(source(640, 480));
    backend.renderFrame(1000);
    expect(ctx.image).toMatchObject({ width: 256, height: 144 });
    expect([canvas.width, canvas.height]).toEqual([256, 144]);
    backend.dispose();
  });

  test("invalid replacement subtitles leave the previous renderer usable", async () => {
    const ctx = new TestContext();
    const canvas = { width: 256, height: 144, getContext: () => ctx } as unknown as HTMLCanvasElement;
    const backend = new DirectBackend();
    await backend.init();
    backend.setFrameTarget(canvas);
    await backend.loadAss(source(256, 144));
    await expect(backend.loadAss("[Script Info]\nPlayResX: nope")).rejects.toThrow();
    backend.renderFrame(1000);
    expect(ctx.image).toMatchObject({ width: 256, height: 144 });
    backend.dispose();
  });

  test("raw UTF-16 and legacy uploads render like the corresponding Unicode text", async () => {
    const ctx = new TestContext();
    const canvas = { width: 256, height: 144, getContext: () => ctx } as unknown as HTMLCanvasElement;
    const backend = new DirectBackend();
    await backend.init();
    backend.setFrameTarget(canvas);
    const script = source(256, 144).replace("hello", "café");
    await backend.loadAss(script);
    backend.renderFrame(1000);
    const expected = ctx.ink;
    expect(expected).toBeGreaterThan(0);
    const utf16 = new Uint8Array(Buffer.concat([Buffer.from([0xff, 0xfe]), Buffer.from(script, "utf16le")]));
    const legacy = new Uint8Array(Buffer.concat([Buffer.from(script.slice(0, -1)), Buffer.from([0xe9])]));
    for (const bytes of [utf16, legacy]) {
      await backend.loadAss(bytes);
      backend.renderFrame(1000);
      expect(ctx.ink).toBe(expected);
    }
    backend.dispose();
  });
});
