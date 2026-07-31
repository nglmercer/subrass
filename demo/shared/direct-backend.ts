// Main-thread rendering backend: the WASM renderer lives next to the UI
// and draws straight into the visible canvas. This is the simplest way to
// use subrass — see the worker example for the off-thread variant.
import init, { SubtitleRenderer } from "../../pkg/subrass.js";
import type { RenderBackend, SubtitleSummary } from "./types.ts";

export class DirectBackend implements RenderBackend {
  readonly kind = "main-thread";
  private renderer: SubtitleRenderer | null = null;
  private canvas: HTMLCanvasElement | null = null;

  async init(): Promise<void> {
    await init();
  }

  setFrameTarget(canvas: HTMLCanvasElement): void {
    this.canvas = canvas;
    this.renderer?.set_canvas(canvas);
  }

  async loadAss(content: string): Promise<SubtitleSummary> {
    if (!this.canvas) throw new Error("setFrameTarget() must be called before loadAss()");
    this.renderer = new SubtitleRenderer(content);
    this.renderer.set_canvas(this.canvas);
    const [w, h] = this.renderer.get_play_resolution();
    return {
      resolution: [w, h],
      styles: this.renderer.get_style_count(),
      events: this.renderer.get_event_count(),
    };
  }

  resize(width: number, height: number): void {
    if (width > 0 && height > 0) {
      this.renderer?.set_video_size(width, height);
    }
  }

  loadFont(name: string, data: Uint8Array): void {
    this.renderer?.load_font(name, data);
  }

  renderFrame(timeMs: number): void {
    if (this.renderer && this.canvas) {
      this.renderer.render_frame(timeMs);
    }
  }

  dispose(): void {
    this.renderer?.free();
    this.renderer = null;
  }
}
