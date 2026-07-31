// Web Worker rendering backend. The WASM renderer lives in a dedicated
// worker; this class mirrors the RenderBackend contract on the main
// thread by proxying messages. Frames cross the boundary as transferred
// ArrayBuffers (zero-copy) and are painted with putImageData.
//
// Protocol (see render-worker.ts in the worker example):
//   main  -> worker: init | load | font | resize | render
//   worker -> main:  ready | loaded | frame | error
//
// Render requests carry a sequence number and stale frames (seq older
// than the last painted one) are dropped, so the canvas never shows an
// older frame that happened to arrive late.
import type { RenderBackend, SubtitleSummary } from "./types.ts";

const DEBUG = true;
function dbg(...args: unknown[]): void {
  if (DEBUG) console.log("[subrass:demo:worker-backend]", ...args);
}

export type WorkerMessage =
  | { type: "ready" }
  | { type: "loaded"; summary: SubtitleSummary }
  | { type: "frame"; seq: number; w: number; h: number; buffer: ArrayBuffer }
  | { type: "error"; message: string };

export interface WorkerBackendOptions {
  onError?: (message: string) => void;
}

export class WorkerBackend implements RenderBackend {
  readonly kind = "web-worker";

  private worker: Worker;
  private canvas: HTMLCanvasElement | null = null;
  private ctx: CanvasRenderingContext2D | null = null;
  private onError: (message: string) => void;

  private nextSeq = 1;
  private lastPaintedSeq = 0;
  private readyPromise: Promise<void>;
  private loadResolvers: Array<(summary: SubtitleSummary) => void> = [];

  constructor(workerUrl: URL, options: WorkerBackendOptions = {}) {
    this.onError = options.onError ?? (() => {});
    dbg("constructor", {
      workerUrl: workerUrl.href,
      importMetaUrl: import.meta.url,
      note:
        "init message only loads WASM inside the worker. Main-thread AssDoc " +
        "in app.ts still needs its own init() on the main-thread module instance.",
    });
    this.worker = new Worker(workerUrl, { type: "module" });
    this.worker.onmessage = (e: MessageEvent<WorkerMessage>) => this.onMessage(e.data);
    this.worker.onerror = (e) => {
      dbg("worker onerror", e.message, e);
      this.onError(`Worker failed: ${e.message}`);
    };

    this.readyPromise = new Promise<void>((resolve) => {
      this.readyResolve = resolve;
    });
    dbg("postMessage init → worker");
    this.worker.postMessage({ type: "init" });
  }

  private readyResolve!: () => void;

  async init(): Promise<void> {
    dbg("init() waiting for worker ready…");
    const t0 = performance.now();
    await this.readyPromise;
    dbg("init() worker ready", { ms: +(performance.now() - t0).toFixed(1) });
  }

  setFrameTarget(canvas: HTMLCanvasElement): void {
    dbg("setFrameTarget", { id: canvas.id, w: canvas.width, h: canvas.height });
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
  }

  loadAss(content: string): Promise<SubtitleSummary> {
    dbg("loadAss → worker", { contentBytes: content.length, pending: this.loadResolvers.length });
    return new Promise((resolve) => {
      this.loadResolvers.push(resolve);
      this.worker.postMessage({ type: "load", content });
    });
  }

  resize(width: number, height: number): void {
    if (width > 0 && height > 0) {
      this.worker.postMessage({ type: "resize", w: width, h: height });
    }
  }

  loadFont(name: string, data: Uint8Array): void {
    // Copy into a detached buffer so the caller keeps its Uint8Array usable.
    const buffer = data.slice().buffer;
    this.worker.postMessage({ type: "font", name, data: buffer }, [buffer]);
  }

  renderFrame(timeMs: number): void {
    this.worker.postMessage({ type: "render", timeMs, seq: this.nextSeq++ });
  }

  dispose(): void {
    this.worker.terminate();
  }

  private onMessage(msg: WorkerMessage): void {
    switch (msg.type) {
      case "ready":
        dbg("← worker ready (WASM inited in worker only)");
        this.readyResolve();
        break;
      case "loaded": {
        dbg("← worker loaded", msg.summary);
        const resolve = this.loadResolvers.shift();
        resolve?.(msg.summary);
        break;
      }
      case "frame":
        this.paintFrame(msg);
        break;
      case "error":
        dbg("← worker error", msg.message);
        this.onError(msg.message);
        break;
    }
  }

  private paintFrame(msg: { seq: number; w: number; h: number; buffer: ArrayBuffer }): void {
    if (msg.seq <= this.lastPaintedSeq) return; // stale frame
    this.lastPaintedSeq = msg.seq;
    if (!this.canvas || !this.ctx) return;
    if (this.canvas.width !== msg.w || this.canvas.height !== msg.h) {
      this.canvas.width = msg.w;
      this.canvas.height = msg.h;
    }
    const pixels = new Uint8ClampedArray(msg.buffer);
    this.ctx.putImageData(new ImageData(pixels, msg.w, msg.h), 0, 0);
  }
}
