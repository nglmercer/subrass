// Main-thread rendering backend: the WASM renderer lives next to the UI
// and draws straight into the visible canvas. This is the simplest way to
// use subrass — see the worker example for the off-thread variant.
import init, { SubtitleRenderer } from "../../pkg/subrass.js";
import type { DebugFrame, RenderBackend, SubtitleSummary } from "./types.ts";
import { dbg } from "./debug.ts";

const log = dbg("direct-backend");

export class DirectBackend implements RenderBackend {
  readonly kind = "main-thread";
  private renderer: SubtitleRenderer | null = null;
  private canvas: HTMLCanvasElement | null = null;
  private frameW = 0;
  private frameH = 0;
  private debugEnabled = false;
  private onDebugFrame: ((frame: DebugFrame) => void) | null = null;

  async init(): Promise<void> {
    log("init() begin", {
      importMetaUrl: import.meta.url,
      // Default init() fetches subrass_bg.wasm next to the JS module.
      expectedWasmUrl: new URL("../../pkg/subrass_bg.wasm", import.meta.url).href,
    });
    const t0 = performance.now();
    try {
      const exports = await init();
      log("init() ok", {
        ms: +(performance.now() - t0).toFixed(1),
        hasExports: !!exports,
        hasMalloc: typeof (exports as { __wbindgen_malloc?: unknown })?.__wbindgen_malloc,
      });
    } catch (err) {
      log("init() FAILED", { ms: +(performance.now() - t0).toFixed(1), err });
      throw err;
    }
  }

  setFrameTarget(canvas: HTMLCanvasElement): void {
    log("setFrameTarget", { id: canvas.id, w: canvas.width, h: canvas.height });
    this.canvas = canvas;
    this.renderer?.set_canvas(canvas);
    this.resize(canvas.width, canvas.height);
  }

  async loadAss(content: string | Uint8Array): Promise<SubtitleSummary> {
    log("loadAss begin", { contentBytes: content.length, hasCanvas: !!this.canvas });
    if (!this.canvas) throw new Error("setFrameTarget() must be called before loadAss()");
    let next: SubtitleRenderer | null = null;
    try {
      next = typeof content === "string"
        ? new SubtitleRenderer(content)
        : SubtitleRenderer.from_bytes(content);
      next.set_canvas(this.canvas);
      next.set_debug_enabled(this.debugEnabled);
      if (this.frameW > 0 && this.frameH > 0) next.set_video_size(this.frameW, this.frameH);
      const [w, h] = next.get_play_resolution();
      const summary = {
        resolution: [w, h] as [number, number],
        styles: next.get_style_count(),
        events: next.get_event_count(),
      };
      this.renderer?.free();
      this.renderer = next;
      next = null;
      log("loadAss ok", summary);
      return summary;
    } catch (err) {
      next?.free();
      log("loadAss FAILED (SubtitleRenderer needs initialized wasm)", err);
      throw err;
    }
  }

  resize(width: number, height: number): void {
    if (width > 0 && height > 0) {
      try {
        this.renderer?.set_video_size(width, height);
        this.frameW = width;
        this.frameH = height;
      } catch (err) {
        log("set_video_size rejected", { width, height, err });
      }
    }
  }

  setDebug(enabled: boolean, onFrame: (frame: DebugFrame) => void): void {
    this.debugEnabled = enabled;
    this.onDebugFrame = enabled ? onFrame : null;
    this.renderer?.set_debug_enabled(enabled);
  }

  loadFont(name: string, data: Uint8Array): void {
    log("loadFont", { name, bytes: data.byteLength });
    try {
      this.renderer?.load_font(name, data);
    } catch (err) {
      log("load_font rejected", { name, err });
    }
  }

  renderFrame(timeMs: number): void {
    if (this.renderer && this.canvas) {
      try {
        this.renderer.render_frame(timeMs);
        if (this.debugEnabled) {
          const [width, height] = this.renderer.get_frame_size();
          this.onDebugFrame?.({ timeMs, width, height, events: this.renderer.get_frame_debug() });
        }
      } catch (err) {
        log("render_frame rejected", { timeMs, err });
      }
    }
  }

  dispose(): void {
    log("dispose");
    this.renderer?.free();
    this.renderer = null;
  }
}
