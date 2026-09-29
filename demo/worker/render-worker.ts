// Worker side of the render protocol: owns the WASM SubtitleRenderer,
// renders frames off the main thread, and transfers RGBA buffers back.
// Every response echoes the incoming `requestId`. Renderer failures are
// reported as { kind: "error", requestId, error } instead of crashing
// the worker.
import init, { SubtitleRenderer } from "../../pkg/subrass.js";
import { dbg } from "../shared/debug.ts";

const log = dbg("render-worker");

let renderer: SubtitleRenderer | null = null;
let frameW = 0;
let frameH = 0;
let debugEnabled = false;

interface InMessage {
  kind: string;
  requestId?: number;
  content?: string | Uint8Array;
  timeMs?: number;
  w?: number;
  h?: number;
  name?: string;
  data?: ArrayBuffer;
  enabled?: boolean;
}

function fail(requestId: number | undefined, error: unknown): void {
  const message = error instanceof Error ? error.message : String(error);
  log("renderer call failed", { requestId, message });
  self.postMessage({ kind: "error", requestId, error: message });
}

self.onmessage = async (event: MessageEvent<InMessage>) => {
  const msg = event.data;
  switch (msg.kind) {
    case "init": {
      try {
        await init();
        log("init ok, posting ready");
        self.postMessage({ kind: "ready" });
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        log("init FAILED", message);
        self.postMessage({ kind: "fatal", error: message });
      }
      break;
    }
    case "loadAss": {
      let next: SubtitleRenderer | null = null;
      try {
        next = typeof msg.content === "string"
          ? new SubtitleRenderer(msg.content)
          : SubtitleRenderer.from_bytes(msg.content ?? new Uint8Array());
        if (frameW > 0 && frameH > 0) {
          next.set_video_size(frameW, frameH);
        }
        next.set_debug_enabled(debugEnabled);
        const [w, h] = next.get_play_resolution();
        const summary = {
          resolution: [w, h] as [number, number],
          styles: next.get_style_count(),
          events: next.get_event_count(),
        };
        renderer?.free();
        renderer = next;
        next = null;
        log("loadAss ok", { requestId: msg.requestId, ...summary });
        self.postMessage({ kind: "loaded", requestId: msg.requestId, summary });
      } catch (err) {
        next?.free();
        fail(msg.requestId, err);
      }
      break;
    }
    case "loadFont": {
      try {
        renderer?.load_font(msg.name ?? "font", new Uint8Array(msg.data ?? []));
      } catch (err) {
        fail(msg.requestId, err);
      }
      break;
    }
    case "setVideoSize": {
      try {
        const w = msg.w ?? 0;
        const h = msg.h ?? 0;
        renderer?.set_video_size(w, h);
        frameW = w;
        frameH = h;
      } catch (err) {
        fail(msg.requestId, err);
      }
      break;
    }
    case "setDebug": {
      debugEnabled = msg.enabled === true;
      renderer?.set_debug_enabled(debugEnabled);
      break;
    }
    case "render": {
      try {
        if (!renderer) throw new Error("No subtitle file loaded");
        renderer.render_frame(msg.timeMs ?? 0);
        const size = renderer.get_frame_size();
        // Frame copies (plan #82 audit): `get_frame_data()` copies the
        // WASM buffer into a fresh JS Uint8Array (required — WASM
        // linear memory cannot be transferred and may grow, so no
        // long-lived view into it is ever kept). The fresh buffer is
        // then *transferred* (zero-copy move, not a copy) to the main
        // thread, which copies it once more into ImageData for canvas
        // painting. Total: one required WASM→JS copy per frame.
        const bytes = renderer.get_frame_data();
        self.postMessage(
          { kind: "frame", requestId: msg.requestId, timeMs: msg.timeMs ?? 0,
            w: size[0], h: size[1], bytes: bytes.buffer,
            ...(debugEnabled ? { debug: renderer.get_frame_debug() } : {}) },
          { transfer: [bytes.buffer] },
        );
      } catch (err) {
        fail(msg.requestId, err);
      }
      break;
    }
    default:
      log("unknown message", msg);
      break;
  }
};
