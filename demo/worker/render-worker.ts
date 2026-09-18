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

interface InMessage {
  kind: string;
  requestId?: number;
  content?: string;
  timeMs?: number;
  w?: number;
  h?: number;
  name?: string;
  data?: ArrayBuffer;
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
      try {
        renderer?.free();
        renderer = new SubtitleRenderer(msg.content ?? "");
        if (frameW > 0 && frameH > 0) {
          renderer.set_video_size(frameW, frameH);
        }
        const [w, h] = renderer.get_play_resolution();
        const summary = {
          resolution: [w, h] as [number, number],
          styles: renderer.get_style_count(),
          events: renderer.get_event_count(),
        };
        log("loadAss ok", { requestId: msg.requestId, ...summary });
        self.postMessage({ kind: "loaded", requestId: msg.requestId, summary });
      } catch (err) {
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
        frameW = msg.w ?? 0;
        frameH = msg.h ?? 0;
        renderer?.set_video_size(frameW, frameH);
      } catch (err) {
        fail(msg.requestId, err);
      }
      break;
    }
    case "render": {
      try {
        if (!renderer) throw new Error("No subtitle file loaded");
        renderer.render_frame(msg.timeMs ?? 0);
        const size = renderer.get_frame_size();
        const bytes = renderer.get_frame_data();
        self.postMessage(
          { kind: "frame", requestId: msg.requestId, w: size[0], h: size[1], bytes: bytes.buffer },
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
