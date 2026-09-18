// Main-thread proxy for the render worker. The WASM renderer lives
// inside the worker; frames come back as RGBA buffers and are painted
// onto the visible canvas here.
//
// Protocol (every message carries a numeric `requestId` echoed by the
// worker so responses route to the right waiter even out of order):
//   main -> worker: { kind: "init" }
//   worker -> main: { kind: "ready" } | { kind: "fatal", error }
//   main -> worker: { kind: "loadAss", requestId, content }
//   worker -> main: { kind: "loaded", requestId, summary }
//                   | { kind: "error", requestId, error }
//   main -> worker: { kind: "render", requestId, timeMs }
//   worker -> main: { kind: "frame", requestId, w, h, bytes }
//                   | { kind: "error", requestId, error }
//   main -> worker: { kind: "setVideoSize", w, h } | { kind: "loadFont", name, data }
import init from "../../pkg/subrass.js";
import type { RenderBackend, SubtitleSummary } from "./types.ts";
import { dbg } from "./debug.ts";

const log = dbg("worker-backend");

interface LoadWaiter {
  resolve: (summary: SubtitleSummary) => void;
  reject: (err: Error) => void;
}

export interface WorkerBackendOptions {
  onError?: (message: string) => void;
  /** Override worker construction (tests inject a fake). */
  createWorker?: (url: URL) => Worker;
}

export class WorkerBackend implements RenderBackend {
  readonly kind = "web-worker";
  private worker: Worker | null = null;
  private workerUrl: URL;
  private options: WorkerBackendOptions;

  private readyPromise: Promise<void> | null = null;
  private readyResolve: (() => void) | null = null;
  private readyReject: ((err: Error) => void) | null = null;
  private readySettled = false;

  private nextRequestId = 1;
  private loadWaiters = new Map<number, LoadWaiter>();

  // Render coalescing: at most one render in flight; a newer request
  // while one is in flight replaces the pending one (backpressure).
  private renderInFlight: number | null = null;
  private pendingRenderMs: number | null = null;

  private canvas: HTMLCanvasElement | null = null;
  private ctx: CanvasRenderingContext2D | null = null;
  private disposed = false;

  constructor(workerUrl: URL, options: WorkerBackendOptions = {}) {
    this.workerUrl = workerUrl;
    this.options = options;
  }

  async init(): Promise<void> {
    log("init() begin", { workerUrl: this.workerUrl.href });
    if (!this.readyPromise) {
      // init() on the main thread initializes this module's wasm bindings,
      // which app.ts needs for AssDoc. The worker has its own module
      // instance and initializes itself on the "init" message.
      await init();
      if (this.disposed) {
        // Disposed while wasm was initializing: never spawn the
        // worker; init() below rejects on this promise instead.
        this.readyPromise = Promise.reject(new Error("Worker backend disposed"));
        this.readyPromise.catch(() => {});
      } else {
        this.readyPromise = new Promise<void>((resolve, reject) => {
          this.readyResolve = resolve;
          this.readyReject = reject;
        });
        const worker = this.options.createWorker
          ? this.options.createWorker(this.workerUrl)
          : new Worker(this.workerUrl, { type: "module" });
        this.worker = worker;
        worker.onmessage = (event: MessageEvent) => this.handleMessage(event.data);
        worker.onerror = (event) => {
          this.fail(new Error(`Render worker error: ${event.message}`));
        };
        worker.onmessageerror = () => {
          this.fail(new Error("Render worker message deserialization failed"));
        };
        worker.postMessage({ kind: "init" });
      }
    }
    await this.readyPromise;
    log("init() done — worker ready");
  }

  setFrameTarget(canvas: HTMLCanvasElement): void {
    log("setFrameTarget", { id: canvas.id, w: canvas.width, h: canvas.height });
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.post({ kind: "setVideoSize", w: canvas.width, h: canvas.height });
  }

  loadAss(content: string): Promise<SubtitleSummary> {
    const requestId = this.nextRequestId++;
    log("loadAss begin", { requestId, contentBytes: content.length });
    return new Promise<SubtitleSummary>((resolve, reject) => {
      if (this.disposed || !this.worker) {
        reject(new Error("Worker is not running"));
        return;
      }
      this.loadWaiters.set(requestId, { resolve, reject });
      this.post({ kind: "loadAss", requestId, content });
    });
  }

  resize(width: number, height: number): void {
    if (width > 0 && height > 0) {
      this.post({ kind: "setVideoSize", w: width, h: height });
    }
  }

  loadFont(name: string, data: Uint8Array): void {
    log("loadFont", { name, bytes: data.byteLength });
    // Copy the exact view range: transferring `data.buffer` directly
    // would detach the caller's whole pool and send the wrong bytes
    // when `data` is a subarray view.
    const buf = data.buffer.slice(
      data.byteOffset,
      data.byteOffset + data.byteLength,
    ) as ArrayBuffer;
    this.post({ kind: "loadFont", name, data: buf }, [buf]);
  }

  renderFrame(timeMs: number): void {
    if (this.disposed) return;
    if (this.renderInFlight !== null) {
      // A render is already in flight: remember only the newest
      // request instead of queueing every animation frame.
      this.pendingRenderMs = timeMs;
      return;
    }
    this.sendRender(timeMs);
  }

  dispose(): void {
    log("dispose");
    this.disposed = true;
    this.fail(new Error("Worker backend disposed"));
    this.worker?.terminate();
    this.worker = null;
  }

  private sendRender(timeMs: number): void {
    if (this.disposed || !this.worker) return;
    const requestId = this.nextRequestId++;
    this.renderInFlight = requestId;
    this.post({ kind: "render", requestId, timeMs });
  }

  private post(message: unknown, transfer: Transferable[] = []): void {
    if (this.disposed || !this.worker) return;
    this.worker.postMessage(message, transfer);
  }

  private handleMessage(msg: {
    kind: string;
    requestId?: number;
    error?: string;
    summary?: SubtitleSummary;
    timeMs?: number;
    w?: number;
    h?: number;
    bytes?: ArrayBuffer;
  }): void {
    switch (msg.kind) {
      case "ready":
        log("worker ready (main-thread and worker wasm both initialized)");
        this.settleReady(null);
        break;
      case "fatal":
        this.fail(new Error(msg.error ?? "Worker failed to initialize"));
        break;
      case "loaded": {
        const waiter = msg.requestId !== undefined ? this.loadWaiters.get(msg.requestId) : undefined;
        if (waiter && msg.summary) {
          this.loadWaiters.delete(msg.requestId!);
          log("loaded", { requestId: msg.requestId, summary: msg.summary });
          waiter.resolve(msg.summary);
        }
        break;
      }
      case "frame": {
        if (msg.requestId !== this.renderInFlight || msg.bytes === undefined) break;
        this.renderInFlight = null;
        const w = msg.w ?? 0;
        const h = msg.h ?? 0;
        if (this.ctx && w > 0 && h > 0) {
          const image = new ImageData(new Uint8ClampedArray(msg.bytes), w, h);
          this.ctx.putImageData(image, 0, 0);
        }
        // Flush a newer pending render, if any.
        if (this.pendingRenderMs !== null) {
          const timeMs = this.pendingRenderMs;
          this.pendingRenderMs = null;
          this.sendRender(timeMs);
        }
        break;
      }
      case "error": {
        const err = new Error(msg.error ?? "Worker render error");
        if (msg.requestId !== undefined) {
          const waiter = this.loadWaiters.get(msg.requestId);
          if (waiter) {
            this.loadWaiters.delete(msg.requestId);
            waiter.reject(err);
            break;
          }
          if (msg.requestId === this.renderInFlight) {
            this.renderInFlight = null;
            this.options.onError?.(err.message);
            if (this.pendingRenderMs !== null) {
              const timeMs = this.pendingRenderMs;
              this.pendingRenderMs = null;
              this.sendRender(timeMs);
            }
            break;
          }
        }
        this.options.onError?.(err.message);
        break;
      }
      default:
        log("unknown worker message", msg);
        break;
    }
  }

  private settleReady(err: Error | null): void {
    if (this.readySettled) return;
    this.readySettled = true;
    if (err) this.readyReject?.(err);
    else this.readyResolve?.();
    this.readyResolve = null;
    this.readyReject = null;
  }

  /** Reject every waiter and report through onError. */
  private fail(err: Error): void {
    this.settleReady(err);
    for (const [, waiter] of this.loadWaiters) waiter.reject(err);
    this.loadWaiters.clear();
    this.renderInFlight = null;
    this.pendingRenderMs = null;
    if (!this.disposed) this.options.onError?.(err.message);
  }
}
