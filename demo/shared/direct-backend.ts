// Main-thread rendering backend: the WASM renderer lives next to the UI
// and draws straight into the visible canvas. This is the simplest way to
// use subrass — see the worker example for the off-thread variant.
import init, { SubtitleRenderer } from "../../pkg/subrass.js";
import type { RenderBackend, SubtitleSummary } from "./types.ts";

const DEBUG = true;
function dbg(...args: unknown[]): void {
  if (DEBUG) console.log("[subrass:demo:direct-backend]", ...args);
}

export class DirectBackend implements RenderBackend {
  readonly kind = "main-thread";
  private renderer: SubtitleRenderer | null = null;
  private canvas: HTMLCanvasElement | null = null;

  async init(): Promise<void> {
    dbg("init() begin", {
      importMetaUrl: import.meta.url,
      // Default init() fetches subrass_bg.wasm next to the JS module.
      expectedWasmUrl: new URL("../../pkg/subrass_bg.wasm", import.meta.url).href,
    });
    const t0 = performance.now();
    try {
      const exports = await init();
      dbg("init() ok", {
        ms: +(performance.now() - t0).toFixed(1),
        hasExports: !!exports,
        hasMalloc: typeof (exports as { __wbindgen_malloc?: unknown })?.__wbindgen_malloc,
      });
    } catch (err) {
      dbg("init() FAILED", { ms: +(performance.now() - t0).toFixed(1), err });
      throw err;
    }
  }

  setFrameTarget(canvas: HTMLCanvasElement): void {
    dbg("setFrameTarget", { id: canvas.id, w: canvas.width, h: canvas.height });
    this.canvas = canvas;
    this.renderer?.set_canvas(canvas);
  }

  async loadAss(content: string): Promise<SubtitleSummary> {
    dbg("loadAss begin", { contentBytes: content.length, hasCanvas: !!this.canvas });
    if (!this.canvas) throw new Error("setFrameTarget() must be called before loadAss()");
    try {
      this.renderer = new SubtitleRenderer(content);
      this.renderer.set_canvas(this.canvas);
      const [w, h] = this.renderer.get_play_resolution();
      const summary = {
        resolution: [w, h] as [number, number],
        styles: this.renderer.get_style_count(),
        events: this.renderer.get_event_count(),
      };
      dbg("loadAss ok", summary);
      return summary;
    } catch (err) {
      dbg("loadAss FAILED (SubtitleRenderer needs initialized wasm)", err);
      throw err;
    }
  }

  resize(width: number, height: number): void {
    if (width > 0 && height > 0) {
      this.renderer?.set_video_size(width, height);
    }
  }

  loadFont(name: string, data: Uint8Array): void {
    dbg("loadFont", { name, bytes: data.byteLength });
    this.renderer?.load_font(name, data);
  }

  renderFrame(timeMs: number): void {
    if (this.renderer && this.canvas) {
      this.renderer.render_frame(timeMs);
    }
  }

  dispose(): void {
    dbg("dispose");
    this.renderer?.free();
    this.renderer = null;
  }
}
